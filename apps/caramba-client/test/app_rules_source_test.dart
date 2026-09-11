// Источник выбора приложений и подписи экрана «Правила по приложениям».
//
// Здесь проверяется то, что живёт БЕЗ экрана: фильтр пикера, сведение
// выбранного файла к имени процесса (то самое, по которому ядро сравнивает
// `PROCESS-NAME`) и сводка строки в Настройках. Виджет-часть — в
// app_rules_screen_test.dart.

import 'package:caramba_vpn/caramba_vpn.dart' show InstalledApp;
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/desktop/desktop_overlay_page.dart';
import 'package:caramba_client/features/settings/app_rules_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/installed_apps_state.dart';

const _apps = <InstalledApp>[
  InstalledApp(packageName: 'com.android.chrome', label: 'Chrome'),
  InstalledApp(packageName: 'org.telegram.messenger', label: 'Telegram'),
  InstalledApp(packageName: 'ru.bank.app', label: 'Банк'),
];

void main() {
  group('фильтр пикера', () {
    test('пустой запрос отдаёт список как есть, в том же порядке', () {
      expect(filterInstalledApps(_apps, '   '), _apps);
    });

    test('ищет по ярлыку без учёта регистра', () {
      final found = filterInstalledApps(_apps, 'TELEG');
      expect(found.map((a) => a.packageName), ['org.telegram.messenger']);
    });

    // Имя пакета ищется наравне с ярлыком: человек, пришедший со списком из
    // инструкции или лога, помнит `com.android.chrome`, а не «Chrome».
    test('ищет по имени пакета', () {
      final found = filterInstalledApps(_apps, 'com.android');
      expect(found.map((a) => a.label), ['Chrome']);
    });

    test('ничего не найдено — пустой список, а не весь', () {
      expect(filterInstalledApps(_apps, 'zzz'), isEmpty);
    });
  });

  group('имя процесса по выбранному файлу', () {
    test('Windows: basename с расширением', () {
      expect(
        processNameFromPath(r'C:\Program Files\Google\Chrome\chrome.exe'),
        'chrome.exe',
      );
    });

    test('Linux: basename без расширения', () {
      expect(processNameFromPath('/usr/bin/firefox'), 'firefox');
    });

    // У бандла и процесса имена расходятся чаще, чем кажется, поэтому
    // единственный правильный источник — CFBundleExecutable внутри бандла.
    test('macOS: имя бинаря берётся из Info.plist бандла', () {
      String? read(String path) {
        expect(path, '/Applications/Some Browser.app/Contents/Info.plist');
        return '''
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Some Browser</string>
  <key>CFBundleExecutable</key><string>SomeBrowserHelperless</string>
</dict></plist>
''';
      }

      expect(
        processNameFromPath('/Applications/Some Browser.app/', readText: read),
        'SomeBrowserHelperless',
      );
    });

    test('плист не прочитался — остаётся имя бандла без .app', () {
      expect(
        processNameFromPath(
          '/Applications/Telegram.app',
          readText: (_) => null,
        ),
        'Telegram',
      );
    });

    test('пустой путь не даёт пустого имени', () {
      expect(processNameFromPath(''), isNull);
    });
  });

  group('список выбранного', () {
    test('добавление не переключает: второй раз не удаляет', () {
      final n = CoreConfigNotifier()
        ..addSplitApp('chrome.exe')
        ..addSplitApp('chrome.exe');
      expect(n.state.splitApps, {'chrome.exe'});
    });

    test('пробелы срезаются, пустая строка не добавляется', () {
      final n = CoreConfigNotifier()
        ..addSplitApp('  Telegram  ')
        ..addSplitApp('   ');
      expect(n.state.splitApps, {'Telegram'});
    });

    test('удаление убирает ровно одно имя', () {
      final n = CoreConfigNotifier()
        ..addSplitApp('a')
        ..addSplitApp('b')
        ..removeSplitApp('a');
      expect(n.state.splitApps, {'b'});
    });

    test('переключатель пикера добавляет и убирает', () {
      final n = CoreConfigNotifier()..toggleSplitApp('com.android.chrome');
      expect(n.state.splitApps, {'com.android.chrome'});
      n.toggleSplitApp('com.android.chrome');
      expect(n.state.splitApps, isEmpty);
    });
  });

  group('сводка строки в Настройках', () {
    test('выключено — без выдуманных счётчиков', () {
      expect(
        appRulesSummary(const CoreConfig(splitApps: {'a', 'b'})),
        'Выключено: списков нет',
      );
    });

    test('«кроме списка» считает то, что уйдёт ядру', () {
      expect(
        appRulesSummary(
          const CoreConfig(
            splitMode: SplitMode.bypassSelected,
            splitApps: {'a', 'b'},
          ),
        ),
        'Кроме списка · 2 приложения мимо VPN',
      );
    });

    test('«только список» называет своё направление', () {
      expect(
        appRulesSummary(
          const CoreConfig(splitMode: SplitMode.onlySelected, splitApps: {'a'}),
        ),
        'Только список · 1 приложение через VPN',
      );
    });

    test('склонение на 5 и на 11', () {
      expect(
        appRulesSummary(
          CoreConfig(
            splitMode: SplitMode.bypassSelected,
            splitApps: {for (var i = 0; i < 5; i++) 'app$i'},
          ),
        ),
        contains('5 приложений'),
      );
      expect(
        appRulesSummary(
          CoreConfig(
            splitMode: SplitMode.bypassSelected,
            splitApps: {for (var i = 0; i < 11; i++) 'app$i'},
          ),
        ),
        contains('11 приложений'),
      );
    });
  });

  // Маршрут накладной, как и у правил по сайтам: экран открывают со строки
  // настроек, и «Назад» обязано возвращать туда, откуда пришли. Забыть его в
  // `AppRoute.overlays` значит уронить стек молча — `go` заменил бы шелл, и
  // системная кнопка «Назад» закрыла бы приложение вместе с туннелем.
  group('маршрут', () {
    test('открывается поверх приложения', () {
      expect(AppRoute.isOverlay(AppRoute.appRules), isTrue);
    });

    test('на десктопе это широкая панель, как у списков сайтов', () {
      expect(
        presentationFor(AppRoute.appRules),
        presentationFor(AppRoute.siteRules),
      );
      expect(presentationFor(AppRoute.appRules), DesktopPresentation.panelWide);
    });
  });

  // Подписи режима на двух экранах РАЗНЫЕ, потому что величина одна, а вопросы
  // разные: там судьба сайтов, здесь — приложений.
  test('режим подписан по-своему на каждом из двух экранов', () {
    for (final m in SplitMode.values) {
      expect(m.appsTitle, isNotEmpty);
      expect(m.appsDesc, isNotEmpty);
    }
    expect(
      SplitMode.onlySelected.appsTitle,
      isNot(SplitMode.onlySelected.title),
    );
    expect(
      SplitMode.bypassSelected.appsTitle.toLowerCase(),
      isNot(contains('сайт')),
    );
  });
}
