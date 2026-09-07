// Закрытие экрана «Серверы».
//
// Экран перестал быть вкладкой и лёг ПОВЕРХ шелла: его открывают со строки
// «Сервер» на «Подключении». Значит крестик обязан снимать экран со стека, а
// не уводить на другую вкладку: `go(/home)` из накладного экрана стирал стек и
// возвращал человека не туда, откуда он пришёл. Второй случай — стека под
// экраном нет вовсе (диплинк, холодный старт, этот тест): тогда выход один —
// само «Подключение».
//
// Тест не зависит от того, как маршрут объявлен в боевом роутере: экран
// кладётся `router.push` на голом [GoRouter] из двух маршрутов.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/features/servers/servers_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_models.dart';
import 'package:caramba_client/widgets/ui.dart' show IconBtn;

import 'support/fake_core.dart';

/// Профили из памяти: secure storage в тесте не поднимаем.
class _FakeProfilesStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;

  _FakeProfilesStore(this.profiles, this.activeId);

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
    profiles = const [];
    activeId = null;
  }
}

const _nodes = <ImportedServer>[
  ImportedServer(
    id: 'nl-1',
    name: 'Amsterdam #1',
    type: 'vless',
    server: 'a.example',
    port: 443,
    country: 'NL',
  ),
];

/// Профиль с узлом: на пустом сторе экран уходит в ветку «профиль не выбран», и
/// проверялась бы не та страница, которую закрывают в жизни.
ConnectionProfile _profile() => ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://sub.example/a',
  rawConfig: 'proxies: []',
  format: 'clash',
  servers: _nodes,
  serversUpdatedMs: DateTime.now().millisecondsSinceEpoch,
);

/// Входы в форме `GET /app/relays`: провайдер спрашивают снизу, и живой HTTP в
/// тесте не поднимается.
final _panelRelays = Relay.fromCountries(<Relay>[
  Relay.fromApiJson(const <String, dynamic>{
    'country_code': 'TR',
    'country_name': 'Турция',
    'node_count': 2,
  }),
]);

/// Роутер из двух маршрутов: «Подключение» здесь — заглушка, потому что
/// проверяется не оно, а то, КУДА уводит крестик.
GoRouter _router(String at) => GoRouter(
  initialLocation: at,
  routes: <RouteBase>[
    GoRoute(
      path: AppRoute.home,
      builder: (context, state) => const Text('ГЛАВНАЯ'),
    ),
    GoRoute(
      path: AppRoute.servers,
      builder: (context, state) => const ServersScreen(),
    ),
  ],
);

Widget _app(GoRouter router) => ProviderScope(
  overrides: [
    vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
    connectionProfilesStoreProvider.overrideWithValue(
      _FakeProfilesStore(<ConnectionProfile>[_profile()], 'cp_1'),
    ),
    apiRelaysProvider.overrideWith((ref) async => _panelRelays),
  ],
  child: MaterialApp.router(theme: AppTheme.dark(), routerConfig: router),
);

/// Высокое окно: список строится ленивым сливером, и на окне 800x600 тест
/// проверял бы прокрутку, а не видимость.
void _useTallView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(900, 2600)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

/// Профили читаются асинхронно: до них экран ещё «без профиля».
Future<void> _settle(WidgetTester tester) async {
  await tester.pump();
  await tester.pump();
  await tester.pump();
}

void main() {
  group('закрытие экрана «Серверы»', () {
    testWidgets('крестик снимает «Серверы» и возвращает туда, откуда пришли', (
      tester,
    ) async {
      _useTallView(tester);
      final router = _router(AppRoute.home);
      addTearDown(router.dispose);
      await tester.pumpWidget(_app(router));
      await _settle(tester);

      // `push` завершается только когда экран снимут; ждать его здесь значит
      // повиснуть до собственного крестика.
      unawaited(router.push(AppRoute.servers));
      await _settle(tester);
      // Экран приезжает анимацией: без её докрутки крестик стоит за краем
      // окна, и тап уходит мимо.
      await tester.pumpAndSettle();
      expect(find.text('Серверы'), findsOneWidget);

      await tester.tap(find.byType(IconBtn));
      await tester.pumpAndSettle();

      // Именно возврат: экран снят со стека, а под ним — тот же «Подключение»,
      // с которого его открыли.
      expect(find.text('ГЛАВНАЯ'), findsOneWidget);
      expect(find.text('Серверы'), findsNothing);
      expect(tester.takeException(), isNull);
    });

    testWidgets('без стека крестик ведёт на «Подключение»', (tester) async {
      _useTallView(tester);
      final router = _router(AppRoute.servers);
      addTearDown(router.dispose);
      await tester.pumpWidget(_app(router));
      await _settle(tester);

      // Снимать нечего — `pop` здесь уронил бы приложение в пустой стек,
      // поэтому запасной выход обязан быть явным.
      await tester.tap(find.byType(IconBtn));
      await tester.pumpAndSettle();

      expect(find.text('ГЛАВНАЯ'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  });
}
