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

  /// Тип outbound'а, ПО КОТОРОМУ ядро реально отбирает узлы, когда просят этот
  /// протокол: `protocolClashType[id]` в
  /// libs/caramba-core/profile/profile.go, а `applyProtocol` сравнивает его с
  /// `m["type"]` каждого элемента `proxies`.
  ///
  /// От [outboundTypes] отличается назначением, и это различие — вся честность
  /// пикера. [outboundTypes] отвечает «как ИСТОЧНИК может назвать этот
  /// протокол» (каталог CSM пишет `shadowsocks`, тело подписки — `ss`) и
  /// служит сопоставлению строки с инвентарём. [coreFamily] — единственная
  /// строка, которой ядро отбирает прокси, и потому именно она задаёт, какие
  /// строки пикера для ядра НЕРАЗЛИЧИМЫ.
  ///
  /// У `VLESS-Reality` и `VLESS` она одна и та же — `vless`. Ядро собирает
  /// url-test группу `Caramba-Proto` по ВСЕМ vless-прокси и Reality среди них
  /// не выделяет, так что выбор Reality поднимает туннель хоть на TLS-инбаунде.
  /// Пока это так, строка обязана считать соседей по [coreFamily], а не по
  /// своему индексу в [defaults]: иначе Reality объявляет себя единственным в
  /// семействе и обещает точность, которой в ядре нет.
  ///
  /// Пусто у «Авто»: отказ от выбора семейства не имеет.
  final String coreFamily;

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
    this.coreFamily = '',
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
      coreFamily: 'wireguard',
    ),
    ProtocolOption(
      id: 'VLESS-Reality',
      name: 'VLESS · Reality',
      desc: 'Невидим для DPI, маскируется под настоящие сайты по TLS.',
      icon: Lucide.shield,
      outboundTypes: <String>['vless'],
      coreFamily: 'vless',
      shape: 'reality',
    ),
    ProtocolOption(
      id: 'Hysteria2',
      name: 'Hysteria2',
      desc: 'Высокая скорость на нестабильных и мобильных сетях.',
      icon: Lucide.zap,
      outboundTypes: <String>['hysteria2'],
      coreFamily: 'hysteria2',
    ),
    ProtocolOption(
      id: 'TUIC',
      name: 'TUIC',
      desc: 'Быстрый UDP с низкой задержкой.',
      icon: Lucide.route,
      outboundTypes: <String>['tuic'],
      coreFamily: 'tuic',
    ),
    ProtocolOption(
      id: 'Shadowsocks',
      name: 'Shadowsocks',
      desc: 'Простой и стабильный протокол.',
      icon: Lucide.globe,
      outboundTypes: <String>['ss', 'shadowsocks'],
      coreFamily: 'ss',
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
      coreFamily: 'vless',
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

  /// Девять пресетов ядра, а не пять перепечатанных.
  ///
  /// Реестр ядра (`presetList` в libs/caramba-core/routing/presets.go) содержит
  /// девять пресетов; приложение показывало пять и называло их своими словами.
  /// Четырёх — `ir-smart`, `by-smart`, `cn-smart` и глобального `global` — для
  /// пользователя не существовало вовсе, а `ru-smart` подписывался «Россия», из
  /// чего нельзя было понять, что это УМНЫЙ режим, а не полный обход.
  ///
  /// Имена и описания взяты из реестра дословно. Факты о составе пресетов живут
  /// в `domain/offering/route_presets.dart` (там же — что именно режет рекламу и
  /// каким пресетам нужны внешние списки); здесь остаётся только то, что нужно
  /// пикеру: подпись и иконка.
  ///
  /// Порядок НЕ переставлен: `CoreConfig.route` это сохранённый ИНДЕКС в этом
  /// списке, и перестановка увела бы живого пользователя на соседний маршрут.
  /// Пять прежних строк остались на местах, четыре новые дописаны в конец —
  /// то же правило, что у [ProtocolOption.defaults]. Соответствие индексов
  /// идентификаторам ядра зафиксировано в `kLegacyRouteIndexByCoreId`.
  static const defaults = <RoutingMode>[
    RoutingMode(
      id: 'ru-smart',
      name: 'Россия (умный)',
      desc:
          'По умолчанию напрямую. Через VPN — только заблокированные сервисы '
          '(Telegram, Instagram, X, YouTube, Discord, ChatGPT и список '
          'заблокированного в РФ). Российские сайты и банки — напрямую.',
      icon: Lucide.shield,
    ),
    RoutingMode(
      id: 'telegram-only',
      name: 'Только Telegram',
      desc:
          'Через VPN идёт только Telegram (приложение + домены + '
          'IP-диапазоны). Всё остальное — напрямую.',
      icon: Lucide.send,
    ),
    // `full` — историческое имя UI для пресета ядра `ru-full`; переименование
    // делает kRoutingPresetWire в core_policy_mapping.dart.
    RoutingMode(
      id: 'full',
      name: 'Россия (полный обход)',
      desc:
          'Весь трафик через VPN, напрямую — только российские сайты, '
          'российские IP и локальная сеть.',
      icon: Lucide.globe,
    ),
    RoutingMode(
      id: 'streaming',
      name: 'Стриминг и AI',
      desc:
          'По умолчанию напрямую. Через VPN — Netflix, YouTube, Spotify, '
          'Disney+, ChatGPT (обход гео-ограничений).',
      icon: Lucide.zap,
    ),
    RoutingMode(
      id: 'adblock',
      name: 'Только блок рекламы',
      desc:
          'VPN не меняет маршрут трафика — только блокирует рекламу и трекеры '
          'на уровне DNS/правил.',
      icon: Lucide.lock,
    ),
    RoutingMode(
      id: 'ir-smart',
      name: 'Иран (умный)',
      desc:
          'По умолчанию напрямую. Через VPN — заблокированные в Иране ресурсы. '
          'Иранские сайты и IP — напрямую.',
      icon: Lucide.shield,
    ),
    RoutingMode(
      id: 'by-smart',
      name: 'Беларусь (умный)',
      desc:
          'По умолчанию напрямую. Через VPN — заблокированные сервисы. '
          'Белорусское и LAN — напрямую.',
      icon: Lucide.shield,
    ),
    RoutingMode(
      id: 'cn-smart',
      name: 'Китай (умный)',
      desc:
          'Весь зарубежный трафик через VPN, китайские сайты и IP — напрямую '
          '(классическая схема GFW).',
      icon: Lucide.globe,
    ),
    RoutingMode(
      id: 'global',
      name: 'Полный обход',
      desc: 'Весь трафик через VPN, напрямую — только локальная сеть.',
      icon: Lucide.net,
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
