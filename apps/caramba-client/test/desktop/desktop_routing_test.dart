// Таблица маршрутов на двух платформах: одна и та же, показанная по-разному.
//
// Что здесь сторожится и почему именно это.
//
//   * ТАБЛИЦА НЕ РАЗЪЕХАЛАСЬ. Десктоп не добавляет ни одного маршрута и ни
//     одной ветки: шелл по-прежнему один, веток три, и порядок тот же, что
//     сверяет `shell_tabs_test`. Ошибка здесь означала бы вторую таблицу для
//     второй платформы — то самое, чего решение избегает.
//   * МОБИЛЬНОЕ ПОВЕДЕНИЕ НЕ ТРОНУТО. Замена `builder:` на `pageBuilder:` —
//     самое опасное изменение задачи: она проходит по тридцати маршрутам
//     разом. На мобильном страница обязана остаться ровно той `MaterialPage`,
//     которую построил бы сам go_router, с тем же ключом и тем же ребёнком.
//     Иначе поехали бы переходы, стек «Назад» и восстановление состояния — и
//     ни один существующий тест этого бы не заметил, потому что все они
//     смотрят на содержимое экрана, а не на тип страницы.
//   * НА ДЕСКТОПЕ СТРАНИЦА ДРУГАЯ. Накладной маршрут перестаёт быть
//     полноэкранной страницей и становится панелью со скримом. Проверяем сам
//     факт развилки: `MaterialPage` на десктопе означала бы, что панель не
//     подключилась и экран снова закрывает приложение целиком.
//   * ВЕТКИ МЕНЯЮТ ЭКРАН, А НЕ МАРШРУТ. Профиль и Настройки на десктопе — свои
//     файлы; путь, ветка и её место в шелле остались прежними.
//
// Платформа переопределяется точечно внутри тестов и возвращается в `null`:
// flutter_test проверяет отладочные переменные foundation ДО tearDown, и
// наследивший тест уронил бы следующий, а не себя.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/features/auth/login_screen.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/features/profile/profile_desktop.dart';
import 'package:caramba_client/features/profile/profile_screen.dart';
import 'package:caramba_client/features/servers/servers_screen.dart';
import 'package:caramba_client/features/settings/settings_desktop.dart';
import 'package:caramba_client/features/settings/settings_screen.dart';
import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/router/routes.dart';

/// Маршруты таблицы по полному пути.
Map<String, GoRoute> _routesByPath() {
  final out = <String, GoRoute>{};
  void walk(List<RouteBase> routes, String parent) {
    for (final route in routes) {
      if (route is GoRoute) {
        final full = route.path.startsWith('/')
            ? route.path
            : '$parent/${route.path}';
        out[full] = route;
        walk(route.routes, full);
      } else if (route is ShellRouteBase) {
        walk(route.routes, parent);
      }
    }
  }

  walk(appRoutes(), '');
  return out;
}

/// Ветки шелла в порядке объявления.
List<StatefulShellBranch> _branches() =>
    appRoutes().whereType<StatefulShellRoute>().single.branches;

/// Пустая конфигурация: [GoRouterState] держит её только ради
/// `namedLocation`, которым мы не пользуемся, но конструктор требует объект.
final RouteConfiguration _configuration = RouteConfiguration(
  ValueNotifier<RoutingConfig>(
    RoutingConfig(
      routes: <RouteBase>[
        GoRoute(path: '/', builder: (_, __) => const SizedBox.shrink()),
      ],
    ),
  ),
  navigatorKey: GlobalKey<NavigatorState>(),
);

GoRouterState _state(String location) => GoRouterState(
  _configuration,
  uri: Uri.parse(location),
  matchedLocation: location,
  fullPath: location,
  pathParameters: const <String, String>{},
  pageKey: ValueKey<String>(location),
);

/// Контекст для вызова билдеров. Сами билдеры его не читают (виджеты
/// строятся константами), но подпись требует настоящий.
Future<BuildContext> _context(WidgetTester tester) async {
  await tester.pumpWidget(const MaterialApp(home: SizedBox.shrink()));
  return tester.element(find.byType(SizedBox));
}

/// Накладные маршруты, объявленные в таблице своими путями.
const List<String> _overlayPaths = <String>[
  AppRoute.login,
  AppRoute.enroll,
  AppRoute.connect,
  AppRoute.settingsAutotune,
  AppRoute.protocol,
  AppRoute.siteRules,
  AppRoute.relay,
  AppRoute.servers,
  AppRoute.connections,
  AppRoute.connectionImport,
  AppRoute.csmOperator,
  AppRoute.csmDocuments,
  AppRoute.csmTransport,
  AppRoute.csmDisclosure,
  AppRoute.plans,
  AppRoute.referrals,
  AppRoute.partner,
  AppRoute.notifications,
  AppRoute.tickets,
  AppRoute.newTicket,
  '/tickets/:id',
];

void main() {
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  test('шелл остался один, веток по-прежнему три и в том же порядке', () {
    expect(appRoutes().whereType<StatefulShellRoute>().length, 1);
    expect(
      _branches()
          .map((b) => (b.routes.first as GoRoute).path)
          .toList(growable: false),
      <String>[AppRoute.home, AppRoute.profile, AppRoute.settings],
    );
  });

  test('каждый накладной маршрут объявлен страницей, а не билдером', () {
    final routes = _routesByPath();
    for (final path in _overlayPaths) {
      final route = routes[path];
      expect(route, isNotNull, reason: 'маршрут $path пропал из таблицы');
      expect(
        route!.pageBuilder,
        isNotNull,
        reason:
            'маршрут $path строится билдером: на десктопе он откроется '
            'полноэкранной страницей и накроет шелл вместо панели',
      );
      expect(route.builder, isNull, reason: 'у $path остался и builder');
    }
  });

  test('сплеш и первый автоподбор страницами НЕ стали', () {
    // Они не лежат поверх приложения, они его заменяют: панель со скримом там
    // накрыла бы пустоту.
    final routes = _routesByPath();
    expect(routes[AppRoute.splash]!.pageBuilder, isNull);
    expect(routes[AppRoute.autotune]!.pageBuilder, isNull);
  });

  testWidgets('на мобильном страница осталась прежней MaterialPage', (
    tester,
  ) async {
    final context = await _context(tester);
    final routes = _routesByPath();

    for (final path in _overlayPaths) {
      // Параметр в пути подставляем: `matchedLocation` у живого маршрута
      // приходит уже разрешённым.
      final location = path == '/tickets/:id' ? '/tickets/12' : path;
      final state = _state(location);
      final page = routes[path]!.pageBuilder!(context, state);
      expect(
        page,
        isA<MaterialPage<void>>(),
        reason: 'на мобильном $path обязан остаться обычной страницей',
      );
      // Ключ и имя — то, чем go_router узнаёт страницу между кадрами.
      expect(page.key, state.pageKey);
      expect(page.name, state.name);
    }

    // Ребёнок тоже тот же самый, а не десктопная копия экрана.
    expect(
      (routes[AppRoute.login]!.pageBuilder!(context, _state(AppRoute.login))
              as MaterialPage<void>)
          .child,
      isA<LoginScreen>(),
    );
    expect(
      (routes[AppRoute.servers]!.pageBuilder!(context, _state(AppRoute.servers))
              as MaterialPage<void>)
          .child,
      isA<ServersScreen>(),
    );
  });

  testWidgets('на десктопе накладной маршрут перестаёт быть MaterialPage', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    final context = await _context(tester);
    final routes = _routesByPath();

    for (final path in _overlayPaths) {
      final location = path == '/tickets/:id' ? '/tickets/12' : path;
      final state = _state(location);
      final page = routes[path]!.pageBuilder!(context, state);
      expect(
        page,
        isNot(isA<MaterialPage<void>>()),
        reason:
            'на десктопе $path обязан лечь панелью поверх шелла, а не '
            'полноэкранной страницей',
      );
      expect(page.key, state.pageKey);
    }

    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('ветки Профиль и Настройки меняют экран, а не маршрут', (
    tester,
  ) async {
    final context = await _context(tester);
    final branches = _branches();
    GoRoute routeOf(int branch) => branches[branch].routes.first as GoRoute;

    // Мобильная платформа: те же экраны, что и были.
    expect(
      routeOf(0).builder!(context, _state(AppRoute.home)),
      isA<HomeScreen>(),
    );
    expect(
      routeOf(1).builder!(context, _state(AppRoute.profile)),
      isA<ProfileScreen>(),
    );
    expect(
      routeOf(2).builder!(context, _state(AppRoute.settings)),
      isA<SettingsScreen>(),
    );

    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    // «Подключение» остаётся одним экраном на обе платформы: раскладку
    // выбирает сам [HomeScreen], потому что вся логика до неё общая.
    expect(
      routeOf(0).builder!(context, _state(AppRoute.home)),
      isA<HomeScreen>(),
    );
    expect(
      routeOf(1).builder!(context, _state(AppRoute.profile)),
      isA<ProfileDesktopScreen>(),
    );
    expect(
      routeOf(2).builder!(context, _state(AppRoute.settings)),
      isA<SettingsDesktopScreen>(),
    );
    debugDefaultTargetPlatformOverride = null;
  });
}
