import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/auth_tokens.dart';
import 'package:caramba_client/data/models/user.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/subscription_state.dart';

/// Стадия сессии — используется роутером для гейтинга `/onboarding`.
enum AuthStage {
  /// Первичная проверка наличия сессии (показываем splash).
  unknown,

  /// Нет валидной сессии — показываем онбординг/логин.
  unauthenticated,

  /// Идёт логин/регистрация.
  authenticating,

  /// Залогинен; [AuthState.user] заполнен (или подгружается).
  authenticated,
}

/// Снимок состояния аутентификации.
class AuthState {
  final AuthStage stage;
  final User? user;

  /// Текст последней ошибки логина/регистрации (для inline-показа в форме).
  final String? error;

  const AuthState({
    this.stage = AuthStage.unknown,
    this.user,
    this.error,
  });

  bool get isAuthenticated => stage == AuthStage.authenticated;
  bool get isBusy => stage == AuthStage.authenticating;

  AuthState copyWith({AuthStage? stage, User? user, String? error}) => AuthState(
        stage: stage ?? this.stage,
        user: user ?? this.user,
        error: error,
      );
}

/// Управляет жизненным циклом сессии: восстановление, логины, профиль, логаут.
class AuthNotifier extends StateNotifier<AuthState> {
  final ApiClient _api;
  final TokenStore _tokens;
  final Ref _ref;

  AuthNotifier(this._api, this._tokens, this._ref) : super(const AuthState()) {
    // Когда refresh окончательно протух — выкидываем в unauthenticated.
    _api.onSessionExpired = _forceLogout;
    unawaited(_restore());
  }

  /// Тихое восстановление сессии при старте: если есть refresh-токен — считаем
  /// пользователя залогиненным и подгружаем профиль (interceptor сам обновит
  /// access при первом 401).
  Future<void> _restore() async {
    bool hasSession;
    try {
      hasSession = await _tokens.hasSession();
    } catch (_) {
      // Secure storage недоступно (например, нет libsecret на Linux) — не виснем
      // на сплеше: трактуем как отсутствие сессии и уводим на логин.
      hasSession = false;
    }
    if (hasSession) {
      state = state.copyWith(stage: AuthStage.authenticated);
      await _loadProfile();
    } else {
      state = state.copyWith(stage: AuthStage.unauthenticated);
    }
  }

  /// Регистрация по email/password. [enrollCode] (P2, опц.) — инвайт-код
  /// энроллмента: панель расходует его при создании аккаунта и, если онбординг
  /// включён, единоразово начисляет онбординг-трафик. Отсутствие кода => прежний
  /// путь регистрации без побочных эффектов.
  Future<void> register({
    required String email,
    required String password,
    String? fullName,
    String? enrollCode,
  }) =>
      _runAuth(() => _api.register(
            email: email,
            password: password,
            fullName: fullName,
            enrollCode: enrollCode,
          ));

  Future<void> loginEmail({
    required String email,
    required String password,
  }) =>
      _runAuth(() => _api.loginEmail(email: email, password: password));

  /// Логин по коду из Telegram-бота (6 цифр). Панель сверяет код в Redis
  /// (`app:logincode:{code}`), привязанный к Telegram-аккаунту, и выдаёт
  /// JWT-пару. На неверный/истёкший код — 401 с inline-ошибкой в форме.
  Future<void> loginCode({required String code, String? enrollCode}) => _runAuth(
        () => _api.loginCode(code: code, enrollCode: enrollCode),
        unauthorizedMessage: 'Код неверный или истёк. Запросите новый в боте.',
      );

  /// Telegram-логин (initData WebApp или поля Login Widget).
  Future<void> loginTelegram({
    String? initData,
    int? id,
    String? firstName,
    String? lastName,
    String? username,
    int? authDate,
    String? hash,
  }) =>
      _runAuth(() => _api.loginTelegram(
            initData: initData,
            id: id,
            firstName: firstName,
            lastName: lastName,
            username: username,
            authDate: authDate,
            hash: hash,
          ));

  /// Общий путь логина/регистрации: сохраняем токены, грузим профиль и
  /// подписку. [unauthorizedMessage] подменяет сырой текст панели на 401
  /// человекочитаемой строкой в форме (без em-dash).
  Future<void> _runAuth(
    Future<AuthTokens> Function() op, {
    String? unauthorizedMessage,
  }) async {
    state = state.copyWith(stage: AuthStage.authenticating, error: null);
    try {
      final tokens = await op();
      await _tokens.save(tokens);
      state = state.copyWith(stage: AuthStage.authenticated, error: null);
      await _loadProfile();
      _preloadSubscription();
    } on ApiException catch (e) {
      state = state.copyWith(
        stage: AuthStage.unauthenticated,
        error: (e.isUnauthorized && unauthorizedMessage != null)
            ? unauthorizedMessage
            : e.message,
      );
    } catch (e) {
      state = state.copyWith(
        stage: AuthStage.unauthenticated,
        error: 'Что-то пошло не так. Повторите попытку.',
      );
    }
  }

  /// Стартует фоновую загрузку подписки сразу после логина, чтобы у home/connect
  /// уже был `subscription_uuid` и URL clash-конфига для Go-ядра. Ошибки
  /// проглатываются: подписка перезапросится при заходе на экран.
  void _preloadSubscription() {
    unawaited(
      Future(() async {
        try {
          await _ref.read(subscriptionProvider.future);
        } catch (_) {
          // Подписка подтянется лениво при первом заходе на home/connect.
        }
      }),
    );
  }

  /// Подгружает `/me` в уже аутентифицированную сессию (не валит логин при сбое).
  Future<void> _loadProfile() async {
    try {
      final user = await _api.getMe();
      state = state.copyWith(stage: AuthStage.authenticated, user: user);
    } on ApiException catch (e) {
      if (e.isUnauthorized) _forceLogout();
      // Иные ошибки профиля не разлогинивают — UI покажет fallback-имя.
    } catch (_) {
      // Сеть/парс — игнорируем, профиль подтянется позже.
    }
  }

  /// Ручное обновление профиля (pull-to-refresh / после покупки плана).
  Future<void> refreshProfile() => _loadProfile();

  /// Явный логаут: отзыв refresh на сервере + локальная очистка.
  Future<void> logout() async {
    final refresh = await _tokens.readRefresh();
    if (refresh != null && refresh.isNotEmpty) {
      await _api.logout(refresh);
    }
    await _tokens.clear();
    _ref.invalidate(subscriptionProvider);
    state = const AuthState(stage: AuthStage.unauthenticated);
  }

  /// Принудительный выход (вызывается из API при протухшем refresh).
  void _forceLogout() {
    unawaited(_tokens.clear());
    _ref.invalidate(subscriptionProvider);
    state = const AuthState(stage: AuthStage.unauthenticated);
  }
}

final authProvider = StateNotifierProvider<AuthNotifier, AuthState>((ref) {
  return AuthNotifier(
    ref.watch(apiClientProvider),
    ref.watch(tokenStoreProvider),
    ref,
  );
});

/// Удобный селектор текущего пользователя.
final currentUserProvider = Provider<User?>(
  (ref) => ref.watch(authProvider).user,
);
