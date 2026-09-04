// Импортированная подписка обязана уезжать в ядро с ПУСТЫМ адресом панели.
//
// Здесь была тихая потеря функции. `FfiVpnConnection` подставляла заглушку
// `https://panel.invalid` везде, где адреса панели нет, — то есть на КАЖДОЙ
// сырой подписке, — под комментарием «домен из RFC 2606 всё равно не
// резолвится, ядро физически никуда по нему не пойдёт». Первая половина правда,
// вторая — нет: ядру важно не то, резолвится ли база, а то, ПУСТА ли она.
// Непустая означает «у оператора есть зеркало rule-set'ов», и пресет блокировки
// рекламы, увидев её, подавляет откат на встроенный тег GEOSITE и выпускает
// провайдера на `https://panel.invalid/rulesets/ads`. Список не приезжает
// никогда, а встроенный уже вытеснен: человек с импортированной подпиской терял
// блокировку рекламы целиком, и ни один слой не мог этого сказать.
//
// Гейтов два. Первый читает исходник: заглушка не должна вернуться под другим
// именем. Второй бьёт по НАСТОЯЩЕЙ библиотеке ядра и проверяет утверждение, на
// котором заглушка держалась, — «CarambaNew требует непустой panelURL».
//
// Что происходит на пустой базе, проверено на своей стороне границы:
// libs/caramba-core/routing/adblock_source_test.go,
// TestAdblockFallsBackToGeositeWhenNoMirrorIsAvailable — правило GEOSITE
// возвращается на место, провайдер выбрасывается, а отчёт называет источник
// `dropped`/`no_mirror`, то есть «зеркала нет», а не «зеркало есть, доедет ли —
// не видно».

import 'dart:io';

import 'package:caramba_vpn/caramba_vpn.dart';
import 'package:flutter_test/flutter_test.dart';

/// Минимальная сырая подписка: один узел, ничего лишнего. Импорт её только
/// разбирает — соединение не поднимается.
const String _rawClash = '''
proxies:
  - name: "probe"
    type: socks5
    server: 127.0.0.1
    port: 1080
''';

/// Ищет файл вверх по дереву: рабочий каталог `flutter test` зависит от того,
/// откуда его запустили.
File _locate(String relative) {
  var dir = Directory.current.absolute;
  for (var i = 0; i < 8; i++) {
    final candidate = File('${dir.path}/$relative');
    if (candidate.existsSync()) return candidate;
    final parent = dir.parent;
    if (parent.path == dir.path) break;
    dir = parent;
  }
  throw StateError('не найден $relative от ${Directory.current.path}');
}

void main() {
  test('в FFI-пути не осталось выдуманного адреса панели', () {
    final source = _locate(
      'packages/caramba_vpn/lib/src/ffi_vpn_connection.dart',
    ).readAsStringSync();

    // Только код: в шапке `_ensureCore` заглушка названа по имени, и запрет на
    // строку целиком запретил бы рассказывать, чего именно делать нельзя.
    final code = source
        .split('\n')
        .where((l) => !l.trimLeft().startsWith('///'))
        .join('\n');

    expect(
      code,
      isNot(contains('.invalid')),
      reason:
          'адрес-заглушка означает для ядра «у оператора есть зеркало» и '
          'выключает откат блокировки рекламы на встроенный список',
    );
    expect(
      code,
      // С запятой: `panelUrl: panelUrl.isEmpty ? ... : panelUrl` содержал бы
      // ту же подстроку без неё, и гейт пропустил бы ровно ту форму, из-за
      // которой он написан.
      contains('panelUrl: panelUrl,'),
      reason: 'адрес панели обязан уезжать в ядро как есть, включая пустой',
    );
  });

  group('настоящая libcaramba_core', () {
    final libPath = Platform.environment[kCarambaCoreLibEnv];
    final available = libPath != null && File(libPath).existsSync();
    final skipReason = available
        ? null
        : 'set $kCarambaCoreLibEnv to an existing libcaramba_core to run this';

    test(
      'сырой импорт проходит на пустом адресе панели',
      () async {
        final workDir = Directory.systemTemp
            .createTempSync('caramba-raw-base')
            .path;
        final conn = FfiVpnConnection<String>(
          describe: (s) => VpnServerArgs(id: s, name: s),
          rawTarget: (label) => label,
          // configResolver не задан вовсе: это и есть путь сырой подписки —
          // панели нет, настраивать ядро нечем.
          libraryPath: libPath,
          workDir: workDir,
        );
        addTearDown(conn.dispose);

        // Хэндл ядра создаётся ВНУТРИ этого вызова, с тем самым пустым адресом.
        // Отвергни ядро пустую базу — здесь была бы CarambaCoreException
        // «core init failed», и заглушка оказалась бы оправданной.
        final result = await conn.importSubscription(
          raw: _rawClash,
          format: 'clash',
        );
        expect(result.servers, isNotEmpty);
      },
      skip: skipReason,
    );
  });
}
