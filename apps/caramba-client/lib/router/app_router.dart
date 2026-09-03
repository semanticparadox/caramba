import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/features/auth/login_screen.dart';
import 'package:caramba_client/features/autotune/autotune_screen.dart';
import 'package:caramba_client/features/connections/connection_import_screen.dart';
import 'package:caramba_client/features/connections/connections_screen.dart';
import 'package:caramba_client/features/csm/documents_screen.dart';
import 'package:caramba_client/features/csm/operator_identity_screen.dart';
import 'package:caramba_client/features/csm/transport_ladder_screen.dart';
import 'package:caramba_client/features/csm/what_we_send_screen.dart';
import 'package:caramba_client/features/enroll/enroll_screen.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/features/partner/partner_screen.dart';
import 'package:caramba_client/features/profile/profile_screen.dart';
import 'package:caramba_client/features/protocol/protocol_screen.dart';
import 'package:caramba_client/features/referrals/referrals_screen.dart';
import 'package:caramba_client/features/servers/servers_screen.dart';
import 'package:caramba_client/features/settings/settings_screen.dart';
import 'package:caramba_client/features/splash/splash_screen.dart';
import 'package:caramba_client/features/split/split_tunnel_screen.dart';
import 'package:caramba_client/features/support/new_ticket_screen.dart';
import 'package:caramba_client/features/support/ticket_detail_screen.dart';
import 'package:caramba_client/features/support/tickets_screen.dart';
import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/main.dart';
import 'package:caramba_client/router/deep_links.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/shell/app_shell.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/bootstrap_state.dart';
import 'package:caramba_client/state/settings_state.dart';

final _rootKey = GlobalKey<NavigatorState>(debugLabel: 'root');

/// Решение гейта навигации: куда отправить пользователя из [location].
/// `null` означает «оставить где есть».
///
/// Вынесено чистой функцией из [routerProvider] намеренно: это единственная
/// нетривиальная логика роутера, и она обязана быть проверяемой без поднятия
/// GoRouter, платформенных каналов и сети.
///
/// Три входа помимо стадии сессии:
///   * [bootReady] — прочитаны локальные настройки (иначе `firstRun` врёт
///     дефолтом `true` и онбординг всплывал бы каждый запуск);
///   * [profilesReady] — прочитан список профилей подключения (иначе холодный
///     старт с импортированной подпиской успевал бы отскочить на `/login`);
///   * [guest] — generic-режим: есть своя подписка либо явно выбран режим без
///     аккаунта. Такой пользователь работает в шелле без входа в панель.
String? resolveRedirect({
  required AuthStage stage,
  required bool firstRun,
  required bool bootReady,
  required bool profilesReady,
  required bool guest,
  required String location,
}) {
  // Deeplink приносит локацию с query (`/connections/import?url=...`), а гейт
  // рассуждает о маршруте: сравниваем только путь, иначе pre-auth поток
  // перестаёт узнаваться ровно на той ссылке, ради которой он существует.
  final path = _pathOf(location);
  final onSplash = path == AppRoute.splash;
  final onLogin = path == AppRoute.login;
  final onAutotune = path == AppRoute.autotune;
  // Энроллмент — pre-auth поток (deeplink/ручной ввод/QR): держим его
  // доступным из unauthenticated, не редиректим на /login.
  final onEnroll = path == AppRoute.enroll;
  // Импорт подписки — второй pre-auth поток (`carambaconnect://import`).
  // Аккаунт для него не нужен вовсе, поэтому уводить на /login тем более
  // нельзя: на холодном старте это съедало ссылку целиком.
  final preAuth = onEnroll || path == AppRoute.connectionImport;

  switch (stage) {
    case AuthStage.authenticated:
      // Настройки ещё не прочитаны: держим сплеш, иначе увели бы в онбординг
      // по дефолтному firstRun ещё до загрузки сохранённого.
      if (!bootReady) return onSplash ? null : AppRoute.splash;
      if (firstRun) return onAutotune ? null : AppRoute.autotune;
      // Уводим из pre-auth экранов (вкл. /enroll) в приложение после входа.
      if (onSplash || onLogin || onAutotune || onEnroll) return AppRoute.home;
      return null;

    case AuthStage.unknown:
    case AuthStage.unauthenticated:
    case AuthStage.authenticating:
      // Deeplink на холодном старте: пока сессия резолвится, не сбрасываем
      // /enroll и /connections/import на сплеш или логин (иначе панель/код или
      // ссылка подписки из URI потеряются).
      if (preAuth) return null;
      // Локальное состояние ещё грузится: решение «гость или нет» без него
      // было бы принято по дефолтам.
      if (!bootReady || !profilesReady) {
        return onSplash ? null : AppRoute.splash;
      }
      // Generic-режим: своя подписка вместо аккаунта панели. Пускаем в шелл,
      // не дожидаясь ни логина, ни завершения auth-пробы — иначе подключаться
      // есть чем, а приложение упирается в /login.
      if (guest) {
        if (onSplash || onAutotune) return AppRoute.home;
        // /login и /enroll остаются доступны по своей воле: гость может в
        // любой момент привязать аккаунт панели.
        return null;
      }
      if (stage == AuthStage.unknown) return onSplash ? null : AppRoute.splash;
      return onLogin ? null : AppRoute.login;
  }
}

/// Путь без query-строки. `state.matchedLocation` её и так не несёт, но гейт
/// вызывается и с сырой локацией (тесты, повтор deeplink'а), поэтому режем.
String _pathOf(String location) {
  final q = location.indexOf('?');
  return q < 0 ? location : location.substring(0, q);
}

/// Гейт роутера открыт: локальные настройки и профили прочитаны, и redirect
/// больше не держит сплеш. До этого момента любая навигация по deeplink была бы
/// съедена, поэтому [DeepLinkHandler] ждёт именно его, чтобы повторить ссылку.
final _routerGateReadyProvider = Provider<bool>(
  (ref) =>
      ref.watch(appBootReadyProvider) &&
      ref.watch(connectionProfilesReadyProvider),
);

/// Application router (Riverpod-aware).
///
/// Auth-gating: cold start lands on `/` (splash), which mounts NO protected
/// providers. While `unknown` we hold on the splash; `unauthenticated`/
/// `authenticating` -> `/login`; `authenticated` + первый вход -> `/autotune`;
/// `authenticated` -> `/home`. Решение про онбординг ждёт [appBootProvider]:
/// до чтения prefs `firstRunProvider` держит дефолтное `true`, и autotune
/// всплывал бы при каждом запуске. Протокол и split-tunnel живут вне табов поверх
/// шелла (полноэкранные пикеры с крестиком).
final routerProvider = Provider<GoRouter>((ref) {
  final refresh = _AuthRefresh(ref);
  ref.onDispose(refresh.dispose);

  final router = GoRouter(
    navigatorKey: _rootKey,
    initialLocation: AppRoute.splash,
    refreshListenable: refresh,
    redirect: (context, state) => resolveRedirect(
      stage: ref.read(authProvider).stage,
      firstRun: ref.read(firstRunProvider),
      bootReady: ref.read(appBootReadyProvider),
      profilesReady: ref.read(connectionProfilesReadyProvider),
      guest: ref.read(guestAllowedProvider),
      location: state.matchedLocation,
    ),
    routes: [
      GoRoute(
        path: AppRoute.splash,
        builder: (context, state) => const SplashScreen(),
      ),
      GoRoute(
        path: AppRoute.login,
        builder: (context, state) => const LoginScreen(),
      ),
      // Энроллмент: открывается deeplink-хендлером (carambaconnect://enroll),
      // переходом из логина или вручную. `panel`/`code` приходят query-строкой.
      GoRoute(
        path: AppRoute.enroll,
        builder: (context, state) => EnrollScreen(
          initialPanel: state.uri.queryParameters['panel'],
          initialCode: state.uri.queryParameters['code'],
          // k это link_pin ссылки энроллмента. Он доезжает до контроллера, а не
          // теряется по дороге: без него закреплённый энроллмент молча стал бы
          // незакреплённым.
          initialLinkPin: state.uri.queryParameters['k'],
        ),
      ),
      GoRoute(
        path: AppRoute.autotune,
        builder: (context, state) => const AutotuneScreen(),
      ),
      // Полноэкранные пикеры поверх шелла.
      GoRoute(
        path: AppRoute.protocol,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const ProtocolScreen(),
      ),
      GoRoute(
        path: AppRoute.splitTunnel,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const SplitTunnelScreen(),
      ),
      // Профили подключения (мульти-профиль) + импорт, поверх шелла.
      GoRoute(
        path: AppRoute.connections,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const ConnectionsScreen(),
        routes: [
          GoRoute(
            path: 'import',
            parentNavigatorKey: _rootKey,
            // `url` приходит из deeplink `carambaconnect://import?url=...`
            // и подставляется в поле ссылки.
            builder: (context, state) => ConnectionImportScreen(
              initialUrl: state.uri.queryParameters['url'],
            ),
          ),
        ],
      ),
      // Экраны проверки CSM/1: личность оператора, состояние документов,
      // лестница транспортов и раскрытие отправляемых полей. Полноэкранные
      // поверх шелла, как остальные пикеры.
      GoRoute(
        path: AppRoute.csmOperator,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const OperatorIdentityScreen(),
      ),
      GoRoute(
        path: AppRoute.csmDocuments,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const CsmDocumentsScreen(),
      ),
      GoRoute(
        path: AppRoute.csmTransport,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const TransportLadderScreen(),
      ),
      GoRoute(
        path: AppRoute.csmDisclosure,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const WhatWeSendScreen(),
      ),
      GoRoute(
        path: AppRoute.referrals,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const ReferralsScreen(),
      ),
      GoRoute(
        path: AppRoute.partner,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const PartnerScreen(),
      ),
      GoRoute(
        path: AppRoute.notifications,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const NotificationsScreen(),
      ),
      GoRoute(
        path: AppRoute.tickets,
        parentNavigatorKey: _rootKey,
        builder: (context, state) => const TicketsScreen(),
        routes: [
          // Статический сегмент идёт раньше параметра: /tickets/new.
          GoRoute(
            path: 'new',
            parentNavigatorKey: _rootKey,
            builder: (context, state) => const NewTicketScreen(),
          ),
          GoRoute(
            path: ':id',
            parentNavigatorKey: _rootKey,
            builder: (context, state) {
              final id = int.tryParse(state.pathParameters['id'] ?? '') ?? 0;
              return TicketDetailScreen(ticketId: id);
            },
          ),
        ],
      ),
      StatefulShellRoute.indexedStack(
        builder: (context, state, shell) => AppShell(navigationShell: shell),
        branches: [
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: AppRoute.home,
                builder: (context, state) => const HomeScreen(),
              ),
            ],
          ),
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: AppRoute.servers,
                builder: (context, state) => const ServersScreen(),
              ),
            ],
          ),
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: AppRoute.profile,
                builder: (context, state) => const ProfileScreen(),
              ),
            ],
          ),
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: AppRoute.settings,
                builder: (context, state) => const SettingsScreen(),
                routes: [
                  GoRoute(
                    path: 'autotune',
                    builder: (context, state) =>
                        const AutotuneScreen(fromSettings: true),
                  ),
                ],
              ),
            ],
          ),
        ],
      ),
    ],
  );

  // Deeplink intake (carambaconnect://enroll|import): стартуем после сборки
  // роутера, чтобы навигация шла в готовый GoRouter. Гасим при dispose.
  final deepLinks = DeepLinkHandler(
    router,
    // Ссылка импорта — вход в generic-режим: без этого флага пользователь без
    // аккаунта панели отскочил бы на /login с уже открытого экрана импорта.
    onImport: () => ref.read(guestModeProvider.notifier).enable(),
    // Отказ показываем: ссылка без TLS (INV-8) или без кода иначе просто
    // ничего не делает, и это неотличимо от зависшего приложения.
    onRefused: (refusal) {
      final messenger = rootMessengerKey.currentState;
      if (messenger == null) return;
      messenger.clearSnackBars();
      messenger.showSnackBar(
        SnackBar(
          duration: const Duration(milliseconds: 3200),
          content: Text(refusal.message),
        ),
      );
    },
  );
  unawaited(deepLinks.start());
  ref.onDispose(deepLinks.dispose);

  // Ссылка холодного старта приходит раньше, чем гейт открылся: повторяем её,
  // как только локальные настройки и профили прочитаны.
  void onGate(bool? prev, bool next) {
    if (next) deepLinks.replayPending();
  }

  final gateSub = ref.listen<bool>(
    _routerGateReadyProvider,
    onGate,
    fireImmediately: true,
  );
  ref.onDispose(gateSub.close);

  return router;
});

/// Bridges [authProvider] + first-run changes into a [Listenable] go_router
/// can refresh on.
class _AuthRefresh extends ChangeNotifier {
  late final ProviderSubscription<AuthState> _authSub;
  late final ProviderSubscription<bool> _firstRunSub;
  late final ProviderSubscription<bool> _bootSub;
  late final ProviderSubscription<bool> _guestSub;
  late final ProviderSubscription<bool> _profilesSub;

  _AuthRefresh(Ref ref) {
    _authSub = ref.listen<AuthState>(authProvider, _onAuth);
    _firstRunSub = ref.listen<bool>(firstRunProvider, _onFlag);
    // Подписки с fireImmediately заодно СТАРТУЮТ провайдеры: пока настройки и
    // профили не прочитаны, редирект держит сплеш.
    _bootSub = ref.listen<bool>(
      appBootReadyProvider,
      _onFlag,
      fireImmediately: true,
    );
    // Generic-режим и наличие профилей решают, пускать ли в шелл без входа.
    _guestSub = ref.listen<bool>(
      guestAllowedProvider,
      _onFlag,
      fireImmediately: true,
    );
    _profilesSub = ref.listen<bool>(
      connectionProfilesReadyProvider,
      _onFlag,
      fireImmediately: true,
    );
  }

  void _onAuth(AuthState? prev, AuthState next) {
    if (prev?.stage != next.stage) notifyListeners();
  }

  void _onFlag(bool? prev, bool next) {
    if (prev != next) notifyListeners();
  }

  @override
  void dispose() {
    _authSub.close();
    _firstRunSub.close();
    _bootSub.close();
    _guestSub.close();
    _profilesSub.close();
    super.dispose();
  }
}
