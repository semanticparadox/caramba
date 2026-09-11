/// Relay-вход (вступительная страна или конкретный узел в цепочке).
///
/// Соответствует `relay_nodes` / `?relay_country=` в caramba-sub: `country`
/// здесь это ISO-страна, которую UI отдаёт ядру как `relay_country`.
/// Четыре вида строк, по [id]:
///   * `null` — «Выкл»: прямое подключение к выбранному выходу;
///   * `'auto'` — «Авто»: вход выбирает панель по стране пользователя;
///   * ISO-2 (`'RU'`) — вся страна: в конфиг попадают все её релеи;
///   * `'node:<id>'` — конкретный релей ([nodeId]): цепочки только через него.
///
/// Узел несёт [country] той же страны: ядру и CSM уходит она (их словарь это
/// ISO-2), а закрепление узла живёт на панели (`subscriptions.relay_node_id`,
/// пишется через `PUT /subscriptions/{id}/selection`, см. [pinValue]).
class Relay {
  final String? id; // null=off, 'auto'=auto, 'XX'=страна, 'node:<id>'=узел
  final String name;
  final String desc;
  final String? country; // relay_country для caramba-sub

  /// `nodes.id` релея; только у строк-узлов.
  final int? nodeId;

  /// Город из карточки узла в панели; `null` — оператор не заполнил.
  final String? city;

  /// RTT машины релея по heartbeat панели, мс. Это число ПАНЕЛИ, не замер с
  /// устройства: сквозь релей приложение мерить пока не может, и подпись в UI
  /// обязана называть источник.
  final int? latencyMs;

  /// Нагрузка машины в процентах по heartbeat панели.
  final double? loadPct;

  /// Флаг от панели (выведен из ISO-2); `null` — вывести самим.
  final String? flag;

  /// Сколько релеев панель насчитала в стране; только у строк-стран.
  final int nodeCount;

  /// Узлы страны, как их отдал `GET /relays`; только у строк-стран. В плоский
  /// список пикера они попадают через [fromCountries].
  final List<Relay> nodes;

  const Relay({
    this.id,
    required this.name,
    required this.desc,
    this.country,
    this.nodeId,
    this.city,
    this.latencyMs,
    this.loadPct,
    this.flag,
    this.nodeCount = 0,
    this.nodes = const <Relay>[],
  });

  bool get isOff => id == null;
  bool get isAuto => id == 'auto';
  bool get isNode => nodeId != null;
  bool get isCountry => !isOff && !isAuto && !isNode;

  /// ISO-2 страны строки в верхнем регистре; пусто у «Выкл»/«Авто».
  String get countryCode =>
      (country ?? (isCountry ? id : null) ?? '').trim().toUpperCase();

  /// Значение для `relay_country` в `PUT /subscriptions/{id}/selection`:
  /// `none` — без релея, ISO-2 — вся страна, `node:<id>` — конкретный релей.
  /// «Авто» значения не имеет: панели уходит `null` (сброс), см.
  /// `ExitSelectionController.selectRelay`.
  String? get pinValue {
    if (isOff) return 'none';
    if (isAuto) return null;
    if (isNode) return 'node:$nodeId';
    return countryCode;
  }

  factory Relay.fromJson(Map<String, dynamic> json) => Relay(
    id: json['id']?.toString(),
    name: (json['name'] as String?) ?? 'Relay',
    desc: (json['description'] as String?) ?? '',
    country: json['country'] as String?,
  );

  /// Один элемент из `GET /api/v2/app/relays` (`AppRelay`):
  /// ```json
  /// { "country_code":"RU", "country_name":"Russia", "flag":"🇷🇺",
  ///   "node_count":2,
  ///   "nodes":[{"id":12,"name":"msk-1","city":"Moscow","load_pct":30.0,
  ///             "latency_ms":31,"sort_order":10}] }
  /// ```
  /// `country_code` напрямую идёт в `?relay_country=` при запросе конфига;
  /// `nodes` (панель старше их не отдаёт, тогда список пуст) разворачиваются
  /// в строки-узлы с `id = 'node:<id>'`.
  factory Relay.fromApiJson(Map<String, dynamic> json) {
    final cc = (json['country_code'] as String?)?.toUpperCase() ?? '';
    final name = (json['country_name'] as String?)?.trim();
    final count = (json['node_count'] as num?)?.toInt() ?? 0;
    final flag = (json['flag'] as String?)?.trim();
    final rawNodes = json['nodes'];
    final nodes = <Relay>[
      if (rawNodes is List)
        for (final n in rawNodes)
          if (n is Map) Relay._nodeFromApiJson(n.cast<String, dynamic>(), cc),
    ];
    return Relay(
      id: cc,
      name: (name != null && name.isNotEmpty) ? name : cc,
      desc: count > 0 ? 'Вход через $cc, узлов: $count' : 'Вход через $cc',
      country: cc,
      flag: (flag != null && flag.isNotEmpty) ? flag : null,
      nodeCount: count,
      nodes: nodes,
    );
  }

  factory Relay._nodeFromApiJson(Map<String, dynamic> json, String cc) {
    final id = (json['id'] as num?)?.toInt() ?? 0;
    final name = (json['name'] as String?)?.trim() ?? '';
    final city = (json['city'] as String?)?.trim();
    final latency = (json['latency_ms'] as num?)?.toInt();
    final load = (json['load_pct'] as num?)?.toDouble();
    return Relay(
      id: 'node:$id',
      name: name.isNotEmpty ? name : 'Релей #$id',
      desc: (city != null && city.isNotEmpty) ? city : 'Релей $cc',
      country: cc,
      nodeId: id,
      city: (city != null && city.isNotEmpty) ? city : null,
      latencyMs: (latency != null && latency > 0) ? latency : null,
      loadPct: load,
    );
  }

  /// Собирает список для пикера: Выкл / Авто + страны с панели + их узлы.
  ///
  /// Порядок закреплён намеренно: индекс этого списка хранится в
  /// `CoreConfig.relay`, и страны стоят там же, где стояли до появления узлов
  /// (сразу за «Авто»), а узлы дописаны в хвост. Так сохранённый индекс
  /// страны не превращается в узел после обновления приложения.
  /// Панель не отдала ничего — остаётся один [defaults], то есть «Выкл».
  static List<Relay> fromCountries(List<Relay> countries) {
    if (countries.isEmpty) return defaults;
    return <Relay>[
      ...defaults,
      ...countries,
      for (final c in countries) ...c.nodes,
    ];
  }

  /// То, что верно про входы БЕЗ единого факта от источника.
  ///
  /// Здесь была выдумка: Турция, Казахстан и Финляндия как «страны-входы по
  /// умолчанию». Их не было ни в панели, ни в конфиге — приложение показывало
  /// три страны, которых у оператора нет, и выбор любой из них уходил в
  /// `relay_country=TR` мимо всей инфраструктуры. Владелец увидел обратную
  /// сторону той же лжи: «там только есть Россия, хотя функционально там могут
  /// быть любые ноды relay, которые настроены в панели».
  ///
  /// Настоящие входы теперь приходят от источника: страны и узлы из
  /// `GET /relays` ([fromApiJson]) и узлы, названные у выходов (`via_relay`,
  /// слой `domain/offering`). Здесь остались только две строки, истинные при
  /// любом флоте и не описывающие ни одной страны: «Выкл» — прямое
  /// подключение к выбранному выходу (ядру уходит явное «без релея»), «Авто»
  /// — решение остаётся за панелью (ядру уходит пустая строка). Это разные
  /// намерения, поэтому строки две, а не одна.
  static const defaults = <Relay>[
    Relay(name: 'Выкл', desc: 'Прямое подключение к выбранному серверу.'),
    Relay(id: 'auto', name: 'Авто', desc: 'Вход подберёт панель.'),
  ];
}
