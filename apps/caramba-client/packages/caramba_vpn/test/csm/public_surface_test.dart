// Публичная поверхность пакета, и запрет лазить в src/.
//
// У пакета две точки входа: `package:caramba_vpn/caramba_vpn.dart` (контракт
// туннеля, политика ядра, ключи устройства) и `package:caramba_vpn/csm.dart`
// (проверяющий CSM/1). Всё остальное под `lib/src/` это внутренности.
//
// Импорт внутренностей мимо барреля это не стилистика. Проверяющий CSM/1 это
// граница доверия: пока приложение и харнессы ходят через баррель, «что именно
// экспортировано» это одно решение в одном файле, и сузить поверхность можно,
// не ломая ничего молча. Как только кто-то импортирует `src/csm/verifier.dart`
// напрямую, поверхность перестаёт существовать, а вместе с ней и возможность
// проверить, что харнесс и приложение видят один и тот же verifier.

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

/// Префикс внутреннего импорта. Собран из двух соседних литералов НАМЕРЕННО:
/// иначе сам этот файл попал бы в собственную выборку и тест ловил бы себя.
const String _internal =
    'package:caramba_vpn/'
    'src/';

/// Корень приложения: каталог, где лежат и `lib/`, и `packages/caramba_vpn/`.
/// Ищется вверх от cwd, как это делает корпусный тест.
Directory _appRoot() {
  var dir = Directory.current.absolute;
  for (var i = 0; i < 8; i++) {
    if (Directory('${dir.path}/packages/caramba_vpn').existsSync() &&
        Directory('${dir.path}/lib/data').existsSync()) {
      return dir;
    }
    final parent = dir.parent;
    if (parent.path == dir.path) break;
    dir = parent;
  }
  throw StateError(
    'корень caramba-client не найден выше ${Directory.current.path}',
  );
}

List<File> _dartFiles(Directory dir) {
  if (!dir.existsSync()) return const <File>[];
  return dir
      .listSync(recursive: true)
      .whereType<File>()
      .where((f) => f.path.endsWith('.dart'))
      .toList(growable: false);
}

void main() {
  test('никто снаружи пакета не импортирует внутренности пакета', () {
    final root = _appRoot();
    final scanned = <File>[
      ..._dartFiles(Directory('${root.path}/lib')),
      ..._dartFiles(Directory('${root.path}/test')),
      ..._dartFiles(Directory('${root.path}/packages/caramba_vpn/test')),
    ];
    expect(scanned, isNotEmpty, reason: 'сканировать нечего — проверь пути');

    final offenders = <String>[];
    for (final file in scanned) {
      if (file.readAsStringSync().contains(_internal)) {
        offenders.add(file.path.substring(root.path.length + 1));
      }
    }
    expect(
      offenders,
      isEmpty,
      reason:
          'эти файлы лезут во внутренности пакета вместо барреля; для '
          'проверяющего CSM/1 точка входа одна — package:caramba_vpn/csm.dart',
    );
  });

  test('баррель csm.dart закрывает всё, что нужно харнессу', () {
    final root = _appRoot();
    final barrel = File('${root.path}/packages/caramba_vpn/lib/csm.dart');
    expect(barrel.existsSync(), isTrue);
    // Сам баррель обязан оставаться единственным реэкспортом внутреннего
    // csm.dart: если его подменят на список отдельных файлов, поверхность
    // начнёт расходиться с внутренним индексом незаметно.
    expect(
      barrel.readAsStringSync(),
      contains(
        "export '$_internal"
        'csm/csm.dart\';',
      ),
    );
  });
}
