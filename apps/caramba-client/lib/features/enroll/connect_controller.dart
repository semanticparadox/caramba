/// Поток подключения панели по ссылке `caramba://connect`.
///
/// Это ЗАМЕНА тупику. Прежний вход в панель был один: экран «введите инвайт-код».
/// Кодов на живой панели ноль, выпускать их было нечем, а кнопка QR показывала
/// тост, поэтому пройти экран не мог никто — включая владельца, у которого
/// аккаунт уже есть. Здесь человек не вводит ничего: ссылка несёт адрес панели
/// и одноразовый секрет, приложение показывает, КУДА оно собирается
/// подключиться, и по подтверждению меняет секрет на сессию.
///
/// Порядок побочных эффектов важен и повторяет `enroll_controller.dart` с одним
/// отличием: профиль панели заводится ПОСЛЕ успешного погашения, а не до.
/// Энроллменту профиль нужен заранее, потому что за валидацией сразу идёт
/// регистрация в той же панели; здесь же неудачное погашение не должно
/// оставлять в списке подключений мёртвую запись, за которой ничего нет.
library;

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/features/enroll/connect_link.dart';
import 'package:caramba_client/features/enroll/connect_redeem.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';

/// Стадия потока.
enum ConnectStage {
  /// Ссылки нет: показываем поле вставки и объяснение, где её взять.
  needLink,

  /// Ссылка разобрана: показываем оператора, адрес и срок, ждём подтверждения.
  confirm,

  /// Ссылка отвергнута разбором. Причина названа, «продолжить всё равно» нет.
  refused,

  /// Идёт погашение кода на панели.
  redeeming,

  /// Панель отказала или не ответила. Ссылка при этом остаётся: одноразовый
  /// код мог и не быть потрачен (сеть), поэтому повтор осмыслен.
  failed,

  /// Сессия получена, профиль панели заведён и активирован.
  done,
}

/// Снимок потока.
class ConnectState {
  const ConnectState({
    this.stage = ConnectStage.needLink,
    this.link,
    this.failure,
    this.detail,
    this.error,
    this.result,
  });

  final ConnectStage stage;

  /// Разобранная ссылка. Непуста начиная с [ConnectStage.confirm].
  final CarambaConnectLink? link;

  /// Причина отказа разбора. Непуста только на [ConnectStage.refused].
  final ConnectLinkFailure? failure;

  /// Техническая деталь отказа: что именно не сошлось. Показывается мелким
  /// шрифтом рядом с человеческим текстом, чтобы отчёт в поддержку был
  /// осмысленным, а не «не работает».
  final String? detail;

  /// Текст ошибки погашения. Непуст только на [ConnectStage.failed].
  final String? error;

  /// Ответ панели. Непуст на [ConnectStage.done].
  final ConnectRedeemResult? result;

  /// Человеческий текст отказа разбора.
  String? get refusalText => failure?.message;

  ConnectState copyWith({
    ConnectStage? stage,
    CarambaConnectLink? link,
    ConnectLinkFailure? failure,
    String? detail,
    String? error,
    ConnectRedeemResult? result,
  }) => ConnectState(
    stage: stage ?? this.stage,
    link: link ?? this.link,
    failure: failure,
    detail: detail,
    error: error,
    result: result ?? this.result,
  );
}

/// Источник текущего времени. Подменяется в тестах, чтобы проверка срока
/// ссылки не зависела от часов машины, на которой их гоняют.
typedef NowSeconds = int Function();

class ConnectNotifier extends StateNotifier<ConnectState> {
  ConnectNotifier(this._ref, {NowSeconds? now, RedeemFn? redeem})
    : _now = now ?? _systemNow,
      _redeem = redeem ?? redeemConnectCode,
      super(const ConnectState());

  final Ref _ref;
  final NowSeconds _now;
  final RedeemFn _redeem;

  static int _systemNow() => DateTime.now().millisecondsSinceEpoch ~/ 1000;

  /// Возвращает поток в исходное состояние.
  void reset() => state = const ConnectState();

  /// Принимает сырую строку: диплинк, вставленный текст или содержимое QR.
  ///
  /// Разбор здесь ПОВТОРЯЕТСЯ, хотя диплинк уже разбирался в маршрутизаторе.
  /// Это не лишняя работа: через этот же вход приходят вставка из буфера и
  /// QR, и единственная точка разбора важнее, чем сэкономленный микросекундный
  /// проход по 60 байтам.
  void open(String raw) {
    final parsed = parseConnectLink(raw, nowSec: _now());
    final link = parsed.link;
    if (link == null) {
      state = ConnectState(
        stage: ConnectStage.refused,
        failure: parsed.failure,
        detail: parsed.detail,
      );
      return;
    }
    state = ConnectState(stage: ConnectStage.confirm, link: link);
  }

  /// Меняет одноразовый секрет на сессию и делает человека клиентом панели.
  Future<void> confirm() async {
    final link = state.link;
    if (link == null) return;
    state = state.copyWith(stage: ConnectStage.redeeming);

    final ConnectRedeemResult result;
    try {
      result = await _redeem(origin: link.origin, code: link.code);
    } on ConnectRedeemException catch (e) {
      if (!mounted) return;
      state = state.copyWith(stage: ConnectStage.failed, error: e.message);
      return;
    } catch (_) {
      if (!mounted) return;
      state = state.copyWith(
        stage: ConnectStage.failed,
        error: 'Не удалось подключить панель. Повторите попытку.',
      );
      return;
    }

    await _attachPanel(link, result);
    if (!mounted) return;
    state = state.copyWith(stage: ConnectStage.done, result: result);
  }

  /// Передаёт сессию общему auth-слою: он перечитает её из хранилища и
  /// переведёт приложение в authenticated.
  ///
  /// Отдельным шагом, а не хвостом [confirm], потому что передача сессии
  /// уводит человека с этого экрана, а уводить его нельзя, пока он не увидел
  /// то, что экран обязан был сказать (например, что подписки у аккаунта нет и
  /// почему). Момент перехода выбирает экран: сразу, когда сказать нечего, и
  /// по кнопке, когда есть.
  void finish() {
    // Сбрасывать здесь провайдер авторизации нельзя: роутер на него подписан, а
    // подписка в Riverpod — зависимость, и сброс пересоздавал ВЕСЬ роутер под
    // живым деревом. См. AuthNotifier.adoptSession.
    unawaited(_ref.read(authProvider.notifier).adoptSession());
  }

  /// Заводит профиль панели и кладёт в него сессию. Передачу общему auth-слою
  /// делает [finish] отдельно, по решению экрана.
  ///
  /// Имя профиля берётся из ответа панели, а не из ссылки: ссылка не подписана,
  /// а ответ пришёл по TLS с того самого origin, который она назвала. Когда имя
  /// пустое, остаётся заявленное ссылкой — иначе профиль назывался бы голым
  /// адресом там, где имя вообще-то известно.
  Future<void> _attachPanel(
    CarambaConnectLink link,
    ConnectRedeemResult result,
  ) async {
    final profiles = _ref.read(connectionProfilesProvider.notifier);
    final name = result.panelName.isNotEmpty
        ? result.panelName
        : (link.operatorName.isNotEmpty ? link.operatorName : link.origin);
    final id = await profiles.addPanelAccount(
      panelUrl: link.origin,
      displayName: name,
    );
    // Активация уже сделана внутри addPanelAccount; хранилище токенов читаем
    // ПОСЛЕ неё, потому что оно ключёвано активным профилем (02-SPEC.md 1.2).
    // Прочитав раньше, мы положили бы сессию нового оператора в корзину
    // прежнего.
    await _ref.read(tokenStoreProvider).save(result.tokens, ownerId: id);
    // Свои креды профилю нужны отдельно от общей корзины: без panelUrl и uuid
    // резолвер конфигурации ядра уходит на дефолтный путь и настраивает
    // туннель против чужой панели.
    await profiles.setPanelCredentials(
      id,
      panelUrl: link.origin,
      subscriptionUuid: result.subscriptionUuid,
      accessToken: result.tokens.accessToken,
    );
  }
}

/// Сигнатура погашения. Вынесена типом, чтобы тест мог подставить своё без
/// поднятия HTTP.
typedef RedeemFn =
    Future<ConnectRedeemResult> Function({
      required String origin,
      required String code,
    });

/// Провайдер потока. autoDispose: состояние живёт, пока открыт экран.
final connectProvider =
    StateNotifierProvider.autoDispose<ConnectNotifier, ConnectState>(
      (ref) => ConnectNotifier(ref),
    );
