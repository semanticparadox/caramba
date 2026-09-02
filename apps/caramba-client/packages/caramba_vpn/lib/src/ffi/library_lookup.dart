/// Поиск динамической библиотеки ядра (`libcaramba_core.dylib` / `.so` /
/// `.dll`) для dart:ffi-пути.
///
/// Логика разрешения пути — ЧИСТАЯ функция [carambaCoreLibraryCandidates]:
/// она ничего не читает с диска, только собирает упорядоченный список
/// кандидатов. Это позволяет покрыть её юнит-тестом без самой библиотеки, а
/// проверку существования файла оставить вызывающему ([resolveCarambaCoreLib]).
library;

/// Имя файла библиотеки для платформы.
///
/// Отдельная функция (а не константа), потому что тесты гоняют её для всех
/// трёх платформ, не завися от текущей.
String carambaCoreLibFileName({
  required bool isMacOS,
  required bool isWindows,
}) {
  if (isMacOS) return 'libcaramba_core.dylib';
  if (isWindows) return 'caramba_core.dll';
  return 'libcaramba_core.so';
}

/// Собирает упорядоченный список путей-кандидатов к библиотеке ядра.
///
/// Порядок (macOS, тот же принцип на остальных desktop-платформах):
///   1. `CARAMBA_CORE_LIB` — явное переопределение из окружения; если задано,
///      идёт первым и не отменяет остальные (чтобы кривой путь не заблокировал
///      рабочую сборку — но обычно он и есть ответ);
///   2. `<каталог исполняемого файла>/../Frameworks/<lib>` — расположение внутри
///      собранного .app (плагин вендорит dylib через podspec);
///   3. `<каталог исполняемого файла>/<lib>` — «плоская» раскладка bundle;
///   4. `<предок>/libs/caramba-core/build/<lib>` для каждого предка рабочего
///      каталога и каталога `Platform.script` — dev-путь для `flutter run`
///      прямо из репозитория.
///
/// Дубликаты убираются с сохранением порядка.
List<String> carambaCoreLibraryCandidates({
  String? envOverride,
  String? executableDir,
  String? scriptDir,
  String? workingDir,
  bool isMacOS = true,
  bool isWindows = false,
}) {
  final lib = carambaCoreLibFileName(isMacOS: isMacOS, isWindows: isWindows);
  final out = <String>[];

  void add(String? path) {
    if (path == null || path.isEmpty) return;
    final normalized = normalizeLibPath(path);
    if (!out.contains(normalized)) out.add(normalized);
  }

  if (envOverride != null && envOverride.trim().isNotEmpty) {
    add(envOverride.trim());
  }

  if (executableDir != null && executableDir.isNotEmpty) {
    add('$executableDir/../Frameworks/$lib');
    add('$executableDir/$lib');
  }

  // Dev-путь из репозитория: поднимаемся вверх от cwd и от каталога скрипта,
  // пока не упрёмся в корень, и на каждом уровне пробуем build-каталог ядра.
  for (final root in <String?>[workingDir, scriptDir]) {
    if (root == null || root.isEmpty) continue;
    for (final ancestor in ancestorDirectories(root)) {
      add('$ancestor/libs/caramba-core/build/$lib');
    }
  }

  return out;
}

/// Все каталоги от [dir] вверх до корня включительно (сам [dir] первый).
List<String> ancestorDirectories(String dir) {
  final normalized = normalizeLibPath(dir);
  final out = <String>[];
  var current = normalized;
  while (current.isNotEmpty) {
    out.add(current);
    final slash = current.lastIndexOf('/');
    if (slash <= 0) break;
    current = current.substring(0, slash);
  }
  return out;
}

/// Схлопывает `//`, `/./` и `a/b/../c` -> `a/c`, приводит разделители к `/`.
/// Достаточно для сравнения кандидатов и для передачи в `DynamicLibrary.open`.
String normalizeLibPath(String path) {
  final unified = path.replaceAll('\\', '/');
  final absolute = unified.startsWith('/');
  final parts = <String>[];
  for (final segment in unified.split('/')) {
    if (segment.isEmpty || segment == '.') continue;
    if (segment == '..') {
      if (parts.isNotEmpty && parts.last != '..') {
        parts.removeLast();
        continue;
      }
      if (absolute) continue; // выше корня не поднимаемся
    }
    parts.add(segment);
  }
  final joined = parts.join('/');
  if (absolute) return '/$joined';
  return joined;
}

/// Каталог, содержащий [filePath] (без завершающего слэша).
String parentDirectory(String filePath) {
  final normalized = normalizeLibPath(filePath);
  final slash = normalized.lastIndexOf('/');
  if (slash < 0) return '';
  if (slash == 0) return '/';
  return normalized.substring(0, slash);
}
