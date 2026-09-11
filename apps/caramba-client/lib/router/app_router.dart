import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/desktop/desktop_overlay_page.dart';
import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/shell/desktop_shell.dart';
import 'package:caramba_client/features/auth/login_screen.dart';
import 'package:caramba_client/features/autotune/autotune_screen.dart';
import 'package:caramba_client/features/billing/plans_screen.dart';
import 'package:caramba_client/features/connections/connection_import_screen.dart';
import 'package:caramba_client/features/connections/connections_screen.dart';
import 'package:caramba_client/features/csm/documents_screen.dart';
import 'package:caramba_client/features/csm/operator_identity_screen.dart';
import 'package:caramba_client/features/csm/transport_ladder_screen.dart';
import 'package:caramba_client/features/csm/what_we_send_screen.dart';
import 'package:caramba_client/features/enroll/connect_screen.dart';
import 'package:caramba_client/features/enroll/enroll_screen.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/features/partner/partner_screen.dart';
import 'package:caramba_client/features/profile/profile_desktop.dart';
import 'package:caramba_client/features/profile/profile_screen.dart';
import 'package:caramba_client/features/protocol/protocol_screen.dart';
import 'package:caramba_client/features/referrals/referrals_screen.dart';
import 'package:caramba_client/features/servers/relay_screen.dart';
import 'package:caramba_client/features/servers/servers_desktop.dart';
import 'package:caramba_client/features/servers/servers_screen.dart';
import 'package:caramba_client/features/settings/app_rules_screen.dart';
import 'package:caramba_client/features/settings/settings_desktop.dart';
import 'package:caramba_client/features/settings/settings_screen.dart';
import 'package:caramba_client/features/settings/site_rules_screen.dart';
import 'package:caramba_client/features/splash/splash_screen.dart';
import 'package:caramba_client/features/support/new_ticket_screen.dart';
import 'package:caramba_client/features/support/ticket_detail_screen.dart';
import 'package:caramba_client/features/support/tickets_screen.dart';
import 'package:caramba_client/features/updates/update_required_screen.dart';
import 'package:caramba_client/features/updates/updates_screen.dart';
import 'package:caramba_client/data/models/enrollment.dart';
import 'package:caramba_client/main.dart';
import 'package:caramba_client/router/deep_links.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/shell/app_shell.dart';
import 'package:caramba_client/state/app_update_state.dart';
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
///     аккаунта. Значит «не ждать auth-пробу»; в шелл теперь пускают и без
///     него;
///   * [updateRequired] — панель требует сборку новее установленной: всё
///     приложение перекрывается экраном «Нужно обновиться», и никакая другая
///     локация с него не уводит, пока требование в силе.
String? resolveRedirect({
  required AuthStage stage,
  required bool firstRun,
  required bool bootReady,
  required bool profilesReady,
  required bool guest,
  required String location,
  bool updateRequired = false,
}) {
  // Deeplink приносит локацию с query (`/connections/import?url=...`), а гейт
  // рассуждает о маршруте: сравниваем только путь, иначе pre-auth поток
  // перестаёт узнаваться ровно на той ссылке, ради которой он существует.
  final path = _pathOf(location);
  // Обязательное обновление сильнее любого другого правила: старая сборка не
  // должна ни подключаться, ни вести в онбординг. Когда требование снято
  // (человек обновился — это уже другой процесс, или панель опустила
  // минимум), с экрана уводим в приложение.
  final onUpdateRequired = path == AppRoute.updateRequired;
  if (updateRequired) return onUpdateRequired ? null : AppRoute.updateRequired;
  if (onUpdateRequired) return AppRoute.home;
  final onSplash = path == AppRoute.splash;
  final onLogin = path == AppRoute.login;
  final onAutotune = path == AppRoute.autotune;
  // Энроллмент — pre-auth поток (deeplink/ручной ввод/QR): держим его
  // доступным из unauthenticated, не редиректим на /login.
  final onEnroll = path == AppRoute.enroll;
  // Импорт подписки — второй pre-auth поток (`carambaconnect://import`).
  // Аккаунт для него не нужен вовсе, поэтому уводить на /login тем более
  // нельзя: на холодном старте это съедало ссылку целиком.
  // Подключение по ссылке (`caramba://connect`) — третий: аккаунт на панели у
  // человека уже есть, и весь смысл ссылки в том, что вводить ничего не надо.
  final preAuth =
      onEnroll || path == AppRoute.connectionImport || path == AppRoute.connect;

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
      if (!bootReady || !profilesReady) {
        return onSplash ? null : AppRoute.splash;
      }
      // Своих профилей нет и сессия ещё резолвится: держим сплеш, чтобы не
      // мигнуть шеллом перед возможным входом. Гость (свой профиль или явный
      // generic-режим) auth-пробу не ждёт. Проба — чтение secure storage без
      // сети (AuthNotifier._restore), так что это доли секунды.
      if (!guest && stage == AuthStage.unknown) {
        return onSplash ? null : AppRoute.splash;
      }
      // Шелл открыт всем: вход в панель — раздел, а не дверь. Без подписки
      // «Подключение» показывает пустое состояние с одним действием.
      if (onSplash || onAutotune) return AppRoute.home;
      // /login и /enroll остаются доступны по своей воле.
      return null;
  }
}

/// Путь без query-строки. `state.matchedLocation` её и так не несёт, но гейт
/// вызывается и с сырой локацией (тесты, повтор deeplink'а), поэтому режем.
String _pathOf(String location) {
  final q = location.indexOf('?');
  return q < 0 ? location : location.substring(0, q);
}

/// Роутер приложения. От [GoRouter] отличается ровно одним: маршрут из
/// [AppRoute.overlays] ЛОЖИТСЯ НА СТЕК, а не заменяет его.
///
/// ПОЧЕМУ ЭТО ЖИВЁТ В РОУТЕРЕ, А НЕ В ЭКРАНАХ. `context.go` не переходит, а
/// заменяет весь стек: после `go('/protocol')` в корневом навигаторе остаётся
/// одна страница, шелл из-под неё исчезает, и системной кнопке «Назад»
/// нечего снимать — Android закрывает приложение целиком, а вместе с ним
/// умирал и туннель. Мест вызова таких переходов около тридцати, в пятнадцати
/// файлах, и каждое из них — отдельный шанс написать `go` там, где нужен
/// `push`. Решение принимается один раз и здесь.
///
/// Обратная сторона уже написана: почти каждый полноэкранный экран закрывается
/// через `if (context.canPop()) context.pop()` и лишь потом падает в запасное
/// `go(...)`. То есть приложение всегда рассчитывало на стек — его просто
/// никто не создавал.
class CarambaRouter extends GoRouter {
  /// [canOverlay] — можно ли сейчас класть [location] поверх стека. Гейт
  /// навигации ([resolveRedirect]) умеет увести с маршрута; класть поверх
  /// приложения экран, который редирект тут же заменит, значит оставить в
  /// стеке чужую страницу. Спрашиваем заранее.
  factory CarambaRouter({
    required List<RouteBase> routes,
    required GoRouterRedirect redirect,
    required GlobalKey<NavigatorState> navigatorKey,
    required String initialLocation,
    Listenable? refreshListenable,
    bool Function(String location)? canOverlay,
  }) {
    final config = ValueNotifier<RoutingConfig>(
      RoutingConfig(routes: routes, redirect: redirect),
    );
    return CarambaRouter._(
      config,
      canOverlay,
      navigatorKey: navigatorKey,
      initialLocation: initialLocation,
      refreshListenable: refreshListenable,
    );
  }

  CarambaRouter._(
    ValueNotifier<RoutingConfig> config,
    this._canOverlay, {
    required GlobalKey<NavigatorState> navigatorKey,
    required String initialLocation,
    super.refreshListenable,
  }) : _config = config,
       super.routingConfig(
         routingConfig: config,
         navigatorKey: navigatorKey,
         initialLocation: initialLocation,
       );

  /// Конфигурация принадлежит нам (у [GoRouter.routingConfig] владельца нет),
  /// поэтому и закрывать её нам.
  final ValueNotifier<RoutingConfig> _config;

  final bool Function(String location)? _canOverlay;

  @override
  void go(String location, {Object? extra}) {
    // Повторный переход на уже открытый экран (двойной тап по строке, возврат
    // по той же ссылке). Ни push, ни go здесь не годятся: первый положил бы
    // вторую копию, и «Назад» вернуло бы на тот же экран; второй СНЁС БЫ стек,
    // из-под уже открытого экрана — и «Назад» снова закрыло бы приложение.
    // Мы уже там, где просят. Для /connect это значит, что повторная ссылка
    // caramba:// при открытом экране подтверждения его не перерисовывает —
    // предсуществующее ограничение (initialLink читается в initState),
    // чинится отдельно.
    if (AppRoute.isOverlay(location) && _topLocation() == _pathOf(location)) {
      return;
    }
    if (opensOverStack(location)) {
      // Результат push'а — это то, что вернёт экран через `pop(value)`. Здесь
      // его никто не ждёт: `go` ничего не возвращает по контракту.
      unawaited(push<void>(location, extra: extra));
      return;
    }
    super.go(location, extra: extra);
  }

  /// Ляжет ли переход на [location] поверх текущего стека.
  ///
  /// Открыто для теста: это единственное решение, которое здесь принимается, и
  /// проверять его наблюдением за живым навигатором дороже и хуже.
  @visibleForTesting
  bool opensOverStack(String location) {
    if (!AppRoute.isOverlay(location)) return false;
    final current = routerDelegate.currentConfiguration;
    // Под экраном обязано быть само приложение. На сплеше, логине холодного
    // старта и в энроллменте шелла ещё нет, и класть поверх нечего: там
    // маршрут именно ЗАМЕНЯЕТ то, что показано.
    if (current.isEmpty || current.matches.first is! ShellRouteMatch) {
      return false;
    }
    final gate = _canOverlay;
    if (gate != null && !gate(location)) return false;
    return true;
  }

  /// Путь верхней СТРАНИЦЫ стека. Ветка шелла раскрывается до листа: на
  /// вкладках верхняя страница это `/home` или `/settings`, а не сам шелл.
  String? _topLocation() {
    final current = routerDelegate.currentConfiguration;
    if (current.isEmpty) return null;
    RouteMatchBase match = current.matches.last;
    while (match is ShellRouteMatch) {
      if (match.matches.isEmpty) return null;
      match = match.matches.last;
    }
    return match.matchedLocation;
  }

  @override
  void dispose() {
    super.dispose();
    _config.dispose();
  }
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
/// `authenticating` -> `/home`: шелл открыт без входа; `authenticated` +
/// первый вход -> `/autotune`;
/// `authenticated` -> `/home`. Решение про онбординг ждёт [appBootProvider]:
/// до чтения prefs `firstRunProvider` держит дефолтное `true`, и autotune
/// всплывал бы при каждом запуске. «Тип подключения» (`/protocol`) и «Правила
/// по сайтам» (`/settings/site-rules`) живут вне табов поверх шелла
/// (полноэкранные пикеры с крестиком).
final routerProvider = Provider<GoRouter>((ref) {
  final refresh = _AuthRefresh(ref);
  ref.onDispose(refresh.dispose);

  String? gate(String location) => resolveRedirect(
    stage: ref.read(authProvider).stage,
    firstRun: ref.read(firstRunProvider),
    bootReady: ref.read(appBootReadyProvider),
    profilesReady: ref.read(connectionProfilesReadyProvider),
    guest: ref.read(guestAllowedProvider),
    location: location,
    updateRequired: ref.read(updateRequiredProvider),
  );

  final router = CarambaRouter(
    navigatorKey: _rootKey,
    initialLocation: AppRoute.splash,
    refreshListenable: refresh,
    redirect: (context, state) => gate(state.matchedLocation),
    // Тем же гейтом проверяется и право лечь поверх стека: экран, с которого
    // редирект тут же уведёт, класть туда нельзя.
    canOverlay: (location) => gate(location) == null,
    routes: appRoutes(),
  );

  // Deeplink intake (carambaconnect://enroll|import): стартуем после сборки
  // роутера, чтобы навигация шла в готовый GoRouter. Гасим при dispose.
  final deepLinks = DeepLinkHandler(
    router,
    // Ссылка импорта — вход в generic-режим: флаг говорит гейту не ждать
    // auth-пробу, пока профиль по ссылке ещё не сохранён.
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

/// Страница накладного маршрута.
///
/// ЗАЧЕМ `pageBuilder`, А НЕ `builder`. На мобильном ничего не меняется:
/// [desktopOverlayPage] отдаёт ту же `MaterialPage` с тем же ключом и именем,
/// которую построил бы сам go_router из `builder:`. На десктопе тот же маршрут
/// обязан лечь ПОВЕРХ шелла панелью справа или диалогом по центру — то есть
/// не заменить приложение, а накрыть его, оставив сайдбар и статус видимыми.
/// Решение о ширине и о том, закрывается ли экран по Esc и скриму, принимает
/// таблица в `desktop_overlay_page.dart` — одна на все маршруты, чтобы не
/// повторять её тридцать раз здесь.
Page<void> _overlay(GoRouterState state, Widget child) =>
    desktopOverlayPage<void>(
      key: state.pageKey,
      // Имя берём то же, что подставил бы go_router: страница участвует в
      // `RouteSettings`, и подмена имени сбила бы наблюдателей навигатора.
      name: state.name,
      presentation: presentationFor(state.matchedLocation),
      dismissible: dismissibleFor(state.matchedLocation),
      child: child,
    );

/// Таблица маршрутов приложения.
///
/// Вынесена из [routerProvider] отдельной функцией, чтобы тест мог прочитать её
/// без Riverpod, платформенных каналов и сети: свойство «каждый полноэкранный
/// маршрут объявлен как накладной» проверяется по самой таблице, а не по
/// памяти автора.
List<RouteBase> appRoutes() => <RouteBase>[
  GoRoute(
    path: AppRoute.splash,
    builder: (context, state) => const SplashScreen(),
  ),
  GoRoute(
    path: AppRoute.login,
    pageBuilder: (context, state) => _overlay(state, const LoginScreen()),
  ),
  // Энроллмент: открывается deeplink-хендлером (carambaconnect://enroll),
  // переходом из логина или вручную. `panel`/`code` приходят query-строкой.
  GoRoute(
    path: AppRoute.enroll,
    pageBuilder: (context, state) => _overlay(
      state,
      EnrollScreen(
        initialPanel: state.uri.queryParameters['panel'],
        initialCode: state.uri.queryParameters['code'],
        // k это link_pin ссылки энроллмента. Он доезжает до контроллера, а не
        // теряется по дороге: без него закреплённый энроллмент молча стал бы
        // незакреплённым.
        initialLinkPin: state.uri.queryParameters['k'],
      ),
    ),
  ),
  // Подключение панели по ссылке: `link` приходит целиком, разбор живёт в
  // экране, а не здесь, потому что причина отказа это часть UI.
  GoRoute(
    path: AppRoute.connect,
    pageBuilder: (context, state) => _overlay(
      state,
      ConnectScreen(initialLink: state.uri.queryParameters['link']),
    ),
  ),
  GoRoute(
    path: AppRoute.autotune,
    builder: (context, state) => const AutotuneScreen(),
  ),
  // Повторный автоподбор. РАНЬШЕ ЭТОТ МАРШРУТ БЫЛ ВЛОЖЕН В ВЕТКУ НАСТРОЕК,
  // и это ломало возврат: открывают его с трёх мест (Подключение, Настройки,
  // экран серверов), а «Назад» из ветки всегда приводило в Настройки — то есть
  // туда, откуда человек, как правило, и не приходил. Следующее «Назад» в
  // корне навигатора снимать было уже нечего, и приложение закрывалось.
  // Здесь он такой же полноэкранный пикер, как остальные, и ложится поверх
  // того, откуда его позвали. Путь не изменился.
  GoRoute(
    path: AppRoute.settingsAutotune,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) =>
        _overlay(state, const AutotuneScreen(fromSettings: true)),
  ),
  // Полноэкранные пикеры поверх шелла.
  GoRoute(
    path: AppRoute.protocol,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const ProtocolScreen()),
  ),
  // Списки сайтов. Путь лежит ПОД настройками (`/settings/site-rules`),
  // но маршрут объявлен здесь, а не веткой таба, — по той же причине, что
  // и `/settings/autotune` выше: экран накладной, и «Назад» с него обязано
  // возвращать туда, откуда пришли.
  GoRoute(
    path: AppRoute.siteRules,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const SiteRulesScreen()),
  ),
  // Списки приложений. Тот же накладной экран под настройками, что и списки
  // сайтов, и по той же причине: открывают его со строки настроек, а «Назад»
  // обязано возвращать туда, откуда пришли.
  GoRoute(
    path: AppRoute.appRules,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const AppRulesScreen()),
  ),
  GoRoute(
    path: AppRoute.relay,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const RelayScreen()),
  ),
  // Серверы: накладной экран со строки «Сервер» на «Подключении». Вкладкой
  // быть перестал — владелец: «можно убрать вкладку серверы».
  GoRoute(
    path: AppRoute.servers,
    parentNavigatorKey: _rootKey,
    // Таблица вместо карточек: в панели 720 строка узла читается целиком, а
    // мобильный список там выглядел бы растянутым.
    pageBuilder: (context, state) => _overlay(
      state,
      isDesktopPlatform ? const ServersDesktopScreen() : const ServersScreen(),
    ),
  ),
  // Профили подключения (мульти-профиль) + импорт, поверх шелла.
  GoRoute(
    path: AppRoute.connections,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const ConnectionsScreen()),
    routes: [
      GoRoute(
        path: 'import',
        parentNavigatorKey: _rootKey,
        // `url` приходит из deeplink `carambaconnect://import?url=...`
        // и подставляется в поле ссылки.
        pageBuilder: (context, state) => _overlay(
          state,
          ConnectionImportScreen(initialUrl: state.uri.queryParameters['url']),
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
    pageBuilder: (context, state) =>
        _overlay(state, const OperatorIdentityScreen()),
  ),
  GoRoute(
    path: AppRoute.csmDocuments,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) =>
        _overlay(state, const CsmDocumentsScreen()),
  ),
  GoRoute(
    path: AppRoute.csmTransport,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) =>
        _overlay(state, const TransportLadderScreen()),
  ),
  GoRoute(
    path: AppRoute.csmDisclosure,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const WhatWeSendScreen()),
  ),
  // Тарифы: поверх шелла, как остальные полноэкранные пикеры. Гейта на
  // авторизацию у маршрута нет намеренно — витрину запрашивает экран, и
  // отказ панели («панель не подключена», 404 старой панели) он объясняет
  // словами; редирект на /login вместо объяснения показал бы человеку,
  // нажавшему «Купить», форму входа без единой причины.
  GoRoute(
    path: AppRoute.plans,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const PlansScreen()),
  ),
  GoRoute(
    path: AppRoute.referrals,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const ReferralsScreen()),
  ),
  GoRoute(
    path: AppRoute.partner,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const PartnerScreen()),
  ),
  GoRoute(
    path: AppRoute.notifications,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) =>
        _overlay(state, const NotificationsScreen()),
  ),
  // Обновления: накладной экран под настройками, как списки сайтов.
  GoRoute(
    path: AppRoute.updates,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const UpdatesScreen()),
  ),
  // «Нужно обновиться» заменяет приложение целиком (см. resolveRedirect).
  GoRoute(
    path: AppRoute.updateRequired,
    builder: (context, state) => const UpdateRequiredScreen(),
  ),
  GoRoute(
    path: AppRoute.tickets,
    parentNavigatorKey: _rootKey,
    pageBuilder: (context, state) => _overlay(state, const TicketsScreen()),
    routes: [
      // Статический сегмент идёт раньше параметра: /tickets/new.
      GoRoute(
        path: 'new',
        parentNavigatorKey: _rootKey,
        pageBuilder: (context, state) =>
            _overlay(state, const NewTicketScreen()),
      ),
      GoRoute(
        path: ':id',
        parentNavigatorKey: _rootKey,
        pageBuilder: (context, state) {
          final id = int.tryParse(state.pathParameters['id'] ?? '') ?? 0;
          return _overlay(state, TicketDetailScreen(ticketId: id));
        },
      ),
    ],
  ),
  StatefulShellRoute.indexedStack(
    // Ветка по ПЛАТФОРМЕ, а не по ширине окна: мобильный шелл со своими двумя
    // раскладками остаётся нетронутым, десктоп получает сайдбар, тулбар и
    // клавиатуру. Узкое окно на Маке остаётся десктопом.
    builder: (context, state, shell) => isDesktopPlatform
        ? DesktopShell(navigationShell: shell)
        : AppShell(navigationShell: shell),
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
            path: AppRoute.profile,
            builder: (context, state) => isDesktopPlatform
                ? const ProfileDesktopScreen()
                : const ProfileScreen(),
          ),
        ],
      ),
      StatefulShellBranch(
        routes: [
          GoRoute(
            path: AppRoute.settings,
            builder: (context, state) => isDesktopPlatform
                ? const SettingsDesktopScreen()
                : const SettingsScreen(),
          ),
        ],
      ),
    ],
  ),
];

/// Bridges [authProvider] + first-run changes into a [Listenable] go_router
/// can refresh on.
class _AuthRefresh extends ChangeNotifier {
  /// Пробуждение роутера уже запланировано на этот тик: несколько провайдеров
  /// меняются одной пачкой, и будить его по разу на каждый — лишние проходы
  /// редиректа с промежуточными состояниями.
  bool _notifyScheduled = false;
  bool _disposed = false;

  late final ProviderSubscription<AuthState> _authSub;
  late final ProviderSubscription<bool> _firstRunSub;
  late final ProviderSubscription<bool> _bootSub;
  late final ProviderSubscription<bool> _guestSub;
  late final ProviderSubscription<bool> _profilesSub;
  late final ProviderSubscription<bool> _updateSub;

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
    // Требование панели обновиться перекрывает приложение экраном: роутер
    // обязан проснуться, когда оно появилось или снято. Подписка заодно
    // запускает саму проверку версии (appUpdateProvider ленивый).
    _updateSub = ref.listen<bool>(
      updateRequiredProvider,
      _onFlag,
      fireImmediately: true,
    );
  }

  void _onAuth(AuthState? prev, AuthState next) {
    if (prev?.stage != next.stage) _notifySoon();
  }

  void _onFlag(bool? prev, bool next) {
    if (prev != next) _notifySoon();
  }

  /// Будит роутер ПОСЛЕ того, как рассылка провайдера закончилась.
  ///
  /// Синхронный `notifyListeners` здесь разрушал приложение. Цепочка такая:
  /// `authProvider` рассылает новое состояние, обходя свой список слушателей;
  /// наш слушатель дёргает роутер; тот синхронно перестраивает дерево; новые
  /// экраны подписываются на `authProvider`, а уходящие отписываются — и список
  /// меняется прямо посреди обхода. `StateNotifier` падал с «Concurrent
  /// modification during iteration», кадр обрывался на полпути, в дереве
  /// оставались ДВА навигатора с одним глобальным ключом, и дальше Flutter кидал
  /// «Duplicate GlobalKey» на каждом кадре. Снаружи это выглядело так, будто
  /// приложение вечно «подбирает настройки»: два экрана друг на друге и
  /// бесконечная перерисовка, из которой нельзя выйти.
  ///
  /// Микрозадача разрывает именно это: к моменту пробуждения роутера рассылка
  /// уже завершилась, и подписки можно менять безнаказанно. Задержка на один
  /// тик безвредна — редирект всё равно считает состояние заново и читает его
  /// свежим.
  void _notifySoon() {
    if (_notifyScheduled) return;
    _notifyScheduled = true;
    Future<void>.microtask(() {
      _notifyScheduled = false;
      // Контейнер мог закрыться между уведомлением и микрозадачей.
      if (_disposed) return;
      notifyListeners();
    });
  }

  @override
  void dispose() {
    _disposed = true;
    _authSub.close();
    _firstRunSub.close();
    _bootSub.close();
    _guestSub.close();
    _profilesSub.close();
    _updateSub.close();
    super.dispose();
  }
}
