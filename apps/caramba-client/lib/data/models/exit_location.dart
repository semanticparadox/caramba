import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/domain/offering/panel_fleet.dart'
    show PanelInboundsRead;
import 'package:caramba_client/vpn/vpn_models.dart';

/// Страна выхода как ТИП, а не как глиф флага.
///
/// До этого файла страна существовала в приложении только как двухбуквенный
/// код внутри `Server.countryCode` / `ImportedServer.country` и рисовалась
/// чипом. Пикер стране привязаться не мог: не к чему было. Здесь страна
/// становится сущностью с именем, числом узлов, лучшим пингом, набором
/// протоколов и — главное — доступностью С ПРИЧИНОЙ.
///
/// Три мира дают инвентарь по-разному ([ExitInventorySource]), но экран обязан
/// видеть один и тот же тип, иначе `ServersScreen` разветвится в третий раз.

/// Откуда взят инвентарь выходов.
///
/// Различие важно не для красоты: от источника зависит, какие возможности
/// вообще существуют (цепочка через relay есть на панельном REST и нет на
/// импортированном конфиге), и какой ключ у узла.
enum ExitInventorySource {
  /// Панель по REST (`GET /servers`, `?node_id=` / `?relay_country=`).
  panelRest,

  /// Импортированная подписка: узлы разобраны ядром, панели нет вообще.
  importedSub,

  /// Подписанный каталог CSM. Инвентарь поставляет CSM-слой; здесь только
  /// маркер, чтобы UI знал происхождение.
  csmCatalog,

  /// Профиля нет — выбирать не из чего.
  none,
}

/// Почему вариант недоступен.
///
/// Домашнее правило приложения: недоступный вариант ВИДЕН и объяснён, а не
/// спрятан. Поэтому доступность — не голый bool: у неё есть машиночитаемая
/// причина, по которой UI сам решает, что показать (подсказку, кнопку
/// «обновить» или ссылку на импорт).
enum ExitUnavailableReason {
  /// Узел не принимает новые подключения (`Server.isSelectable == false`).
  nodeFull,

  /// Узел не в сети. Панельный REST этой причины не порождает — статусы там
  /// толкует один [Server.isSelectable]; её ставит источник, который про
  /// состояние узла знает больше (каталог CSM).
  nodeOffline,

  /// В стране есть узлы, но ни один сейчас не принимает подключения.
  allNodesBusy,

  /// Источник не сообщил ни одного узла (пустая подписка, пустой ответ).
  sourceEmpty,

  /// Цепочка через вход не реализована на импортированном/сыром пути sing-box.
  /// Контрол показывается выключенным с этой причиной, а не прячется.
  relayChainingUnsupported,

  /// Релэй у узла в панели назначен, а цепочки в конфиге нет: генератор Clash,
  /// чьё тело и потребляет ядро, игнорирует relay-ноды — он не выпускает ни
  /// `dialer-proxy`, ни группы `type: relay`, и в `server:` остаётся адрес
  /// выхода. Суффикс `↪` в имени прокси и группа `Auto-Relay` — ярлык.
  ///
  /// Причина отдельная от [relayChainingUnsupported]: там источник цепочку
  /// выразить не может в принципе, здесь оператор её настроил, а до провода она
  /// не доехала. Пользователю это разные новости.
  relayNotChainedByGenerator,

  /// `GET /relays` называет только страны входа; какие узлы за ними стоят и
  /// строится ли через них цепочка, панель на этом эндпоинте не сообщает.
  relaysReportedByCountryOnly,

  /// Каталог CSM ещё не ведёт инвентарь выходов.
  catalogNotReady,

  /// Профиль подключения не выбран.
  noProfile,

  /// Действие имеет смысл только с подключённой панелью.
  panelRequired,

  /// Панель есть, но вызов сейчас невозможен (нет origin/подписки/сети).
  panelUnavailable,

  /// Панель ответила отказом; текст в [ExitAvailability.detail].
  panelRejected,
}

/// Доступность варианта: либо доступен, либо недоступен С ПРИЧИНОЙ.
class ExitAvailability {
  /// `null` — вариант доступен.
  final ExitUnavailableReason? reason;

  /// Уточнение к причине (имя страны, текст отказа панели). Может быть `null`.
  final String? detail;

  const ExitAvailability._(this.reason, this.detail);

  /// Доступен.
  static const ExitAvailability available = ExitAvailability._(null, null);

  const ExitAvailability.unavailable(
    ExitUnavailableReason reason, {
    String? detail,
  }) : this._(reason, detail);

  bool get isAvailable => reason == null;

  /// Готовый текст для UI. Экран волен показать своё, но по умолчанию причина
  /// уже объяснена здесь — чтобы «недоступно» никогда не осталось без ответа
  /// на вопрос «почему».
  String get message {
    switch (reason) {
      case null:
        return 'Доступно';
      case ExitUnavailableReason.nodeFull:
        return 'Узел переполнен и не принимает новые подключения.';
      case ExitUnavailableReason.nodeOffline:
        return 'Узел не в сети.';
      case ExitUnavailableReason.allNodesBusy:
        return 'Все узлы страны сейчас недоступны.';
      case ExitUnavailableReason.sourceEmpty:
        return 'Источник не сообщил ни одного узла.';
      case ExitUnavailableReason.relayChainingUnsupported:
        return 'Вход через другую страну недоступен для импортированной '
            'подписки: цепочку такой конфиг выразить не может.';
      case ExitUnavailableReason.relayNotChainedByGenerator:
        return 'Оператор назначил узлам вход, но в конфиг, который читает '
            'приложение, цепочка не попадает: трафик идёт прямо на выход.';
      case ExitUnavailableReason.relaysReportedByCountryOnly:
        return 'Панель называет входы только странами и не сообщает, строится '
            'ли через них цепочка.';
      case ExitUnavailableReason.catalogNotReady:
        return 'Каталог оператора ещё не передаёт список стран.';
      case ExitUnavailableReason.noProfile:
        return 'Профиль подключения не выбран.';
      case ExitUnavailableReason.panelRequired:
        return 'Доступно только с подключённой панелью.';
      case ExitUnavailableReason.panelUnavailable:
        return 'Панель сейчас недоступна, выбор сохранён локально.';
      case ExitUnavailableReason.panelRejected:
        return detail?.isNotEmpty == true
            ? 'Панель отклонила выбор: $detail'
            : 'Панель отклонила выбор.';
    }
  }

  @override
  bool operator ==(Object other) =>
      other is ExitAvailability &&
      other.reason == reason &&
      other.detail == detail;

  @override
  int get hashCode => Object.hash(reason, detail);

  @override
  String toString() =>
      isAvailable ? 'ExitAvailability.available' : 'ExitAvailability($reason)';
}

/// Один узел выхода, приведённый к общему виду из любого источника.
///
/// Ключ узла разный по природе: у панели это `nodes.id` (число), у импорта —
/// имя прокси (строка). [key] несёт единый строковый ключ для UI, [panelNodeId]
/// заполнен только там, где число вообще существует.
class ExitNode {
  /// Стабильный ключ в пределах источника.
  final String key;

  /// `nodes.id` — только [ExitInventorySource.panelRest]; иначе `null`.
  final int? panelNodeId;

  final String name;

  /// ISO-2 в верхнем регистре; пусто — страна неизвестна.
  final String countryCode;

  /// Пинг в мс: `null` — не мерили, отрицательное — таймаут.
  final int? pingMs;

  /// Протокол/тип outbound'а (`vless`, `hysteria2`, ...). Пусто — источник его
  /// не сообщает (панельный `/servers` протоколы не отдаёт).
  final String protocol;

  /// Загрузка 0..100; у импорта всегда 0 (неизвестна).
  final double load;

  final ExitInventorySource source;

  final ExitAvailability availability;

  const ExitNode({
    required this.key,
    required this.name,
    required this.countryCode,
    required this.source,
    this.panelNodeId,
    this.pingMs,
    this.protocol = '',
    this.load = 0,
    this.availability = ExitAvailability.available,
  });

  bool get isAvailable => availability.isAvailable;

  PingBucket get pingBucket => pingBucketOf(pingMs);

  /// Узел панели. Пригодность узла НЕ переопределяется здесь: её решает
  /// [Server.isSelectable] — тот же предикат, по которому узел выбирает путь
  /// подключения.
  ///
  /// Раньше пикер вёл собственный белый список статусов (`online`/`busy`), и
  /// живая панель, присылающая `active`, делала недоступной каждую страну:
  /// список рисовался, но не нажимался ни в одной строке, кроме «Авто». Второй
  /// словарь статусов — это гарантированное расхождение между тем, что показано,
  /// и тем, к чему приложение может подключиться; поэтому словарь остался один.
  factory ExitNode.fromServer(Server s) {
    final availability = s.isSelectable
        ? ExitAvailability.available
        : ExitAvailability.unavailable(
            ExitUnavailableReason.nodeFull,
            detail: s.status,
          );
    // Панель отдаёт инбаунды на том же `/servers`, и молчать о них здесь
    // значило бы оставить панельный режим «источником, который протоколы не
    // сообщает» — тем самым, из-за которого пикер показывал весь список ядра,
    // ничего не проверив. Тройки склеиваются в одну строку намеренно: разбор
    // формы принадлежит слою предложения (`domain/offering`), а этому полю
    // достаточно нести токены узла.
    final read = PanelInboundsRead.fromRow(s.rawJson);
    final protocol = read.known.isAvailable
        ? read.rows
              .where((r) => r.available)
              .map(
                (r) => <String>[
                  r.protocol,
                  r.network,
                  r.security,
                ].where((p) => p.isNotEmpty && p != 'none').join('+'),
              )
              .where((t) => t.isNotEmpty)
              .join(' ')
        // Панель инбаунды не прочитала — это молчание, и пустая строка здесь
        // означает именно его.
        : '';
    return ExitNode(
      key: s.id.toString(),
      panelNodeId: s.id,
      name: s.name,
      countryCode: normalizeCountryCode(s.countryCode),
      pingMs: s.pingMs,
      load: s.load,
      protocol: protocol,
      source: ExitInventorySource.panelRest,
      availability: availability,
    );
  }

  /// Узел импортированной подписки. Статуса у него нет: ядро отдаёт то, что
  /// разобрало, и недоступным узел делает только пустой инвентарь страны.
  /// Таймаут пинга (`-1`) — это качество, а не недоступность.
  factory ExitNode.fromImported(ImportedServer s, {int? pingMs}) => ExitNode(
    key: s.id,
    name: s.name.isNotEmpty ? s.name : s.id,
    countryCode: normalizeCountryCode(s.country),
    pingMs: pingMs,
    protocol: s.type,
    source: ExitInventorySource.importedSub,
  );

  @override
  bool operator ==(Object other) =>
      other is ExitNode && other.key == key && other.source == source;

  @override
  int get hashCode => Object.hash(key, source);
}

/// Страна выхода: то, к чему привязывается пикер.
class ExitLocation {
  /// ISO-2 в верхнем регистре. Пустая строка — страна неизвестна (узлы, из
  /// имени которых ядро её не вывело); такая группа не прячется.
  final String countryCode;

  /// Человекочитаемое имя («Германия»), иначе сам код.
  final String displayName;

  /// Сколько узлов этой страны в источнике (включая недоступные).
  final int nodeCount;

  /// Лучший пинг среди ДОСТУПНЫХ узлов; `null` — не мерили или все недоступны.
  final int? bestPingMs;

  /// Протоколы, встречающиеся в стране (отсортированы). Пусто — источник
  /// протоколы не сообщает; это НЕ значит «протоколов нет».
  final List<String> protocols;

  final ExitInventorySource source;

  final ExitAvailability availability;

  const ExitLocation({
    required this.countryCode,
    required this.displayName,
    required this.nodeCount,
    required this.source,
    this.bestPingMs,
    this.protocols = const <String>[],
    this.availability = ExitAvailability.available,
  });

  bool get isAvailable => availability.isAvailable;

  /// Страну вывести не удалось — узлы всё равно показываются одной группой.
  bool get isUnknownCountry => countryCode.isEmpty;

  PingBucket get pingBucket => pingBucketOf(bestPingMs);

  /// Собирает страну из её узлов. Недоступной страна становится тогда и только
  /// тогда, когда недоступны ВСЕ её узлы: одного живого достаточно, чтобы
  /// пользователю было куда подключиться.
  factory ExitLocation.fromNodes(
    String countryCode,
    List<ExitNode> nodes, {
    required ExitInventorySource source,
    String? displayName,
  }) {
    final code = normalizeCountryCode(countryCode);
    final live = nodes.where((n) => n.isAvailable).toList(growable: false);
    int? best;
    for (final n in live) {
      final ms = n.pingMs;
      if (ms == null || ms < 0) continue;
      if (best == null || ms < best) best = ms;
    }
    final protocols =
        nodes
            .map((n) => n.protocol)
            .where((p) => p.isNotEmpty)
            .toSet()
            .toList(growable: false)
          ..sort();
    final ExitAvailability availability;
    if (nodes.isEmpty) {
      availability = const ExitAvailability.unavailable(
        ExitUnavailableReason.sourceEmpty,
      );
    } else if (live.isEmpty) {
      availability = ExitAvailability.unavailable(
        ExitUnavailableReason.allNodesBusy,
        detail: countryNameOf(code),
      );
    } else {
      availability = ExitAvailability.available;
    }
    return ExitLocation(
      countryCode: code,
      displayName: displayName ?? countryNameOf(code),
      nodeCount: nodes.length,
      bestPingMs: best,
      protocols: protocols,
      source: source,
      availability: availability,
    );
  }

  @override
  bool operator ==(Object other) =>
      other is ExitLocation &&
      other.countryCode == countryCode &&
      other.source == source &&
      other.nodeCount == nodeCount &&
      other.bestPingMs == bestPingMs &&
      other.availability == availability;

  @override
  int get hashCode =>
      Object.hash(countryCode, source, nodeCount, bestPingMs, availability);

  @override
  String toString() =>
      'ExitLocation($countryCode, nodes: $nodeCount, ${availability.reason})';
}

/// Разрешённый панелью выбор — ответ `PUT /subscriptions/{id}/selection`.
///
/// Панель отвечает тем, что применилось ФАКТИЧЕСКИ: запрошенный узел мог не
/// подойти плану, и тогда вернётся другой. Локальное состояние обязано
/// подстраиваться под этот ответ, а не под то, что мы просили.
class ExitSelection {
  /// `nodes.id` выхода; `null` — выбор сброшен в дефолт.
  final int? nodeId;

  /// ISO-2 страны входа; `null` — цепочка выключена/дефолт.
  final String? relayCountry;

  const ExitSelection({this.nodeId, this.relayCountry});

  /// Пустой выбор (панель ничего не закрепила).
  static const ExitSelection none = ExitSelection();

  factory ExitSelection.fromJson(Map<String, dynamic> json) => ExitSelection(
    nodeId: (json['node_id'] as num?)?.toInt(),
    relayCountry: _nonEmpty(json['relay_country']),
  );

  Map<String, dynamic> toJson() => <String, dynamic>{
    'node_id': nodeId,
    'relay_country': relayCountry,
  };

  static String? _nonEmpty(Object? v) {
    if (v is String && v.trim().isNotEmpty) return v.trim().toUpperCase();
    return null;
  }

  @override
  bool operator ==(Object other) =>
      other is ExitSelection &&
      other.nodeId == nodeId &&
      other.relayCountry == relayCountry;

  @override
  int get hashCode => Object.hash(nodeId, relayCountry);

  @override
  String toString() => 'ExitSelection(node: $nodeId, relay: $relayCountry)';
}

/// Поле частичного обновления: «не трогать» / «сбросить в дефолт» / «значение».
///
/// JSON различает отсутствие ключа и `null`, Dart-аргумент — нет: `null` в
/// сигнатуре значит и «не передали», и «передали null». Из-за этого без явной
/// обёртки сброс выбора неотличим от его сохранения, и одно из двух намерений
/// молча теряется по дороге к панели.
class SelectionField<T extends Object> {
  /// Ключ вообще попадёт в тело запроса.
  final bool present;

  /// Значение; `null` при [present] означает «сбросить в дефолт».
  final T? value;

  const SelectionField._(this.present, this.value);

  /// Ключ не отправляется — панель оставляет текущее значение.
  const SelectionField.unchanged() : this._(false, null);

  /// Отправляется `null` — панель возвращается к значению по умолчанию.
  const SelectionField.reset() : this._(true, null);

  /// Отправляется значение.
  const SelectionField.of(T value) : this._(true, value);

  /// Из nullable-значения: `null` читается как сброс, а не как «не трогать».
  factory SelectionField.orReset(T? value) =>
      value == null ? SelectionField<T>.reset() : SelectionField<T>.of(value);
}

/// Имя прокси, которым страна представлена на СЫРОМ пути (импортированная
/// подписка). `null` — страна не задана или её узлов в подписке нет.
///
/// У ядра на этом пути нет понятия страны: `connectRaw` получает одну строку —
/// имя прокси, и пустая строка значит «любой узел». Страна, не превращённая в
/// имя ДО connect, остаётся галочкой в интерфейсе, пока трафик выходит где
/// угодно; чтобы такое превращение было одинаковым и при выборе, и при
/// подключении, оно живёт здесь, рядом с моделью, а не в двух состояниях.
String? rawProxyNameForCountry(
  List<ImportedServer> servers,
  String? countryCode,
) {
  final code = normalizeCountryCode(countryCode);
  if (code.isEmpty) return null;
  for (final s in servers) {
    if (normalizeCountryCode(s.country) == code) return s.id;
  }
  return null;
}

/// Приводит код страны к каноническому виду: верхний регистр без пробелов.
/// Пустая/мусорная строка становится `''` — «страна неизвестна».
String normalizeCountryCode(String? raw) {
  final v = (raw ?? '').trim().toUpperCase();
  if (v.length != 2) return '';
  return v;
}

/// Имя страны по ISO-2. Незнакомый код отдаётся как есть: неизвестная страна
/// показывается кодом, но не исчезает из списка.
String countryNameOf(String? code) {
  final c = normalizeCountryCode(code);
  if (c.isEmpty) return 'Без страны';
  return _countryNames[c] ?? c;
}

/// Названия стран, которые реально встречаются в узлах панели и в чужих
/// подписках. Список намеренно неполный: неизвестный код показывается кодом.
const Map<String, String> _countryNames = <String, String>{
  'AE': 'ОАЭ',
  'AM': 'Армения',
  'AR': 'Аргентина',
  'AT': 'Австрия',
  'AU': 'Австралия',
  'AZ': 'Азербайджан',
  'BE': 'Бельгия',
  'BG': 'Болгария',
  'BR': 'Бразилия',
  'BY': 'Беларусь',
  'CA': 'Канада',
  'CH': 'Швейцария',
  'CL': 'Чили',
  'CN': 'Китай',
  'CY': 'Кипр',
  'CZ': 'Чехия',
  'DE': 'Германия',
  'DK': 'Дания',
  'EE': 'Эстония',
  'ES': 'Испания',
  'FI': 'Финляндия',
  'FR': 'Франция',
  'GB': 'Великобритания',
  'GE': 'Грузия',
  'GR': 'Греция',
  'HK': 'Гонконг',
  'HU': 'Венгрия',
  'ID': 'Индонезия',
  'IE': 'Ирландия',
  'IL': 'Израиль',
  'IN': 'Индия',
  'IS': 'Исландия',
  'IT': 'Италия',
  'JP': 'Япония',
  'KG': 'Киргизия',
  'KR': 'Южная Корея',
  'KZ': 'Казахстан',
  'LT': 'Литва',
  'LU': 'Люксембург',
  'LV': 'Латвия',
  'MD': 'Молдова',
  'MX': 'Мексика',
  'MY': 'Малайзия',
  'NL': 'Нидерланды',
  'NO': 'Норвегия',
  'NZ': 'Новая Зеландия',
  'PL': 'Польша',
  'PT': 'Португалия',
  'RO': 'Румыния',
  'RS': 'Сербия',
  'RU': 'Россия',
  'SE': 'Швеция',
  'SG': 'Сингапур',
  'SK': 'Словакия',
  'TH': 'Таиланд',
  'TR': 'Турция',
  'TW': 'Тайвань',
  'UA': 'Украина',
  'US': 'США',
  'UZ': 'Узбекистан',
  'VN': 'Вьетнам',
  'ZA': 'ЮАР',
};
