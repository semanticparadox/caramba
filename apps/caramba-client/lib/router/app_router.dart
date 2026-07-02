import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/features/auth/login_screen.dart';
import 'package:caramba_client/features/autotune/autotune_screen.dart';
import 'package:caramba_client/features/connections/connection_import_screen.dart';
import 'package:caramba_client/features/connections/connections_screen.dart';
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
import 'package:caramba_client/router/deep_links.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/shell/app_shell.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/settings_state.dart';

final _rootKey = GlobalKey<NavigatorState>(debugLabel: 'root');

/// Application router (Riverpod-aware).
///
/// Auth-gating: cold start lands on `/` (splash), which mounts NO protected
/// providers. While `unknown` we hold on the splash; `unauthenticated`/
/// `authenticating` -> `/login`; `authenticated` + первый вход -> `/autotune`;
/// `authenticated` -> `/home`. Протокол и split-tunnel живут вне табов поверх
/// шелла (полноэкранные пикеры с крестиком).
final routerProvider = Provider<GoRouter>((ref) {
  final refresh = _AuthRefresh(ref);
  ref.onDispose(refresh.dispose);

  final router = GoRouter(
    navigatorKey: _rootKey,
    initialLocation: AppRoute.splash,
    refreshListenable: refresh,
    redirect: (context, state) {
      final auth = ref.read(authProvider);
      final firstRun = ref.read(firstRunProvider);
      final loc = state.matchedLocation;
      final onSplash = loc == AppRoute.splash;
      final onLogin = loc == AppRoute.login;
      final onAutotune = loc == AppRoute.autotune;
      // Энроллмент — pre-auth поток (deeplink/ручной ввод/QR): держим его
      // доступным из unauthenticated, не редиректим на /login.
      final onEnroll = loc == AppRoute.enroll;

      switch (auth.stage) {
        case AuthStage.unknown:
          // Deeplink на холодном старте: пока сессия резолвится, не сбрасываем
          // /enroll на сплеш (иначе панель/код из ссылки потеряются).
          if (onEnroll) return null;
          return onSplash ? null : AppRoute.splash;
        case AuthStage.unauthenticated:
        case AuthStage.authenticating:
          if (onEnroll) return null;
          return onLogin ? null : AppRoute.login;
        case AuthStage.authenticated:
          if (firstRun) {
            return onAutotune ? null : AppRoute.autotune;
          }
          // Уводим из pre-auth экранов (вкл. /enroll) в приложение после входа.
          if (onSplash || onLogin || onAutotune || onEnroll) {
            return AppRoute.home;
          }
          return null;
      }
    },
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
            builder: (context, state) => const ConnectionImportScreen(),
          ),
        ],
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

  // Deeplink intake (carambaconnect://enroll): стартуем после сборки роутера,
  // чтобы навигация на /enroll шла в готовый GoRouter. Гасим при dispose.
  final deepLinks = DeepLinkHandler(router);
  unawaited(deepLinks.start());
  ref.onDispose(deepLinks.dispose);

  return router;
});

/// Bridges [authProvider] + first-run changes into a [Listenable] go_router
/// can refresh on.
class _AuthRefresh extends ChangeNotifier {
  late final ProviderSubscription<AuthState> _authSub;
  late final ProviderSubscription<bool> _firstRunSub;

  _AuthRefresh(Ref ref) {
    _authSub = ref.listen<AuthState>(authProvider, (prev, next) {
      if (prev?.stage != next.stage) notifyListeners();
    });
    _firstRunSub = ref.listen<bool>(firstRunProvider, (prev, next) {
      if (prev != next) notifyListeners();
    });
  }

  @override
  void dispose() {
    _authSub.close();
    _firstRunSub.close();
    super.dispose();
  }
}
