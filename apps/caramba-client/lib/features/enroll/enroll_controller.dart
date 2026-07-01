import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/auth_tokens.dart';
import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';

/// Контроллер энроллмента (P2, contract A/B/C).
///
/// Держит разобранную enroll-ссылку, результат публичной валидации и стадию
/// потока. Регистрация/вход-по-коду идут через ОТДЕЛЬНЫЙ [ApiClient],
/// нацеленный на URL панели из ссылки (а не на дефолтный тенант-1
/// [kApiBaseUrl]) — иначе токены выпустит не тот инстанс (риск из recon).
/// После успешного входа токены ложатся в общий [TokenStore], а сессия
/// перенимается инвалидацией [authProvider] (его конструктор делает restore).

/// Стадия потока энроллмента.
enum EnrollStage {
  /// Нет/невалидная ссылка: показываем ручной ввод (код + URL панели).
  needInput,

  /// Идёт публичная валидация кода на панели.
  validating,

  /// Код валиден: показываем имя панели, онбординг-трафик и выбор register/вход.
  valid,

  /// Код невалиден/истёк/исчерпан, либо панель недоступна.
  invalid,

  /// Идёт регистрация/вход (создание аккаунта на панели).
  submitting,

  /// Аккаунт создан/привязан: показываем итог (вкл. онбординг-трафик).
  done,
}

/// Снимок состояния энроллмента.
class EnrollState {
  final EnrollStage stage;

  /// Разобранная ссылка (панель + код). `null`, пока ввод не дал валидной пары.
  final EnrollLink? link;

  /// Результат валидации (имя панели + онбординг-трафик). Заполнен на [valid].
  final EnrollValidation? validation;

  /// Человекочитаемая ошибка для inline-показа (без em-dash, без слоп-слов).
  final String? error;

  const EnrollState({
    this.stage = EnrollStage.needInput,
    this.link,
    this.validation,
    this.error,
  });

  /// Онбординг-трафик (МБ), полученный новым неоплаченным аккаунтом, или 0.
  int get onboardingTrafficMb => validation?.onboardingTrafficMb ?? 0;

  EnrollState copyWith({
    EnrollStage? stage,
    EnrollLink? link,
    EnrollValidation? validation,
    String? error,
    bool clearError = false,
  }) =>
      EnrollState(
        stage: stage ?? this.stage,
        link: link ?? this.link,
        validation: validation ?? this.validation,
        error: clearError ? null : (error ?? this.error),
      );
}

class EnrollNotifier extends StateNotifier<EnrollState> {
  final Ref _ref;

  EnrollNotifier(this._ref) : super(const EnrollState());

  /// Сбрасывает поток в исходное состояние (повторный заход / отмена).
  void reset() => state = const EnrollState();

  /// Принимает сырой deeplink `carambaconnect://enroll?...`. Если разбор удался
  /// — заводит профиль панели и валидирует код. Иначе оставляет ручной ввод.
  Future<void> openDeeplink(String raw) async {
    final link = EnrollLink.tryParse(raw);
    if (link == null) {
      state = state.copyWith(
        stage: EnrollStage.needInput,
        error: 'Ссылка энроллмента не распознана. Введите код и URL панели.',
      );
      return;
    }
    await startWith(link);
  }

  /// Принимает код + URL панели из ручного ввода (или QR-парсера).
  Future<void> submitManual({
    required String panelUrl,
    required String code,
  }) async {
    final link = EnrollLink.fromParts(panelUrl: panelUrl, code: code);
    if (link == null) {
      state = state.copyWith(
        stage: EnrollStage.needInput,
        error: 'Проверьте код и URL панели (нужен https-адрес).',
      );
      return;
    }
    await startWith(link);
  }

  /// Заводит профиль панели (P1-провайдер) и запускает валидацию кода.
  Future<void> startWith(EnrollLink link) async {
    state = EnrollState(stage: EnrollStage.validating, link: link);

    // Профиль панели заводится сразу: аккаунт обязателен, профиль ведёт
    // последующий вход и подключение. Имя уточним из panel_name после валидации.
    await _ref.read(connectionProfilesProvider.notifier).addPanelAccount(
          panelUrl: link.panelUrl,
          displayName: link.panelUrl,
        );

    final client = _ref.read(enrollApiClientProvider(link.panelUrl));
    try {
      final v = await client.validateEnroll(link.code);
      if (!mounted) return;
      if (!v.valid) {
        state = state.copyWith(
          stage: EnrollStage.invalid,
          validation: v,
          error: _reasonText(v.reason),
        );
        return;
      }
      // Уточняем имя профиля из panel_name (ранний брендинг до P3).
      if (v.panelName != null && v.panelName!.isNotEmpty) {
        await _ref
            .read(connectionProfilesProvider.notifier)
            .addPanelAccount(panelUrl: link.panelUrl, displayName: v.panelName!);
      }
      state = state.copyWith(
        stage: EnrollStage.valid,
        validation: v,
        clearError: true,
      );
    } on ApiException catch (e) {
      if (!mounted) return;
      state = state.copyWith(
        stage: EnrollStage.invalid,
        error: e.isUnauthorized
            ? 'Код недействителен.'
            : 'Не удалось проверить код. Проверьте URL панели и связь.',
      );
    } catch (_) {
      if (!mounted) return;
      state = state.copyWith(
        stage: EnrollStage.invalid,
        error: 'Не удалось связаться с панелью. Проверьте URL и связь.',
      );
    }
  }

  /// Регистрация email/password с расходованием enroll-кода на панели из ссылки.
  Future<void> registerWithEnroll({
    required String email,
    required String password,
    String? fullName,
  }) =>
      _runAuth((client, code) => client.register(
            email: email,
            password: password,
            fullName: fullName,
            enrollCode: code,
          ));

  /// Вход по 6-значному коду из бота, с передачей enroll-кода. login/code
  /// расходует enroll только если за ним стоит создание нового аккаунта.
  Future<void> loginCodeWithEnroll({required String botCode}) =>
      _runAuth((client, code) => client.loginCode(
            code: botCode,
            enrollCode: code,
          ));

  /// Общий путь создания/привязки аккаунта на панели из ссылки. Токены кладём
  /// в общий [TokenStore] и перенимаем сессию инвалидацией [authProvider].
  Future<void> _runAuth(
    Future<AuthTokens> Function(ApiClient client, String enrollCode) op,
  ) async {
    final link = state.link;
    if (link == null) return;
    state = state.copyWith(stage: EnrollStage.submitting, clearError: true);
    final client = _ref.read(enrollApiClientProvider(link.panelUrl));
    try {
      final tokens = await op(client, link.code);
      await _ref.read(tokenStoreProvider).save(tokens);
      if (!mounted) return;
      // Итоговый экран (онбординг-трафик) показываем ДО передачи сессии, чтобы
      // пользователь увидел подтверждение. Короткая пауза даёт кадру с итогом
      // отрисоваться; затем роутер уведёт в приложение по auth-гейту.
      state = state.copyWith(stage: EnrollStage.done, clearError: true);
      await Future<void>.delayed(const Duration(milliseconds: 1200));
      if (!mounted) return;
      _ref.invalidate(authProvider);
    } on ApiException catch (e) {
      if (!mounted) return;
      state = state.copyWith(
        stage: EnrollStage.valid,
        error: e.isUnauthorized
            ? 'Не удалось войти. Проверьте данные и попробуйте снова.'
            : e.message,
      );
    } catch (_) {
      if (!mounted) return;
      state = state.copyWith(
        stage: EnrollStage.valid,
        error: 'Что-то пошло не так. Повторите попытку.',
      );
    }
  }

  /// Маппит машинную причину невалидности в плоский текст (без em-dash).
  String _reasonText(String? reason) {
    switch (reason) {
      case 'expired':
        return 'Срок действия кода истёк.';
      case 'exhausted':
        return 'Код уже использован максимальное число раз.';
      case 'unknown':
        return 'Такого кода нет.';
      default:
        return 'Код недействителен.';
    }
  }
}

/// Провайдер потока энроллмента. autoDispose: состояние живёт, пока открыт экран.
final enrollProvider =
    StateNotifierProvider.autoDispose<EnrollNotifier, EnrollState>(
  (ref) => EnrollNotifier(ref),
);

/// Per-panel [ApiClient], нацеленный на URL панели из enroll-ссылки. Нужен,
/// потому что [apiClientProvider] жёстко зашит на [kApiBaseUrl] (тенант-1):
/// валидацию и регистрацию энроллмента шлём на инстанс из ссылки, иначе токены
/// выпустит не та панель. Токены берём из общего [TokenStore].
final enrollApiClientProvider =
    Provider.autoDispose.family<ApiClient, String>((ref, panelUrl) {
  // [ApiClient] сам строит Dio с baseUrl '$panelUrl/api/v2/app' и нашими
  // интерсепторами (skipAuth на публичных вызовах энроллмента).
  return ApiClient(
    tokens: ref.watch(tokenStoreProvider),
    baseUrl: panelUrl,
  );
});
