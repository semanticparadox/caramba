/// Установленное приложение — то, из чего человек выбирает на экране правил по
/// приложениям (раздельное туннелирование, `CorePolicySplit.apps`).
///
/// МОДЕЛЬ ЖИВЁТ В ПЛАГИНЕ, а не в приложении, потому что источник у неё
/// платформенный: список умеет перечислить только Android (PackageManager), и
/// форма записи — часть контракта канала `com.caramba/vpn`, а не форма экрана.
///
/// Иконка приезжает БАЙТАМИ PNG, а не путём к ресурсу: ресурс принадлежит
/// чужому пакету, и нарисовать его Flutter нечем.
library;

import 'dart:typed_data';

/// Одно приложение в списке выбора.
class InstalledApp {
  const InstalledApp({
    required this.packageName,
    required this.label,
    this.iconPng,
  });

  /// Имя пакета (`com.example.app`) — ровно то, что уходит в политику ядра и в
  /// `VpnService.Builder`.
  final String packageName;

  /// Имя, которое человек видит в лаунчере. Пустым не бывает: платформа
  /// подставляет имя пакета, если ярлыка нет.
  final String label;

  /// Иконка в PNG или null, если платформа её не отдала. Приложение без иконки
  /// ОСТАЁТСЯ в списке: исчезнуть из выбора — цена несоразмерная отсутствию
  /// картинки.
  final Uint8List? iconPng;

  /// Разбор одной записи ответа канала. `null` означает «запись не годится» —
  /// без имени пакета выбирать нечего.
  static InstalledApp? fromChannel(Object? raw) {
    if (raw is! Map) return null;
    final packageName =
        (raw['packageName'] as Object?)?.toString().trim() ?? '';
    if (packageName.isEmpty) return null;
    final label = (raw['label'] as Object?)?.toString().trim() ?? '';
    final icon = raw['iconPng'] as Object?;
    return InstalledApp(
      packageName: packageName,
      label: label.isNotEmpty ? label : packageName,
      iconPng: icon is Uint8List
          ? icon
          : icon is List<int>
          ? Uint8List.fromList(icon)
          : null,
    );
  }

  /// Тождество — имя пакета.
  ///
  /// Ярлык и иконка это ПРЕДСТАВЛЕНИЕ приложения, а не оно само: и то и другое
  /// меняется при смене языка системы или обновлении приложения, а выбор
  /// человека при этом остаётся тем же самым. Сравнение по байтам иконки к тому
  /// же стоило бы килобайтов на каждую строку списка.
  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is InstalledApp && other.packageName == packageName;

  @override
  int get hashCode => packageName.hashCode;

  @override
  String toString() => 'InstalledApp($packageName, $label)';
}

/// Разбор всего ответа канала `listInstalledApps`.
///
/// Негодные записи ПРОПУСКАЮТСЯ, а не роняют разбор: одна битая строка не
/// стоит пустого экрана выбора. Порядок платформенный (она сортирует по
/// ярлыку без учёта регистра) и здесь сохраняется — пересортировка в Dart
/// была бы второй, расходящейся с первой.
List<InstalledApp> installedAppsFromChannel(Object? reply) {
  if (reply is! List) return const <InstalledApp>[];
  final out = <InstalledApp>[];
  for (final entry in reply) {
    final app = InstalledApp.fromChannel(entry);
    if (app != null) out.add(app);
  }
  return out;
}
