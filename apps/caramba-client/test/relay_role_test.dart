// Узел с ролью ВХОДА не предлагается как выход — ни в списке, ни автоподбору.
//
// Дефект был снят на устройстве: на «Серверах» строка «relay 🇷🇺» стояла рядом
// с Канадой и Германией и нажималась. Это российская машина, через которую
// НАБИРАЮТ немецкую и канадскую; выбрав её выходом, человек вышел бы в интернет
// из России — ровно оттуда, откуда уходил, ставя VPN.
//
// ФИКСТУРЫ СНЯТЫ С ЖИВОЙ ПОДПИСКИ, а не придуманы. Тело подписки 34
// (`https://panel.exarobot.top/sub/3d9847b3-…`, sing-box) пропущено через сам
// импортёр ядра (`subimport.Import` + `subscription.ServersFromProxies`), и
// ядро вернуло 29 узлов, из которых ровно один носит `role: relay`:
//
//   {"id":"relay 🇷🇺","type":"hysteria2","server":"141.98.191.214",
//    "port":11464,"role":"relay"}
//   {"id":"🇩🇪 Speed","type":"hysteria2","server":"85.215.196.151",
//    "port":11466,"country":"DE","role":"exit"}
//   {"id":"🇨🇦 Speed","type":"hysteria2","server":"158.69.213.88",
//    "port":11474,"country":"CA","role":"exit"}
//
// Отсюда и главное правило теста: признак срабатывает на настоящем входе и
// молчит на 🇩🇪/🇨🇦 выходах, у которых и семейство (hysteria2), и соседний порт
// те же. Ни имя, ни порт входом не выдают — выдаёт только роль.
//
// Роль ядро выводит не из имени, а из ссылки `dialer-proxy`/`detour`: на входе
// висят 14 прокси «via 🇷🇺». Ядро старше поля `role` отдаёт пустую строку — и
// тогда НИЧЕГО не прячется: молчание источника не запрет.

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/domain/autopilot/auto_pick.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_builder.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

// ─── фикстуры живой подписки 34 ────────────────────────────────────────────

/// Вход цепочки, как его вернуло ядро. Страны у него НЕТ: `countryFromName`
/// флаг в хвосте имени не разобрал — тем важнее, что решение принимается по
/// роли, а не по стране и не по имени.
const _relayRu = ImportedServer(
  id: 'relay 🇷🇺',
  name: 'relay 🇷🇺',
  type: 'hysteria2',
  server: '141.98.191.214',
  port: 11464,
  country: '',
  security: 'tls',
  role: 'relay',
);

/// Немецкий выход того же семейства и почти того же порта — контрольный.
const _deSpeed = ImportedServer(
  id: '🇩🇪 Speed',
  name: '🇩🇪 Speed',
  type: 'hysteria2',
  server: '85.215.196.151',
  port: 11466,
  country: 'DE',
  security: 'tls',
  role: 'exit',
);

/// Тот же немецкий вход, набираемый ЧЕРЕЗ российский. Машина та же (адрес и
/// порт совпадают), выходом остаётся Германия.
const _deSpeedViaRu = ImportedServer(
  id: '🇩🇪 Speed via 🇷🇺',
  name: '🇩🇪 Speed via 🇷🇺',
  type: 'hysteria2',
  server: '85.215.196.151',
  port: 11466,
  country: 'DE',
  security: 'tls',
  role: 'exit',
);

const _caSpeed = ImportedServer(
  id: '🇨🇦 Speed',
  name: '🇨🇦 Speed',
  type: 'hysteria2',
  server: '158.69.213.88',
  port: 11474,
  country: 'CA',
  security: 'tls',
  role: 'exit',
);

const _liveImport = <ImportedServer>[
  _deSpeed,
  _relayRu,
  _deSpeedViaRu,
  _caSpeed,
];

/// То же тело, разобранное ядром СТАРШЕ поля `role`: роли нет ни у кого.
const _importWithoutRole = <ImportedServer>[
  ImportedServer(
    id: 'relay 🇷🇺',
    name: 'relay 🇷🇺',
    type: 'hysteria2',
    server: '141.98.191.214',
    port: 11464,
    country: '',
  ),
  ImportedServer(
    id: '🇩🇪 Speed',
    name: '🇩🇪 Speed',
    type: 'hysteria2',
    server: '85.215.196.151',
    port: 11466,
    country: 'DE',
  ),
];

Map<String, dynamic> _inbound(String tag, String proxyName, int port) =>
    <String, dynamic>{
      'id': port,
      'tag': tag,
      'protocol': 'hysteria2',
      'network': 'udp',
      'security': 'tls',
      'port': port,
      'label': 'Speed',
      'proxy_name': proxyName,
      'available': true,
    };

/// Панельная строка Германии: узел 1 привязан к релэю 2 колонкой
/// `nodes.relay_id` — ровно то, что стоит в живой базе.
final _panelGermany = Server.fromJson(<String, dynamic>{
  'id': 1,
  'name': 'Node #1 (1000 Mbps)',
  'country_code': 'DE',
  'flag': '🇩🇪',
  'latency_ms': 40,
  'load_pct': 12.0,
  'status': 'active',
  'via_relay': <String, dynamic>{
    'node_id': 2,
    'name': 'Russia',
    'country_code': 'RU',
    'flag': '🇷🇺',
    'chained_in_config': false,
  },
  'inbounds': <Map<String, dynamic>>[
    _inbound('Hysteria2-4b6c7b66', '🇩🇪 Speed ↪', 11466),
  ],
});

final _panelCanada = Server.fromJson(<String, dynamic>{
  'id': 5,
  'name': 'Node #5 (1000 Mbps)',
  'country_code': 'CA',
  'flag': '🇨🇦',
  'latency_ms': 90,
  'load_pct': 8.0,
  'status': 'active',
  'inbounds': <Map<String, dynamic>>[
    _inbound('Hysteria2-2e338377', '🇨🇦 Speed', 11474),
  ],
});

/// Российский релэй, ПРОСОЧИВШИЙСЯ в `/servers`. В норме панель его вырезает,
/// но вырезает по одной колонке `is_relay`, а релэем в её же модели считается и
/// узел с `node_type = 'relay'`. Строка собрана ровно на этот случай.
final _panelRelayLeak = Server.fromJson(<String, dynamic>{
  'id': 2,
  'name': 'Node #2 (1000 Mbps)',
  'country_code': 'RU',
  'flag': '🇷🇺',
  'latency_ms': 682,
  'load_pct': 4.0,
  'status': 'active',
  'inbounds': <Map<String, dynamic>>[
    _inbound('Hysteria2-32e37652', 'relay 🇷🇺', 11464),
  ],
});

// ─── инвентарь ─────────────────────────────────────────────────────────────

class _MemoryStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles = <ConnectionProfile>[];
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
    profiles = <ConnectionProfile>[];
    activeId = null;
  }
}

ConnectionProfile _rawProfile(List<ImportedServer> servers) =>
    ConnectionProfile(
      id: 'cp_raw',
      type: ProfileType.rawSub,
      displayName: 'Импорт',
      source: 'https://panel.exarobot.top/sub/3d9847b3',
      servers: servers,
    );

Future<ProviderContainer> _boot(ConnectionProfile profile) async {
  final store = _MemoryStore()
    ..profiles = <ConnectionProfile>[profile]
    ..activeId = profile.id;
  final container = ProviderContainer(
    overrides: <Override>[
      connectionProfilesStoreProvider.overrideWithValue(store),
    ],
  );
  addTearDown(container.dispose);
  container.read(connectionProfilesProvider);
  for (var i = 0; i < 100; i++) {
    if (!container.read(connectionProfilesProvider).loading) break;
    await Future<void>.delayed(Duration.zero);
  }
  return container;
}

// ─── автоподбор ────────────────────────────────────────────────────────────

ProbeResult _probe(String id, int ms, {String country = ''}) => ProbeResult(
  id: id,
  latencyMs: ms,
  country: country,
  verdict: ProbeVerdict.ok,
);

void main() {
  group('импортированная подписка: роль приходит от ядра', () {
    test('вход опознан, оба выхода того же семейства — нет', () {
      final offering = buildImportedOffering(servers: _liveImport);

      final relay = offering.exits.firstWhere((e) => e.key == '141.98.191.214');
      expect(relay.role, NodeRole.relay);
      expect(relay.isRelay, isTrue);
      expect(relay.isAvailable, isFalse);
      expect(relay.availability.reason, OfferingReason.nodeIsRelay);
      // Причина не должна оставаться машинным словом: строка списка показывает
      // именно этот текст вместо подписи.
      expect(relay.availability.message, contains('вход цепочки'));

      final de = offering.exits.firstWhere((e) => e.key == '85.215.196.151');
      final ca = offering.exits.firstWhere((e) => e.key == '158.69.213.88');
      expect(de.role, NodeRole.exit);
      expect(ca.role, NodeRole.exit);
      expect(de.isAvailable, isTrue);
      expect(ca.isAvailable, isTrue);
    });

    test('«via 🇷🇺» остаётся выходом: он немецкий, а не российский', () {
      final offering = buildImportedOffering(servers: _liveImport);
      final de = offering.exits.firstWhere((e) => e.key == '85.215.196.151');
      // Оба прокси немецкой машины — выходы, хотя один набирается через РФ.
      expect(de.inbounds.map((i) => i.proxyName).toList(), <String>[
        '🇩🇪 Speed',
        '🇩🇪 Speed via 🇷🇺',
      ]);
      expect(de.inbounds.every((i) => i.role == NodeRole.exit), isTrue);
    });

    test('ядро без поля role не прячет НИЧЕГО', () {
      final offering = buildImportedOffering(servers: _importWithoutRole);
      expect(offering.exits.length, 2);
      expect(offering.exits.every((e) => e.role == NodeRole.unknown), isTrue);
      expect(offering.exits.every((e) => e.isAvailable), isTrue);
      // Имя «relay 🇷🇺» само по себе ничего не решает: чужая подписка вправе так
      // назвать обычный выход, и выключить его по слову значило бы отнять
      // рабочий узел.
      expect(
        offering.exits.any((e) => e.key == '141.98.191.214' && e.isAvailable),
        isTrue,
      );
    });

    test('машина со СМЕШАННЫМ набором остаётся выходом', () {
      // Один адрес, два прокси: входной и выходной. Убрать такую машину значило
      // бы отнять рабочий выход ради соседнего прокси.
      final offering = buildImportedOffering(
        servers: const <ImportedServer>[
          ImportedServer(
            id: 'mixed-relay',
            name: 'mixed-relay',
            type: 'hysteria2',
            server: '10.0.0.1',
            port: 1,
            country: 'NL',
            role: 'relay',
          ),
          ImportedServer(
            id: 'mixed-exit',
            name: 'mixed-exit',
            type: 'vless',
            server: '10.0.0.1',
            port: 2,
            country: 'NL',
            role: 'exit',
          ),
        ],
      );
      final machine = offering.exits.single;
      expect(machine.role, NodeRole.exit);
      expect(machine.isAvailable, isTrue);
      // Запрет остаётся точечным — на самом прокси.
      expect(
        machine.inbounds
            .firstWhere((i) => i.proxyName == 'mixed-relay')
            .role
            .isRelay,
        isTrue,
      );
    });
  });

  group('инвентарь выходов', () {
    test('вход не попадает ни в узлы, ни в страны', () async {
      final c = await _boot(_rawProfile(_liveImport));
      final inv = c.read(exitInventoryProvider);

      expect(inv.nodes.map((n) => n.key), isNot(contains('relay 🇷🇺')));
      expect(inv.nodes.map((n) => n.key).toList(), <String>[
        '🇩🇪 Speed',
        '🇩🇪 Speed via 🇷🇺',
        '🇨🇦 Speed',
      ]);
      // Страна входа не должна появиться среди стран выхода даже пустым кодом:
      // у живого `relay 🇷🇺` страна не разобрана, и он ушёл бы в группу «Без
      // страны», где так же нажимался бы.
      expect(inv.locations.map((l) => l.countryCode).toSet(), <String>{
        'DE',
        'CA',
      });
    });

    test('автоподбор страны не может закрепить вход', () async {
      final c = await _boot(_rawProfile(_liveImport));
      // Инвентарь — единственный источник, из которого selectCountry берёт узел.
      expect(c.read(exitInventoryProvider).nodesIn('').isEmpty, isTrue);
    });

    test('ядро без поля role оставляет узел в списке', () async {
      final c = await _boot(_rawProfile(_importWithoutRole));
      final inv = c.read(exitInventoryProvider);
      expect(inv.nodes.length, 2);
      expect(inv.nodes.map((n) => n.key), contains('relay 🇷🇺'));
    });
  });

  group('панельный REST', () {
    test('живая выдача (релэя в ней нет) никого не помечает входом', () {
      final offering = buildPanelOffering(
        servers: <Server>[_panelGermany, _panelCanada],
      );
      expect(offering.exits.every((e) => e.role == NodeRole.exit), isTrue);
      expect(offering.exits.every((e) => e.isAvailable), isTrue);
      // Германия ССЫЛАЕТСЯ на релэй 2 — и это не делает входом её саму.
      final de = offering.exits.firstWhere((e) => e.panelNodeId == 1);
      expect(de.viaRelay?.nodeId, 2);
      expect(de.isRelay, isFalse);
    });

    test('просочившийся релэй опознан встречной ссылкой соседа', () {
      final offering = buildPanelOffering(
        servers: <Server>[_panelGermany, _panelCanada, _panelRelayLeak],
      );
      final relay = offering.exits.firstWhere((e) => e.panelNodeId == 2);
      expect(relay.role, NodeRole.relay);
      expect(relay.availability.reason, OfferingReason.nodeIsRelay);
      // Выходы не задеты.
      expect(
        offering.exits
            .where((e) => e.panelNodeId != 2)
            .every((e) => e.isAvailable),
        isTrue,
      );
    });

    test('инвентарь выкидывает просочившийся релэй из списка узлов', () {
      final servers = <Server>[_panelGermany, _panelCanada, _panelRelayLeak];
      // Тот же предикат, что и в инвентаре: узел, на который ссылается сосед.
      final offering = buildPanelOffering(servers: servers);
      final selectable = offering.exits.where((e) => e.isAvailable).toList();
      expect(selectable.map((e) => e.panelNodeId).toList(), <int>[1, 5]);
    });
  });

  group('автоподбор', () {
    test('вход не выигрывает, даже будучи САМЫМ быстрым', () {
      final facts = fleetFactsOf(buildImportedOffering(servers: _liveImport));
      // 100 мс у входа против 300/400 у выходов: по счёту он первый с запасом.
      final out = autoPick(
        results: <ProbeResult>[
          _probe('relay 🇷🇺', 100),
          _probe('🇩🇪 Speed', 300, country: 'DE'),
          _probe('🇨🇦 Speed', 400, country: 'CA'),
        ],
        facts: facts,
      );
      expect(out.pick?.proxyName, '🇩🇪 Speed');
      expect(out.pick?.reasonCode, 'relay_skipped');
      // Из ИТОГА он не исчезает: человек мерил его своими секундами.
      expect(out.ranked.map((c) => c.name), contains('relay 🇷🇺'));
      expect(
        out.ranked.firstWhere((c) => c.name == 'relay 🇷🇺').routable,
        isFalse,
      );
    });

    test('гистерезис не удержит вход, закреплённый прошлым проходом', () {
      final facts = fleetFactsOf(buildImportedOffering(servers: _liveImport));
      final out = autoPick(
        results: <ProbeResult>[
          _probe('relay 🇷🇺', 100),
          _probe('🇩🇪 Speed', 300, country: 'DE'),
        ],
        facts: facts,
        previous: const AutoPickRecord(
          proxyName: 'relay 🇷🇺',
          latencyMs: 100,
          updatedMs: 1,
        ),
      );
      expect(out.pick?.proxyName, '🇩🇪 Speed');
    });

    test('одни входы — это отказ, а не «лучше, чем ничего»', () {
      final facts = fleetFactsOf(buildImportedOffering(servers: _liveImport));
      final out = autoPick(
        results: <ProbeResult>[_probe('relay 🇷🇺', 100)],
        facts: facts,
      );
      expect(out.hasPick, isFalse);
      expect(out.failure?.kind, AutoFailureKind.onlyRelays);
      // Повторять нечего: следующий проход даст то же самое.
      expect(out.failure?.retryable, isFalse);
    });

    test('вход не подставляется как «честный близнец» имени с обещанием', () {
      // Провод у них разный, но защита от подмены обязана быть явной: близнец
      // предлагается человеку как то же самое подключение.
      final facts = fleetFactsOf(buildImportedOffering(servers: _liveImport));
      final out = autoPick(
        results: <ProbeResult>[
          _probe('🇩🇪 Speed via 🇷🇺', 300, country: 'DE'),
          _probe('relay 🇷🇺', 100),
        ],
        facts: facts,
      );
      final labelled = out.ranked.firstWhere(
        (c) => c.name == '🇩🇪 Speed via 🇷🇺',
      );
      expect(labelled.plainTwin, isNot('relay 🇷🇺'));
      expect(out.pick?.proxyName, '🇩🇪 Speed via 🇷🇺');
    });

    test('роль доезжает до фактов флота поимённо', () {
      final facts = fleetFactsOf(buildImportedOffering(servers: _liveImport));
      expect(facts['relay 🇷🇺']?.isRelay, isTrue);
      expect(facts['🇩🇪 Speed']?.isRelay, isFalse);
      expect(facts['🇩🇪 Speed via 🇷🇺']?.isRelay, isFalse);
      expect(facts['🇨🇦 Speed']?.isRelay, isFalse);
    });

    test('ядро без роли — автоподбор работает как прежде', () {
      final facts = fleetFactsOf(
        buildImportedOffering(servers: _importWithoutRole),
      );
      expect(facts.values.every((f) => !f.isRelay), isTrue);
      final out = autoPick(
        results: <ProbeResult>[
          _probe('relay 🇷🇺', 100),
          _probe('🇩🇪 Speed', 300, country: 'DE'),
        ],
        facts: facts,
      );
      // Прятать по молчанию источника нельзя: узел остаётся выбираемым.
      expect(out.pick?.proxyName, 'relay 🇷🇺');
    });
  });
}
