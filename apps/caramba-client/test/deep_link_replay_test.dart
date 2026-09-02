// Ссылка холодного старта не должна теряться на гейте роутера.
//
// Воспроизводится баг с эмулятора: `am start -d "carambaconnect://import?url=..."`
// на холодном старте приводил на /login. Причина двойная — гейт уводил
// /connections/import на логин, и сама навигация случалась раньше, чем гейт
// открылся. Здесь проверяется вторая половина: [DeepLinkHandler] помнит ссылку
// и повторяет её, когда роутер готов.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/router/deep_links.dart';
import 'package:caramba_client/router/routes.dart';

/// Роутер с гейтом, который до открытия уводит всё на /login — ровно как
/// `resolveRedirect` до чтения настроек и профилей.
({GoRouter router, void Function() open}) _gatedRouter() {
  var gateOpen = false;
  final router = GoRouter(
    initialLocation: AppRoute.splash,
    redirect: (context, state) {
      if (gateOpen) return null;
      return state.matchedLocation == AppRoute.login ? null : AppRoute.login;
    },
    routes: [
      GoRoute(path: AppRoute.splash, builder: (_, __) => const Text('splash')),
      GoRoute(path: AppRoute.login, builder: (_, __) => const Text('login')),
      GoRoute(
        path: AppRoute.connectionImport,
        builder: (_, state) =>
            Text('import ${state.uri.queryParameters['url']}'),
      ),
    ],
  );
  return (router: router, open: () => gateOpen = true);
}

void main() {
  test('targetOf разбирает оба действия схемы', () {
    expect(
      DeepLinkHandler.targetOf(
        'carambaconnect://import?url=https%3A%2F%2Fsub.example%2Fa',
      ),
      '${AppRoute.connectionImport}?url=https%3A%2F%2Fsub.example%2Fa',
    );
    expect(
      DeepLinkHandler.targetOf(
        'carambaconnect://enroll?panel=https://p.example&code=X1',
      ),
      startsWith(AppRoute.enroll),
    );
    expect(DeepLinkHandler.targetOf('https://sub.example/a'), isNull);
  });

  testWidgets('съеденная гейтом ссылка повторяется, когда роутер готов', (
    tester,
  ) async {
    final gated = _gatedRouter();
    addTearDown(gated.router.dispose);
    await tester.pumpWidget(MaterialApp.router(routerConfig: gated.router));
    await tester.pumpAndSettle();
    expect(find.text('login'), findsOneWidget);

    final handler = DeepLinkHandler(gated.router);
    addTearDown(handler.dispose);
    handler.handle(
      Uri.parse('carambaconnect://import?url=https%3A%2F%2Fsub.example%2Fa'),
    );
    await tester.pumpAndSettle();

    // Гейт закрыт: ссылка съедена, но не забыта.
    expect(find.text('login'), findsOneWidget);
    expect(handler.pendingLocation, isNotNull);

    gated.open();
    handler.replayPending();
    await tester.pumpAndSettle();

    expect(find.text('import https://sub.example/a'), findsOneWidget);
    // Повтор одноразовый: второй вызов уже никуда не уводит.
    expect(handler.pendingLocation, isNull);
    handler.replayPending();
    await tester.pumpAndSettle();
    expect(find.text('import https://sub.example/a'), findsOneWidget);
  });

  testWidgets('ссылка импорта включает generic-режим', (tester) async {
    final gated = _gatedRouter();
    addTearDown(gated.router.dispose);
    gated.open();
    await tester.pumpWidget(MaterialApp.router(routerConfig: gated.router));
    await tester.pumpAndSettle();

    var guestEnabled = false;
    final handler = DeepLinkHandler(
      gated.router,
      onImport: () => guestEnabled = true,
    );
    addTearDown(handler.dispose);
    handler.handle(
      Uri.parse('carambaconnect://import?url=https%3A%2F%2Fsub.example%2Fb'),
    );
    await tester.pumpAndSettle();

    expect(guestEnabled, isTrue);
    expect(find.text('import https://sub.example/b'), findsOneWidget);
  });
}
