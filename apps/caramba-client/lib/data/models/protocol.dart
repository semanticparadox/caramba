import 'package:caramba_client/widgets/lucide.dart';

/// Транспортный протокол маскировки. `id` совпадает со строкой `Policy.Protocol`
/// в caramba-core (`AmneziaWG` / `VLESS-Reality` / `VLESS` / `Hysteria2` /
/// `TUIC` / `Shadowsocks`), пустая строка = «Авто» (ядро само выбирает
/// url-test).
///
/// Список НЕ описывает флот: он описывает то, что ядро умеет ПОПРОСИТЬ. Какие
/// из этих протоколов действительно раздаёт текущий источник, считает
/// `protocol_inventory_state.dart` по живому инвентарю — иначе экран обещает
/// протокол, которого нет ни на одном узле, и попытка его применить деградирует
/// молча.
class ProtocolOption {
  final String id; // '' = авто
  final String name;
  final String desc;
  final String icon; // Lucide glyph
  final bool recommended;
  final bool auto;

  /// Типы outbound'а, которыми этот протокол приходит от источника (`type` у
  /// элемента `proxies` в clash/mihomo, `proto_name` в каталоге CSM).
  ///
  /// Зеркалит `protocolClashType` из libs/caramba-core/profile/profile.go и
  /// нужен ровно для одного: сопоставить строку опции с тем, что перечислил
  /// источник. Пустой список у «Авто» — сопоставлять нечего, оно доступно
  /// всегда.
  final List<String> outboundTypes;

  /// Уточнение формы, без которого опция это не она: у `VLESS-Reality` это
  /// `reality`. Проверяется ТОЛЬКО когда источник вообще сообщает уточнения
  /// (каталог CSM отдаёт `security`/`network`, подписка — один голый тип), —
  /// иначе Reality молча объявлялся бы недоступным на каждой подписке, которая
  /// просто не рассказывает про TLS.
  final String? shape;

  const ProtocolOption({
    required this.id,
    required this.name,
    required this.desc,
    required this.icon,
    this.recommended = false,
    this.auto = false,
    this.outboundTypes = const <String>[],
    this.shape,
  });

  static const defaults = <ProtocolOption>[
    ProtocolOption(
      id: '',
      name: 'Авто',
      desc: 'Приложение само выбирает протокол и переключает при блокировке.',
      icon: Lucide.gauge,
      auto: true,
    ),
    ProtocolOption(
      id: 'AmneziaWG',
      name: 'AmneziaWG',
      desc: 'Маскировка под обычный трафик. Лучший обход DPI в России.',
      icon: Lucide.lock,
      recommended: true,
      outboundTypes: <String>['wireguard'],
    ),
    ProtocolOption(
      id: 'VLESS-Reality',
      name: 'VLESS · Reality',
      desc: 'Невидим для DPI, маскируется под настоящие сайты по TLS.',
      icon: Lucide.shield,
      outboundTypes: <String>['vless'],
      shape: 'reality',
    ),
    ProtocolOption(
      id: 'Hysteria2',
      name: 'Hysteria2',
      desc: 'Высокая скорость на нестабильных и мобильных сетях.',
      icon: Lucide.zap,
      outboundTypes: <String>['hysteria2'],
    ),
    ProtocolOption(
      id: 'TUIC',
      name: 'TUIC',
      desc: 'Быстрый UDP с низкой задержкой.',
      icon: Lucide.route,
      outboundTypes: <String>['tuic'],
    ),
    ProtocolOption(
      id: 'Shadowsocks',
      name: 'Shadowsocks',
      desc: 'Простой и стабильный протокол.',
      icon: Lucide.globe,
      outboundTypes: <String>['ss', 'shadowsocks'],
    ),
    // VLESS без Reality дописан В КОНЕЦ намеренно: `CoreConfig.protocol` это
    // сохранённый ИНДЕКС в этом списке, и вставка в середину переставила бы
    // чужой сохранённый выбор на соседний протокол. Строка `VLESS` уже есть и
    // в `protocolClashType` ядра, и в закрытом словаре CSM
    // (kCsmProtocolVocabulary), так что выбор доезжает до обоих концов.
    ProtocolOption(
      id: 'VLESS',
      name: 'VLESS',
      desc: 'VLESS поверх TLS: ws, grpc, tcp или httpupgrade, без Reality.',
      icon: Lucide.shield,
      outboundTypes: <String>['vless'],
    ),
  ];
}

/// Пресет маршрутизации (caramba-core routing presets / `SetRouting`).
class RoutingMode {
  final String id; // 'ru-smart' и т.п.
  final String name;
  final String desc;
  final String icon;

  const RoutingMode({
    required this.id,
    required this.name,
    required this.desc,
    required this.icon,
  });

  static const defaults = <RoutingMode>[
    RoutingMode(
      id: 'ru-smart',
      name: 'Россия',
      desc: 'Напрямую по умолчанию. Через VPN только заблокированные сервисы.',
      icon: Lucide.shield,
    ),
    RoutingMode(
      id: 'telegram-only',
      name: 'Только Telegram',
      desc: 'Через VPN идёт только Telegram, остальное напрямую.',
      icon: Lucide.send,
    ),
    RoutingMode(
      id: 'full',
      name: 'Полный обход',
      desc: 'Весь трафик через VPN, кроме российских сайтов.',
      icon: Lucide.globe,
    ),
    RoutingMode(
      id: 'streaming',
      name: 'Стриминг',
      desc: 'Через VPN идут Netflix, YouTube и подобные сервисы.',
      icon: Lucide.zap,
    ),
    RoutingMode(
      id: 'adblock',
      name: 'Блок рекламы',
      desc: 'Маршрут не меняется, режется реклама и трекеры.',
      icon: Lucide.lock,
    ),
  ];
}

/// Простой вариант для пикеров (стек/DNS/MTU) — имя + описание.
class CoreOption {
  final String id;
  final String name;
  final String desc;
  const CoreOption(this.id, this.name, this.desc);

  static const stacks = <CoreOption>[
    CoreOption('auto', 'Авто', 'Выбирается под платформу.'),
    CoreOption('system', 'System', 'Стек ОС. Быстрее, но менее совместим.'),
    CoreOption(
      'gvisor',
      'gVisor',
      'Изолированный стек. Стабильнее в сложных сетях.',
    ),
    CoreOption('mixed', 'Mixed', 'TCP через system, UDP через gVisor.'),
  ];

  static const dns = <CoreOption>[
    CoreOption('auto', 'Авто', 'DNS из конфигурации сервера.'),
    CoreOption('cloudflare', 'Cloudflare', '1.1.1.1, DoH.'),
    CoreOption('google', 'Google', '8.8.8.8, DoH.'),
    CoreOption('adguard', 'AdGuard', 'С блокировкой рекламы и трекеров.'),
  ];

  static const mtu = <CoreOption>[
    CoreOption('auto', 'Авто', '1280 для AmneziaWG, иначе по протоколу.'),
    CoreOption('1280', '1280', 'Совместимо с большинством сетей.'),
    CoreOption('1420', '1420', 'Выше пропускная способность.'),
    CoreOption('1500', '1500', 'Максимум, если сеть позволяет.'),
  ];
}
