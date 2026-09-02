import 'dart:convert';
import 'dart:io';

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

/// Сквозной тест ядра на macOS через dart:ffi: импорт подписки, подъём в
/// proxy-режиме (mixed-port, без root) и HTTP-запрос через этот порт.
///
/// Пропускается, если не заданы `CARAMBA_CORE_LIB` (путь к libcaramba_core)
/// и `CARAMBA_TEST_SUB` (путь к clash-конфигу с рабочим узлом, например
/// локальным SOCKS5). Порт: `CARAMBA_TEST_PORT` (по умолчанию 7899).
void main() {
  final libPath = Platform.environment['CARAMBA_CORE_LIB'];
  final subPath = Platform.environment['CARAMBA_TEST_SUB'];
  final port = int.tryParse(Platform.environment['CARAMBA_TEST_PORT'] ?? '') ?? 7899;
  final available = libPath != null &&
      File(libPath).existsSync() &&
      subPath != null &&
      File(subPath).existsSync();

  test(
    'ffi tunnel: import, up in proxy mode, HTTP through the mixed port',
    () async {
      final lib = CarambaCoreLibrary.open(libPath!);
      final workDir = Directory.systemTemp.createTempSync('caramba-ffi-tunnel').path;
      final h = lib.create(panelUrl: '', workDir: workDir, tokenPath: '$workDir/tokens.json');
      expect(h, greaterThan(0));
      try {
        expect(lib.setTunnelMode(h, 'proxy', port), isEmpty);
        final meta = jsonDecode(lib.importSubscription(h, File(subPath!).readAsStringSync(), 'auto'))
            as Map<String, dynamic>;
        expect(meta['error'], isNull, reason: 'import: ${meta['error']}');
        final servers = (meta['servers'] as List?) ?? const [];
        expect(servers, isNotEmpty);
        // ignore: avoid_print
        print('imported ${servers.length} node(s): ${servers.map((s) => s['id']).join(', ')}');

        final upJson = jsonDecode(lib.up(h, '')) as Map<String, dynamic>;
        expect(upJson['error'], isNull, reason: 'up: ${upJson['error']}');

        Map<String, dynamic> st = const {};
        for (var i = 0; i < 40; i++) {
          st = jsonDecode(lib.status(h)) as Map<String, dynamic>;
          if (st['stage'] == 'connected') break;
          await Future<void>.delayed(const Duration(milliseconds: 500));
        }
        // ignore: avoid_print
        print('status: $st');
        expect(st['stage'], 'connected');
        expect(st['mode'], 'proxy');
        expect(st['mixedPort'], port);

        final client = HttpClient();
        client.findProxy = (_) => 'PROXY 127.0.0.1:$port';
        client.connectionTimeout = const Duration(seconds: 15);
        final req = await client.getUrl(Uri.parse('http://example.com/'));
        final res = await req.close();
        final body = await res.transform(utf8.decoder).join();
        client.close(force: true);
        // ignore: avoid_print
        print('http via 127.0.0.1:$port -> ${res.statusCode}, ${body.length} bytes');
        expect(res.statusCode, 200);
        expect(body, contains('Example Domain'));

        final tr = jsonDecode(lib.traffic(h)) as Map<String, dynamic>;
        // ignore: avoid_print
        print('traffic: $tr');
        expect((tr['downTotal'] as num) > 0, isTrue);
        expect(lib.down(h), isEmpty);
      } finally {
        lib.free(h);
      }
    },
    skip: available ? false : 'set CARAMBA_CORE_LIB and CARAMBA_TEST_SUB to run',
    timeout: const Timeout(Duration(minutes: 2)),
  );
}
