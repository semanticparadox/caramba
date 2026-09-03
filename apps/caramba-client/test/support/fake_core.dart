/// Ядро-заглушка для тестов, которым нужен только мост CSM.
///
/// Отдаёт ровно то, что ему положили в [csmStateJson] и [csmLadderJson], и
/// ничего не выдумывает: выдуманный каталог поднял бы карточку 02-SPEC.md 7.7.1
/// о сужении, которого не было, а выдуманная попытка попала бы в историю
/// INV-17 как событие, которого не происходило.
library;

import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

import 'fake_csm_device.dart';

class FakeVpnCore with FakeCsmDevice implements VpnConnection {
  @override
  final VpnStatus currentStatus = const VpnStatus(stage: VpnStage.disconnected);

  @override
  Stream<VpnStatus> get status => Stream<VpnStatus>.value(currentStatus);

  @override
  Stream<TrafficStats> get traffic => const Stream<TrafficStats>.empty();

  @override
  Future<void> connect(Object server) async {}

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {}

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async => const ImportResult(servers: <ImportedServer>[]);

  @override
  Future<List<ProbeResult>> probe({Duration timeout = Duration.zero}) async =>
      const <ProbeResult>[];

  @override
  Future<void> setPolicy(CorePolicy policy) async {}

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 0}) async {}

  @override
  Future<void> disconnect() async {}

  @override
  Future<void> dispose() async {}
}
