// Накладной маршрут на десктопе лежит ПОВЕРХ приложения, а не вместо него.
//
// Инвентарь E1 снял на живой сборке ровно обратное: «Тип подключения» и
// «Серверы» открывались полноэкранным листом, Esc не закрывал ничего (фокус
// оставался в экране под листом), а приложение из-под них исчезало. Здесь
// сторожатся четыре свойства этой страницы: под панелью остаётся страница, с
// которой пришли; ширина панели задана представлением, а не содержимым; Esc и
// клик по скриму закрывают панель; на экранах, где случайный клик стоит данных
// (ссылка подключения, энроллмент, автонастройка), не закрывают.
//
// И пятое, самое дорогое: на мобильной платформе всё это не включается вовсе —
// там та же самая MaterialPage, что и до десктопа.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/desktop/desktop_overlay_page.dart';
import 'package:caramba_client/desktop/desktop_panel.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/theme/colors.dart';

/// Ключи тел экранов: по ним меряется РЕАЛЬНАЯ раскладка панели, а не значение
/// поля виджета.
const Key kHomeBody = Key('home-body');
const Key kOverlayBody = Key('overlay-body');

/// Собирает страницу маршрута руками, как это будет делать боевая таблица.
Widget _body(Key key, String label) => Scaffold(
  key: key,
  body: Center(child: Text(label)),
);

/// Ловит `Route`, который положил навигатор: свойства страницы (opaque,
/// barrierColor, barrierDismissible) проверяются на самом маршруте, а не по
/// картинке — иначе тест не отличит непрозрачную страницу от прозрачной.
class _RouteSpy extends NavigatorObserver {
  final List<Route<dynamic>> pushed = <Route<dynamic>>[];

  @override
  void didPush(Route<dynamic> route, Route<dynamic>? previousRoute) {
    pushed.add(route);
  }
}

GoRouter _router(_RouteSpy spy) => GoRouter(
  initialLocation: AppRoute.home,
  observers: <NavigatorObserver>[spy],
  routes: <RouteBase>[
    GoRoute(path: AppRoute.home, builder: (_, __) => _body(kHomeBody, 'home')),
    GoRoute(
      path: AppRoute.servers,
      pageBuilder: (_, state) => desktopOverlayPage<void>(
        key: state.pageKey,
        name: state.uri.path,
        presentation: DesktopPresentation.panelWide,
        child: _body(kOverlayBody, 'servers'),
      ),
    ),
    GoRoute(
      path: AppRoute.protocol,
      pageBuilder: (_, state) => desktopOverlayPage<void>(
        key: state.pageKey,
        name: state.uri.path,
        child: _body(kOverlayBody, 'protocol'),
      ),
    ),
    GoRoute(
      path: AppRoute.connect,
      pageBuilder: (_, state) => desktopOverlayPage<void>(
        key: state.pageKey,
        name: state.uri.path,
        presentation: DesktopPresentation.dialog,
        dismissible: false,
        child: _body(kOverlayBody, 'connect'),
      ),
    ),
    GoRoute(
      path: AppRoute.login,
      pageBuilder: (_, state) => desktopOverlayPage<void>(
        key: state.pageKey,
        name: state.uri.path,
        presentation: DesktopPresentation.dialog,
        child: _body(kOverlayBody, 'login'),
      ),
    ),
  ],
);

/// Тест десктопной ветки в окне 1280x800 с уже смонтированным приложением.
///
/// Сброс `debugDefaultTargetPlatformOverride` стоит в `finally` ВНУТРИ тела:
/// `testWidgets` сверяет debug-переменные сразу после тела, раньше любых
/// tearDown, и оставленный override валит тест сообщением «The value of a
/// foundation debug variable was changed». tearDown ниже — страховка на случай
/// падения до `finally`.
void _desktopTest(
  String description,
  Future<void> Function(
    WidgetTester tester,
    GoRouter router,
    ModalRoute<dynamic> Function() top,
  )
  body,
) {
  testWidgets(description, (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final spy = _RouteSpy();
    final router = _router(spy);
    addTearDown(router.dispose);
    try {
      await tester.pumpWidget(
        MaterialApp.router(theme: AppTheme.dark(), routerConfig: router),
      );
      await tester.pumpAndSettle();
      expect(find.text('home'), findsOneWidget);
      // Верхний маршрут стека: свойства страницы проверяются на нём, а не по
      // картинке — иначе тест не отличит непрозрачную страницу от прозрачной.
      await body(tester, router, () => spy.pushed.last as ModalRoute<dynamic>);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}

void main() {
  group('таблица представлений', () {
    test('широкая панель: таблицы, списки, экраны панели', () {
      for (final path in const <String>[
        AppRoute.servers,
        AppRoute.connections,
        AppRoute.connectionImport,
        AppRoute.siteRules,
        AppRoute.protocol,
        AppRoute.relay,
        AppRoute.csmOperator,
        AppRoute.csmDocuments,
        AppRoute.csmTransport,
        AppRoute.csmDisclosure,
        AppRoute.tickets,
        AppRoute.newTicket,
        '/tickets/12',
        AppRoute.notifications,
        AppRoute.plans,
        AppRoute.partner,
        AppRoute.referrals,
      ]) {
        expect(
          presentationFor(path),
          DesktopPresentation.panelWide,
          reason: path,
        );
      }
    });

    test('диалог: вход, энроллмент, ссылка, автонастройка', () {
      for (final path in const <String>[
        AppRoute.login,
        AppRoute.enroll,
        AppRoute.connect,
        AppRoute.settingsAutotune,
      ]) {
        expect(presentationFor(path), DesktopPresentation.dialog, reason: path);
      }
    });

    test('всё остальное — узкая панель', () {
      expect(presentationFor('/whatever'), DesktopPresentation.panelNarrow);
    });

    test('query и хвостовой слэш маршрута не меняют', () {
      // Ссылка приходит параметром (`?link=`), а не другим экраном.
      expect(
        presentationFor('/connect?link=caramba://x'),
        DesktopPresentation.dialog,
      );
      expect(
        presentationFor('/enroll?panel=demo&code=42'),
        DesktopPresentation.dialog,
      );
      expect(presentationFor('/servers/'), DesktopPresentation.panelWide);
      expect(dismissibleFor('/connect?link=caramba://x'), isFalse);
    });

    test('закрытие по скриму запрещено там, где клик стоит данных', () {
      expect(dismissibleFor(AppRoute.connect), isFalse);
      expect(dismissibleFor(AppRoute.enroll), isFalse);
      expect(dismissibleFor(AppRoute.settingsAutotune), isFalse);

      expect(dismissibleFor(AppRoute.login), isTrue);
      expect(dismissibleFor(AppRoute.servers), isTrue);
      expect(dismissibleFor(AppRoute.protocol), isTrue);
    });
  });

  group('мобильная платформа', () {
    tearDown(() => debugDefaultTargetPlatformOverride = null);

    test('страница остаётся обычной MaterialPage', () {
      final page = desktopOverlayPage<void>(
        key: const ValueKey<String>('servers'),
        name: AppRoute.servers,
        presentation: DesktopPresentation.panelWide,
        child: const Text('servers'),
      );

      expect(page, isA<MaterialPage<void>>());
      expect(page.key, const ValueKey<String>('servers'));
      expect(page.name, AppRoute.servers);
      expect((page as MaterialPage<void>).child, isA<Text>());
    });

    testWidgets('маршрут открывается на весь экран, без скрима и панели', (
      tester,
    ) async {
      final spy = _RouteSpy();
      final router = _router(spy);
      addTearDown(router.dispose);
      await tester.pumpWidget(
        MaterialApp.router(theme: AppTheme.dark(), routerConfig: router),
      );
      await tester.pumpAndSettle();

      unawaited(router.push(AppRoute.servers));
      await tester.pumpAndSettle();

      expect(find.byType(DesktopPanelFrame), findsNothing);
      final route = spy.pushed.last as ModalRoute<dynamic>;
      expect(route.opaque, isTrue, reason: 'мобильный лист непрозрачен');
      expect(
        tester.getSize(find.byKey(kOverlayBody)).width,
        tester.getSize(find.byType(MaterialApp)).width,
      );
    });
  });

  group('десктоп', () {
    // Страховка: тело каждого теста снимает override само, в `finally`.
    tearDown(() => debugDefaultTargetPlatformOverride = null);

    _desktopTest('панель не прячет приложение и держит скрим', (
      tester,
      router,
      top,
    ) async {
      unawaited(router.push(AppRoute.servers));
      await tester.pumpAndSettle();

      expect(find.text('servers'), findsOneWidget);
      // Главное свойство: страница, с которой пришли, осталась в дереве.
      expect(find.text('home'), findsOneWidget);
      expect(tester.getSize(find.byKey(kHomeBody)), const Size(1280, 800));

      expect(top().opaque, isFalse);
      expect(top().barrierColor, AppColors.dark.overlayScrim);
      expect(top().barrierDismissible, isTrue);
    });

    _desktopTest('ширина панели задана представлением', (
      tester,
      router,
      top,
    ) async {
      unawaited(router.push(AppRoute.servers));
      await tester.pumpAndSettle();
      expect(find.byType(DesktopPanelFrame), findsOneWidget);
      expect(
        tester.getSize(find.byKey(kOverlayBody)),
        const Size(DesktopTokens.panelWide, 800),
      );
      // Панель прижата к правому краю окна.
      expect(
        tester.getTopLeft(find.byKey(kOverlayBody)).dx,
        1280 - DesktopTokens.panelWide,
      );

      router.pop();
      await tester.pumpAndSettle();

      unawaited(router.push(AppRoute.protocol));
      await tester.pumpAndSettle();
      expect(
        tester.getSize(find.byKey(kOverlayBody)),
        const Size(DesktopTokens.panelNarrow, 800),
      );
    });

    _desktopTest('диалог по центру и не шире 520', (tester, router, top) async {
      unawaited(router.push(AppRoute.login));
      await tester.pumpAndSettle();

      expect(find.byType(DesktopDialogFrame), findsOneWidget);
      final rect = tester.getRect(find.byKey(kOverlayBody));
      expect(rect.width, DesktopTokens.dialogMaxWidth);
      expect(rect.height, 800 * kDesktopDialogMaxHeightFactor);
      expect(rect.center.dx, 1280 / 2);
      expect(rect.center.dy, 800 / 2);
    });

    _desktopTest('Esc закрывает панель', (tester, router, top) async {
      unawaited(router.push(AppRoute.servers));
      await tester.pumpAndSettle();
      expect(find.text('servers'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();

      expect(find.text('servers'), findsNothing);
      expect(find.text('home'), findsOneWidget);
    });

    _desktopTest('клик по скриму закрывает панель', (
      tester,
      router,
      top,
    ) async {
      unawaited(router.push(AppRoute.servers));
      await tester.pumpAndSettle();

      // Левее панели: там лежит скрим, а под ним приложение.
      await tester.tapAt(const Offset(100, 400));
      await tester.pumpAndSettle();

      expect(find.text('servers'), findsNothing);
      expect(find.text('home'), findsOneWidget);
    });

    _desktopTest('неотменяемый диалог не закрывают ни скрим, ни Esc', (
      tester,
      router,
      top,
    ) async {
      unawaited(router.push(AppRoute.connect));
      await tester.pumpAndSettle();
      expect(find.text('connect'), findsOneWidget);
      expect(top().barrierDismissible, isFalse);

      await tester.tapAt(const Offset(100, 400));
      await tester.pumpAndSettle();
      expect(find.text('connect'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(
        find.text('connect'),
        findsOneWidget,
        reason: 'ссылка подключения закрывается только своей кнопкой',
      );
    });

    _desktopTest('фокус уходит внутрь панели', (tester, router, top) async {
      unawaited(router.push(AppRoute.servers));
      await tester.pumpAndSettle();

      // Ровно то, чего не было у мобильного листа в E1: без фокуса внутри
      // модалки Esc уходит в экран под ней.
      final focused = FocusManager.instance.primaryFocus;
      expect(focused, isNotNull);
      expect(
        find.descendant(
          of: find.byType(DesktopPanelFrame),
          matching: find.byKey(kOverlayBody),
        ),
        findsOneWidget,
      );
      expect(
        focused!.context!.findAncestorWidgetOfExactType<DesktopPanelFrame>(),
        isNotNull,
      );
    });
  });
}
