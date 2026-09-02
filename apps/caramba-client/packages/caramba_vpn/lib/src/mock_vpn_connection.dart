/// Имитация ядра для dev-сборок без нативных артефактов.
library;

import 'dart:async';

import 'package:caramba_vpn/src/contract.dart';
import 'package:caramba_vpn/src/core_models.dart';
import 'package:caramba_vpn/src/core_policy.dart';

/// Имитация ядра для desktop/dev: проходит реальный жизненный цикл состояний
/// и генерирует «дышащий» трафик, чтобы UI работал end-to-end без нативного
/// бэка.
///
/// Generic-методы ABI v2 отвечают правдоподобными заглушками: импорт ничего не
/// парсит и отдаёт три выдуманных узла, probe отвечает фиксированными
/// задержками через 300 мс, setPolicy/setTunnelMode лишь запоминают последнее
/// значение (доступно через [lastPolicy] / [lastMode] / [lastMixedPort]).
class MockVpnConnection<S extends Object> implements VpnConnection<S> {
  final _statusCtrl = StreamController<VpnStatus<S>>.broadcast();
  final _trafficCtrl = StreamController<TrafficStats>.broadcast();

  final VpnRawTargetFactory<S>? _rawTarget;

  VpnStatus<S> _last = VpnStatus<S>.disconnected();
  Timer? _trafficTimer;
  Timer? _phaseTimer;
  int _seed = 0;

  /// Последняя политика, переданная в [setPolicy] (null — не вызывали).
  CorePolicy? lastPolicy;

  /// Последний режим захвата трафика из [setTunnelMode].
  TunnelMode lastMode = TunnelMode.tun;

  /// Последний порт mixed-инбаунда из [setTunnelMode].
  int lastMixedPort = 7890;

  MockVpnConnection({VpnRawTargetFactory<S>? rawTarget})
    : _rawTarget = rawTarget;

  /// Выдуманные узлы, которые возвращает [importSubscription] и [probe].
  static const List<ImportedServer> mockServers = <ImportedServer>[
    ImportedServer(
      id: 'NL-01',
      name: 'Amsterdam 01',
      type: 'vless',
      server: 'nl-01.example',
      port: 443,
      country: 'NL',
    ),
    ImportedServer(
      id: 'DE-02',
      name: 'Frankfurt 02',
      type: 'hysteria2',
      server: 'de-02.example',
      port: 8443,
      country: 'DE',
    ),
    ImportedServer(
      id: 'TR-03',
      name: 'Istanbul 03',
      type: 'ss',
      server: 'tr-03.example',
      port: 8388,
      country: 'TR',
    ),
  ];

  @override
  Stream<VpnStatus<S>> get status async* {
    yield _last;
    yield* _statusCtrl.stream;
  }

  @override
  Stream<TrafficStats> get traffic => _trafficCtrl.stream;

  @override
  VpnStatus<S> get currentStatus => _last;

  void _emit(VpnStatus<S> s) {
    _last = s;
    _statusCtrl.add(s);
  }

  @override
  Future<void> connect(S server) async {
    _phaseTimer?.cancel();
    _emit(
      VpnStatus<S>(
        stage: VpnStage.connecting,
        server: server,
        detail: 'Securing tunnel',
      ),
    );
    _phaseTimer = Timer(const Duration(milliseconds: 1400), () {
      _emit(
        VpnStatus<S>(
          stage: VpnStage.connected,
          server: server,
          connectedSince: DateTime.now(),
          mode: lastMode,
          mixedPort: lastMode == TunnelMode.proxy ? lastMixedPort : null,
        ),
      );
      _startTraffic();
    });
  }

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {
    _phaseTimer?.cancel();
    final server = _rawTarget?.call(label);
    _emit(
      VpnStatus<S>(
        stage: VpnStage.connecting,
        server: server,
        detail: 'Importing profile',
      ),
    );
    _phaseTimer = Timer(const Duration(milliseconds: 1400), () {
      _emit(
        VpnStatus<S>(
          stage: VpnStage.connected,
          server: server,
          connectedSince: DateTime.now(),
          mode: lastMode,
          mixedPort: lastMode == TunnelMode.proxy ? lastMixedPort : null,
          // Пин узла отражаем как активный прокси; иначе первый из мок-списка.
          activeProxy: (serverId != null && serverId.isNotEmpty)
              ? serverId
              : mockServers.first.id,
        ),
      );
      _startTraffic();
    });
  }

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async {
    // Мок ничего не парсит: сырые данные игнорируются, отдаётся фиксированный
    // набор узлов, чтобы UI generic-режима работал без ядра.
    await Future<void>.delayed(const Duration(milliseconds: 200));
    return const ImportResult(name: 'Mock subscription', servers: mockServers);
  }

  @override
  Future<List<ProbeResult>> probe({
    Duration timeout = const Duration(seconds: 5),
  }) async {
    await Future<void>.delayed(const Duration(milliseconds: 300));
    const latencies = <int>[42, 128, -1];
    return <ProbeResult>[
      for (var i = 0; i < mockServers.length; i++)
        ProbeResult(
          id: mockServers[i].id,
          name: mockServers[i].name,
          country: mockServers[i].country,
          latencyMs: latencies[i % latencies.length],
        ),
    ];
  }

  @override
  Future<void> setPolicy(CorePolicy policy) async {
    lastPolicy = policy;
  }

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {
    lastMode = mode;
    lastMixedPort = mixedPort;
  }

  @override
  Future<void> disconnect() async {
    _phaseTimer?.cancel();
    _stopTraffic();
    _emit(VpnStatus<S>(stage: VpnStage.disconnected, server: _last.server));
  }

  void _startTraffic() {
    _stopTraffic();
    _trafficTimer = Timer.periodic(const Duration(seconds: 1), (_) {
      _seed++;
      // Псевдослучайный, но плавный профиль (без dart:math import — детерминирован).
      final wobble = (_seed * 2654435761) & 0x7fffffff;
      final down = 4 * 1024 * 1024 + (wobble % (10 * 1024 * 1024));
      final up = 256 * 1024 + (wobble % (1024 * 1024));
      _trafficCtrl.add(
        TrafficStats(
          downBps: down,
          upBps: up,
          downTotal: down * _seed,
          upTotal: up * _seed,
        ),
      );
    });
  }

  void _stopTraffic() {
    _trafficTimer?.cancel();
    _trafficTimer = null;
    _trafficCtrl.add(TrafficStats.zero);
  }

  @override
  Future<void> dispose() async {
    _phaseTimer?.cancel();
    _trafficTimer?.cancel();
    await _statusCtrl.close();
    await _trafficCtrl.close();
  }
}
