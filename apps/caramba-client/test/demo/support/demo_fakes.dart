/// Двойники для демо-сцен: ядро, которым управляет сценарий, хранилище
/// профилей в памяти и демо-подписка с узлами в стиле живого флота.
library;

import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

import '../../support/fake_csm_device.dart';

/// Ядро, чьи стадии и трафик выставляет сценарий кадр за кадром.
///
/// `connectRaw`/`connect` переводят его в «подключение» и на этом
/// останавливаются: момент «подключено», рост трафика и таймер сессии задаёт
/// тест через [emit]/[pushTraffic] — так каждый кадр предсказуем.
class DemoCore with FakeCsmDevice implements VpnConnection {
  final StreamController<VpnStatus> _status =
      StreamController<VpnStatus>.broadcast();
  final StreamController<TrafficStats> _traffic =
      StreamController<TrafficStats>.broadcast();

  VpnStatus _last = const VpnStatus(stage: VpnStage.disconnected);

  /// Ответ замера; если задан [probeGate], `probe` ждёт его завершения.
  List<ProbeResult> probeResults = const <ProbeResult>[];
  Completer<List<ProbeResult>>? probeGate;

  String? lastRawServerId;

  @override
  VpnStatus get currentStatus => _last;

  @override
  Stream<VpnStatus> get status async* {
    yield _last;
    yield* _status.stream;
  }

  @override
  Stream<TrafficStats> get traffic => _traffic.stream;

  void emit(VpnStatus s) {
    _last = s;
    _status.add(s);
  }

  void pushTraffic(TrafficStats t) => _traffic.add(t);

  @override
  Future<void> connect(Server server) async {
    emit(
      VpnStatus(
        stage: VpnStage.connecting,
        server: server,
        detail: 'Securing tunnel',
      ),
    );
  }

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {
    lastRawServerId = serverId;
    emit(
      VpnStatus(
        stage: VpnStage.connecting,
        server: rawProfileServer(label),
        detail: 'Importing profile',
      ),
    );
  }

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async =>
      const ImportResult(servers: <ImportedServer>[]);

  @override
  Future<List<ProbeResult>> probe({Duration timeout = Duration.zero}) {
    final gate = probeGate;
    if (gate != null) return gate.future;
    return Future<List<ProbeResult>>.value(probeResults);
  }

  @override
  Future<void> setPolicy(CorePolicy policy) async {}

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 0}) async {}

  @override
  Future<void> disconnect() async {
    emit(VpnStatus(stage: VpnStage.disconnected, server: _last.server));
  }

  @override
  Future<VpnStatus> refreshStatus() async => _last;

  @override
  Future<void> dispose() async {
    await _status.close();
    await _traffic.close();
  }
}

/// Профили из памяти: secure storage в тесте не поднимаем.
class DemoProfilesStore implements ConnectionProfilesStore {
  DemoProfilesStore(this.profiles, this.activeId);

  List<ConnectionProfile> profiles;
  String? activeId;

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;

  @override
  Future<String?> readActiveId() async => activeId;

  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async {
    profiles = next;
  }

  @override
  Future<void> writeActiveId(String? id) async {
    activeId = id;
  }

  @override
  Future<void> clear() async {
    profiles = const <ConnectionProfile>[];
    activeId = null;
  }
}

/// Демо-флот: три машины, имена инбаундов как у живой панели.
const List<ImportedServer> demoServers = <ImportedServer>[
  ImportedServer(
    id: '🇩🇪 Stealth',
    name: '🇩🇪 Stealth',
    type: 'vless',
    server: '85.215.196.151',
    port: 443,
    country: 'DE',
    transport: 'tcp',
    security: 'reality',
  ),
  ImportedServer(
    id: '🇩🇪 Speed',
    name: '🇩🇪 Speed',
    type: 'hysteria2',
    server: '85.215.196.151',
    port: 11466,
    country: 'DE',
    security: 'tls',
  ),
  ImportedServer(
    id: '🇩🇪 AmneziaWG',
    name: '🇩🇪 AmneziaWG',
    type: 'wireguard',
    server: '85.215.196.151',
    port: 51820,
    country: 'DE',
  ),
  ImportedServer(
    id: '🇳🇱 Stealth',
    name: '🇳🇱 Stealth',
    type: 'vless',
    server: '45.142.212.10',
    port: 443,
    country: 'NL',
    transport: 'tcp',
    security: 'reality',
  ),
  ImportedServer(
    id: '🇳🇱 Speed',
    name: '🇳🇱 Speed',
    type: 'hysteria2',
    server: '45.142.212.10',
    port: 11466,
    country: 'NL',
    security: 'tls',
  ),
  ImportedServer(
    id: '🇫🇮 Stealth',
    name: '🇫🇮 Stealth',
    type: 'vless',
    server: '95.217.33.4',
    port: 443,
    country: 'FI',
    transport: 'tcp',
    security: 'reality',
  ),
];

Map<String, dynamic> _inbound(
  int id,
  String tag,
  String protocol,
  String network,
  String security,
  String label,
  String proxyName,
) =>
    <String, dynamic>{
      'id': id,
      'tag': tag,
      'protocol': protocol,
      'network': network,
      'security': security,
      'port': protocol == 'hysteria2'
          ? 11466
          : (protocol == 'amneziawg' ? 51820 : 443),
      'label': label,
      'proxy_name': proxyName,
      'available': true,
      'unavailable_reason': null,
    };

Map<String, dynamic> _machine({
  required int id,
  required String name,
  required String cc,
  required String flag,
  required int latency,
  required double load,
  bool awg = true,
}) =>
    <String, dynamic>{
      'id': id,
      'name': name,
      'country_code': cc,
      'latency_ms': latency,
      'load_pct': load,
      'status': 'active',
      // Вход, через который панель строит цепочку к этой машине.
      'via_relay': <String, dynamic>{
        'node_id': 12,
        'name': 'msk-1',
        'country_code': 'RU',
        'chained_in_config': true,
      },
      'inbounds': <Map<String, dynamic>>[
        _inbound(id * 10 + 1, 'reality-in', 'vless', 'tcp', 'reality',
            'Stealth', '$flag Stealth'),
        _inbound(id * 10 + 2, 'hy2-in', 'hysteria2', 'udp', 'tls', 'Speed',
            '$flag Speed'),
        if (awg)
          _inbound(id * 10 + 3, 'awg-in', 'amneziawg', 'udp', 'none',
              'AmneziaWG', '$flag AmneziaWG'),
      ],
      'inbounds_error': null,
    };

/// Панельный флот в форме `GET /app/servers`: три машины с инбаундами и
/// подписями генератора (Stealth / Speed / AmneziaWG).
final List<Server> demoPanelServers = <Server>[
  Server.fromJson(
    _machine(
        id: 1,
        name: 'Frankfurt',
        cc: 'DE',
        flag: '🇩🇪',
        latency: 41,
        load: 18),
  ),
  Server.fromJson(
    _machine(
        id: 2,
        name: 'Amsterdam',
        cc: 'NL',
        flag: '🇳🇱',
        latency: 54,
        load: 31),
  ),
  Server.fromJson(
    _machine(
        id: 3,
        name: 'Helsinki',
        cc: 'FI',
        flag: '🇫🇮',
        latency: 63,
        load: 9,
        awg: false),
  ),
];

/// Входы панели в форме `GET /app/relays`: Россия с двумя релеями и Казахстан.
final List<Relay> demoPanelRelays = Relay.fromCountries(<Relay>[
  Relay.fromApiJson(const <String, dynamic>{
    'country_code': 'RU',
    'country_name': 'Россия',
    'node_count': 2,
    'nodes': <Map<String, dynamic>>[
      <String, dynamic>{
        'id': 12,
        'name': 'msk-1',
        'city': 'Москва',
        'load_pct': 24.0,
        'latency_ms': 9,
        'sort_order': 10,
      },
      <String, dynamic>{
        'id': 13,
        'name': 'spb-1',
        'city': 'Санкт-Петербург',
        'load_pct': 15.0,
        'latency_ms': 14,
        'sort_order': 20,
      },
    ],
  }),
  Relay.fromApiJson(const <String, dynamic>{
    'country_code': 'KZ',
    'country_name': 'Казахстан',
    'node_count': 1,
    'nodes': <Map<String, dynamic>>[
      <String, dynamic>{
        'id': 21,
        'name': 'ala-1',
        'city': 'Алматы',
        'load_pct': 12.0,
        'latency_ms': 38,
        'sort_order': 10,
      },
    ],
  }),
]);

/// Аккаунт панели с закреплённой немецкой машиной.
ConnectionProfile demoPanelProfile({int? nodeId = 1, String? country = 'DE'}) =>
    ConnectionProfile(
      id: 'cp_panel',
      type: ProfileType.panelAccount,
      displayName: 'Caramba Connect',
      source: 'https://app.exarobot.top',
      selectedExitNodeId: nodeId,
      selectedExitCountry: country,
    );

/// Роутер для накладных экранов: `context.pop()`/`context.go()` в них живые,
/// и без GoRouter закрытие экрана уронило бы тест.
GoRouter demoRouter({
  required String initial,
  required Map<String, WidgetBuilder> routes,
}) =>
    GoRouter(
      initialLocation: initial,
      routes: <RouteBase>[
        for (final e in routes.entries)
          GoRoute(path: e.key, builder: (context, state) => e.value(context)),
      ],
    );

/// Своя подписка с закреплённым немецким узлом.
ConnectionProfile demoRawProfile({
  String? selectedServerId = '🇩🇪 Stealth',
  String? selectedExitCountry,
}) =>
    ConnectionProfile(
      id: 'cp_demo',
      type: ProfileType.rawSub,
      displayName: 'Caramba Connect',
      source: 'https://app.exarobot.top/sub/demo',
      rawConfig: 'proxies: []',
      format: 'clash',
      servers: demoServers,
      selectedServerId: selectedServerId,
      selectedExitCountry: selectedExitCountry,
      serversUpdatedMs: DateTime.now().millisecondsSinceEpoch,
    );
