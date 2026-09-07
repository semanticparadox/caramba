// Выходы с экрана подключения по ссылке, когда он лежит ПОВЕРХ приложения.
//
// Экран `caramba://connect` перестал быть дверью холодного старта: ссылку
// вставляют из «Добавить подключение», открывают из «Аккаунта панели» и ловят
// диплинком — и почти всегда под экраном уже есть шелл. Значит, каждый выход с
// него это работа со стеком, а не переход «куда-нибудь».
//
// Две ловушки, обе снимаются только живым навигатором, а не чтением кода:
//   • «Посмотреть тарифы» закрывает поток (`finish`), но `go` кладёт витрину
//     ПОВЕРХ — и «Назад» с тарифов возвращало бы на уже завершённое «Панель
//     подключена», экран без смысла и без выхода;
//   • «Отмена» обязана вернуть туда, откуда пришли, а без стека под собой (тот
//     самый диплинк холодного старта) — увести в приложение, а не в никуда.
//
// Роутер здесь настоящий ([CarambaRouter]), а экраны вокруг заменены текстом:
// проверяется стек, а не их содержимое.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/auth_tokens.dart';
import 'package:caramba_client/features/enroll/connect_controller.dart';
import 'package:caramba_client/features/enroll/connect_link.dart';
import 'package:caramba_client/features/enroll/connect_redeem.dart';
import 'package:caramba_client/features/enroll/connect_screen.dart';
import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/theme/app_theme.dart';

/// Нотифаер с заранее выставленным состоянием: проверяется навигация, а не
/// поток, и гонять ради неё разбор ссылки с погашением значило бы проверять их
/// второй раз.
class _Seeded extends ConnectNotifier {
  _Seeded(super.ref, ConnectState seed) {
    state = seed;
  }

  /// Передача сессии общему auth-слою к стеку навигации отношения не имеет, а
  /// потянула бы за собой хранилище и auth-провайдер целиком.
  @override
  void finish() {}
}

const _link = CarambaConnectLink(
  origin: 'https://panel.exarobot.top',
  code: '000102030405060708090a0b0c0d0e0f',
  operatorName: 'Caramba Connect',
  expiresAtSec: 1780000000,
);

const _tokens = AuthTokens(
  accessToken: 'a',
  refreshToken: 'r',
  tokenType: 'Bearer',
  expiresIn: 900,
  userId: 46,
);

/// Подключение прошло, но подписки у аккаунта нет: единственное состояние, где
/// экран висит до нажатия кнопки и где вообще есть «Посмотреть тарифы».
const _doneWithProblem = ConnectState(
  stage: ConnectStage.done,
  link: _link,
  result: ConnectRedeemResult(
    tokens: _tokens,
    panelName: 'Caramba Connect',
    subscriptionReason: 'no_subscription_on_account',
  ),
);

const _confirm = ConnectState(stage: ConnectStage.confirm, link: _link);

/// Таблица той же ФОРМЫ, что боевая: шелл с вкладкой «Подключение», накладной
/// экран ссылки и витрина тарифов сиблингами шелла.
CarambaRouter _router(String at) {
  final root = GlobalKey<NavigatorState>(debugLabel: 'connect-exit-root');
  return CarambaRouter(
    navigatorKey: root,
    initialLocation: at,
    redirect: (_, __) => null,
    routes: [
      StatefulShellRoute.indexedStack(
        builder: (_, __, shell) => shell,
        branches: [
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: AppRoute.home,
                builder: (_, __) => const Text('ГЛАВНАЯ'),
              ),
            ],
          ),
        ],
      ),
      GoRoute(
        path: AppRoute.connect,
        builder: (_, __) => const ConnectScreen(),
      ),
      GoRoute(
        path: AppRoute.plans,
        parentNavigatorKey: root,
        builder: (_, __) => const Text('ТАРИФЫ'),
      ),
    ],
  );
}

Future<void> _mount(
  WidgetTester tester,
  GoRouter router,
  ConnectState seed,
) async {
  tester.view
    ..physicalSize = const Size(900, 2400)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [connectProvider.overrideWith((ref) => _Seeded(ref, seed))],
      child: MaterialApp.router(theme: AppTheme.dark(), routerConfig: router),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('витрина тарифов не ложится поверх закрытого потока', (
    tester,
  ) async {
    final router = _router(AppRoute.home);
    addTearDown(router.dispose);
    await _mount(tester, router, _doneWithProblem);

    // Экран кладётся push'ом напрямую: так он и живёт, когда ссылку вставили из
    // приложения, и тест не зависит от того, попал ли `/connect` в
    // [AppRoute.overlays].
    unawaited(router.push<void>(AppRoute.connect));
    await tester.pumpAndSettle();
    expect(find.text('Посмотреть тарифы'), findsOneWidget);

    await tester.tap(find.text('Посмотреть тарифы'));
    await tester.pumpAndSettle();
    expect(find.text('ТАРИФЫ'), findsOneWidget);

    // Вот ради чего всё: «Назад» с тарифов обязано привести в приложение, а не
    // обратно на завершённое подключение.
    expect(await router.routerDelegate.popRoute(), isTrue);
    await tester.pumpAndSettle();
    expect(find.text('ГЛАВНАЯ'), findsOneWidget);
    expect(
      find.text('Посмотреть тарифы'),
      findsNothing,
      reason: 'поток закрыт, а его последний кадр остался лежать под витриной',
    );
  });

  testWidgets('«Отмена» возвращает туда, откуда пришли', (tester) async {
    final router = _router(AppRoute.home);
    addTearDown(router.dispose);
    await _mount(tester, router, _confirm);

    unawaited(router.push<void>(AppRoute.connect));
    await tester.pumpAndSettle();
    expect(find.text('Подключить панель'), findsOneWidget);

    await tester.tap(find.text('Отмена'));
    await tester.pumpAndSettle();

    expect(find.text('ГЛАВНАЯ'), findsOneWidget);
    expect(find.text('Подключить панель'), findsNothing);
  });

  testWidgets('без стека «Отмена» ведёт на «Подключение»', (tester) async {
    // Диплинк холодного старта: приложения под экраном ещё нет, снимать нечего,
    // и запасной выход обязан привести в приложение. Раньше он вёл на /login —
    // на форму входа, которой в этом потоке уже не место.
    final router = _router(AppRoute.connect);
    addTearDown(router.dispose);
    await _mount(tester, router, _confirm);

    await tester.tap(find.text('Отмена'));
    await tester.pumpAndSettle();

    expect(find.text('ГЛАВНАЯ'), findsOneWidget);
  });
}
