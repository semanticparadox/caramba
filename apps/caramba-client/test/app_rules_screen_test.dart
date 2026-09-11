// Экран «Правила по приложениям».
//
// Проверяется ровно то, что легко сломать молча:
//   * выбор режима правит состояние ядра (и режим здесь ТОТ ЖЕ, что у правил
//     по сайтам, — в ядре `Policy.Split.Mode` один);
//   * список выбранного показывает и даёт убрать то, что реально уйдёт ядру;
//   * пикер установленных приложений ищет и добавляет по имени пакета;
//   * на десктопе источник другой (файловый диалог и имя процесса руками), и
//     пикера установленных там нет вовсе;
//   * на iOS экран ЗАКРЫТ с причиной, а не спрятан и не притворяется рабочим;
//   * режим «только список», который сейчас не доезжает до ядра без единого
//     сайта, честно говорит об этом баннером, а не молчит.

import 'package:flutter/foundation.dart'
    show TargetPlatform, debugDefaultTargetPlatformOverride;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_vpn/caramba_vpn.dart' show InstalledApp;

import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/features/settings/app_rules_screen.dart';
import 'package:caramba_client/features/settings/settings_screen.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/installed_apps_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';

import 'support/fake_core.dart';

const _installed = <InstalledApp>[
  InstalledApp(packageName: 'com.android.chrome', label: 'Chrome'),
  InstalledApp(packageName: 'org.telegram.messenger', label: 'Telegram'),
];

/// Хост экрана: ядро подменено, список приложений — фикстура (канала в тестах
/// нет), конфигурация задаётся снимком.
Widget _host(
  Widget screen, {
  CoreConfig config = const CoreConfig(),
  List<InstalledApp> installed = _installed,
  Future<String?> Function()? processPicker,
  void Function(WidgetRef ref)? capture,
}) {
  return ProviderScope(
    overrides: <Override>[
      vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
      coreConfigProvider.overrideWith(
        (ref) => CoreConfigNotifier()..state = config,
      ),
      installedAppsLoaderProvider.overrideWithValue(() async => installed),
      if (processPicker != null)
        processPickerProvider.overrideWithValue(processPicker),
    ],
    child: MaterialApp(
      theme: AppTheme.dark(),
      home: Consumer(
        builder: (_, ref, __) {
          capture?.call(ref);
          return screen;
        },
      ),
    ),
  );
}

void _phone(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 9000)
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
}

/// Платформенная ветка выбирается ПЛАТФОРМОЙ, а не шириной окна.
///
/// Возврат переопределения стоит в `finally`, а не в `tearDown`: flutter_test
/// проверяет «отладочные переменные foundation вернули в исходное» ещё ДО
/// tearDown, и наследивший тест уронил бы следующий, а не себя.
Future<void> _onPlatform(
  WidgetTester tester,
  TargetPlatform platform,
  Size size,
  Future<void> Function() body,
) async {
  debugDefaultTargetPlatformOverride = platform;
  tester.view
    ..physicalSize = size
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
  try {
    await body();
  } finally {
    debugDefaultTargetPlatformOverride = null;
  }
}

Future<void> _desktop(WidgetTester tester, Future<void> Function() body) =>
    _onPlatform(tester, TargetPlatform.macOS, const Size(1600, 2200), body);

Future<void> _ios(WidgetTester tester, Future<void> Function() body) =>
    _onPlatform(tester, TargetPlatform.iOS, const Size(780, 9000), body);

void main() {
  group('режим', () {
    testWidgets('выбор режима правит конфигурацию ядра', (tester) async {
      _phone(tester);
      late WidgetRef ref;
      await tester.pumpWidget(
        _host(const AppRulesScreen(), capture: (r) => ref = r),
      );
      await tester.pump();

      expect(ref.read(coreConfigProvider).splitMode, SplitMode.off);
      await tester.tap(find.text(SplitMode.bypassSelected.appsTitle));
      await tester.pump();
      expect(ref.read(coreConfigProvider).splitMode, SplitMode.bypassSelected);
    });

    // Общий режим — свойство ядра, а не экрана, и умолчать о нём значило бы
    // дать человеку переставить правила по сайтам, ничего ему не сказав.
    testWidgets('экран говорит, что режим общий с правилами по сайтам', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(_host(const AppRulesScreen()));
      await tester.pump();
      expect(find.textContaining('Режим общий'), findsOneWidget);
    });

    testWidgets('при выключенном режиме списка на экране нет', (tester) async {
      _phone(tester);
      await tester.pumpWidget(_host(const AppRulesScreen()));
      await tester.pump();
      expect(find.byKey(const ValueKey('app-rules-add')), findsNothing);
      expect(find.text('В списке'), findsNothing);
    });
  });

  group('список выбранного', () {
    testWidgets('показывает ярлык системы и имя пакета', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _host(
          const AppRulesScreen(),
          config: const CoreConfig(
            splitMode: SplitMode.bypassSelected,
            splitApps: {'com.android.chrome'},
          ),
        ),
      );
      await tester.pump();

      expect(find.text('Chrome'), findsOneWidget);
      expect(find.text('com.android.chrome'), findsOneWidget);
    });

    // Неизвестное системе имя (удалённый пакет, имя процесса с десктопа)
    // остаётся в списке под собственным именем: спрятать выбор человека
    // молча хуже, чем показать строку без ярлыка.
    testWidgets('незнакомое имя показывается как есть', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _host(
          const AppRulesScreen(),
          config: const CoreConfig(
            splitMode: SplitMode.bypassSelected,
            splitApps: {'com.deleted.app'},
          ),
        ),
      );
      await tester.pump();
      expect(find.text('com.deleted.app'), findsOneWidget);
    });

    testWidgets('удаление убирает приложение из состояния', (tester) async {
      _phone(tester);
      late WidgetRef ref;
      await tester.pumpWidget(
        _host(
          const AppRulesScreen(),
          config: const CoreConfig(
            splitMode: SplitMode.bypassSelected,
            splitApps: {'com.android.chrome', 'org.telegram.messenger'},
          ),
          capture: (r) => ref = r,
        ),
      );
      await tester.pump();

      await tester.tap(
        find.byKey(const ValueKey('app-rules-remove-com.android.chrome')),
      );
      await tester.pump();
      expect(ref.read(coreConfigProvider).splitApps, {
        'org.telegram.messenger',
      });
    });

    testWidgets('пустой список говорит, что правило ничего не меняет', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _host(
          const AppRulesScreen(),
          config: const CoreConfig(splitMode: SplitMode.bypassSelected),
        ),
      );
      await tester.pump();
      expect(find.byKey(const ValueKey('app-rules-empty')), findsOneWidget);
    });
  });

  group('пикер установленных приложений (Android)', () {
    testWidgets('добавляет выбранное и ищет по строке', (tester) async {
      _phone(tester);
      late WidgetRef ref;
      await tester.pumpWidget(
        _host(
          const AppRulesScreen(),
          config: const CoreConfig(splitMode: SplitMode.bypassSelected),
          capture: (r) => ref = r,
        ),
      );
      await tester.pump();

      await tester.tap(find.byKey(const ValueKey('app-rules-add')));
      await tester.pumpAndSettle();

      expect(find.text('Chrome'), findsOneWidget);
      expect(find.text('Telegram'), findsOneWidget);

      await tester.enterText(
        find.byKey(const ValueKey('app-rules-search')),
        'teleg',
      );
      await tester.pump();
      expect(find.text('Chrome'), findsNothing);

      await tester.tap(
        find.byKey(const ValueKey('app-rules-pick-org.telegram.messenger')),
      );
      await tester.pump();
      expect(ref.read(coreConfigProvider).splitApps, {
        'org.telegram.messenger',
      });
    });

    // Пустой список системы не притворяется «ничего не найдено»: это разные
    // ответы, и второй отправил бы человека править запрос, которого нет.
    testWidgets('пустой ответ системы объяснён отдельно', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _host(
          const AppRulesScreen(),
          config: const CoreConfig(splitMode: SplitMode.bypassSelected),
          installed: const <InstalledApp>[],
        ),
      );
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('app-rules-add')));
      await tester.pumpAndSettle();

      expect(
        find.byKey(const ValueKey('app-rules-picker-empty')),
        findsOneWidget,
      );
      expect(find.textContaining('не умеет перечислять'), findsOneWidget);
    });
  });

  group('десктоп', () {
    testWidgets('файловый диалог даёт имя процесса', (tester) async {
      await _desktop(tester, () async {
        late WidgetRef ref;
        await tester.pumpWidget(
          _host(
            const AppRulesScreen(),
            config: const CoreConfig(splitMode: SplitMode.bypassSelected),
            processPicker: () async => 'chrome.exe',
            capture: (r) => ref = r,
          ),
        );
        await tester.pump();

        await tester.tap(find.byKey(const ValueKey('app-rules-add')));
        await tester.pumpAndSettle();
        expect(ref.read(coreConfigProvider).splitApps, {'chrome.exe'});
      });
    });

    testWidgets('имя процесса вводится руками', (tester) async {
      await _desktop(tester, () async {
        late WidgetRef ref;
        await tester.pumpWidget(
          _host(
            const AppRulesScreen(),
            config: const CoreConfig(splitMode: SplitMode.bypassSelected),
            capture: (r) => ref = r,
          ),
        );
        await tester.pump();

        await tester.enterText(
          find.byKey(const ValueKey('app-rules-process-field')),
          '  Telegram  ',
        );
        await tester.tap(find.byKey(const ValueKey('app-rules-add-typed')));
        await tester.pump();
        expect(ref.read(coreConfigProvider).splitApps, {'Telegram'});
      });
    });

    // Пикера установленных на десктопе нет физически: реестра программ в этом
    // смысле в системе нет, и показывать там пустой список значило бы обещать
    // источник, которого не существует.
    testWidgets('пикера установленных на десктопе нет', (tester) async {
      await _desktop(tester, () async {
        await tester.pumpWidget(
          _host(
            const AppRulesScreen(),
            config: const CoreConfig(splitMode: SplitMode.bypassSelected),
          ),
        );
        await tester.pump();
        expect(find.text('Добавить приложение'), findsNothing);
        expect(
          find.byKey(const ValueKey('app-rules-process-field')),
          findsOneWidget,
        );
      });
    });
  });

  group('iOS', () {
    testWidgets('экран закрыт с причиной, а не спрятан', (tester) async {
      await _ios(tester, () async {
        await tester.pumpWidget(_host(const AppRulesScreen()));
        await tester.pump();

        expect(
          find.byKey(const ValueKey('app-rules-ios-locked')),
          findsOneWidget,
        );
        expect(find.textContaining('MDM'), findsOneWidget);
        // Ни одного переключателя режима: он бы ничего не включил.
        for (final m in SplitMode.values) {
          expect(
            find.byKey(ValueKey('app-rules-mode-${m.name}')),
            findsNothing,
          );
        }
        expect(find.byKey(const ValueKey('app-rules-add')), findsNothing);
      });
    });
  });

  group('честность режима «только список»', () {
    // `_split` в core_policy_mapping понижает allow без единой цели до `off`:
    // пустой «только выбранные» увёл бы мимо туннеля весь трафик. Значит выбор
    // ОДНИХ приложений сейчас до ядра не доходит — и экран обязан сказать это.
    testWidgets('без единого сайта баннер предупреждает', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _host(
          const AppRulesScreen(),
          config: const CoreConfig(
            splitMode: SplitMode.onlySelected,
            splitApps: {'com.android.chrome'},
          ),
        ),
      );
      await tester.pump();
      expect(
        find.byKey(const ValueKey('app-rules-allow-needs-sites')),
        findsOneWidget,
      );
    });

    testWidgets('с сайтом в списке предупреждения нет', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _host(
          const AppRulesScreen(),
          config: const CoreConfig(
            splitMode: SplitMode.onlySelected,
            splitApps: {'com.android.chrome'},
            allowDomains: 'youtube.com',
          ),
        ),
      );
      await tester.pump();
      expect(
        find.byKey(const ValueKey('app-rules-allow-needs-sites')),
        findsNothing,
      );
    });
  });

  group('строка в Настройках', () {
    testWidgets('называется именем экрана и несёт сводку', (tester) async {
      _phone(tester);
      const cfg = CoreConfig(
        splitMode: SplitMode.bypassSelected,
        splitApps: {'com.android.chrome'},
      );
      await tester.pumpWidget(
        _host(const Scaffold(body: SettingsScreen()), config: cfg),
      );
      await tester.pump();
      await tester.pump();

      expect(find.text(kAppRulesTitle), findsOneWidget);
      expect(find.text(appRulesSummary(cfg)), findsOneWidget);
    });
  });
}
