import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('ImportResult.fromJson', () {
    test('parses the ABI v2 metadata shape', () {
      const json = '''
{"name":"My subscription","servers":[
  {"id":"NL-01","name":"Amsterdam 01","type":"vless","server":"nl-01.example",
   "port":443,"country":"NL"},
  {"id":"DE-02","name":"Frankfurt 02","type":"hysteria2",
   "server":"de-02.example","port":8443,"country":"DE"}]}
''';
      final result = ImportResult.fromJson(json);

      expect(result.name, 'My subscription');
      expect(result.count, 2);
      final first = result.servers.first;
      expect(first.id, 'NL-01');
      expect(first.name, 'Amsterdam 01');
      expect(first.type, 'vless');
      expect(first.server, 'nl-01.example');
      expect(first.port, 443);
      expect(first.country, 'NL');
      expect(result.servers.last.id, 'DE-02');
    });

    test('tolerates a missing servers array and a missing name', () {
      final result = ImportResult.fromJson('{}');
      expect(result.name, isNull);
      expect(result.servers, isEmpty);
    });

    test('skips non-object entries instead of throwing', () {
      final result = ImportResult.fromJson(
        '{"servers":[{"id":"A"},"junk",42,null]}',
      );
      expect(result.count, 1);
      expect(result.servers.single.id, 'A');
      // Отсутствующие поля дают нейтральные значения, а не null-падение.
      expect(result.servers.single.port, 0);
      expect(result.servers.single.country, '');
    });

    test('broken JSON degrades to an empty result', () {
      expect(ImportResult.fromJson('not json').servers, isEmpty);
      expect(ImportResult.fromJson('').servers, isEmpty);
      expect(ImportResult.fromJson('[1,2,3]').servers, isEmpty);
    });
  });

  group('ProbeResult.listFromJson', () {
    test('parses latencies and marks -1 as a timeout', () {
      const json = '''
{"servers":[
  {"id":"NL-01","name":"Amsterdam 01","country":"NL","latencyMs":42},
  {"id":"TR-03","name":"Istanbul 03","country":"TR","latencyMs":-1}]}
''';
      final results = ProbeResult.listFromJson(json);

      expect(results, hasLength(2));
      expect(results.first.id, 'NL-01');
      expect(results.first.latencyMs, 42);
      expect(results.first.timedOut, isFalse);
      expect(results.last.latencyMs, -1);
      expect(results.last.timedOut, isTrue);
    });

    test('a missing latencyMs counts as a timeout', () {
      final results = ProbeResult.listFromJson('{"servers":[{"id":"X"}]}');
      expect(results.single.latencyMs, -1);
      expect(results.single.timedOut, isTrue);
    });

    test('an error payload yields no results rather than throwing', () {
      expect(ProbeResult.listFromJson('{"error":"no config loaded"}'), isEmpty);
    });
  });

  group('VpnStatus.fromMap', () {
    test('parses the ABI v2 status map with mode/mixedPort/activeProxy', () {
      final status = VpnStatus<Object>.fromMap(<Object?, Object?>{
        'stage': 'connected',
        'detail': null,
        'connectedSinceMs': 1756800000000,
        'mode': 'proxy',
        'mixedPort': 7890,
        'activeProxy': 'NL-01',
      });

      expect(status.stage, VpnStage.connected);
      expect(status.isConnected, isTrue);
      expect(status.mode, TunnelMode.proxy);
      expect(status.mixedPort, 7890);
      expect(status.activeProxy, 'NL-01');
      expect(
        status.connectedSince,
        DateTime.fromMillisecondsSinceEpoch(1756800000000),
      );
    });

    test('the legacy map without ABI v2 fields still parses', () {
      final status = VpnStatus<Object>.fromMap(<Object?, Object?>{
        'stage': 'connecting',
        'detail': 'Securing tunnel',
        'connectedSinceMs': 0,
      });

      expect(status.stage, VpnStage.connecting);
      expect(status.isBusy, isTrue);
      expect(status.detail, 'Securing tunnel');
      expect(status.connectedSince, isNull);
      expect(status.mode, isNull);
      expect(status.mixedPort, isNull);
      expect(status.activeProxy, isNull);
    });

    test('an unknown stage and an empty activeProxy degrade safely', () {
      final status = VpnStatus<Object>.fromMap(<Object?, Object?>{
        'stage': 'whatever',
        'mode': 'wireguard',
        'activeProxy': '',
      });

      expect(status.stage, VpnStage.disconnected);
      expect(status.mode, isNull);
      expect(status.activeProxy, isNull);
    });

    test('carries the app-side server through unchanged', () {
      final status = VpnStatus<String>.fromMap(
        <Object?, Object?>{'stage': 'connected'},
        server: 'Node #7',
      );
      expect(status.server, 'Node #7');
      expect(status.copyWith(detail: 'x').server, 'Node #7');
    });
  });

  group('TrafficStats.fromMap', () {
    test('reads the four counters and defaults the missing ones', () {
      final t = TrafficStats.fromMap(<Object?, Object?>{
        'downBps': 1024,
        'upBps': 512,
      });
      expect(t.downBps, 1024);
      expect(t.upBps, 512);
      expect(t.downTotal, 0);
      expect(t.upTotal, 0);
    });
  });
}
