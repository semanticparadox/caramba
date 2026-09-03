// Сужение, приходящее в каталоге, доходит до пользователя карточкой.
//
// Нормативно: 02-SPEC.md 7.7 (закрытый список сужений), 7.7.1 (почему смена
// ресурса это сужение), INV-22, 04-THREAT-MODEL.md 7.3 шаг 5 и строка таблицы
// `rs`, `geo`, `ro[].rs`: враждебный оператор публикует набор правил, уводящий
// названные домены в DIRECT, хеш при этом сходится, INV-12 доволен, и трафик
// уходит открытым, пока туннель показывает «подключено». Карточка это
// единственная защита, которая здесь есть.

import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/features/csm/keep_or_revert_card.dart';
import 'package:caramba_client/state/csm_catalog_guard.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';

import 'support/fake_core.dart';

String _snapshot({
  required List<Map<String, Object?>> resources,
  List<Map<String, Object?>> routes = const <Map<String, Object?>>[],
}) => jsonEncode(<String, Object?>{
  'enrolled': true,
  'resources': resources,
  'routes': routes,
});

Map<String, Object?> _rs(String name, String hash) => <String, Object?>{
  'kind': 'rs',
  'name': name,
  'hash': hash,
};

const String _hashA =
    '11111111111111111111111111111111111111111111111111111111111111aa';
const String _hashB =
    '22222222222222222222222222222222222222222222222222222222222222bb';

(ProviderContainer, FakeVpnCore) _boot() {
  final core = FakeVpnCore();
  final container = ProviderContainer(
    overrides: <Override>[vpnConnectionProvider.overrideWithValue(core)],
  );
  addTearDown(container.dispose);
  return (container, core);
}

Future<bool> _pump(ProviderContainer container, {required int nowMs}) =>
    csmPumpCatalogGuard(
      connection: container.read(vpnConnectionProvider) as FakeVpnCore,
      guard: container.read(csmCatalogGuardProvider.notifier),
      nowMs: nowMs,
    );

void main() {
  test('первый каталог принимается молча: сужать не от чего', () async {
    final (container, core) = _boot();
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
    );

    expect(await _pump(container, nowMs: 1000), isFalse);
    expect(container.read(csmCatalogChangesProvider), isEmpty);
    expect(container.read(csmCatalogGuardProvider).verified, isNotNull);
  });

  test('хеш поменялся при прежнем имени: карточка поднимается', () async {
    final (container, core) = _boot();
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
    );
    await _pump(container, nowMs: 1000);

    // Путь и имя те же, байты другие. Хеш сходится, INV-12 доволен, и что
    // внутри, приложение не знает.
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashB)],
    );
    expect(await _pump(container, nowMs: 2000), isTrue);

    final cards = container.read(csmCatalogChangesProvider);
    expect(cards, hasLength(1));
    expect(cards.single.rows, hasLength(1));
    expect(
      cards.single.rows.single.kind,
      CsmCatalogChangeKind.resourceHashChanged,
    );
    expect(cards.single.rows.single.name, 'geosite');
    // Клиент остаётся на прежнем наборе, пока пользователь не ответил.
    expect(
      container.read(csmCatalogGuardProvider).verified!.byName['geosite']!.hash,
      _hashA,
    );
  });

  test('запись добавлена и запись убрана: обе строки в карточке', () async {
    final (container, core) = _boot();
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
    );
    await _pump(container, nowMs: 1000);

    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('adblock', _hashB)],
    );
    expect(await _pump(container, nowMs: 2000), isTrue);

    final rows = container.read(csmCatalogChangesProvider).single.rows;
    expect(rows.map((r) => r.kind).toSet(), <CsmCatalogChangeKind>{
      CsmCatalogChangeKind.resourceAdded,
      CsmCatalogChangeKind.resourceRemoved,
    });
    expect(rows.map((r) => r.name).toSet(), <String>{'adblock', 'geosite'});
  });

  test('прежний хеш под другим именем читается как переименование', () async {
    final (container, core) = _boot();
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
    );
    await _pump(container, nowMs: 1000);

    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite-v2', _hashA)],
    );
    expect(await _pump(container, nowMs: 2000), isTrue);

    final rows = container.read(csmCatalogChangesProvider).single.rows;
    expect(rows, hasLength(1));
    expect(rows.single.kind, CsmCatalogChangeKind.resourceRenamed);
    expect(rows.single.previous, 'geosite');
    expect(rows.single.proposed, 'geosite-v2');
  });

  test('список правил маршрута поменялся: карточка поднимается', () async {
    final (container, core) = _boot();
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
      routes: <Map<String, Object?>>[
        <String, Object?>{
          'id': 'ru-smart',
          'rs': <String>['geosite', 'geoip'],
        },
      ],
    );
    await _pump(container, nowMs: 1000);

    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
      routes: <Map<String, Object?>>[
        <String, Object?>{
          'id': 'ru-smart',
          'rs': <String>['geosite'],
        },
      ],
    );
    expect(await _pump(container, nowMs: 2000), isTrue);

    final rows = container.read(csmCatalogChangesProvider).single.rows;
    expect(rows, hasLength(1));
    expect(rows.single.kind, CsmCatalogChangeKind.routeRulesChanged);
    expect(rows.single.name, 'ru-smart');
  });

  test('тот же набор второй раз второй карточки не поднимает', () async {
    final (container, core) = _boot();
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
    );
    await _pump(container, nowMs: 1000);
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashB)],
    );
    await _pump(container, nowMs: 2000);
    expect(await _pump(container, nowMs: 3000), isFalse);
    expect(container.read(csmCatalogChangesProvider), hasLength(1));
  });

  test('Оставить прежние удерживает набор, Принять новые его меняет', () async {
    final (container, core) = _boot();
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
    );
    await _pump(container, nowMs: 1000);
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashB)],
    );
    await _pump(container, nowMs: 2000);

    final guard = container.read(csmCatalogGuardProvider.notifier);
    final id = container.read(csmCatalogChangesProvider).single.id;
    guard.keep(id);
    expect(container.read(csmCatalogChangesProvider), isEmpty);
    expect(
      container.read(csmCatalogGuardProvider).verified!.byName['geosite']!.hash,
      _hashA,
    );

    // Тот же набор приходит снова: вопрос не отвечен насовсем, он отвечен про
    // тот раз, и оператор вправе предложить снова.
    expect(await _pump(container, nowMs: 3000), isTrue);
    guard.accept(container.read(csmCatalogChangesProvider).single.id);
    expect(
      container.read(csmCatalogGuardProvider).verified!.byName['geosite']!.hash,
      _hashB,
    );
  });

  test('ядро без ABI v3 отвечает пусто: это не изменение каталога', () async {
    final (container, core) = _boot();
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
    );
    await _pump(container, nowMs: 1000);

    core.csmStateJson = '';
    expect(await _pump(container, nowMs: 2000), isFalse);
    expect(container.read(csmCatalogChangesProvider), isEmpty);
    expect(
      container.read(csmCatalogGuardProvider).verified!.byName['geosite']!.hash,
      _hashA,
    );
  });

  testWidgets('карточка называет провайдера и обе кнопки на месте', (
    tester,
  ) async {
    final core = FakeVpnCore();
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashA)],
    );
    final container = ProviderContainer(
      overrides: <Override>[vpnConnectionProvider.overrideWithValue(core)],
    );
    addTearDown(container.dispose);
    final guard = container.read(csmCatalogGuardProvider.notifier);
    await csmPumpCatalogGuard(connection: core, guard: guard, nowMs: 1000);
    core.csmStateJson = _snapshot(
      resources: <Map<String, Object?>>[_rs('geosite', _hashB)],
    );
    await csmPumpCatalogGuard(connection: core, guard: guard, nowMs: 2000);

    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: MaterialApp(
          theme: AppTheme.dark(),
          home: const Scaffold(body: CsmPendingChangesSection()),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('Оператор поменял правила маршрутизации'), findsOneWidget);
    expect(find.text('geosite'), findsOneWidget);
    expect(find.text('Оставить прежние'), findsOneWidget);
    expect(find.text('Принять новые'), findsOneWidget);
  });
}
