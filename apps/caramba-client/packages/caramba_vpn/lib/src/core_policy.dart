/// Модели политики ядра caramba-core (ABI v2, `CarambaSetPolicy` /
/// `Client.SetPolicyJSON`) и способа захвата трафика (`CarambaSetTunnelMode`).
///
/// JSON, который отдаёт [CorePolicy.toJson], — точная копия контракта из
/// `docs/CORE-ABI-v2.md`: все поля опциональны, неизвестные ядро игнорирует,
/// отсутствующие не переопределяют текущее значение. Поэтому здесь всё
/// nullable, а `toJson` не пишет ключи со значением null.
library;

import 'dart:convert';

/// Способ захвата трафика ядром.
///
/// * [tun] — системный TUN-инбаунд; требует привилегий (root/CAP_NET_ADMIN,
///   администратор на Windows, Network Extension на Apple).
/// * [proxy] — локальный mixed-инбаунд (SOCKS5+HTTP) на 127.0.0.1:port, БЕЗ
///   привилегий; трафик в него направляет приложение или системный прокси ОС.
enum TunnelMode {
  tun('tun'),
  proxy('proxy');

  const TunnelMode(this.wire);

  /// Строка, которую понимает ядро (`CarambaSetTunnelMode(h, mode, port)`).
  final String wire;

  /// Разбор строки статуса/аргумента. Неизвестное значение -> null.
  static TunnelMode? fromWire(String? s) {
    switch (s) {
      case 'tun':
        return TunnelMode.tun;
      case 'proxy':
        return TunnelMode.proxy;
      default:
        return null;
    }
  }
}

/// Раздельное туннелирование (`Policy.Split`).
class CorePolicySplit {
  /// `off` | `bypass` | `allow`.
  final String mode;

  /// Идентификаторы приложений (Android package names и т.п.).
  final List<String> apps;

  /// Домены, которые уходят мимо туннеля.
  final List<String> bypassDomains;

  /// Домены, которые в режиме `allow` идут ЧЕРЕЗ туннель, а всё остальное —
  /// мимо. Правило доменное: «youtube.com» покрывает поддомены, соединение по
  /// голому IP под него не попадает.
  final List<String> allowDomains;

  /// Теги GEOSITE того же allow-списка (готовые наборы доменов сервисов).
  /// Ядро принимает только закрытый словарь: незнакомый тег отвергается
  /// целиком, а не пропускается молча.
  final List<String> allowSites;

  const CorePolicySplit({
    this.mode = 'off',
    this.apps = const <String>[],
    this.bypassDomains = const <String>[],
    this.allowDomains = const <String>[],
    this.allowSites = const <String>[],
  });

  /// Пустые списки НЕ пишутся.
  ///
  /// `allowDomains`/`allowSites` появились позже остальной политики, и ABI v2
  /// их не знает. Слать их всегда значит менять форму сообщения у всех, кто
  /// раздельным туннелем не пользуется вовсе; ядро читает отсутствие поля как
  /// пустой список, и это ровно то же самое, только совместимо.
  Map<String, Object?> toJson() => <String, Object?>{
    'mode': mode,
    'apps': apps,
    'bypassDomains': bypassDomains,
    if (allowDomains.isNotEmpty) 'allowDomains': allowDomains,
    if (allowSites.isNotEmpty) 'allowSites': allowSites,
  };
}

/// DNS-часть политики (`Policy.DNS`).
class CorePolicyDns {
  /// Основные резолверы (DoH/DoT/plain), в порядке приоритета.
  final List<String> nameservers;

  /// Фолбэк-резолверы.
  final List<String> fallback;

  const CorePolicyDns({
    this.nameservers = const <String>[],
    this.fallback = const <String>[],
  });

  Map<String, Object?> toJson() => <String, Object?>{
    'nameservers': nameservers,
    'fallback': fallback,
  };
}

/// Политика ядра, применяемая ДО `Up` (ABI v2 `CarambaSetPolicy`).
///
/// Пример из контракта:
/// ```json
/// {"protocol":"auto","preset":"ru-smart","relay":"TR","stack":"gvisor",
///  "mtu":1280,"ipv6":false,"fakeIp":true,"killSwitch":true,
///  "dns":{"nameservers":["https://1.1.1.1/dns-query"],
///         "fallback":["tls://1.1.1.1:853"]},
///  "split":{"mode":"bypass","apps":["com.example.app"],
///           "bypassDomains":["example.com"]}}
/// ```
class CorePolicy {
  /// `auto` | `AmneziaWG` | `VLESS-Reality` | `Hysteria2` | `TUIC` | `Shadowsocks`.
  final String? protocol;

  /// `ru-smart` | `ru-full` | `telegram-only` | `ir-smart` | `by-smart` |
  /// `cn-smart` | `streaming` | `adblock` | `global` | `` (без пресета).
  final String? preset;

  /// ISO-2 код relay-входа (`TR`, `KZ`, `FI`) либо `` — без relay.
  final String? relay;

  /// `gvisor` | `system` | `mixed`.
  final String? stack;

  final int? mtu;
  final bool? ipv6;
  final bool? fakeIp;
  final bool? killSwitch;

  /// Блок рекламы и трекеров ПОВЕРХ выбранного пресета (`Policy.BlockAds`).
  ///
  /// Отдельно от [preset] намеренно: до него «резать рекламу» означало сменить
  /// режим страны, то есть в интерфейсе это была не галочка, а выбор из
  /// списка.
  final bool? adblock;

  final CorePolicyDns? dns;
  final CorePolicySplit? split;

  const CorePolicy({
    this.protocol,
    this.preset,
    this.relay,
    this.stack,
    this.mtu,
    this.ipv6,
    this.fakeIp,
    this.killSwitch,
    this.adblock,
    this.dns,
    this.split,
  });

  /// Пустая политика: ничего не переопределяет (валидный вход для ядра).
  static const CorePolicy empty = CorePolicy();

  CorePolicy copyWith({
    String? protocol,
    String? preset,
    String? relay,
    String? stack,
    int? mtu,
    bool? ipv6,
    bool? fakeIp,
    bool? killSwitch,
    bool? adblock,
    CorePolicyDns? dns,
    CorePolicySplit? split,
  }) => CorePolicy(
    protocol: protocol ?? this.protocol,
    preset: preset ?? this.preset,
    relay: relay ?? this.relay,
    stack: stack ?? this.stack,
    mtu: mtu ?? this.mtu,
    ipv6: ipv6 ?? this.ipv6,
    fakeIp: fakeIp ?? this.fakeIp,
    killSwitch: killSwitch ?? this.killSwitch,
    adblock: adblock ?? this.adblock,
    dns: dns ?? this.dns,
    split: split ?? this.split,
  );

  /// JSON ровно по ABI v2. Ключи со значением null не пишутся вовсе — ядро
  /// трактует отсутствие поля как «не менять».
  Map<String, Object?> toJson() {
    final map = <String, Object?>{};
    if (protocol != null) map['protocol'] = protocol;
    if (preset != null) map['preset'] = preset;
    if (relay != null) map['relay'] = relay;
    if (stack != null) map['stack'] = stack;
    if (mtu != null) map['mtu'] = mtu;
    if (ipv6 != null) map['ipv6'] = ipv6;
    if (fakeIp != null) map['fakeIp'] = fakeIp;
    if (killSwitch != null) map['killSwitch'] = killSwitch;
    if (adblock != null) map['adblock'] = adblock;
    if (dns != null) map['dns'] = dns!.toJson();
    if (split != null) map['split'] = split!.toJson();
    return map;
  }
}

/// Сериализует политику в строку JSON для провода (канал `setPolicy` и
/// `CarambaSetPolicy`). Вынесено из класса, чтобы модель осталась чистой.
String jsonEncodePolicy(CorePolicy policy) => jsonEncode(policy.toJson());
