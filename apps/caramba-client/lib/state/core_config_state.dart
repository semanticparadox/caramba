import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/state/account_state.dart';

/// Выбор пользователя по конфигурации ядра (caramba-core `Policy`). Один срез
/// состояния под Home config-rows и экран Настройки. Хранит индексы выбранных
/// опций в соответствующих списках; провайдеры списков ниже отдают сами опции.
///
/// Маппинг в caramba-core: protocol -> Policy.Protocol; route -> ApplyPreset;
/// relay -> SetRelay(relay_country); stack/dns/mtu/fakeip/ipv6 -> Policy.Tun/DNS;
/// split -> Policy.Split. Пока держится in-memory; персист — отдельным раном.
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
  });

  int get splitCount => splitMode == SplitMode.off ? 0 : splitApps.length;

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
  }) =>
      CoreConfig(
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
      );
}

class CoreConfigNotifier extends StateNotifier<CoreConfig> {
  CoreConfigNotifier() : super(const CoreConfig());

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
  (ref) => CoreConfigNotifier(),
);

/// Список протоколов (пока статичный набор caramba-core).
final protocolsProvider =
    Provider<List<ProtocolOption>>((ref) => ProtocolOption.defaults);

/// Список пресетов маршрутизации.
final routingModesProvider =
    Provider<List<RoutingMode>>((ref) => RoutingMode.defaults);

/// Список relay-входов для синхронного чтения (Home config-row). Берёт реальные
/// relay-страны из [apiRelaysProvider] когда они загружены, иначе [Relay.defaults].
/// Picker'ы, которым нужны loading/error, читают [apiRelaysProvider] напрямую.
final relaysProvider = Provider<List<Relay>>((ref) {
  return ref.watch(apiRelaysProvider).valueOrNull ?? Relay.defaults;
});

/// Установленные приложения для split-tunnel (демо/desktop; на мобильных
/// заменится платформенным каналом перечисления приложений).
final installedAppsProvider =
    Provider<List<SplitApp>>((ref) => SplitApp.demo);
