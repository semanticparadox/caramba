/// Три состояния предложения — и происхождение каждого из них.
///
/// Весь слой `domain/offering` строится вокруг одного правила: у любой опции
/// РОВНО три исхода, а не два. «Доступно» и «недоступно» описывают флот, про
/// который источник рассказал. Третий исход — «неизвестно» — описывает молчание
/// источника, и раньше его не было: молчание читалось как разрешение (см.
/// удалённую ветку `sourceSilent` в protocol_inventory_state.dart), то есть
/// приложение обещало протокол, которого могло не быть ни на одном узле.
/// Обратная ошибка не лучше: прочитать молчание как запрет значит выключить
/// рабочий выбор.
///
/// Поэтому «неизвестно» — отдельное состояние с собственной причиной, и экран
/// обязан его назвать: «панель не сообщает инбаунды», а не пустое место и не
/// выдуманная строка.
library;

/// Исход для одной опции. Никогда не bool.
enum OfferingStatus {
  /// Источник сказал, что опция есть, и она пригодна.
  available,

  /// Источник сказал, что опции нет или что она непригодна.
  unavailable,

  /// Источник про эту опцию НЕ сказал ничего (или сказал, что не смог узнать).
  /// Это не разрешение и не запрет — это отсутствие ответа.
  unknown,
}

/// Откуда взят факт. Порядок объявления — порядок доверия, по убыванию.
enum OfferingSource {
  /// REST панели (`GET /api/v2/app/servers`, `/relays`) — самый полный
  /// источник: он знает `nodes.id`, страну, инбаунды и релэй-привязку.
  panelRest,

  /// Тело подписки, которое ФАКТИЧЕСКИ потребило ядро. На сегодня это всегда
  /// clash/mihomo: libs/caramba-core/subscription/subscription.go принудительно
  /// дописывает `?client=clash`. Оно беднее REST'а: у прокси нет ни id узла, ни
  /// страны, а транспорт и TLS схлопнуты в один `type:`.
  subscriptionBody,

  /// Встроенный реестр ядра (libs/caramba-core/routing/presets.go). Не сеть:
  /// это то, что ядро умеет применить в этой сборке.
  coreRegistry,

  /// Подписанный каталог CSM. Шов существует, но слой его не заполняет.
  csmCatalog,

  /// Источника нет (профиль не выбран).
  none,
}

/// Точное место, откуда взят факт: источник плюс поле на проводе.
///
/// Нужно ровно для одного: экран должен уметь сказать «панель этого не
/// сообщает», а не молча показать пустоту. Без ссылки на конкретное поле такое
/// сообщение через полгода станет неправдой и никто не заметит.
class Provenance {
  final OfferingSource source;

  /// Поле на проводе или файл в ядре: `/app/servers[].inbounds[]`,
  /// `clash proxies[].type`, `routing/presets.go`.
  final String wire;

  const Provenance(this.source, this.wire);

  /// Никто ничего не сказал.
  static const Provenance nothing = Provenance(OfferingSource.none, '—');

  @override
  bool operator ==(Object other) =>
      other is Provenance && other.source == source && other.wire == wire;

  @override
  int get hashCode => Object.hash(source, wire);

  @override
  String toString() => 'Provenance(${source.name}, $wire)';
}

/// Машиночитаемая причина. Одна причина — одна конкретная ситуация на проводе;
/// общих причин вида «ошибка» здесь нет намеренно, иначе экран снова начнёт
/// гадать.
enum OfferingReason {
  // ─── недоступно ──────────────────────────────────────────────────────────
  /// Профиль подключения не выбран.
  noProfile,

  /// Источник ответил, но узлов в ответе нет.
  sourceEmpty,

  /// Источник перечислил варианты, и этого среди них нет.
  notInFleet,

  /// Узел не принимает новые подключения (`status = full`).
  nodeFull,

  /// Узел не в сети.
  nodeOffline,

  /// В стране есть узлы, но ни один сейчас не принимает подключения.
  allNodesBusy,

  /// Инбаунд включён в панели, но генератор Clash его не выпускает: у
  /// `generate_clash_config` нет ветки под этот протокол. Ровно так живёт
  /// `naive` на узле 1 — его имя попадает в группу `Auto-Relay`, а самого
  /// прокси в теле нет. Строка не прячется: оператор его включил.
  inboundNotEmittedByClash,

  /// AmneziaWG выключен настройкой панели (`amneziawg_client_enabled`).
  inboundAmneziawgDisabled,

  /// Транспорт (`xhttp`/`splithttp`) существует только в Xray; mihomo его не
  /// понимает, и прокси в теле не будет.
  inboundTransportNotSupported,

  /// Релэй у узла есть, но цепочки в конфиге нет: `generate_clash_config`
  /// игнорирует `_relay_nodes`, не выпускает `dialer-proxy` и оставляет в
  /// `server:` адрес выхода. Суффикс `↪` и группа `Auto-Relay` — ярлык.
  /// Настоящую цепочку строит только генератор sing-box (через `detour`).
  relayNotChainedByGenerator,

  /// Импортированное тело цепочку выразить не может в принципе: в clash-форме
  /// нет ни `dialer-proxy`, ни `detour`.
  relayChainingUnsupportedBySource,

  /// Узел — ВХОД цепочки, а не выход: через него набирают другие узлы.
  ///
  /// Не качество и не отказ оператора: относительно вопроса «куда выйти в
  /// интернет» такой машины в предложении нет вовсе. Строка остаётся видимой
  /// (её измеряли, её видно в теле конфига), но выбрать её как выход нельзя —
  /// человек, поставивший VPN, вышел бы в интернет из страны входа, то есть
  /// ровно оттуда, откуда собирался уйти.
  ///
  /// Причина ставится ТОЛЬКО по прямому свидетельству источника: `role: relay`
  /// у ядра (оно выводит её из `dialer-proxy`/`detour`) или ссылка другого
  /// узла панели на этот как на свой `via_relay`. Догадка по имени сюда не
  /// доезжает — см. комментарий в `offering_builder._importedRoleOf`.
  nodeIsRelay,

  /// Действие имеет смысл только с подключённой панелью.
  panelRequired,

  /// Закрепить протокол нельзя, пока не закреплён узел: инбаунд принадлежит
  /// конкретной машине, и «протокол вообще» ничего не выбирает.
  protocolPinNeedsExit,

  // ─── неизвестно ──────────────────────────────────────────────────────────
  /// Панель вернула `inbounds: null` — она сама не смогла их прочитать.
  /// Текст её объяснения лежит в [Availability.detail].
  panelDidNotReportInbounds,

  /// Тело подписки называет только `type:` прокси. Транспорт (`ws`/`grpc`/
  /// `tcp`) и TLS (`reality`/`tls`) в нём схлопнуты, и `vless/tcp/reality` от
  /// `vless/tcp/tls` не отличить. Именно поэтому пикер протоколов нельзя
  /// строить на одном импортированном теле.
  sourceDoesNotReportTransport,

  /// У прокси в теле нет ни id узла, ни страны: тождество машины
  /// восстанавливается только по одинаковому `server:` и по флагу-эмодзи
  /// внутри свободного имени.
  sourceDoesNotReportExitIdentity,

  /// `GET /relays` агрегирует релэи по СТРАНАМ и не называет узлы. Какие
  /// конкретно релэй-ноды стоят за страной, панель на этом эндпоинте не
  /// говорит; узел виден только там, где выход к нему привязан (`via_relay`).
  panelReportsRelaysByCountryOnly,

  /// Пресет опирается на внешние списки (`{BASE}/rulesets/...`), а сообщить,
  /// отдаёт ли их зеркало, некому: ни панель, ни ядро об этом не отчитываются.
  rulesetMirrorUnverified,

  /// Опция применяется, но канала обратной связи нет: ни панель, ни ядро не
  /// сообщают, сработала ли она. Ровно этим неудобен блок рекламы —
  /// «непонятно, работает или нет».
  noFeedbackChannel,

  /// Каталог CSM ещё не ведёт эту часть инвентаря.
  catalogNotReady,

  /// Значение измеряется, но замера ещё нет (пинг до первой пробы).
  notProbedYet,

  /// Панель подключена, но ответ сейчас недоступен (сеть, 5xx, нет подписки).
  panelUnavailable,
}

/// Исход + причина + происхождение. Неизменяемое значение.
class Availability {
  final OfferingStatus status;

  /// `null` только у [OfferingStatus.available].
  final OfferingReason? reason;

  /// Уточнение: имя страны, текст ошибки панели, имя недостающего списка.
  final String? detail;

  /// Откуда взят этот вывод.
  final Provenance origin;

  const Availability._(this.status, this.reason, this.detail, this.origin);

  const Availability.available(Provenance origin)
    : this._(OfferingStatus.available, null, null, origin);

  const Availability.unavailable(
    OfferingReason reason,
    Provenance origin, {
    String? detail,
  }) : this._(OfferingStatus.unavailable, reason, detail, origin);

  const Availability.unknown(
    OfferingReason reason,
    Provenance origin, {
    String? detail,
  }) : this._(OfferingStatus.unknown, reason, detail, origin);

  bool get isAvailable => status == OfferingStatus.available;
  bool get isUnavailable => status == OfferingStatus.unavailable;
  bool get isUnknown => status == OfferingStatus.unknown;

  /// Можно ли дать пользователю нажать. «Неизвестно» — можно: запретить по
  /// молчанию источника значит выключить рабочий выбор, а это та же ложь, что и
  /// разрешить по молчанию, только в другую сторону. Экран обязан показать
  /// строку с пометкой «не подтверждено» и текстом [message].
  bool get isSelectable => status != OfferingStatus.unavailable;

  /// Подтверждено ли утверждение источником (а не выведено из его молчания).
  bool get isVerified => status != OfferingStatus.unknown;

  /// Готовый текст причины. Живёт рядом с самой причиной, чтобы «недоступно» и
  /// «неизвестно» нигде не остались без ответа на вопрос «почему».
  String get message {
    switch (reason) {
      case null:
        return 'Доступно';
      case OfferingReason.noProfile:
        return 'Профиль подключения не выбран.';
      case OfferingReason.sourceEmpty:
        return 'Источник не сообщил ни одного узла.';
      case OfferingReason.notInFleet:
        return 'Ни один узел источника этого не предлагает.';
      case OfferingReason.nodeFull:
        return 'Узел переполнен и не принимает новые подключения.';
      case OfferingReason.nodeOffline:
        return 'Узел не в сети.';
      case OfferingReason.allNodesBusy:
        return detail == null
            ? 'Все узлы страны сейчас недоступны.'
            : 'Все узлы страны «$detail» сейчас недоступны.';
      case OfferingReason.inboundNotEmittedByClash:
        return 'Оператор включил этот инбаунд, но в конфиг, который читает '
            'приложение, он не попадает.';
      case OfferingReason.inboundAmneziawgDisabled:
        return 'AmneziaWG выключен в панели оператора.';
      case OfferingReason.inboundTransportNotSupported:
        return 'Этот транспорт приложение не поддерживает: прокси такого вида '
            'в конфиге нет.';
      case OfferingReason.relayNotChainedByGenerator:
        return 'Вход показан у узла как метка, но цепочка в конфиге не '
            'строится: трафик идёт прямо на выход.';
      case OfferingReason.relayChainingUnsupportedBySource:
        return 'Импортированная подписка цепочку через вход выразить не может.';
      case OfferingReason.nodeIsRelay:
        return 'Это вход цепочки, а не выход: через него набирают другие узлы. '
            'Подключившись к нему напрямую, вы вышли бы в интернет из его '
            'страны.';
      case OfferingReason.panelRequired:
        return 'Доступно только с подключённой панелью.';
      case OfferingReason.protocolPinNeedsExit:
        return 'Сначала выберите сервер: протокол принадлежит конкретному узлу.';
      case OfferingReason.panelDidNotReportInbounds:
        return detail == null || detail!.isEmpty
            ? 'Панель не сообщила инбаунды этого узла.'
            : 'Панель не смогла прочитать инбаунды этого узла: $detail';
      case OfferingReason.sourceDoesNotReportTransport:
        return 'Подписка называет только вид прокси и не различает транспорт и '
            'TLS.';
      case OfferingReason.sourceDoesNotReportExitIdentity:
        return 'Подписка не называет узлы: машины разделены по адресу сервера.';
      case OfferingReason.panelReportsRelaysByCountryOnly:
        return 'Панель отдаёт входы странами и не называет их узлы.';
      case OfferingReason.rulesetMirrorUnverified:
        return detail == null
            ? 'Этот режим опирается на внешние списки, и проверить их '
                  'доставку нечем.'
            : 'Этот режим опирается на внешние списки ($detail), и '
                  'проверить их доставку нечем.';
      case OfferingReason.noFeedbackChannel:
        return 'Настройка применяется, но подтвердить её работу источник не '
            'умеет.';
      case OfferingReason.catalogNotReady:
        return 'Каталог оператора эту часть ещё не передаёт.';
      case OfferingReason.notProbedYet:
        return 'Замера ещё не было.';
      case OfferingReason.panelUnavailable:
        return detail == null
            ? 'Панель сейчас недоступна.'
            : 'Панель сейчас недоступна: $detail';
    }
  }

  @override
  bool operator ==(Object other) =>
      other is Availability &&
      other.status == status &&
      other.reason == reason &&
      other.detail == detail &&
      other.origin == origin;

  @override
  int get hashCode => Object.hash(status, reason, detail, origin);

  @override
  String toString() => reason == null
      ? 'Availability.available(${origin.source.name})'
      : 'Availability.${status.name}(${reason!.name}, ${origin.source.name})';
}
