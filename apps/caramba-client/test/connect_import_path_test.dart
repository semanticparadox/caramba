// Импорт подписки больше не выводит к экрану инвайт-кода.
//
// Владелец вставил ссылку своей подписки и получил требование кода или QR.
// Требование было невыполнимым: коды на живой панели никем не выпускались
// (в панели не было ни одного INSERT в enrollment_codes), а кнопка QR
// показывала тост. Эти тесты стерегут обе двери, которыми человек попадает на
// экран импорта со ссылкой в руках.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/features/connections/connection_import_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/app_theme.dart';

const String _goldenLink =
    'caramba://connect?d=8D5320D405W1GT3MEHR76EHF5XGQ0W1ECNW62WKFC9QQ8BKMDXR0'
    '4M00041061050R3GG28A1C60T3GF0DQM6RBJC5PP4R908DQPWVK5CDT0A6KA32JG0063SHSG';

/// Роутер с тремя интересующими нас адресами. Экраны-заглушки: проверяется
/// КУДА ведёт экран импорта, а не что там нарисовано.
GoRouter _router() => GoRouter(
  initialLocation: AppRoute.connectionImport,
  routes: [
    GoRoute(
      path: AppRoute.connectionImport,
      builder: (context, state) => const ConnectionImportScreen(),
    ),
    GoRoute(
      path: AppRoute.connect,
      builder: (context, state) => const Text('ЭКРАН ПОДТВЕРЖДЕНИЯ'),
    ),
    GoRoute(
      path: AppRoute.enroll,
      builder: (context, state) => const Text('ЭКРАН ИНВАЙТ-КОДА'),
    ),
  ],
);

void main() {
  testWidgets(
    'ссылка подключения в поле импорта ведёт на подтверждение, а не за кодом',
    (tester) async {
      // Экран целиком в ListView, а ListView строит только видимое: на
      // телефонном холсте кнопка «Проверить» просто не существует в дереве.
      tester.view.physicalSize = const Size(1000, 2600);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);

      final router = _router();
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp.router(
            theme: AppTheme.dark(),
            routerConfig: router,
          ),
        ),
      );
      await tester.pumpAndSettle();

      // Бот присылает ссылку подписки и ссылку подключения одним сообщением, и
      // перепутать поля проще простого. Раньше это давало ошибку разбора
      // конфига, из которой человек не мог понять вообще ничего.
      await tester.enterText(
        find.byType(TextField).at(1),
        _goldenLink,
      );
      await tester.tap(find.text('Проверить'));
      await tester.pumpAndSettle();

      expect(
        router.routerDelegate.currentConfiguration.uri.path,
        AppRoute.connect,
      );
      expect(find.text('ЭКРАН ИНВАЙТ-КОДА'), findsNothing);
    },
  );

  test('экран импорта нигде не ведёт на экран инвайт-кода', () {
    // Вторая дверь — предложение после сохранения профиля: раньше единственная
    // кнопка листа делала `context.go('/enroll?panel=...')` БЕЗ кода, то есть
    // приводила ровно в тот тупик. Она стоит за сетевой пробой панели и за
    // модальным листом, поэтому дешёвая и долгоживущая гарантия здесь одна:
    // маршрут энроллмента в этом файле не упоминается вовсе.
    // Путь относительный: `flutter test` работает из корня пакета, тем же
    // приёмом читает корпус векторов csm_corpus_test.dart.
    final source = File(
      'lib/features/connections/connection_import_screen.dart',
    ).readAsStringSync();
    expect(source, isNot(contains('AppRoute.enroll')));
  });
}
