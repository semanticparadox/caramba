// «Аккаунт панели»: одна дверь и ни одной формы.
//
// Экран стоял первым и держал форму подключения — человек упирался в поле
// ввода раньше, чем видел приложение, а строку для этого поля выдаёт оператор.
// Теперь первым идёт шелл с пустой вкладкой «Подключение», а сюда приходят по
// своей воле.
//
// Раунд 5 убрал отсюда и два оставшихся входа. «У меня код приглашения» вёл на
// экран, где нужно было вписать руками адрес панели, а адрес панели приложение
// больше не показывает и не спрашивает. «Войти кодом из бота» ушёл вместе со
// всем режимом кода, вплоть до `POST /login/code` на панели. Осталась ссылка
// caramba:// — единственный способ подключить аккаунт.

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
      builder: (context, state) => const Text('ЭКРАН ЭНРОЛЛМЕНТА'),
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
  testWidgets('экран «Аккаунт панели»: одна дверь и ни одной формы', (
    tester,
  ) async {
    await _pumpLogin(tester);

    expect(find.text('Аккаунт панели'), findsOneWidget);
    expect(find.text('Вставить ссылку подключения'), findsOneWidget);

    // Ни второй двери, ни ввода: способ подключения ровно один.
    expect(find.text('У меня код приглашения'), findsNothing);
    expect(find.text('Войти кодом из бота'), findsNothing);
    expect(find.byType(TextField), findsNothing);
    expect(find.text('Продолжить'), findsNothing);
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

  test('в коде экрана не осталось ни бота, ни режима кода', () {
    final source = File(
      'lib/features/auth/login_screen.dart',
    ).readAsStringSync();
    // Публичная сборка не привязана к оператору: вписанный бот выдавал бы
    // чужой адрес за адрес этой панели. Сейчас бота здесь нет вовсе.
    expect(source, isNot(contains('exa_robot')));
    expect(source, isNot(contains('CARAMBA_BOT_USERNAME')));
    expect(
      source,
      isNot(contains('loginCode')),
      reason: 'вход кодом из бота удалён целиком, включая эндпоинт панели',
    );
  });
}
