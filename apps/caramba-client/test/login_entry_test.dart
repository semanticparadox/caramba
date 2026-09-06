// Первый экран приложения: одно поле, а не три раздела.
//
// Экран входа спрашивал у человека, к какому из разделов — «Подписка»,
// «Панель Caramba», «Код из бота» — относится строка, которую ему прислал
// оператор. Ответить на это нельзя: по виду строки разделы не различаются.
// Теперь на входе одно поле, а редкие входы (код приглашения, вход кодом из
// бота, файл) живут под «Ещё» — не удалены, но и не на дороге.
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
  testWidgets('на первом экране одно поле и одна кнопка', (tester) async {
    await _pumpLogin(tester);

    expect(find.byType(TextField), findsOneWidget);
    expect(find.text('Продолжить'), findsOneWidget);

    // Разделов больше нет: выбор между ними и был вопросом без ответа.
    expect(find.text('Подписка'), findsNothing);
    expect(find.text('Панель Caramba'), findsNothing);
    expect(find.text('Код из бота'), findsNothing);
    expect(find.text('Добавить подписку'), findsNothing);
  });

  testWidgets('редкие входы не удалены — они под «Ещё»', (tester) async {
    // Исчезнувшая функция неотличима от несуществующей: человек с кодом
    // приглашения в руках должен его найти, просто не на главной дороге.
    await _pumpLogin(tester);

    expect(find.text('У меня код приглашения'), findsNothing);
    await tester.tap(find.text('Ещё'));
    await tester.pump();
    expect(find.text('У меня код приглашения'), findsOneWidget);
    expect(find.text('Из файла'), findsOneWidget);
  });

  testWidgets('код приглашения ведёт на экран энроллмента', (tester) async {
    final router = await _pumpLogin(tester);

    await tester.tap(find.text('Ещё'));
    await tester.pump();
    await tester.tap(find.text('У меня код приглашения'));
    await tester.pumpAndSettle();

    expect(
      router.routerDelegate.currentConfiguration.uri.path,
      AppRoute.enroll,
    );
  });

  test('в коде первого экрана нет выдуманного бота', () {
    final source = File('lib/features/auth/login_screen.dart').readAsStringSync();
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
