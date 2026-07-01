/// Relay-вход (вступительная страна в цепочке).
///
/// Соответствует `relay_nodes` / `?relay_country=` в caramba-sub: `country`
/// здесь это ISO-страна, которую UI отдаёт ядру как `relay_country`.
/// `id == null` (Выкл) и `id == 'auto'` (Авто) — псевдо-варианты.
class Relay {
  final String? id; // null=off, 'auto'=auto, иначе ISO-код страны
  final String name;
  final String desc;
  final String? country; // relay_country для caramba-sub

  const Relay({
    this.id,
    required this.name,
    required this.desc,
    this.country,
  });

  bool get isOff => id == null;
  bool get isAuto => id == 'auto';

  factory Relay.fromJson(Map<String, dynamic> json) => Relay(
        id: json['id']?.toString(),
        name: (json['name'] as String?) ?? 'Relay',
        desc: (json['description'] as String?) ?? '',
        country: json['country'] as String?,
      );

  /// Один элемент из `GET /api/v2/app/relays` (`AppRelay`):
  /// ```json
  /// { "country_code":"NL", "country_name":"Netherlands", "node_count":3 }
  /// ```
  /// `country_code` напрямую идёт в `?relay_country=` при запросе конфига.
  factory Relay.fromApiJson(Map<String, dynamic> json) {
    final cc = (json['country_code'] as String?)?.toUpperCase() ?? '';
    final name = (json['country_name'] as String?)?.trim();
    final count = (json['node_count'] as num?)?.toInt() ?? 0;
    return Relay(
      id: cc,
      name: (name != null && name.isNotEmpty) ? name : cc,
      desc: count > 0
          ? 'Вход через $cc, узлов: $count'
          : 'Вход через $cc',
      country: cc,
    );
  }

  /// Собирает полный список для пикера: Выкл / Авто + relay-страны с панели.
  /// Когда панель не отдала ничего — возвращает [defaults].
  static List<Relay> fromCountries(List<Relay> countries) {
    if (countries.isEmpty) return defaults;
    return <Relay>[
      const Relay(name: 'Выкл', desc: 'Прямое подключение к выбранному серверу.'),
      const Relay(
          id: 'auto',
          name: 'Авто',
          desc: 'Приложение выберет устойчивый вход само.'),
      ...countries,
    ];
  }

  /// Дефолтный набор: Выкл / Авто + страны-входы. Используется, пока панель
  /// не отдала список (или для desktop/dev).
  static const defaults = <Relay>[
    Relay(name: 'Выкл', desc: 'Прямое подключение к выбранному серверу.'),
    Relay(id: 'auto', name: 'Авто', desc: 'Приложение выберет устойчивый вход само.'),
    Relay(id: 'TR', name: 'Турция', desc: 'Вход через TR, выход через выбранный сервер.', country: 'TR'),
    Relay(id: 'KZ', name: 'Казахстан', desc: 'Вход через KZ.', country: 'KZ'),
    Relay(id: 'FI', name: 'Финляндия', desc: 'Вход через FI.', country: 'FI'),
  ];
}
