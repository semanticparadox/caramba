import 'package:caramba_client/widgets/lucide.dart';

/// Транспортный протокол маскировки. `id` совпадает со строкой `Policy.Protocol`
/// в caramba-core (`AmneziaWG` / `VLESS-Reality` / `Hysteria2` / `TUIC` /
/// `Shadowsocks`), пустая строка = «Авто» (ядро само выбирает url-test).
class ProtocolOption {
  final String id; // '' = авто
  final String name;
  final String desc;
  final String icon; // Lucide glyph
  final bool recommended;
  final bool auto;

  const ProtocolOption({
    required this.id,
    required this.name,
    required this.desc,
    required this.icon,
    this.recommended = false,
    this.auto = false,
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
    ),
    ProtocolOption(
      id: 'VLESS-Reality',
      name: 'VLESS · Reality',
      desc: 'Невидим для DPI, маскируется под настоящие сайты по TLS.',
      icon: Lucide.shield,
    ),
    ProtocolOption(
      id: 'Hysteria2',
      name: 'Hysteria2',
      desc: 'Высокая скорость на нестабильных и мобильных сетях.',
      icon: Lucide.zap,
    ),
    ProtocolOption(
      id: 'TUIC',
      name: 'TUIC',
      desc: 'Быстрый UDP с низкой задержкой.',
      icon: Lucide.route,
    ),
    ProtocolOption(
      id: 'Shadowsocks',
      name: 'Shadowsocks',
      desc: 'Простой и стабильный протокол.',
      icon: Lucide.globe,
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
    CoreOption('gvisor', 'gVisor', 'Изолированный стек. Стабильнее в сложных сетях.'),
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
