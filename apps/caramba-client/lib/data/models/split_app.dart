import 'package:caramba_client/widgets/lucide.dart';

/// Режим раздельного туннелирования (caramba-core `Policy.Split`):
/// off = всё через VPN; onlySelected = через VPN только выбранные приложения
/// (`AllowProcesses`); bypassSelected = выбранные мимо VPN (`BypassProcesses`).
enum SplitMode { off, onlySelected, bypassSelected }

extension SplitModeX on SplitMode {
  String get title => switch (this) {
        SplitMode.off => 'Выключено',
        SplitMode.onlySelected => 'Только выбранные',
        SplitMode.bypassSelected => 'Кроме выбранных',
      };

  String get desc => switch (this) {
        SplitMode.off => 'Весь трафик идёт через VPN.',
        SplitMode.onlySelected =>
          'Через VPN идут только отмеченные приложения, остальное напрямую.',
        SplitMode.bypassSelected =>
          'Отмеченные приложения идут напрямую, остальное через VPN.',
      };
}

/// Приложение в списке split-tunnel. `id` = package/bundle id или путь процесса
/// (caramba-core сопоставляет с `BypassProcesses`/`AllowProcesses`).
class SplitApp {
  final String id;
  final String name;
  final String icon; // Lucide glyph (плейсхолдер пакетной иконки)

  const SplitApp({required this.id, required this.name, required this.icon});

  factory SplitApp.fromJson(Map<String, dynamic> json) => SplitApp(
        id: (json['id'] as String?) ?? (json['package'] as String?) ?? '',
        name: (json['name'] as String?) ?? 'App',
        icon: Lucide.appWindow,
      );

  /// Демо-список установленных приложений (desktop/dev).
  static const demo = <SplitApp>[
    SplitApp(id: 'org.telegram', name: 'Telegram', icon: Lucide.send),
    SplitApp(id: 'com.google.chrome', name: 'Chrome', icon: Lucide.globe),
    SplitApp(id: 'com.netflix', name: 'Netflix', icon: Lucide.zap),
    SplitApp(id: 'com.spotify', name: 'Spotify', icon: Lucide.appWindow),
    SplitApp(id: 'ru.sberbank', name: 'СберБанк', icon: Lucide.creditCard),
    SplitApp(id: 'ru.gosuslugi', name: 'Госуслуги', icon: Lucide.shield),
    SplitApp(id: 'com.youtube', name: 'YouTube', icon: Lucide.appWindow),
    SplitApp(id: 'com.whatsapp', name: 'WhatsApp', icon: Lucide.phone),
    SplitApp(id: 'com.discord', name: 'Discord', icon: Lucide.users),
    SplitApp(id: 'ru.yandex.maps', name: 'Яндекс Карты', icon: Lucide.route),
  ];
}
