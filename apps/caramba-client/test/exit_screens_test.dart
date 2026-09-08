// Экраны выбора выхода, входа и протокола.
//
// Проверяется ровно одно свойство, ради которого эта половина переписывалась:
// вариант, который выбрать НЕЛЬЗЯ, остаётся видимым, приглушённым, названным
// причиной и без цели для нажатия. Спрятанная строка неотличима от «такого не
// бывает», и пользователь ищет её в обновлении приложения, которого ему не
// нужно.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/features/protocol/protocol_screen.dart';
import 'package:caramba_client/features/servers/relay_screen.dart';
import 'package:caramba_client/features/servers/servers_screen.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/protocol_inventory_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_models.dart';
import 'package:caramba_client/widgets/ui.dart';

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
  ImportedServer(
    id: 'nl-2',
    name: 'Amsterdam #2',
    type: 'vless',
    server: 'b.example',
    port: 443,
    country: 'NL',
  ),
  ImportedServer(
    id: 'de-1',
    name: 'Frankfurt #1',
    type: 'hysteria2',
    server: 'c.example',
    port: 443,
    country: 'DE',
  ),
];

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

/// Входы в форме `GET /app/relays`, плюс псевдо-варианты Выкл/Авто, которые
/// дописывает клиент.
final _panelRelays = Relay.fromCountries(<Relay>[
  Relay.fromApiJson(const <String, dynamic>{
    'country_code': 'TR',
    'country_name': 'Турция',
    'node_count': 2,
  }),
]);

Widget _app(
  Widget screen, {
  ExitInventory? catalog,
  _FakeProfilesStore? store,
}) => ProviderScope(
  overrides: [
    vpnConnectionProvider.overrideWithValue(FakeVpnCore()),
    connectionProfilesStoreProvider.overrideWithValue(
      store ?? _FakeProfilesStore(<ConnectionProfile>[_profile()], 'cp_1'),
    ),
    // Список входов приходит с панели; в тесте подставляем ровно то, что
    // отдаёт `GET /app/relays`. Прежний `Relay.defaults` для этого не годится:
    // выдуманных стран в нём больше нет, и экран, собранный по нему, проверял
    // бы отрисовку фикции.
    apiRelaysProvider.overrideWith((ref) async => _panelRelays),
    if (catalog != null) csmExitCatalogProvider.overrideWithValue(catalog),
  ],
  child: MaterialApp(theme: AppTheme.dark(), home: screen),
);

/// Высокое окно: списки здесь строятся ленивым сливером, и строка, не попавшая
/// в вьюпорт, не попадает и в дерево элементов — тест «строка видна» на окне
/// 800x600 проверял бы прокрутку, а не видимость.
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

/// Строка видна, приглушена, названа причиной и не нажимается.
void _expectDisabledRow(WidgetTester tester, String title, String reason) {
  expect(find.text(title), findsOneWidget);
  final opacities = tester.widgetList<Opacity>(
    find.ancestor(of: find.text(title), matching: find.byType(Opacity)),
  );
  expect(
    opacities.any((o) => o.opacity == 0.45),
    isTrue,
    reason: 'строка «$title» обязана быть приглушённой',
  );
  expect(find.textContaining(reason), findsWidgets);
  final card = tester.widget<ListItemCard>(
    find.ancestor(of: find.text(title), matching: find.byType(ListItemCard)),
  );
  expect(card.onTap, isNull, reason: 'у строки «$title» не должно быть тапа');
}

ExitInventory _catalogWith(List<ExitNode> nodes) {
  final byCountry = <String, List<ExitNode>>{};
  for (final n in nodes) {
    byCountry.putIfAbsent(n.countryCode, () => <ExitNode>[]).add(n);
  }
  return ExitInventory(
    source: ExitInventorySource.csmCatalog,
    nodes: nodes,
    locations: byCountry.entries
        .map(
          (e) => ExitLocation.fromNodes(
            e.key,
            e.value,
            source: ExitInventorySource.csmCatalog,
          ),
        )
        .toList(growable: false),
  );
}

void main() {
  group('инвентарь протоколов', () {
    test('сверенный список выключает то, чего флот не раздаёт', () {
      final inventory = _catalogWith(const <ExitNode>[
        ExitNode(
          key: 'n1',
          name: 'DE vless ws',
          countryCode: 'DE',
          source: ExitInventorySource.csmCatalog,
          protocol: 'vless+ws+tls',
        ),
        ExitNode(
          key: 'n2',
          name: 'DE naive',
          countryCode: 'DE',
          source: ExitInventorySource.csmCatalog,
          protocol: 'naive',
        ),
      ]);
      final protocols = buildProtocolInventory(
        ProtocolOption.defaults,
        inventory,
      );

      expect(protocols.verified, isTrue);

      // Флот раздаёт vless — значит VLESS доступен, а его формы названы.
      final vless = protocols.choices.firstWhere((c) => c.name == 'VLESS');
      expect(vless.isAvailable, isTrue);
      expect(vless.shapes, containsAll(<String>['ws', 'tls']));

      // Reality среди форм нет, а формы источник называет: значит это не он.
      final reality = protocols.choices.firstWhere(
        (c) => c.name == 'VLESS · Reality',
      );
      expect(reality.isAvailable, isFalse);
      expect(
        reality.availability.reason,
        ProtocolUnavailableReason.shapeNotInFleet,
      );

      // Ни одного wireguard/hysteria2/tuic/ss-узла нет.
      for (final name in <String>[
        'AmneziaWG',
        'Hysteria2',
        'TUIC',
        'Shadowsocks',
      ]) {
        final ch = protocols.choices.firstWhere((c) => c.name == name);
        expect(ch.isAvailable, isFalse, reason: name);
        expect(
          ch.availability.reason,
          ProtocolUnavailableReason.notInFleet,
          reason: name,
        );
      }

      // «Авто» это отказ от выбора, он валиден на любом флоте.
      expect(protocols.choices.first.isAvailable, isTrue);

      // naive флот раздаёт, а попросить его ядру нечем: строка есть, выбора нет.
      final naive = protocols.choices.firstWhere((c) => c.name == 'naive');
      expect(naive.isRequestable, isFalse);
      expect(
        naive.availability.reason,
        ProtocolUnavailableReason.notRequestable,
      );
    });

    test('незнакомый ПРОТОКОЛ не выдаёт себя за отсутствие формы', () {
      // Обычная сторонняя подписка: голый `vless` и голый `trojan`. Про формы
      // она не говорит ни слова, а `trojan` ядру просто неизвестен — и это
      // факт о ПРОТОКОЛЕ, а не о том, называет ли источник формы. Считая
      // «источник называет формы» по всему инвентарю, приложение объявляло
      // VLESS·Reality недоступным «формы reality нет»: неправда, и строка при
      // этом не нажималась, то есть выбрать рабочий протокол было нечем.
      final inventory = _catalogWith(const <ExitNode>[
        ExitNode(
          key: 'n1',
          name: 'VLESS node',
          countryCode: 'DE',
          source: ExitInventorySource.importedSub,
          protocol: 'vless',
        ),
        ExitNode(
          key: 'n2',
          name: 'Trojan node',
          countryCode: 'DE',
          source: ExitInventorySource.importedSub,
          protocol: 'trojan',
        ),
      ]);
      final protocols = buildProtocolInventory(
        ProtocolOption.defaults,
        inventory,
      );

      final reality = protocols.choices.firstWhere(
        (c) => c.name == 'VLESS · Reality',
      );
      expect(reality.isAvailable, isTrue);
      expect(reality.availability.reason, isNull);
      expect(reality.nodeCount, 1);

      // Сам trojan назван отдельной строкой: попросить его нечем, но и
      // промолчать о нём нельзя.
      final trojan = protocols.choices.firstWhere((c) => c.name == 'trojan');
      expect(trojan.isRequestable, isFalse);
      expect(
        trojan.availability.reason,
        ProtocolUnavailableReason.notRequestable,
      );
    });

    test('молчащий источник не выключает НИЧЕГО, но говорит об этом', () {
      final inventory = _catalogWith(const <ExitNode>[
        ExitNode(
          key: 'p1',
          name: 'Panel node',
          countryCode: 'DE',
          source: ExitInventorySource.panelRest,
        ),
      ]);
      final protocols = buildProtocolInventory(
        ProtocolOption.defaults,
        inventory,
      );

      expect(protocols.verified, isFalse);
      expect(
        protocols.unverified?.reason,
        ProtocolUnavailableReason.sourceSilent,
      );
      expect(protocols.choices.every((c) => c.isAvailable), isTrue);
      expect(protocols.choices.length, ProtocolOption.defaults.length);
    });
  });

  group('экран серверов', () {
    // Экран переехал с двух уровней (страна → узел) на один плоский список по
    // прямой просьбе владельца сервиса. Тесты держат ровно то, ради чего этот
    // список и переделывали, и то, что было завоёвано раньше и потеряться не
    // должно.

    testWidgets('все машины в одном списке, страна — на строке', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(_app(const ServersScreen()));
      await _settle(tester);

      // Ни одного лишнего нажатия: машины всех стран видны сразу.
      expect(find.text('Amsterdam #1'), findsOneWidget);
      expect(find.text('Amsterdam #2'), findsOneWidget);
      expect(find.text('Frankfurt #1'), findsOneWidget);

      // Страна больше не строка списка и не уровень — она стоит НА строке
      // машины кодом рядом с флагом.
      expect(find.text('Нидерланды'), findsNothing);
      expect(find.text('Германия'), findsNothing);
      expect(find.text('NL'), findsNWidgets(2));
      expect(find.text('DE'), findsOneWidget);

      // Второго уровня больше нет, значит и возвращаться неоткуда.
      expect(find.text('Все страны'), findsNothing);

      // Строка — МАШИНА, и она называет, сколько инбаундов предлагает. Без
      // этого счётчика восемь прокси одной машины читались как восемь
      // серверов — ровно то, на что владелец жаловался раньше.
      expect(find.text('ИНБАУНДОВ: 1'), findsNWidgets(3));

      // «Авто» первым и выбрано: пина нет.
      final auto = tester.widget<ListItemCard>(
        find.ancestor(
          of: find.text('Авто'),
          matching: find.byType(ListItemCard),
        ),
      );
      expect(auto.selected, isTrue);
    });

    testWidgets('недоступная машина видна, приглушена и названа причиной', (
      tester,
    ) async {
      final catalog = _catalogWith(const <ExitNode>[
        ExitNode(
          key: 'ru-1',
          name: 'RU relay',
          countryCode: 'RU',
          source: ExitInventorySource.csmCatalog,
          availability: ExitAvailability.unavailable(
            ExitUnavailableReason.nodeOffline,
          ),
        ),
        ExitNode(
          key: 'de-1',
          name: 'DE exit',
          countryCode: 'DE',
          source: ExitInventorySource.csmCatalog,
        ),
      ]);
      _useTallView(tester);
      await tester.pumpWidget(_app(const ServersScreen(), catalog: catalog));
      await _settle(tester);

      expect(find.text('DE exit'), findsOneWidget);
      _expectDisabledRow(tester, 'RU relay', 'Узел не в сети');
    });

    testWidgets('адрес машины заголовком не становится', (tester) async {
      // Заголовком раньше мог оказаться АДРЕС машины («158.69.213.88»): в теле
      // подписки имени машины не существует, и ключом ей служит адрес. Адрес не
      // имя, и показывать его там, где есть что показать вместо него, значит
      // светить адрес узла без нужды. Здесь у каждой машины по одному прокси,
      // и его имя — единственное имя этой машины.
      _useTallView(tester);
      await tester.pumpWidget(_app(const ServersScreen()));
      await _settle(tester);

      expect(find.textContaining('example'), findsNothing);
      expect(find.text('Amsterdam #1'), findsOneWidget);
      expect(find.text('Frankfurt #1'), findsOneWidget);
    });

    testWidgets('выбор узла закрепляется на профиле', (tester) async {
      final store = _FakeProfilesStore(<ConnectionProfile>[_profile()], 'cp_1');
      _useTallView(tester);
      await tester.pumpWidget(_app(const ServersScreen(), store: store));
      await _settle(tester);

      await tester.tap(find.text('Frankfurt #1'));
      await tester.pumpAndSettle();

      expect(store.profiles.single.selectedServerId, 'de-1');
      final card = tester.widget<ListItemCard>(
        find.ancestor(
          of: find.text('Frankfurt #1'),
          matching: find.byType(ListItemCard),
        ),
      );
      expect(card.selected, isTrue);
    });

    testWidgets('«Авто» отпускает и страну, и узел', (tester) async {
      final store = _FakeProfilesStore(<ConnectionProfile>[_profile()], 'cp_1');
      _useTallView(tester);
      await tester.pumpWidget(_app(const ServersScreen(), store: store));
      await _settle(tester);

      await tester.tap(find.text('Amsterdam #1'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Авто'));
      await tester.pumpAndSettle();

      // Отказ от выбора настоящий: узел выбирает ядро.
      expect(store.profiles.single.selectedExitCountry, isNull);
      expect(store.profiles.single.selectedServerId, isNull);
      final auto = tester.widget<ListItemCard>(
        find.ancestor(
          of: find.text('Авто'),
          matching: find.byType(ListItemCard),
        ),
      );
      expect(auto.selected, isTrue);
    });
  });

  group('экран relay', () {
    testWidgets('в режиме импортированной подписки строки есть, но выключены', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(_app(const RelayScreen()));
      await _settle(tester);

      // Ни одна строка не пропала.
      expect(find.text('Выкл'), findsOneWidget);
      expect(find.text('Авто'), findsOneWidget);
      expect(find.text('Турция'), findsOneWidget);

      // Причина приходит из возможности слоя предложения
      // (`Capabilities.relayChaining`), а не из литерала экрана, и относится к
      // ВХОДАМ ОПЕРАТОРА: цепочку не выражает источник.
      const reason = 'цепочку через вход выразить не может';
      _expectDisabledRow(tester, 'Турция', reason);

      // «Выкл» и «Авто» цепочкой не являются: первый просит ядро её не
      // строить, второй оставляет решение оператору, и оба уходят на провод
      // пустой строкой при любом источнике. Раньше они гасились той же
      // возможностью, и «Выкл» стоял выключенным с подписью, которая описывала
      // работу самого «Выкл» как причину его недоступности.
      for (final title in <String>['Выкл', 'Авто']) {
        final opacities = tester.widgetList<Opacity>(
          find.ancestor(of: find.text(title), matching: find.byType(Opacity)),
        );
        expect(
          opacities.any((o) => o.opacity == 0.45),
          isFalse,
          reason: '«$title» истинен при любом источнике и гаснуть не должен',
        );
        final card = tester.widget<ListItemCard>(
          find.ancestor(
            of: find.text(title),
            matching: find.byType(ListItemCard),
          ),
        );
        expect(card.onTap, isNotNull, reason: '«$title» обязан нажиматься');
      }

      // Значение в силе видно: сохранённый индекс по умолчанию — «Выкл».
      // Раньше `selected` был завязан на ту же возможность, и экран не мог
      // показать НИЧЕГО.
      final cards = <String, ListItemCard>{
        for (final title in <String>['Выкл', 'Авто', 'Турция'])
          title: tester.widget<ListItemCard>(
            find.ancestor(
              of: find.text(title),
              matching: find.byType(ListItemCard),
            ),
          ),
      };
      expect(cards['Выкл']!.selected, isTrue);
      expect(cards['Авто']!.selected, isFalse);
      expect(cards['Турция']!.selected, isFalse);
    });
  });

  group('экран протокола', () {
    testWidgets('список — инбаунды флота, а не перечень запросов ядра', (
      tester,
    ) async {
      // Правило владельца: «протокол это и есть выбор inbounds доступные в
      // конфиге». Раньше экран печатал СЕМЬ строк из `ProtocolOption.defaults`
      // независимо от того, что раздаёт источник, и помечал лишние
      // недоступными. Строка про AmneziaWG на подписке без единого
      // wireguard-прокси — это выдумка, и её быть не должно вовсе.
      _useTallView(tester);
      await tester.pumpWidget(_app(const ProtocolScreen()));
      await _settle(tester);

      // В теле три прокси на трёх машинах: два vless и один hysteria2.
      expect(find.text('VLESS'), findsOneWidget);
      expect(find.text('Hysteria2'), findsOneWidget);
      // Отказ от выбора верен при любом флоте и остаётся.
      expect(find.text('Авто'), findsOneWidget);

      // Ничего из того, чего в теле нет, экран больше не перечисляет.
      for (final absent in <String>[
        'AmneziaWG',
        'TUIC',
        'Shadowsocks',
        'VLESS · Reality',
      ]) {
        expect(find.text(absent), findsNothing, reason: absent);
      }
    });

    testWidgets('узел не закреплён — область названа, а не выдана за узел', (
      tester,
    ) async {
      _useTallView(tester);
      await tester.pumpWidget(_app(const ProtocolScreen()));
      await _settle(tester);

      expect(
        find.textContaining('Сервер не закреплён'),
        findsOneWidget,
        reason: 'список всего флота нельзя выдавать за инбаунды одного узла',
      );
      // Импортированное тело называет только вид прокси: транспорт и TLS в нём
      // схлопнуты, и об этом сказано ДО списка.
      expect(
        find.textContaining('не различает транспорт и TLS'),
        findsOneWidget,
      );
    });
  });
}
