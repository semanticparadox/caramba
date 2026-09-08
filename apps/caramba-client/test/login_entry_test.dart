// «Аккаунт панели»: три двери и ни одной формы.
//
// Экран стоял первым и держал форму подключения — человек упирался в поле
// ввода раньше, чем видел приложение, а строку для этого поля выдаёт оператор.
// Теперь первым идёт шелл с пустой вкладкой «Подключение», а сюда приходят по
// своей воле, и лежит здесь только относящееся к аккаунту панели: ссылка
// подключения, код приглашения, вход кодом из бота. Форма одного поля живёт на
// «Добавить подключение» — одна форма означает один ответ на одну строку.
//
// Отдельный гейт на выдуманного бота. В коде стоял
// `defaultValue: 'exa_robot'`, то есть публичная сборка, не привязанная ни к
// какому оператору, предлагала открыть КОНКРЕТНОГО чужого бота и выдавала его
// за бота этой панели. Адрес публикует оператор; пусто — говорим словами.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/features/auth/login_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/app_theme.dart';

GoRouter _router() => GoRouter(
  initialLocation: AppRoute.login,
  routes: [
    GoRoute(
      path: AppRoute.login,
      builder: (context, state) => const LoginScreen(),
    ),
    GoRoute(
      path: AppRoute.enroll,
      builder: (context, state) => const Text('ЭКРАН ИНВАЙТ-КОДА'),
    ),
    GoRoute(
      path: AppRoute.connect,
      builder: (context, state) => const Text('ЭКРАН ПОДТВЕРЖДЕНИЯ'),
    ),
    GoRoute(
      path: AppRoute.home,
      builder: (context, state) => const Text('ГЛАВНАЯ'),
    ),
  ],
);

Future<GoRouter> _pumpLogin(WidgetTester tester) async {
  tester.view.physicalSize = const Size(1000, 2600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  final router = _router();
  await tester.pumpWidget(
    ProviderScope(
      child: MaterialApp.router(theme: AppTheme.dark(), routerConfig: router),
    ),
  );
  await tester.pump();
  return router;
}

void main() {
  testWidgets('экран «Аккаунт панели»: три двери и ни одной формы', (
    tester,
  ) async {
    await _pumpLogin(tester);

    expect(find.text('Аккаунт панели'), findsOneWidget);
    expect(find.text('Вставить ссылку подключения'), findsOneWidget);
    expect(find.text('У меня код приглашения'), findsOneWidget);

    // Панели в тестовой сборке нет, значит нет и блока кода из бота: без
    // панели ни бота, ни кодов не существует.
    expect(find.byType(TextField), findsNothing);
    // Форма подключения уехала на «Добавить подключение» целиком — вместе с
    // «Продолжить» и её «Ещё».
    expect(find.text('Продолжить'), findsNothing);
    expect(find.text('Ещё'), findsNothing);
  });

  testWidgets('код приглашения ведёт на экран энроллмента', (tester) async {
    final router = await _pumpLogin(tester);

    await tester.tap(find.text('У меня код приглашения'));
    await tester.pumpAndSettle();

    expect(
      router.routerDelegate.currentConfiguration.uri.path,
      AppRoute.enroll,
    );
  });

  testWidgets('ссылка подключения ведёт на экран подтверждения', (
    tester,
  ) async {
    final router = await _pumpLogin(tester);

    await tester.tap(find.text('Вставить ссылку подключения'));
    await tester.pumpAndSettle();

    expect(
      router.routerDelegate.currentConfiguration.uri.path,
      AppRoute.connect,
    );
  });

  test('в коде первого экрана нет выдуманного бота', () {
    final source = File(
      'lib/features/auth/login_screen.dart',
    ).readAsStringSync();
    expect(
      source,
      isNot(contains('exa_robot')),
      reason:
          'публичная сборка не привязана к оператору: вписанный бот выдавал бы '
          'чужой адрес за адрес этой панели',
    );
    expect(
      source,
      contains("String.fromEnvironment('CARAMBA_BOT_USERNAME')"),
      reason: 'адрес бота задаёт брендированная сборка, а не константа в коде',
    );
  });
}
