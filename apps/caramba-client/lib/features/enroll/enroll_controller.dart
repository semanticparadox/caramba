import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/auth_tokens.dart';
import 'package:caramba_client/data/models/csm_enrollment.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/csm_enrollment_bridge.dart';
import 'package:caramba_client/state/csm_state.dart';
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
  }) => EnrollState(
    stage: stage ?? this.stage,
    link: link ?? this.link,
    validation: validation ?? this.validation,
    error: clearError ? null : (error ?? this.error),
  );
}

class EnrollNotifier extends StateNotifier<EnrollState> {
  final Ref _ref;

  /// id профиля панели, СОЗДАННОГО этим потоком. Заполнен только когда профиля
  /// с таким URL раньше не было: на невалидном коде убираем за собой именно
  /// его, чтобы в списке подключений не оставался мёртвый плейсхолдер. Профиль,
  /// который существовал до нас, не трогаем ни при каком исходе.
  String? _createdProfileId;

  EnrollNotifier(this._ref) : super(const EnrollState());

  /// Сбрасывает поток в исходное состояние (повторный заход / отмена).
  void reset() {
    _createdProfileId = null;
    state = const EnrollState();
  }

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
    String? linkPin,
  }) async {
    final link = EnrollLink.fromParts(panelUrl: panelUrl, code: code);
    if (link == null) {
      state = state.copyWith(
        stage: EnrollStage.needInput,
        error: 'Проверьте код и URL панели (нужен https-адрес).',
      );
      return;
    }
    await startWith(link, linkPin: linkPin);
  }

  /// Закрепляет link_pin ссылки на активном профиле.
  ///
  /// Пин это единственный момент, когда доверие СОЗДАЁТСЯ, поэтому он ставится
  /// один раз и только на профиле, который его ещё не закреплял; повторный
  /// вызов ничего не меняет (02-SPEC.md 2.1 правило 1).
  Future<void> _pinFromLink(EnrollLink link, String? linkPin) async {
    if (linkPin == null || linkPin.isEmpty) {
      return;
    }
    final csmLink = CsmEnrollLink.fromParts(
      origin: link.panelUrl,
      code: link.code,
      linkPin: linkPin,
    );
    if (csmLink == null) {
      return;
    }
    await _ref.read(csmNotifierProvider).establishPinFromLink(csmLink);
  }

  /// Заводит профиль панели (P1-провайдер) и запускает валидацию кода.
  Future<void> startWith(EnrollLink link, {String? linkPin}) async {
    state = EnrollState(stage: EnrollStage.validating, link: link);

    // Профиль панели заводится сразу: аккаунт обязателен, профиль ведёт
    // последующий вход и подключение. Имя уточним из panel_name после валидации.
    final profiles = _ref.read(connectionProfilesProvider.notifier);
    final preexisting = profiles.findPanelId(link.panelUrl);
    final profileId = await profiles.addPanelAccount(
      panelUrl: link.panelUrl,
      displayName: link.panelUrl,
    );
    _createdProfileId = preexisting == null ? profileId : null;
    // Энроллмент означает переход на эту панель: делаем её профиль активным,
    // иначе `configure` уйдёт на прежний профиль.
    await profiles.activate(profileId);

    final client = _ref.read(enrollApiClientProvider(link.panelUrl));
    try {
      final v = await client.validateEnroll(link.code);
      if (!mounted) return;
      if (!v.valid) {
        await _dropOrphanProfile();
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
            .addPanelAccount(
              panelUrl: link.panelUrl,
              displayName: v.panelName!,
            );
      }
      // Ссылка несёт k, значит она несёт link_pin, и профиль обязан его
      // закрепить. Без этого шага закреплённая ссылка молча превращается в
      // незакреплённую, а разницу между продиктованным вне полосы и пришедшим
      // в приложение пином экран личности оператора показывает как свойство
      // безопасности (INV-18).
      await _pinFromLink(link, linkPin);
      state = state.copyWith(
        stage: EnrollStage.valid,
        validation: v,
        clearError: true,
      );
    } on ApiException catch (e) {
      await _dropOrphanProfile();
      if (!mounted) return;
      state = state.copyWith(
        stage: EnrollStage.invalid,
        error: e.isUnauthorized
            ? 'Код недействителен.'
            : 'Не удалось проверить код. Проверьте URL панели и связь.',
      );
    } catch (_) {
      await _dropOrphanProfile();
      if (!mounted) return;
      state = state.copyWith(
        stage: EnrollStage.invalid,
        error: 'Не удалось связаться с панелью. Проверьте URL и связь.',
      );
    }
  }

  /// Убирает профиль-плейсхолдер, созданный этим потоком, когда энроллмент не
  /// состоялся. Идемпотентно: повторный вызов ничего не делает.
  Future<void> _dropOrphanProfile() async {
    final id = _createdProfileId;
    if (id == null) return;
    _createdProfileId = null;
    await _ref.read(connectionProfilesProvider.notifier).remove(id);
  }

  /// Регистрация email/password с расходованием enroll-кода на панели из ссылки.
  Future<void> registerWithEnroll({
    required String email,
    required String password,
    String? fullName,
  }) => _runAuth(
    (client, code) => client.register(
      email: email,
      password: password,
      fullName: fullName,
      enrollCode: code,
    ),
  );

  /// Вход по 6-значному коду из бота, с передачей enroll-кода. login/code
  /// расходует enroll только если за ним стоит создание нового аккаунта.
  Future<void> loginCodeWithEnroll({required String botCode}) => _runAuth(
    (client, code) => client.loginCode(code: botCode, enrollCode: code),
  );

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
      // Владелец записывается ВМЕСТЕ с токенами. Пока профиль не закрепил
      // корень, его сессия лежит в legacy-корзине, а корзина одна на установку:
      // без метки будущая миграция не знала бы, чья это сессия, и отдала бы её
      // первому спросившему профилю (06-MIGRATION.md 7.1).
      final profileId = _ref
          .read(connectionProfilesProvider.notifier)
          .findPanelId(link.panelUrl);
      await _ref
          .read(tokenStoreProvider)
          .save(tokens, ownerId: profileId ?? '');
      // Профиль этой панели получает СВОИ креды: без них `_resolveVpnConfig`
      // падает на дефолтный путь тенанта-1 и конфигурирует ядро против чужой
      // панели. UUID подписки тянем панель-скоупным клиентом (baseUrl из
      // ссылки), токены он берёт из общего TokenStore, куда мы их только что
      // положили.
      await _storePanelCredentials(link, client, tokens);
      // Регистрация в CSM/1 идёт ПОСЛЕ входа и ЧЕРЕЗ ЯДРО: она поднимает
      // ключевой документ по лестнице транспортов, проверяет его против
      // закреплённого пина и считает pid. Без этого шага профиль навсегда
      // остаётся в стадии `pinned`: набор возможностей падает до пустого,
      // запись настроек не уходит, каталог не проверяется и история попыток
      // пуста, то есть весь слой CSM инертен (02-SPEC.md 9).
      //
      // Отказ регистрации НЕ отменяет вход: аккаунт на панели создан, токены
      // сохранены, и подключение по подписке работает. Пользователь видит
      // приложение без раздела проверки, а не экран ошибки.
      await _enrollCsm(link, tokens.accessToken);
      // Вход состоялся: профиль больше не сирота.
      _createdProfileId = null;
      if (!mounted) return;
      // Итоговый экран (онбординг-трафик) показываем ДО передачи сессии, чтобы
      // пользователь увидел подтверждение. Короткая пауза даёт кадру с итогом
      // отрисоваться; затем роутер уведёт в приложение по auth-гейту.
      state = state.copyWith(stage: EnrollStage.done, clearError: true);
      await Future<void>.delayed(const Duration(milliseconds: 1200));
      if (!mounted) return;
      // Не сброс провайдера, а приём сессии тем же экземпляром: сброс утаскивал
      // за собой роутер. См. AuthNotifier.adoptSession.
      unawaited(_ref.read(authProvider.notifier).adoptSession());
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

  /// Регистрирует профиль в CSM/1 и закрепляет проверенный ключевой документ.
  ///
  /// Возвращает `true`, когда профиль перешёл в `anchored`. Отказ намеренно не
  /// поднимается наверх исключением: вход уже состоялся, и ронять его из-за
  /// оператора, который CSM/1 не говорит, было бы регрессом для всех, кто
  /// подключается по обычной подписке.
  Future<bool> _enrollCsm(EnrollLink link, String accessToken) async {
    final csm = _ref.read(csmProfileStateProvider);
    // Профиль без закреплённого пина регистрировать нечем: пин это то, против
    // чего проверяется первый ключевой документ (02-SPEC.md 7.2).
    if (csm == null) {
      return false;
    }
    if (csm.stage == CsmProfileStage.anchored) {
      return true;
    }
    final result = await csmEnrollAndAnchor(
      connection: _ref.read(vpnConnectionProvider),
      notifier: _ref.read(csmNotifierProvider),
      origin: link.panelUrl,
      code: link.code,
      linkPin: csm.pin.linkPin,
      accountJwt: accessToken,
    );
    if (!result.ok) {
      return false;
    }
    // Первый цикл выборки сразу за регистрацией: без него каталог и директива
    // появятся только к следующему запуску, а до тех пор действующий набор
    // возможностей пуст и половина экранов показывает пустое состояние.
    await csmRefreshAndAnchor(
      connection: _ref.read(vpnConnectionProvider),
      notifier: _ref.read(csmNotifierProvider),
    );
    return true;
  }

  /// Кладёт на профиль панели её URL, UUID подписки и свежий access-токен.
  /// Подписки может ещё не быть (новый неоплаченный аккаунт) — тогда сохраняем
  /// хотя бы URL и токен, а UUID подтянется при следующем энроллменте/входе.
  Future<void> _storePanelCredentials(
    EnrollLink link,
    ApiClient client,
    AuthTokens tokens,
  ) async {
    final profiles = _ref.read(connectionProfilesProvider.notifier);
    final id = profiles.findPanelId(link.panelUrl);
    if (id == null) return;
    String? uuid;
    try {
      final sub = await client.getSubscription();
      if (sub.subscriptionUuid.isNotEmpty) uuid = sub.subscriptionUuid;
    } catch (_) {
      // Подписки нет или панель не ответила: не роняем энроллмент из-за неё.
    }
    await profiles.setPanelCredentials(
      id,
      panelUrl: link.panelUrl,
      subscriptionUuid: uuid,
      accessToken: tokens.accessToken,
    );
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
final enrollApiClientProvider = Provider.autoDispose.family<ApiClient, String>((
  ref,
  panelUrl,
) {
  // [ApiClient] сам строит Dio с baseUrl '$panelUrl/api/v2/app' и нашими
  // интерсепторами (skipAuth на публичных вызовах энроллмента).
  return ApiClient(tokens: ref.watch(tokenStoreProvider), baseUrl: panelUrl);
});
