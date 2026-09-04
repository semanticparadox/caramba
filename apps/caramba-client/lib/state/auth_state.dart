import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/auth_tokens.dart';
import 'package:caramba_client/data/models/user.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
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

  const AuthState({this.stage = AuthStage.unknown, this.user, this.error});

  bool get isAuthenticated => stage == AuthStage.authenticated;
  bool get isBusy => stage == AuthStage.authenticating;

  AuthState copyWith({AuthStage? stage, User? user, String? error}) =>
      AuthState(
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
    // Провайдер мог быть выброшен (инвалидация после энроллмента), пока мы
    // ждали secure storage: писать в state после dispose нельзя.
    if (!mounted) return;
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
  }) => _runAuth(
    () => _api.register(
      email: email,
      password: password,
      fullName: fullName,
      enrollCode: enrollCode,
    ),
  );

  Future<void> loginEmail({required String email, required String password}) =>
      _runAuth(() => _api.loginEmail(email: email, password: password));

  /// Логин по коду из Telegram-бота (6 цифр). Панель сверяет код в Redis
  /// (`app:logincode:{code}`), привязанный к Telegram-аккаунту, и выдаёт
  /// JWT-пару. На неверный/истёкший код — 401 с inline-ошибкой в форме.
  ///
  /// После успеха профиль панели ОБЯЗАН существовать и быть активным: см.
  /// [_ensurePanelProfile].
  Future<void> loginCode({required String code, String? enrollCode}) =>
      _runAuth(
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
  }) => _runAuth(
    () => _api.loginTelegram(
      initData: initData,
      id: id,
      firstName: firstName,
      lastName: lastName,
      username: username,
      authDate: authDate,
      hash: hash,
    ),
  );

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
      // Профиль подключения заводится ПОСЛЕДНИМ: он может пересобрать граф
      // провайдеров (список профилей -> активный -> хранилище токенов -> клиент
      // -> этот нотифаер), и всё, что должно случиться в текущей сессии
      // нотифаера, обязано случиться до этого.
      await _ensurePanelProfile(tokens);
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

  /// Заводит и активирует профиль панели после успешного входа.
  ///
  /// ПОЧЕМУ ЭТО ЗДЕСЬ. Сессия и профиль подключения это две разные вещи, и до
  /// сих пор вход по коду из бота создавал только первую. В результате у
  /// человека была панельная сессия, но не было ни одной записи
  /// [ConnectionProfile] типа panelAccount — а именно она несёт `panelUrl`,
  /// `subscriptionUuid` и токен, из которых резолвер собирает конфигурацию
  /// ядра. Вкладка «Серверы» оставалась пустой ровно у тех, кто вошёл. Путь
  /// энроллмента профиль заводил, путь входа по коду — нет, и это была не
  /// разница в замысле, а пропущенный шаг.
  ///
  /// Origin берётся у клиента, которым вход и делался: в публичной сборке он
  /// приходит из активного профиля, в брендированной — из `kApiBaseUrl`. Когда
  /// origin пуст, заводить нечего: такой вход невозможен, потому что клиент без
  /// панели отказывает ещё в интерсепторе.
  ///
  /// UUID подписки здесь НЕ запрашивается: его подтянет `_preloadSubscription`
  /// и запишет владелец этого поля. Молчаливый лишний запрос на пути входа
  /// стоил бы дороже, чем задержка появления uuid на пару секунд.
  ///
  /// НИЧЕГО НЕ ПИШЕМ, КОГДА ПИСАТЬ НЕЧЕГО. Список профилей ведёт активный
  /// профиль, тот ведёт хранилище токенов, а то ведёт API-клиент, от которого
  /// зависит этот самый нотифаер: любая запись сюда пересобирает половину
  /// графа и роняет сессию в перезапуск. На повторном входе в ту же панель
  /// менять нечего, и правильный ответ это выйти сразу.
  ///
  /// Токен кладётся на профиль только при СОЗДАНИИ, как запасной снимок:
  /// `_resolveVpnConfig` предпочитает свежий токен из общего хранилища, потому
  /// что клиент ротирует его при 401, и запись на профиле устаревает за час.
  Future<void> _ensurePanelProfile(AuthTokens tokens) async {
    final origin = _api.panelOrigin.trim();
    if (origin.isEmpty) return;
    final profiles = _ref.read(connectionProfilesProvider.notifier);
    final existing = profiles.findPanelId(origin);
    if (existing != null &&
        _ref.read(connectionProfilesProvider).activeId == existing) {
      return;
    }
    // Имя оператора приложение здесь ещё не знает, поэтому берём хост: он
    // правдив. Уже названный профиль это не переименует — `addPanelAccount`
    // трогает имя, только пока оно равно самому URL панели.
    final id = await profiles.addPanelAccount(
      panelUrl: origin,
      displayName: Uri.parse(origin).host,
    );
    await profiles.setPanelCredentials(
      id,
      panelUrl: origin,
      accessToken: tokens.accessToken,
    );
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

  /// Принимает сессию, которую положил в хранилище другой поток — энроллмент по
  /// коду или подключение панели по ссылке.
  ///
  /// Существует вместо `ref.invalidate(authProvider)`, которым это делалось
  /// раньше, и разница не косметическая. Роутер подписан на этот провайдер,
  /// а подписка в Riverpod — это зависимость: сброс провайдера сбрасывал и
  /// роутер, приложение получало НОВЫЙ GoRouter поверх ещё живого старого, и
  /// оболочка навигации оказывалась в дереве дважды с одним глобальным ключом.
  /// Flutter валился с «Duplicate GlobalKey», кадр не достраивался, и наружу
  /// это выглядело как намертво зависший экран подбора настроек.
  ///
  /// Здесь же состояние меняет тот же самый экземпляр: роутер остаётся на
  /// месте и просто узнаёт новую стадию.
  Future<void> adoptSession() => _restore();

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
