// Десктопная оболочка: сайдбар, переключение веток, накладные «Серверы».
//
// Что здесь сторожится и почему именно это.
//
//   * ЧЕТЫРЕ ПУНКТА ПРИ ТРЁХ ВЕТКАХ. «Серверы» — накладной маршрут, а не
//     вкладка, и подсветка у него считается по верхней странице стека. Ошибка
//     здесь выглядит безобидно (пункт просто не загорается), а означает, что
//     сайдбар перестал показывать, где человек находится.
//   * ПЕРЕКЛЮЧЕНИЕ ВЕТКИ ДВУМЯ ПУТЯМИ. Клик по пункту и ⌘3 обязаны делать одно
//     и то же: шорткаты объявлены отдельно от сайдбара, и разъехаться им проще
//     всего. На macOS сочетание живёт в СТРОКЕ МЕНЮ (проверка на живой сборке
//     показала, что объявленное только в [Shortcuts] ⌘1/2/3 до приложения не
//     доходит вовсе), поэтому и проверяется оно через пункт меню — тем же
//     путём, которым его нажимает человек.
//   * ШЕЛЛ ОСТАЁТСЯ ПОД ПАНЕЛЬЮ. Накладной маршрут кладётся ПОВЕРХ шелла
//     (`CarambaRouter.go` делает push), и сайдбар из-под него не исчезает.
//     Ровно это ломалось на E1, где `go` сносил стек целиком.
//
// Платформа переопределяется на macOS и возвращается в `null` внутри тела
// теста: flutter_test проверяет отладочные переменные foundation ДО tearDown,
// и наследивший тест уронил бы следующий, а не себя. Плагины окна в тесте не
// зовутся: [WindowPort] подменён фейком, а `DragToMoveArea` уходит в плагин
// только по жесту.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/desktop/ports/window_port.dart';
import 'package:caramba_client/desktop/shell/desktop_shell.dart';
import 'package:caramba_client/desktop/shell/desktop_shortcuts.dart';
import 'package:caramba_client/desktop/shell/desktop_sidebar.dart';
import 'package:caramba_client/desktop/shell/sidebar_status.dart';
import 'package:caramba_client/desktop/window_service.dart';
import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/theme/colors.dart';

import '../support/fake_core.dart';

/// Окно, которое ничего не умеет и никуда не ходит.
///
/// Живой [WindowManagerPort] дошёл бы до метод-канала на первом же чтении
/// («развёрнуто ли окно») и упал бы `MissingPluginException`.
class _FakeWindowPort implements WindowPort {
  bool visible = true;

  @override
  Future<void> show() async => visible = true;

  @override
  Future<void> hide() async => visible = false;

  @override
  Future<void> focus() async {}

  @override
  Future<bool> isVisible() async => visible;

  @override
  Future<Rect> getBounds() async => const Rect.fromLTWH(0, 0, 1120, 720);

  @override
  Future<void> setBounds(Rect bounds) async {}

  @override
  Future<bool> isMaximized() async => false;

  @override
  Future<void> maximize() async {}

  @override
  Future<void> unmaximize() async {}

  @override
  Future<void> restore() async {}

  @override
  Future<void> setPreventClose(bool value) async {}

  @override
  Future<void> setTitle(String title) async {}

  @override
  Future<void> setMinimizeToTray(bool value) async {}

  @override
  void addListener(WindowPortListener listener) {}

  @override
  void removeListener(WindowPortListener listener) {}
}

class _Store implements ConnectionProfilesStore {
  _Store(this.profiles, this.activeId);

  List<ConnectionProfile> profiles;
  String? activeId;

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;

  @override
  Future<String?> readActiveId() async => activeId;

  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async {
    profiles = next;
  }

  @override
  Future<void> writeActiveId(String? id) async {
    activeId = id;
  }

  @override
  Future<void> clear() async {
    profiles = const <ConnectionProfile>[];
    activeId = null;
  }
}

/// Сырая подписка: панельных провайдеров она не будит, и бренд остаётся
/// дефолтным.
const _profile = ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://sub.example/a',
  rawConfig: 'proxies: []',
  format: 'clash',
);

/// Таблица той же ФОРМЫ, что боевая: накладные маршруты сиблингами шелла с
/// тремя ветками. Экраны заменены текстом — проверяется оболочка, а не они.
List<RouteBase> _routes(GlobalKey<NavigatorState> root) => <RouteBase>[
  GoRoute(
    path: AppRoute.servers,
    parentNavigatorKey: root,
    // Панель на десктопе полупрозрачная и НЕ снимает шелл со сцены: без
    // `opaque: false` страница под ней ушла бы offstage, и проверка «сайдбар
    // виден из-под панели» стала бы бессмысленной.
    pageBuilder: (context, state) => CustomTransitionPage<void>(
      key: state.pageKey,
      opaque: false,
      transitionsBuilder: (_, __, ___, child) => child,
      child: const Align(
        alignment: Alignment.centerRight,
        child: SizedBox(width: 720, child: Text('servers')),
      ),
    ),
  ),
  GoRoute(
    path: AppRoute.connectionImport,
    parentNavigatorKey: root,
    builder: (_, __) => const Text('import'),
  ),
  StatefulShellRoute.indexedStack(
    builder: (_, __, shell) => DesktopShell(navigationShell: shell),
    branches: <StatefulShellBranch>[
      StatefulShellBranch(
        routes: <RouteBase>[
          GoRoute(
            path: AppRoute.home,
            builder: (_, __) => const Text('home-screen'),
          ),
        ],
      ),
      StatefulShellBranch(
        routes: <RouteBase>[
          GoRoute(
            path: AppRoute.profile,
            builder: (_, __) => const Text('profile-screen'),
          ),
        ],
      ),
      StatefulShellBranch(
        routes: <RouteBase>[
          GoRoute(
            path: AppRoute.settings,
            builder: (_, __) => const Text('settings-screen'),
          ),
        ],
      ),
    ],
  ),
];

/// Секретное хранилище и строка меню живут за метод-каналами: без заглушек
/// первый же кадр уходит в `MissingPluginException`.
void _mockChannels() {
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  const secure = MethodChannel('plugins.it_nomads.com/flutter_secure_storage');
  messenger.setMockMethodCallHandler(secure, (call) async => null);
  messenger.setMockMethodCallHandler(SystemChannels.menu, (call) async => null);
  addTearDown(() {
    messenger.setMockMethodCallHandler(secure, null);
    messenger.setMockMethodCallHandler(SystemChannels.menu, null);
  });
}

Future<void> _desktop(WidgetTester tester, Future<void> Function() body) async {
  debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
  tester.view
    ..physicalSize = const Size(1280, 800)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  try {
    await body();
  } finally {
    debugDefaultTargetPlatformOverride = null;
  }
}

Future<CarambaRouter> _pump(WidgetTester tester, {bool empty = false}) async {
  _mockChannels();
  final root = GlobalKey<NavigatorState>(debugLabel: 'test-root');
  final router = CarambaRouter(
    navigatorKey: root,
    initialLocation: AppRoute.home,
    redirect: (_, __) => null,
    routes: _routes(root),
  );
  addTearDown(router.dispose);

  await tester.pumpWidget(
    ProviderScope(
      overrides: <Override>[
        vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
        connectionProfilesStoreProvider.overrideWithValue(
          _Store(
            empty ? <ConnectionProfile>[] : <ConnectionProfile>[_profile],
            empty ? null : _profile.id,
          ),
        ),
        windowPortProvider.overrideWithValue(_FakeWindowPort()),
      ],
      child: MaterialApp.router(theme: AppTheme.dark(), routerConfig: router),
    ),
  );
  await tester.pumpAndSettle();
  return router;
}

/// Пункт сайдбара по подписи. Через сайдбар, а не по тексту: заголовок раздела
/// в тулбаре называется теми же словами.
Finder _navRow(String label) => find.descendant(
  of: find.byType(DesktopSidebar),
  matching: find.text(label),
);

/// Фон строки навигации: активная стоит на surface2, спящая прозрачна.
Color? _navBackground(WidgetTester tester, String label) {
  final material = tester.widget<Material>(
    find.ancestor(of: _navRow(label), matching: find.byType(Material)).first,
  );
  return material.color;
}

/// Все пункты строки меню, включая вложенные и сгруппированные.
Iterable<PlatformMenuItem> _allMenuItems(
  Iterable<PlatformMenuItem> items,
) sync* {
  for (final item in items) {
    yield item;
    yield* _allMenuItems(item.members);
    yield* _allMenuItems(item.descendants);
  }
}

/// Сочетание «⌘ + клавиша» без прочих модификаторов.
bool _isMetaShortcut(ShortcutActivator? s, LogicalKeyboardKey trigger) =>
    s is SingleActivator &&
    s.trigger == trigger &&
    s.meta &&
    !s.shift &&
    !s.control &&
    !s.alt;

/// Нажимает пункт строки меню с сочетанием ⌘+[key] — ровно то, что делает
/// система, когда человек жмёт это сочетание на маке.
Future<void> _pressMeta(WidgetTester tester, LogicalKeyboardKey key) async {
  final bar = tester.widget<PlatformMenuBar>(find.byType(PlatformMenuBar));
  final item = _allMenuItems(bar.menus).firstWhere(
    (i) => _isMetaShortcut(i.shortcut, key) && i.onSelected != null,
    orElse: () =>
        throw StateError('в строке меню нет пункта с ⌘${key.keyLabel}'),
  );
  item.onSelected!();
  await tester.pumpAndSettle();
}

void main() {
  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  // Платформа глобальна: оставленный override увёл бы в десктопную ветку тесты
  // мобильного шелла.
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  testWidgets('сайдбар: четыре пункта, состояние туннеля, ширина 240', (
    tester,
  ) async {
    await _desktop(tester, () async {
      await _pump(tester);

      expect(find.byType(DesktopSidebar), findsOneWidget);
      for (final label in const <String>[
        'Подключение',
        'Серверы',
        'Профиль',
        'Настройки',
      ]) {
        expect(_navRow(label), findsOneWidget, reason: 'нет пункта $label');
      }

      // Статус-блок отвечает на главный вопрос сайдбара до всякой навигации.
      expect(find.byType(SidebarStatus), findsOneWidget);
      expect(find.text('Отключено'), findsOneWidget);
      expect(find.text('Подключить'), findsOneWidget);

      expect(
        tester.getSize(find.byType(DesktopSidebar)).width,
        DesktopTokens.sidebarWidth,
      );

      // Заголовок раздела в тулбаре: слово то же, что у пункта сайдбара.
      expect(find.text('Подключение'), findsNWidgets(2));
      expect(find.text('home-screen'), findsOneWidget);

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets('empty sidebar action keeps its full label', (tester) async {
    await _desktop(tester, () async {
      await _pump(tester, empty: true);
      final label = find.text('Добавить подключение');
      expect(label, findsOneWidget);
      final paragraph = tester.renderObject<RenderParagraph>(
        find.descendant(of: label, matching: find.byType(RichText)),
      );
      expect(paragraph.didExceedMaxLines, isFalse);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets('клик по пункту и ⌘3 переключают ветку одинаково', (
    tester,
  ) async {
    await _desktop(tester, () async {
      await _pump(tester);

      await tester.tap(_navRow('Настройки'));
      await tester.pumpAndSettle();
      expect(find.text('settings-screen'), findsOneWidget);
      expect(find.text('home-screen'), findsNothing);
      expect(
        _navBackground(tester, 'Настройки'),
        AppColors.dark.surface2,
        reason: 'активный пункт подсвечен',
      );

      // Возврат на первую ветку и обратно — уже клавиатурой.
      await _pressMeta(tester, LogicalKeyboardKey.digit1);
      expect(find.text('home-screen'), findsOneWidget);
      expect(find.text('settings-screen'), findsNothing);

      await _pressMeta(tester, LogicalKeyboardKey.digit3);
      expect(find.text('settings-screen'), findsOneWidget);
      expect(find.text('home-screen'), findsNothing);

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  // Ручная проверка увидела ⌘1/⌘2/⌘3, которые «не срабатывают»: сочетание
  // объявлено, интент летит — а ветка не меняется. Проверять только пункт меню
  // мало: он проверяет ПРОВОДКУ сочетания, а не то, что интент вообще кто-то
  // обрабатывает. Здесь интент подаётся напрямую в [Actions] шелла — то, во что
  // превращаются оба пути (и меню macOS, и [Shortcuts] других платформ).
  testWidgets('Actions шелла обрабатывают SelectTabIntent и меняют ветку', (
    tester,
  ) async {
    await _desktop(tester, () async {
      await _pump(tester);

      // Контекст ПОД [Actions]: сайдбар лежит внутри той же обёртки, что и
      // весь контент, — оттуда интент и поднимается у живого приложения.
      final inside = tester.element(find.byType(DesktopSidebar));

      Actions.invoke(inside, const SelectTabIntent(2));
      await tester.pumpAndSettle();
      expect(find.text('settings-screen'), findsOneWidget);
      expect(find.text('home-screen'), findsNothing);

      Actions.invoke(inside, const SelectTabIntent(1));
      await tester.pumpAndSettle();
      expect(find.text('profile-screen'), findsOneWidget);

      Actions.invoke(inside, const SelectTabIntent(0));
      await tester.pumpAndSettle();
      expect(find.text('home-screen'), findsOneWidget);

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  // Обратная сторона того же решения: где строки меню нет, сочетания обязаны
  // быть в [Shortcuts] и доходить до ветки настоящим нажатием.
  testWidgets('без строки меню ветку переключает Ctrl+3', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    tester.view
      ..physicalSize = const Size(1280, 800)
      ..devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    try {
      await _pump(tester);

      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.digit3);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await tester.pumpAndSettle();

      expect(find.text('settings-screen'), findsOneWidget);
      expect(find.text('home-screen'), findsNothing);

      await tester.pumpWidget(const SizedBox.shrink());
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('на macOS сочетания живут только в строке меню', (tester) async {
    await _desktop(tester, () async {
      await _pump(tester);

      // Все три ветки объявлены пунктами меню: иначе ⌘1/2/3 на маке не
      // работают вовсе — ровно этот дефект ловила ручная проверка.
      final bar = tester.widget<PlatformMenuBar>(find.byType(PlatformMenuBar));
      final menuItems = _allMenuItems(bar.menus).toList();
      for (final key in const <LogicalKeyboardKey>[
        LogicalKeyboardKey.digit1,
        LogicalKeyboardKey.digit2,
        LogicalKeyboardKey.digit3,
      ]) {
        expect(
          menuItems.where((i) => _isMetaShortcut(i.shortcut, key)),
          hasLength(1),
          reason: 'в меню нет ⌘${key.keyLabel}',
        );
      }

      // И ни одного дубля в [Shortcuts]: объявленное дважды сочетание
      // срабатывает дважды, то есть подключает и тут же отключает.
      final shortcuts = tester.widget<Shortcuts>(
        find
            .descendant(
              of: find.byType(DesktopShortcuts),
              matching: find.byType(Shortcuts),
            )
            .first,
      );
      expect(shortcuts.shortcuts, isEmpty);

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets('«Серверы» ложатся поверх шелла и подсвечиваются', (
    tester,
  ) async {
    await _desktop(tester, () async {
      final router = await _pump(tester);

      expect(_navBackground(tester, 'Серверы'), Colors.transparent);

      await tester.tap(_navRow('Серверы'));
      await tester.pumpAndSettle();

      expect(find.text('servers'), findsOneWidget);
      // Шелл никуда не делся: панель лежит ПОВЕРХ него, а не вместо.
      expect(find.byType(DesktopSidebar), findsOneWidget);
      expect(
        _navBackground(tester, 'Серверы'),
        AppColors.dark.surface2,
        reason: 'подсветка считается по верхней странице стека',
      );
      // Ветка под панелью остаётся выбранной: человек всё ещё «в подключении».
      expect(_navBackground(tester, 'Подключение'), AppColors.dark.surface2);

      // «Назад» снимает панель, и подсветка гаснет вместе с ней.
      expect(await router.routerDelegate.popRoute(), isTrue);
      await tester.pumpAndSettle();
      expect(find.text('servers'), findsNothing);
      expect(_navBackground(tester, 'Серверы'), Colors.transparent);

      await tester.pumpWidget(const SizedBox.shrink());
    });
  });
}
