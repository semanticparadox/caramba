/// Откуда экран «Правила по приложениям» берёт сами приложения.
///
/// Источник РАЗНЫЙ на разных платформах, и это не недоделка, а свойство систем:
///   * Android перечисляет установленное сам (`PackageManager` за каналом
///     `com.caramba/vpn`, метод `listInstalledApps`), и выбор там уходит в
///     `VpnService.Builder` именами пакетов;
///   * на десктопе реестра «установленных приложений» в этом смысле нет вовсе:
///     правила ядра совпадают по ИМЕНИ ПРОЦЕССА, и это имя выбирают файловым
///     диалогом (или вводят руками);
///   * на iOS выбирать нечего ни там, ни там — per-app VPN у Apple живёт только
///     в MDM-профиле.
///
/// Оба источника вынесены сюда, а не в экран, по одной причине: у обоих есть
/// платформенный хвост (канал, файловый диалог, чтение `Info.plist`), а у
/// экрана — виджет-тесты. Здесь их подменяют одной строкой override, и экран
/// проверяется без единого канала.
library;

import 'dart:io' show File;

import 'package:caramba_vpn/caramba_vpn.dart' show CarambaVpn, InstalledApp;
import 'package:file_picker/file_picker.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Кто перечисляет установленные приложения.
typedef InstalledAppsLoader = Future<List<InstalledApp>> Function();

/// Кто спрашивает у человека исполняемый файл и возвращает имя процесса.
/// `null` — «диалог закрыли, ничего не выбрали».
typedef ProcessPicker = Future<String?> Function();

/// Перечислитель приложений. Отдельным провайдером, чтобы тест подменил его
/// списком-фикстурой, не трогая [installedAppsProvider] и его состояния
/// загрузки/ошибки.
final installedAppsLoaderProvider = Provider<InstalledAppsLoader>(
  (ref) => CarambaVpn.instance.listInstalledApps,
);

/// Установленные приложения для пикера.
///
/// На не-Android плагин отдаёт пустой список сам (см. `listInstalledApps`), и
/// разбирать платформу здесь второй раз незачем: у экрана на этих платформах
/// другой источник выбора, а не пустой этот.
final installedAppsProvider = FutureProvider<List<InstalledApp>>(
  (ref) => ref.watch(installedAppsLoaderProvider)(),
);

/// Выбор исполняемого файла на десктопе.
final processPickerProvider = Provider<ProcessPicker>((ref) => pickProcessName);

/// Фильтр списка по строке поиска.
///
/// Ищем И по ярлыку, И по имени пакета: человек, который пришёл сюда со списком
/// из другого места (инструкция, лог), помнит `com.android.chrome`, а не
/// «Chrome», и обратное тоже верно. Пустой запрос отдаёт список как есть —
/// порядок платформенный, и пересобирать его нечем.
List<InstalledApp> filterInstalledApps(List<InstalledApp> apps, String query) {
  final q = query.trim().toLowerCase();
  if (q.isEmpty) return apps;
  return apps
      .where(
        (a) =>
            a.label.toLowerCase().contains(q) ||
            a.packageName.toLowerCase().contains(q),
      )
      .toList(growable: false);
}

/// Спрашивает исполняемый файл и сводит его к имени процесса.
///
/// Возвращает ровно то, что ядро сравнивает с `PROCESS-NAME`: basename файла
/// (`chrome.exe`, `firefox`), а на macOS — имя бинаря ИЗ бандла, а не имя самого
/// бандла. Это не придирка: у Telegram бандл называется `Telegram.app`, а
/// процесс — `Telegram`, у многих других они расходятся сильнее.
Future<String?> pickProcessName() async {
  final picked = await FilePicker.platform.pickFiles();
  final files = picked?.files ?? const <PlatformFile>[];
  if (files.isEmpty) return null;
  final path = files.first.path;
  if (path == null || path.isEmpty) return null;
  return processNameFromPath(path);
}

/// Имя процесса по пути к приложению или исполняемому файлу.
///
/// [readText] — чтение файла; подменяется в тестах, чтобы разбор `Info.plist`
/// проверялся без настоящего бандла на диске.
String? processNameFromPath(
  String path, {
  String? Function(String path)? readText,
}) {
  var p = path.replaceAll('\\', '/');
  while (p.length > 1 && p.endsWith('/')) {
    p = p.substring(0, p.length - 1);
  }
  final name = p.split('/').last;
  if (name.isEmpty) return null;
  if (!name.toLowerCase().endsWith('.app')) return name;

  // macOS-бандл: имя процесса лежит в `CFBundleExecutable`.
  final plist = (readText ?? _readTextFile)('$p/Contents/Info.plist');
  final exe = plist == null ? null : _bundleExecutable(plist);
  if (exe != null && exe.isNotEmpty) return exe;
  // Плист не прочитался (права, повреждённый бандл) — остаётся имя бандла без
  // расширения. Оно совпадает с именем процесса у большинства приложений, и
  // это заведомо лучше, чем отказать человеку в выборе целиком.
  return name.substring(0, name.length - 4);
}

/// `<key>CFBundleExecutable</key><string>…</string>` из XML-плиста.
///
/// Регулярное выражение, а не XML-разбор: нужна ОДНА строка из документа,
/// который мы не редактируем, а зависимость ради неё жила бы в приложении
/// вечно. Бинарный плист сюда не подходит и честно даёт null — тогда выше
/// работает запасное имя бандла.
String? _bundleExecutable(String plist) {
  final m = RegExp(
    r'<key>\s*CFBundleExecutable\s*</key>\s*<string>([^<]*)</string>',
  ).firstMatch(plist);
  return m?.group(1)?.trim();
}

String? _readTextFile(String path) {
  try {
    return File(path).readAsStringSync();
  } on Object {
    // Нет файла, нет прав, не текст — всё это означает одно: имени процесса
    // отсюда не взять. Решение (запасное имя бандла) принимает вызывающий.
    return null;
  }
}
