// Экраны обновлений: баннер на «Подключении», «Обновления», «Нужно
// обновиться».
//
// Состояние подставляется готовым (без сети): проверяется, что каждый экран
// показывает и КОГДА. Баннер пуст без новой версии и после «Позже»; экран
// «Обновления» показывает отложенную версию всё равно; «Нужно обновиться» не
// имеет крестика.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/token_store.dart';
import 'package:caramba_client/features/updates/update_banner.dart';
import 'package:caramba_client/features/updates/update_installer.dart';
import 'package:caramba_client/features/updates/update_required_screen.dart';
import 'package:caramba_client/features/updates/updates_screen.dart';
import 'package:caramba_client/state/app_update_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Нотифаер с готовым состоянием: ни таймеров, ни сети.
class _FixedUpdate extends AppUpdateNotifier {
  final AppUpdateState fixed;
  _FixedUpdate(this.fixed);

  @override
  AppUpdateState build() => fixed;

  @override
  Future<void> check() async {
    state = state.copyWith(checkedAt: DateTime.now());
  }
}

class _NoopInstaller implements UpdateInstaller {
  int calls = 0;
  @override
  Future<String> install(AppVersionInfo info) async {
    calls += 1;
    return 'установщик вызван';
  }
}

const _installed = InstalledVersion(version: '1.0.0', build: 109);

AppVersionInfo _latest({int build = 110, int minBuild = 0}) => AppVersionInfo(
  platform: 'android',
  version: '1.0.0',
  build: build,
  minBuild: minBuild,
  downloadUrl: 'https://app.example.com/downloads/a.apk',
  size: 3 * 1024 * 1024,
  notes: 'Трей на Windows\nАвтозапуск',
);

/// Баннер — виджет внутри списка, экраны — сами по себе `home`.
Widget _app(
  Widget child,
  AppUpdateState state, {
  _NoopInstaller? installer,
  bool screen = false,
}) => ProviderScope(
  overrides: [
    appUpdateProvider.overrideWith(() => _FixedUpdate(state)),
    updateInstallerProvider.overrideWithValue(installer ?? _NoopInstaller()),
    apiClientProvider.overrideWithValue(
      ApiClient(tokens: TokenStore(), baseUrl: 'https://panel.example'),
    ),
  ],
  child: MaterialApp(
    theme: AppTheme.dark(),
    home: screen ? child : Scaffold(body: ListView(children: [child])),
  ),
);

void main() {
  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  group('UpdateBanner', () {
    testWidgets('пуст, пока обновляться не на что', (tester) async {
      await tester.pumpWidget(
        _app(const UpdateBanner(), const AppUpdateState(installed: _installed)),
      );
      expect(find.textContaining('Доступна версия'), findsNothing);
    });

    testWidgets('показывает версию и первую строку «что нового»', (
      tester,
    ) async {
      await tester.pumpWidget(
        _app(
          const UpdateBanner(),
          AppUpdateState(installed: _installed, latest: _latest()),
        ),
      );
      expect(find.text('Скачать'), findsOneWidget);
      expect(find.text('Позже'), findsOneWidget);
      final text = tester.widget<Text>(find.textContaining('Доступна версия'));
      expect(text.data, contains('1.0.0 (110)'));
      expect(text.data, contains('Трей на Windows'));
      expect(text.data, isNot(contains('Автозапуск')));
    });

    testWidgets('«Позже» прячет баннер', (tester) async {
      await tester.pumpWidget(
        _app(
          const UpdateBanner(),
          AppUpdateState(installed: _installed, latest: _latest()),
        ),
      );
      await tester.tap(find.text('Позже'));
      await tester.pump();
      expect(find.textContaining('Доступна версия'), findsNothing);
    });

    testWidgets('«Скачать» зовёт установщик и показывает его ответ', (
      tester,
    ) async {
      final installer = _NoopInstaller();
      await tester.pumpWidget(
        _app(
          const UpdateBanner(),
          AppUpdateState(installed: _installed, latest: _latest()),
          installer: installer,
        ),
      );
      await tester.tap(find.text('Скачать'));
      await tester.pump();
      expect(installer.calls, 1);
      expect(find.text('установщик вызван'), findsOneWidget);
    });

    testWidgets('обязательное обновление баннером не показывается', (
      tester,
    ) async {
      await tester.pumpWidget(
        _app(
          const UpdateBanner(),
          AppUpdateState(installed: _installed, latest: _latest(minBuild: 110)),
        ),
      );
      expect(find.textContaining('Доступна версия'), findsNothing);
    });
  });

  group('UpdatesScreen', () {
    testWidgets('показывает обе версии, заметки и кнопку скачать', (
      tester,
    ) async {
      await tester.pumpWidget(
        _app(
          const UpdatesScreen(),
          AppUpdateState(
            installed: _installed,
            latest: _latest(),
            // Даже отложенная «Позже» версия здесь видна.
            dismissedBuild: 110,
          ),
          screen: true,
        ),
      );
      expect(find.text('1.0.0 (109)'), findsOneWidget);
      expect(find.text('1.0.0 (110)'), findsOneWidget);
      expect(find.text('Что нового'.toUpperCase()), findsOneWidget);
      expect(find.textContaining('Автозапуск'), findsOneWidget);
      expect(find.text('Скачать обновление'), findsOneWidget);
      expect(find.text('Проверить'), findsOneWidget);
      expect(find.text('3.0 МБ'), findsOneWidget);
    });

    testWidgets('последняя версия: без кнопки скачать', (tester) async {
      await tester.pumpWidget(
        _app(
          const UpdatesScreen(),
          AppUpdateState(
            installed: _installed,
            latest: _latest(build: 109),
            checkedAt: DateTime.now(),
          ),
          screen: true,
        ),
      );
      expect(find.text('У вас последняя версия.'), findsOneWidget);
      expect(find.text('Скачать обновление'), findsNothing);
    });

    testWidgets('ошибка проверки названа словами', (tester) async {
      await tester.pumpWidget(
        _app(
          const UpdatesScreen(),
          const AppUpdateState(installed: _installed, error: 'сеть'),
          screen: true,
        ),
      );
      expect(find.textContaining('Не удалось проверить: сеть'), findsOneWidget);
    });
  });

  group('UpdateRequiredScreen', () {
    testWidgets('называет обе версии, без крестика', (tester) async {
      await tester.pumpWidget(
        _app(
          const UpdateRequiredScreen(),
          AppUpdateState(installed: _installed, latest: _latest(minBuild: 110)),
          screen: true,
        ),
      );
      expect(find.text('Нужно обновиться'), findsOneWidget);
      expect(find.textContaining('1.0.0 (109)'), findsOneWidget);
      expect(find.textContaining('1.0.0 (110)'), findsOneWidget);
      expect(find.text('Скачать обновление'), findsOneWidget);
      expect(find.text('Проверить ещё раз'), findsOneWidget);
      expect(find.byType(IconBtn), findsNothing);
    });
  });

  test('статус экрана «Обновления» без панели зовёт в бота', () {
    expect(
      updatesStatusText(
        const AppUpdateState(installed: _installed),
        hasPanel: false,
      ),
      contains('/apk'),
    );
  });
}
