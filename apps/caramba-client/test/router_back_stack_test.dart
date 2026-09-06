// «Назад» с внутреннего экрана не должно закрывать приложение.
//
// Снято на устройстве дважды: «Тип подключения», «Улучшения» и логин
// открывались через `context.go`, а `go` в go_router не переходит, а ЗАМЕНЯЕТ
// стек целиком. В корневом навигаторе оставалась одна страница, системная
// кнопка «Назад» не находила что снять — и Android закрывал приложение. Через
// ~300 мс за приложением уходил и туннель: «Mihomo shutting down», tun0
// исчезал. То есть жест «вернуться назад» рвал защиту.
//
// Здесь проверяется само свойство, а не место вызова: после перехода на
// полноэкранный маршрут `popRoute()` (ровно то, что зовёт системная кнопка)
// обязан вернуть true и показать экран, с которого пришли. Первым же тестом
// зафиксировано и обратное — что делал обычный GoRouter, — чтобы разница была
// видна, а не подразумевалась.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/router/routes.dart';

/// Таблица той же ФОРМЫ, что боевая: гейтовые экраны и полноэкранные пикеры
/// сиблингами шелла с вкладками. Экраны заменены текстом намеренно — проверяется
/// навигация, а не их содержимое.
List<RouteBase> _routes(GlobalKey<NavigatorState> root) => <RouteBase>[
  GoRoute(path: AppRoute.splash, builder: (_, __) => const Text('splash')),
  GoRoute(path: AppRoute.login, builder: (_, __) => const Text('login')),
  GoRoute(path: AppRoute.enroll, builder: (_, __) => const Text('enroll')),
  GoRoute(
    path: AppRoute.protocol,
    parentNavigatorKey: root,
    builder: (_, __) => const Text('protocol'),
  ),
  GoRoute(
    path: AppRoute.siteRules,
    parentNavigatorKey: root,
    builder: (_, __) => const Text('site-rules'),
  ),
  GoRoute(
    path: AppRoute.settingsAutotune,
    parentNavigatorKey: root,
    builder: (_, __) => const Text('autotune'),
  ),
  StatefulShellRoute.indexedStack(
    builder: (_, __, shell) => shell,
    branches: [
      StatefulShellBranch(
        routes: [
          GoRoute(path: AppRoute.home, builder: (_, __) => const Text('home')),
        ],
      ),
      StatefulShellBranch(
        routes: [
          GoRoute(
            path: AppRoute.settings,
            builder: (_, __) => const Text('settings'),
          ),
        ],
      ),
    ],
  ),
];

CarambaRouter _router({
  String at = AppRoute.home,
  bool Function(String location)? canOverlay,
}) {
  final root = GlobalKey<NavigatorState>(debugLabel: 'test-root');
  return CarambaRouter(
    navigatorKey: root,
    initialLocation: at,
    redirect: (_, __) => null,
    canOverlay: canOverlay,
    routes: _routes(root),
  );
}

Future<void> _mount(WidgetTester tester, GoRouter router) async {
  await tester.pumpWidget(MaterialApp.router(routerConfig: router));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('БАГ: обычный GoRouter оставляет «Назад» без стека', (
    tester,
  ) async {
    // Ровно то поведение, что было на устройстве. Тест держит его на виду:
    // разница между go и push здесь не теоретическая, а «приложение закрылось».
    final root = GlobalKey<NavigatorState>(debugLabel: 'plain-root');
    final router = GoRouter(
      navigatorKey: root,
      initialLocation: AppRoute.home,
      routes: _routes(root),
    );
    addTearDown(router.dispose);
    await _mount(tester, router);

    router.go(AppRoute.protocol);
    await tester.pumpAndSettle();
    expect(find.text('protocol'), findsOneWidget);
    expect(find.text('home'), findsNothing, reason: 'шелл заменён целиком');

    expect(
      await router.routerDelegate.popRoute(),
      isFalse,
      reason: 'снимать нечего — система закрывает приложение',
    );
  });

  testWidgets('«Тип подключения»: «Назад» возвращает на Главную', (
    tester,
  ) async {
    final router = _router();
    addTearDown(router.dispose);
    await _mount(tester, router);
    expect(find.text('home'), findsOneWidget);

    router.go(AppRoute.protocol);
    await tester.pumpAndSettle();
    expect(find.text('protocol'), findsOneWidget);

    expect(await router.routerDelegate.popRoute(), isTrue);
    await tester.pumpAndSettle();
    expect(find.text('home'), findsOneWidget);
    expect(find.text('protocol'), findsNothing);
  });

  // Путь «Правил по сайтам» лежит ПОД настройками (`/settings/site-rules`), и
  // это отдельная ловушка: похожий на ветку таба путь так и тянет объявить
  // маршрут внутри ветки — и тогда «Назад» уводило бы в Настройки даже с тех
  // экранов, откуда сюда не приходили. Маршрут объявлен накладным, и тест
  // проверяет именно стек, а не совпадение префикса.
  testWidgets(
    '«Правила по сайтам»: «Назад» возвращает в Настройки, если пришли оттуда',
    (tester) async {
      final router = _router(at: AppRoute.settings);
      addTearDown(router.dispose);
      await _mount(tester, router);

      router.go(AppRoute.siteRules);
      await tester.pumpAndSettle();
      expect(find.text('site-rules'), findsOneWidget);

      expect(await router.routerDelegate.popRoute(), isTrue);
      await tester.pumpAndSettle();
      expect(find.text('settings'), findsOneWidget);
    },
  );

  testWidgets('логин из шелла тоже возвращает назад, а не закрывает', (
    tester,
  ) async {
    // «Войти или подключить панель» в generic-режиме: уходят с него по своей
    // воле, и возвращаться есть куда.
    final router = _router(at: AppRoute.settings);
    addTearDown(router.dispose);
    await _mount(tester, router);

    router.go(AppRoute.login);
    await tester.pumpAndSettle();
    expect(find.text('login'), findsOneWidget);

    expect(await router.routerDelegate.popRoute(), isTrue);
    await tester.pumpAndSettle();
    expect(find.text('settings'), findsOneWidget);
  });

  group('автоподбор', () {
    // Верификатор отметил его отдельно: маршрут лежал ВНУТРИ ветки настроек, и
    // «Назад» с него всегда приводило в Настройки — даже когда открывали с
    // Главной. Второе «Назад» из корня ветки уже закрывало приложение.
    testWidgets('с Главной возвращает на Главную', (tester) async {
      final router = _router();
      addTearDown(router.dispose);
      await _mount(tester, router);

      router.go(AppRoute.settingsAutotune);
      await tester.pumpAndSettle();
      expect(find.text('autotune'), findsOneWidget);

      expect(await router.routerDelegate.popRoute(), isTrue);
      await tester.pumpAndSettle();
      expect(find.text('home'), findsOneWidget);
      expect(find.text('settings'), findsNothing);
    });

    testWidgets('из Настроек возвращает в Настройки', (tester) async {
      final router = _router(at: AppRoute.settings);
      addTearDown(router.dispose);
      await _mount(tester, router);

      router.go(AppRoute.settingsAutotune);
      await tester.pumpAndSettle();

      expect(await router.routerDelegate.popRoute(), isTrue);
      await tester.pumpAndSettle();
      expect(find.text('settings'), findsOneWidget);
    });
  });

  testWidgets('повторный переход не кладёт вторую копию экрана', (
    tester,
  ) async {
    final router = _router();
    addTearDown(router.dispose);
    await _mount(tester, router);

    router.go(AppRoute.protocol);
    await tester.pumpAndSettle();
    router.go(AppRoute.protocol);
    await tester.pumpAndSettle();

    // Одно «Назад» обязано вернуть на Главную, а не на тот же экран.
    expect(await router.routerDelegate.popRoute(), isTrue);
    await tester.pumpAndSettle();
    expect(find.text('home'), findsOneWidget);
  });

  testWidgets('вне шелла маршрут по-прежнему ЗАМЕНЯЕТ показанное', (
    tester,
  ) async {
    // Холодный старт на логине: приложения под экраном ещё нет, и класть
    // поверх нечего. Иначе «Назад» возвращало бы на форму входа, из которой
    // человек только что вышел по ссылке.
    final router = _router(at: AppRoute.login);
    addTearDown(router.dispose);
    await _mount(tester, router);

    router.go(AppRoute.enroll);
    await tester.pumpAndSettle();
    expect(find.text('enroll'), findsOneWidget);
    expect(await router.routerDelegate.popRoute(), isFalse);
  });

  testWidgets('гейт запретил экран — поверх стека его не кладём', (
    tester,
  ) async {
    // Если redirect всё равно уведёт с маршрута, на стеке осталась бы чужая
    // страница, которую «Назад» показало бы человеку.
    final router = _router(canOverlay: (location) => false);
    addTearDown(router.dispose);
    await _mount(tester, router);

    router.go(AppRoute.protocol);
    await tester.pumpAndSettle();
    expect(find.text('protocol'), findsOneWidget);
    expect(await router.routerDelegate.popRoute(), isFalse);
  });

  test('каждый полноэкранный маршрут таблицы объявлен накладным', () {
    // Свойство проверяется по САМОЙ таблице, а не по памяти: новый пикер,
    // добавленный завтра с `parentNavigatorKey`, но забытый в
    // [AppRoute.overlays], откроется без стека — и «Назад» снова закроет
    // приложение. Тест падает раньше, чем это увидит человек.
    final missing = <String>[];
    void walk(List<RouteBase> routes, String parent) {
      for (final route in routes) {
        if (route is GoRoute) {
          final full = route.path.startsWith('/')
              ? route.path
              : '$parent/${route.path}';
          if (route.parentNavigatorKey != null && !AppRoute.isOverlay(full)) {
            missing.add(full);
          }
          walk(route.routes, full);
        } else if (route is ShellRouteBase) {
          walk(route.routes, parent);
        }
      }
    }

    walk(appRoutes(), '');

    expect(
      missing,
      isEmpty,
      reason:
          'маршрут объявлен поверх шелла, но не входит в AppRoute.overlays: '
          'открывать его будут через go, а go заменит стек целиком, и '
          'системная кнопка «Назад» закроет приложение вместе с туннелем',
    );
  });
}
