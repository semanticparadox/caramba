import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/prefs_store.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/providers.dart';

/// Выбор пользователя по конфигурации ядра (caramba-core `Policy`). Один срез
/// состояния под Home config-rows и экран Настройки. Хранит индексы выбранных
/// опций в соответствующих списках; провайдеры списков ниже отдают сами опции.
///
/// Маппинг в caramba-core: protocol -> Policy.Protocol; route -> ApplyPreset;
/// relay -> SetRelay(relay_country); stack/dns/mtu/fakeip/ipv6 -> Policy.Tun/DNS;
/// split -> Policy.Split. Готовая политика собирается в
/// `core_policy_mapping.dart` и уходит ядру через `setPolicy` перед каждым
/// поднятием туннеля. Персист — [PrefsStore] (JSON под одним ключом).
class CoreConfig {
  final int protocol; // индекс в ProtocolOption.defaults (0 = Авто)
  final int route; // индекс в RoutingMode.defaults
  final int relay; // индекс в Relay.defaults (0 = Выкл)
  final int stack; // индекс в CoreOption.stacks
  final int dns; // индекс в CoreOption.dns
  final int mtu; // индекс в CoreOption.mtu
  final bool fakeIp;
  final bool ipv6;
  final bool killSwitch;
  final bool autoConnect;

  // Раздельное туннелирование.
  final SplitMode splitMode;
  final Set<String> splitApps; // выбранные id приложений

  /// Домены мимо туннеля, как их ввёл пользователь (запятые/переводы строк).
  /// Сырой текст храним намеренно: список приложений на desktop пока демо, а
  /// домены работают на всех платформах и должны переживать редактирование.
  final String bypassDomains;

  const CoreConfig({
    this.protocol = 0,
    this.route = 0,
    this.relay = 0,
    this.stack = 0,
    this.dns = 0,
    this.mtu = 0,
    this.fakeIp = true,
    this.ipv6 = false,
    this.killSwitch = true,
    this.autoConnect = false,
    this.splitMode = SplitMode.off,
    this.splitApps = const {},
    this.bypassDomains = '',
  });

  int get splitCount => splitMode == SplitMode.off ? 0 : splitApps.length;

  /// Домены из [bypassDomains], разобранные по запятым/переводам строк.
  /// Пустые куски и пробелы отбрасываются.
  List<String> get bypassDomainList => bypassDomains
      .split(RegExp(r'[,\n\r;]+'))
      .map((e) => e.trim())
      .where((e) => e.isNotEmpty)
      .toList(growable: false);

  CoreConfig copyWith({
    int? protocol,
    int? route,
    int? relay,
    int? stack,
    int? dns,
    int? mtu,
    bool? fakeIp,
    bool? ipv6,
    bool? killSwitch,
    bool? autoConnect,
    SplitMode? splitMode,
    Set<String>? splitApps,
    String? bypassDomains,
  }) => CoreConfig(
    protocol: protocol ?? this.protocol,
    route: route ?? this.route,
    relay: relay ?? this.relay,
    stack: stack ?? this.stack,
    dns: dns ?? this.dns,
    mtu: mtu ?? this.mtu,
    fakeIp: fakeIp ?? this.fakeIp,
    ipv6: ipv6 ?? this.ipv6,
    killSwitch: killSwitch ?? this.killSwitch,
    autoConnect: autoConnect ?? this.autoConnect,
    splitMode: splitMode ?? this.splitMode,
    splitApps: splitApps ?? this.splitApps,
    bypassDomains: bypassDomains ?? this.bypassDomains,
  );

  Map<String, dynamic> toJson() => {
    'protocol': protocol,
    'route': route,
    'relay': relay,
    'stack': stack,
    'dns': dns,
    'mtu': mtu,
    'fake_ip': fakeIp,
    'ipv6': ipv6,
    'kill_switch': killSwitch,
    'auto_connect': autoConnect,
    'split_mode': splitMode.name,
    'split_apps': splitApps.toList(growable: false),
    'bypass_domains': bypassDomains,
  };

  /// Читает сохранённый снимок. Каждое поле независимо падает на дефолт, так
  /// что запись, сделанная более старой версией, грузится без миграции.
  /// Индексы клампятся по длине соответствующих списков: набор опций мог
  /// сократиться между версиями, а выход за границы уронил бы экраны.
  factory CoreConfig.fromJson(Map<String, dynamic> json) {
    const d = CoreConfig();
    return CoreConfig(
      protocol: _idx(
        json['protocol'],
        ProtocolOption.defaults.length,
        d.protocol,
      ),
      route: _idx(json['route'], RoutingMode.defaults.length, d.route),
      // Список relay приходит с панели и может быть любой длины: здесь только
      // отсекаем отрицательные значения, кламп по длине делает провайдер.
      relay: _idx(json['relay'], 1 << 20, d.relay),
      stack: _idx(json['stack'], CoreOption.stacks.length, d.stack),
      dns: _idx(json['dns'], CoreOption.dns.length, d.dns),
      mtu: _idx(json['mtu'], CoreOption.mtu.length, d.mtu),
      fakeIp: _bool(json['fake_ip'], d.fakeIp),
      ipv6: _bool(json['ipv6'], d.ipv6),
      killSwitch: _bool(json['kill_switch'], d.killSwitch),
      autoConnect: _bool(json['auto_connect'], d.autoConnect),
      splitMode: _splitMode(json['split_mode']),
      splitApps: <String>{
        ...?(json['split_apps'] as List?)?.whereType<String>(),
      },
      bypassDomains: json['bypass_domains'] is String
          ? json['bypass_domains'] as String
          : d.bypassDomains,
    );
  }

  /// Не-булево значение (запись чужой версии) читается как дефолт.
  static bool _bool(Object? v, bool fallback) => v is bool ? v : fallback;

  static int _idx(Object? v, int length, int fallback) {
    if (v is! num) return fallback;
    final i = v.toInt();
    if (i < 0 || i >= length) return fallback;
    return i;
  }

  static SplitMode _splitMode(Object? v) {
    for (final m in SplitMode.values) {
      if (m.name == v) return m;
    }
    return SplitMode.off;
  }
}

class CoreConfigNotifier extends StateNotifier<CoreConfig> {
  final PrefsStore? _prefs;

  CoreConfigNotifier([this._prefs]) : super(const CoreConfig());

  /// Ставит снимок, прочитанный из [PrefsStore] на старте. Не пишет обратно:
  /// это загрузка, а не пользовательская правка.
  void hydrate(CoreConfig config) => super.state = config;

  /// Любая пользовательская правка сразу уходит в prefs (write-through).
  @override
  set state(CoreConfig value) {
    super.state = value;
    unawaited(_prefs?.writeJson(PrefsStore.kCoreConfig, value.toJson()));
  }

  void setProtocol(int i) => state = state.copyWith(protocol: i);
  void setRoute(int i) => state = state.copyWith(route: i);
  void setRelay(int i) => state = state.copyWith(relay: i);
  void setStack(int i) => state = state.copyWith(stack: i);
  void setDns(int i) => state = state.copyWith(dns: i);
  void setMtu(int i) => state = state.copyWith(mtu: i);
  void setFakeIp(bool v) => state = state.copyWith(fakeIp: v);
  void setIpv6(bool v) => state = state.copyWith(ipv6: v);
  void setKillSwitch(bool v) => state = state.copyWith(killSwitch: v);
  void setAutoConnect(bool v) => state = state.copyWith(autoConnect: v);

  void setSplitMode(SplitMode m) => state = state.copyWith(splitMode: m);

  void setBypassDomains(String v) => state = state.copyWith(bypassDomains: v);

  void toggleSplitApp(String id) {
    final next = {...state.splitApps};
    if (!next.add(id)) next.remove(id);
    state = state.copyWith(splitApps: next);
  }

  /// Применяет результат автоподбора (autotune).
  void applyAutotune({int? protocol, int? stack}) {
    state = state.copyWith(protocol: protocol, stack: stack);
  }
}

final coreConfigProvider =
    StateNotifierProvider<CoreConfigNotifier, CoreConfig>(
      (ref) => CoreConfigNotifier(ref.watch(prefsStoreProvider)),
    );

/// Список протоколов (пока статичный набор caramba-core).
final protocolsProvider = Provider<List<ProtocolOption>>(
  (ref) => ProtocolOption.defaults,
);

/// Список пресетов маршрутизации.
final routingModesProvider = Provider<List<RoutingMode>>(
  (ref) => RoutingMode.defaults,
);

/// Список relay-входов для синхронного чтения (Home config-row). Берёт реальные
/// relay-страны из [apiRelaysProvider] когда они загружены, иначе
/// [Relay.defaults].
///
/// Откат на [Relay.defaults] больше не подставляет стран: там остались только
/// «Выкл» и «Авто», истинные при любом флоте. Пока панель молчит, пикер честно
/// пуст на страны — вместо трёх выдуманных, которые он показывал раньше. Сами
/// входы (узлами, а не странами) живут в слое предложения:
/// `domain/offering/offering_providers.dart`, `relayOffersProvider`.
/// Picker'ы, которым нужны loading/error, читают [apiRelaysProvider] напрямую.
final relaysProvider = Provider<List<Relay>>((ref) {
  return ref.watch(apiRelaysProvider).valueOrNull ?? Relay.defaults;
});

/// Установленные приложения для split-tunnel (демо/desktop; на мобильных
/// заменится платформенным каналом перечисления приложений).
final installedAppsProvider = Provider<List<SplitApp>>((ref) => SplitApp.demo);
