/// Разрешение пути к `libcaramba_core` на диске и открытие библиотеки.
///
/// Чистая часть логики (список кандидатов) живёт в `library_lookup.dart` и
/// покрыта тестами; здесь только дисковые проверки и `dart:io`.
library;

import 'dart:io';

import 'package:caramba_vpn/src/ffi/caramba_core_bindings.dart';
import 'package:caramba_vpn/src/ffi/library_lookup.dart';

/// Имя переменной окружения, которой можно жёстко указать библиотеку ядра.
const String kCarambaCoreLibEnv = 'CARAMBA_CORE_LIB';

/// Библиотека ядра не найдена ни по одному из кандидатов.
class CarambaCoreLibraryNotFound implements Exception {
  /// Пути, которые были проверены, в порядке проверки.
  final List<String> tried;

  const CarambaCoreLibraryNotFound(this.tried);

  @override
  String toString() =>
      'caramba-core: libcaramba_core not found. Tried:\n'
      '${tried.map((p) => '  - $p').join('\n')}\n'
      'Build it with libs/caramba-core/scripts/build-desktop-lib.sh or point '
      '$kCarambaCoreLibEnv at an existing dylib.';
}

/// Кандидаты для текущего процесса (env + каталог бинаря + dev-путь репозитория).
List<String> currentProcessLibraryCandidates() {
  final env = Platform.environment[kCarambaCoreLibEnv];
  final executableDir = parentDirectory(Platform.resolvedExecutable);
  String? scriptDir;
  final script = Platform.script;
  if (script.scheme == 'file') {
    scriptDir = parentDirectory(script.toFilePath());
  }
  return carambaCoreLibraryCandidates(
    envOverride: env,
    executableDir: executableDir,
    scriptDir: scriptDir,
    workingDir: Directory.current.path,
    isMacOS: Platform.isMacOS,
    isWindows: Platform.isWindows,
  );
}

/// Первый существующий кандидат. Бросает [CarambaCoreLibraryNotFound], если
/// ни одного файла нет.
String resolveCarambaCoreLibraryPath() {
  final candidates = currentProcessLibraryCandidates();
  for (final path in candidates) {
    if (File(path).existsSync()) return path;
  }
  throw CarambaCoreLibraryNotFound(candidates);
}

/// Открывает библиотеку ядра, разрешив путь ([explicitPath] минует поиск).
CarambaCoreLibrary openCarambaCoreLibrary({String? explicitPath}) {
  final path = explicitPath ?? resolveCarambaCoreLibraryPath();
  return CarambaCoreLibrary.open(path);
}
