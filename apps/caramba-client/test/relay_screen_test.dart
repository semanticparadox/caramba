// Экран «Вход»: объяснение, «Авто»/«Выкл» и группы «страна → её релеи».
//
// Заказ владельца: «логика Relay непонятна, нельзя выбрать вход через Россию
// вручную; будет много релеев по регионам». Проверяется ровно это:
//  * `GET /relays` теперь отдаёт узлы внутри страны, и модель их разворачивает
//    в строки пикера, не сдвигая индексы стран (индекс хранится в настройках);
//  * экран показывает страну и под ней её релеи с городом и пингом панели, и
//    выбрать можно как всю страну, так и конкретный релей;
//  * выбор уходит в `CoreConfig.relay` тем индексом, который показан, а на
//    панель — точной формой (`none` / страна / `node:<id>`).

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/features/servers/relay_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/widgets/ui.dart';

import 'support/fake_core.dart';

/// Ответ `GET /relays` новой формы: одна страна, два релея с городом и пингом
/// панели.
const _ruJson = <String, dynamic>{
  'country_code': 'RU',
  'country_name': 'Россия',
  'flag': '🇷🇺',
  'node_count': 2,
  'nodes': <Map<String, dynamic>>[
    <String, dynamic>{
      'id': 12,
      'name': 'msk-1',
      'city': 'Moscow',
      'load_pct': 30.0,
      'latency_ms': 31,
      'sort_order': 10,
    },
    <String, dynamic>{
      'id': 13,
      'name': 'spb-1',
      'city': null,
      'load_pct': 12.5,
      'latency_ms': 44,
      'sort_order': 20,
    },
  ],
};

/// Прежняя форма: только страна и счётчик (панель старше узлов не отдаёт).
const _ruLegacyJson = <String, dynamic>{
  'country_code': 'RU',
  'country_name': 'Россия',
  'node_count': 1,
};

const _wire = Provenance(OfferingSource.panelRest, '/app/servers');

RelayOffer _hop(int id, String name, {bool chained = false}) => RelayOffer(
  panelNodeId: id,
  countryCode: 'RU',
  countryName: 'Россия',
  label: name,
  reachableFromExitKeys: const <String>['1'],
  reachability: const Availability.available(_wire),
  availability: chained
      ? const Availability.available(_wire)
      : const Availability.unavailable(
          OfferingReason.relayNotChainedByGenerator,
          _wire,
        ),
);

class _Store implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;
  _Store(this.profiles, this.activeId);

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

ConnectionProfile _panelProfile() => const ConnectionProfile(
  id: 'cp_panel',
  type: ProfileType.panelAccount,
  displayName: 'Оператор',
  source: 'https://panel.example',
  selectedExitNodeId: 1,
  selectedExitCountry: 'DE',
);

GoRouter _router() => GoRouter(
  initialLocation: AppRoute.relay,
  routes: <RouteBase>[
    GoRoute(
      path: AppRoute.home,
      builder: (context, state) => const Text('ГЛАВНАЯ'),
    ),
    GoRoute(
      path: AppRoute.relay,
      builder: (context, state) => const RelayScreen(),
    ),
  ],
);

Widget _app(List<Relay> relays, {int? relayIndex}) => ProviderScope(
  overrides: [
    vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
    connectionProfilesStoreProvider.overrideWithValue(
      _Store(<ConnectionProfile>[_panelProfile()], 'cp_panel'),
    ),
    serversProvider.overrideWith((ref) async => const <Server>[]),
    apiRelaysProvider.overrideWith((ref) async => relays),
    // Без хранилища prefs: тест читает индекс из состояния, а не с диска.
    coreConfigProvider.overrideWith(
      (ref) =>
          CoreConfigNotifier()..hydrate(CoreConfig(relay: relayIndex ?? 0)),
    ),
  ],
  child: MaterialApp.router(theme: AppTheme.dark(), routerConfig: _router()),
);

void _useTallView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(900, 3000)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump();
  }
}

ListItemCard _cardOf(WidgetTester tester, String title) =>
    tester.widget<ListItemCard>(
      find.ancestor(of: find.text(title), matching: find.byType(ListItemCard)),
    );

void main() {
  group('модель Relay: узлы из /relays', () {
    test('страна разворачивается в узлы, индексы стран не сдвигаются', () {
      final ru = Relay.fromApiJson(_ruJson);
      expect(ru.isCountry, isTrue);
      expect(ru.nodeCount, 2);
      expect(ru.flag, '🇷🇺');
      expect(ru.nodes.map((n) => n.name), <String>['msk-1', 'spb-1']);
      expect(ru.nodes.first.isNode, isTrue);
      expect(ru.nodes.first.nodeId, 12);
      expect(ru.nodes.first.city, 'Moscow');
      expect(ru.nodes.first.latencyMs, 31);
      expect(ru.nodes.first.country, 'RU');
      expect(ru.nodes[1].city, isNull);

      // Порядок списка записи: Выкл, Авто, страны, потом узлы. Индекс страны
      // остаётся 2, как и до появления узлов: он хранится в настройках.
      final list = Relay.fromCountries(<Relay>[ru]);
      expect(list.length, 5);
      expect(list[0].isOff, isTrue);
      expect(list[1].isAuto, isTrue);
      expect(list[2].isCountry, isTrue);
      expect(list[3].nodeId, 12);
      expect(list[4].nodeId, 13);
    });

    test('прежняя форма без узлов читается как раньше', () {
      final ru = Relay.fromApiJson(_ruLegacyJson);
      expect(ru.nodes, isEmpty);
      expect(Relay.fromCountries(<Relay>[ru]).length, 3);
    });

    test('форма выбора для панели: none / страна / node:<id> / сброс', () {
      final ru = Relay.fromApiJson(_ruJson);
      expect(Relay.defaults[0].pinValue, 'none');
      expect(Relay.defaults[1].pinValue, isNull);
      expect(ru.pinValue, 'RU');
      expect(ru.nodes.first.pinValue, 'node:12');
      // Ядру и CSM узел уходит страной: их словарь это ISO-2.
      expect(ru.nodes.first.countryCode, 'RU');
    });
  });

  group('buildRelayGroups', () {
    const chainOk = Availability.available(
      Provenance(OfferingSource.panelRest, '/app/servers'),
    );

    test('узлы из /relays идут под своей страной со своими индексами', () {
      final relays = Relay.fromCountries(<Relay>[Relay.fromApiJson(_ruJson)]);
      final groups = buildRelayGroups(relays, const <RelayOffer>[], chainOk);
      expect(groups.length, 1);
      final ru = groups.single;
      expect(ru.countryCode, 'RU');
      expect(ru.countryName, 'Россия');
      expect(ru.writeIndex, 2);
      expect(ru.pin.pinValue, 'RU');
      expect(ru.nodes.map((n) => n.nodeId), <int>[12, 13]);
      expect(ru.nodes.map((n) => n.writeIndex), <int>[3, 4]);
      expect(ru.nodes.first.pin.pinValue, 'node:12');
      expect(ru.nodes.first.availability.isAvailable, isTrue);
    });

    test('узел только из via_relay пишется индексом страны', () {
      final relays = Relay.fromCountries(<Relay>[
        Relay.fromApiJson(_ruLegacyJson),
      ]);
      final hops = <RelayOffer>[_hop(2, 'RU relay')];
      final groups = buildRelayGroups(relays, hops, chainOk);
      final ru = groups.single;
      expect(ru.writeIndex, 2);
      expect(ru.nodes.single.nodeId, 2);
      expect(ru.nodes.single.writeIndex, 2, reason: 'индекс страны');
      expect(ru.nodes.single.pin.pinValue, 'node:2');
      // Свидетельство панели по узлу важнее общей возможности.
      expect(ru.nodes.single.availability.isUnavailable, isTrue);
      expect(ru.availability.isUnavailable, isFalse);
    });

    test('свидетельство панели по узлу склеивается с узлом из /relays', () {
      final relays = Relay.fromCountries(<Relay>[Relay.fromApiJson(_ruJson)]);
      final hops = <RelayOffer>[_hop(12, 'msk-1', chained: true)];
      final groups = buildRelayGroups(relays, hops, chainOk);
      final ru = groups.single;
      expect(ru.nodes.length, 2, reason: 'узел не задвоился');
      expect(ru.nodes.first.reachableExits, 1);
      expect(ru.availability.isAvailable, isTrue);
    });

    test('страна, которой нет в /relays, не закрепляется и названа', () {
      final hops = <RelayOffer>[_hop(2, 'RU relay', chained: true)];
      final groups = buildRelayGroups(Relay.defaults, hops, chainOk);
      final ru = groups.single;
      expect(ru.writeIndex, isNull);
      expect(ru.nodes.single.writeIndex, isNull);
      expect(
        ru.nodes.single.availability.reason,
        OfferingReason.panelReportsRelaysByCountryOnly,
      );
    });
  });

  group('экран «Вход»', () {
    testWidgets('страна и под ней релеи с городом и пингом панели', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(
        _app(Relay.fromCountries(<Relay>[Relay.fromApiJson(_ruJson)])),
      );
      await _settle(tester);

      expect(find.textContaining('Вход это сервер'), findsOneWidget);
      expect(find.text('Выкл'), findsOneWidget);
      expect(find.text('Авто'), findsOneWidget);
      expect(find.text('Россия'), findsOneWidget);
      expect(find.text('msk-1'), findsOneWidget);
      expect(find.text('spb-1'), findsOneWidget);

      final msk = _cardOf(tester, 'msk-1');
      expect(msk.subtitle, contains('Moscow'));
      expect(msk.subtitle, contains('пинг панели: 31 мс'));
      expect(msk.subtitle, contains('нагрузка 30%'));
      expect(_cardOf(tester, 'Россия').subtitle, contains('Любой релей'));
      // Числа названы своим источником.
      expect(find.textContaining('по данным панели'), findsOneWidget);
    });

    testWidgets('выбор релея пишет его индекс, выбор страны — её', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(
        _app(Relay.fromCountries(<Relay>[Relay.fromApiJson(_ruJson)])),
      );
      await _settle(tester);
      final container = ProviderScope.containerOf(
        tester.element(find.byType(RelayScreen)),
      );

      await tester.tap(find.text('spb-1'));
      await tester.pump();
      expect(container.read(coreConfigProvider).relay, 4);
      expect(find.text('Вход: spb-1'), findsOneWidget);
      // Экран закрывается с задержкой; ждём её, чтобы таймер не остался.
      await tester.pump(const Duration(milliseconds: 400));
      await tester.pump();
    });

    testWidgets('строка страны пишет индекс страны', (tester) async {
      _useTallView(tester);
      await tester.pumpWidget(
        _app(Relay.fromCountries(<Relay>[Relay.fromApiJson(_ruJson)])),
      );
      await _settle(tester);
      final container = ProviderScope.containerOf(
        tester.element(find.byType(RelayScreen)),
      );

      await tester.tap(find.text('Россия'));
      await tester.pump();
      expect(container.read(coreConfigProvider).relay, 2);
      await tester.pump(const Duration(milliseconds: 400));
      await tester.pump();
    });

    testWidgets('галочка стоит на выбранном релее, а не на его стране', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(
        _app(
          Relay.fromCountries(<Relay>[Relay.fromApiJson(_ruJson)]),
          relayIndex: 3,
        ),
      );
      await _settle(tester);
      expect(_cardOf(tester, 'msk-1').selected, isTrue);
      expect(_cardOf(tester, 'Россия').selected, isFalse);
      expect(_cardOf(tester, 'Выкл').selected, isFalse);
    });
  });
}
