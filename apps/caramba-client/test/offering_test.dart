// Слой предложения: что пользователь может выбрать прямо сейчас.
//
// Фикстуры собраны по ЖИВОЙ системе, а не по удобным для теста формам:
//  * Германия — одна машина с восемью инбаундами, седьмой из которых
//    (`naive`) генератор Clash не выпускает;
//  * Канада — шесть инбаундов и никакого релэя;
//  * релэй — РФ-узел, привязанный к Германии колонкой `nodes.relay_id`, с
//    честным `chained_in_config: false`: цепочки в теле нет;
//  * импортированная подписка — то же железо, но описанное беднее: голый
//    `type:` без транспорта и TLS.
//
// Проверяется главное правило слоя: три состояния вместо двух, ничего
// выдуманного и ничего спрятанного.

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/domain/offering/offering.dart';
import 'package:caramba_client/domain/offering/offering_builder.dart';
import 'package:caramba_client/domain/offering/route_presets.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/protocol_inventory_state.dart';
import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

// ─── фикстуры ──────────────────────────────────────────────────────────────

Map<String, dynamic> _inbound(
  int id,
  String tag,
  String protocol,
  String network,
  String security,
  int port,
  String label, {
  String? proxyName,
  bool available = true,
  String? reason,
}) => <String, dynamic>{
  'id': id,
  'tag': tag,
  'protocol': protocol,
  'network': network,
  'security': security,
  'port': port,
  'label': label,
  'proxy_name': proxyName,
  'available': available,
  'unavailable_reason': reason,
};

/// Германия: восемь включённых инбаундов ОДНОЙ машины. `naive` в теле конфига
/// не появляется — у `generate_clash_config` нет под него ветки.
final _germany = Server.fromJson(<String, dynamic>{
  'id': 1,
  'name': 'Node #1 (Germany)',
  'country_code': 'DE',
  'flag': '🇩🇪',
  'latency_ms': 40,
  'load_pct': 12.0,
  'status': 'online',
  'via_relay': <String, dynamic>{
    'node_id': 2,
    'name': 'RU relay',
    'country_code': 'RU',
    'flag': '🇷🇺',
    'chained_in_config': false,
  },
  'inbounds': <Map<String, dynamic>>[
    _inbound(
      11,
      'reality-in',
      'vless',
      'tcp',
      'reality',
      443,
      'Stealth',
      proxyName: '🇩🇪 Stealth ↪',
    ),
    _inbound(
      12,
      'tls-in',
      'vless',
      'tcp',
      'tls',
      8443,
      'Secure',
      proxyName: '🇩🇪 Secure ↪',
    ),
    _inbound(
      13,
      'ws-in',
      'vless',
      'ws',
      'tls',
      2087,
      'WebSocket',
      proxyName: '🇩🇪 WebSocket ↪',
    ),
    _inbound(
      14,
      'grpc-in',
      'vless',
      'grpc',
      'tls',
      2096,
      'Stream',
      proxyName: '🇩🇪 Stream ↪',
    ),
    _inbound(
      15,
      'hy2-in',
      'hysteria2',
      'udp',
      'tls',
      8444,
      'Speed',
      proxyName: '🇩🇪 Speed ↪',
    ),
    _inbound(
      16,
      'tuic-in',
      'tuic',
      'udp',
      'tls',
      8445,
      'TUIC',
      proxyName: '🇩🇪 TUIC ↪',
    ),
    _inbound(
      17,
      'ss-in',
      'shadowsocks',
      'tcp',
      'none',
      8388,
      'Shadow',
      proxyName: '🇩🇪 Shadow ↪',
    ),
    _inbound(
      18,
      'naive-in',
      'naive',
      'tcp',
      'tls',
      8446,
      'Naive',
      available: false,
      reason: 'protocol_not_emitted_by_clash',
    ),
  ],
  'inbounds_error': null,
});

/// Канада: шесть инбаундов, релэя нет.
final _canada = Server.fromJson(<String, dynamic>{
  'id': 5,
  'name': 'Node #5 (Canada)',
  'country_code': 'CA',
  'flag': '🇨🇦',
  'latency_ms': 120,
  'load_pct': 30.0,
  'status': 'online',
  'inbounds': <Map<String, dynamic>>[
    _inbound(
      51,
      'reality-in',
      'vless',
      'tcp',
      'reality',
      443,
      'Stealth',
      proxyName: '🇨🇦 Stealth',
    ),
    _inbound(
      52,
      'tls-in',
      'vless',
      'tcp',
      'tls',
      8443,
      'Secure',
      proxyName: '🇨🇦 Secure',
    ),
    _inbound(
      53,
      'ws-in',
      'vless',
      'ws',
      'tls',
      2087,
      'WebSocket',
      proxyName: '🇨🇦 WebSocket',
    ),
    _inbound(
      54,
      'hy2-in',
      'hysteria2',
      'udp',
      'tls',
      8444,
      'Speed',
      proxyName: '🇨🇦 Speed',
    ),
    _inbound(
      55,
      'tuic-in',
      'tuic',
      'udp',
      'tls',
      8445,
      'TUIC',
      proxyName: '🇨🇦 TUIC',
    ),
    _inbound(
      56,
      'ss-in',
      'shadowsocks',
      'tcp',
      'none',
      8388,
      'Shadow',
      proxyName: '🇨🇦 Shadow',
    ),
  ],
});

/// Узел, инбаунды которого панель прочитать не смогла.
final _mute = Server.fromJson(<String, dynamic>{
  'id': 9,
  'name': 'Node #9 (Netherlands)',
  'country_code': 'NL',
  'status': 'online',
  'inbounds': null,
  'inbounds_error': 'panel_could_not_read_inbounds',
});

const _relayCountries = <RelayCountryRow>[
  RelayCountryRow(countryCode: 'RU', countryName: 'Россия', nodeCount: 1),
];

/// Импортированное тело: та же машина, но описанная беднее — только `type:`.
const _imported = <ImportedServer>[
  ImportedServer(
    id: '🇩🇪 Stealth ↪',
    name: '🇩🇪 Stealth ↪',
    type: 'vless',
    server: 'de1.example.net',
    port: 443,
    country: 'DE',
  ),
  ImportedServer(
    id: '🇩🇪 Speed ↪',
    name: '🇩🇪 Speed ↪',
    type: 'hysteria2',
    server: 'de1.example.net',
    port: 8444,
    country: 'DE',
  ),
  ImportedServer(
    id: '🇩🇪 Shadow ↪',
    name: '🇩🇪 Shadow ↪',
    type: 'ss',
    server: 'de1.example.net',
    port: 8388,
    country: 'DE',
  ),
  ImportedServer(
    id: '🇨🇦 Secure',
    name: '🇨🇦 Secure',
    type: 'vless',
    server: 'ca1.example.net',
    port: 8443,
    country: 'CA',
  ),
];

Offering _panel({String? selected}) => buildPanelOffering(
  servers: <Server>[_germany, _canada],
  relayCountries: _relayCountries,
  selectedExitKey: selected,
);

void main() {
  group('выходы: сервер это нода', () {
    test('восемь инбаундов одной машины остаются одним сервером', () {
      final o = _panel();

      // Два сервера, а не четырнадцать строк.
      expect(o.exits.length, 2);
      final de = o.exitByKey('1')!;
      expect(de.panelNodeId, 1);
      expect(de.countryCode, 'DE');
      expect(de.countryName, 'Германия');
      expect(de.label, 'Node #1 (Germany)');
      expect(de.inbounds.length, 8);
      expect(o.exitByKey('5')!.inbounds.length, 6);
    });

    test('каждое поле приходит с происхождением', () {
      final de = _panel().exitByKey('1')!;
      expect(de.origin.source, OfferingSource.panelRest);
      expect(de.origin.wire, 'GET /app/servers[]');
      expect(de.inbounds.first.origin.wire, 'GET /app/servers[].inbounds[]');
    });

    test('переполненный узел виден и назван, а не убран из списка', () {
      final full = Server.fromJson(<String, dynamic>{
        'id': 7,
        'name': 'Node #7',
        'country_code': 'DE',
        'status': 'full',
        'inbounds': <Map<String, dynamic>>[],
      });
      final o = buildPanelOffering(servers: <Server>[_germany, full]);
      final row = o.exitByKey('7')!;
      expect(o.exits.length, 2);
      expect(row.isAvailable, isFalse);
      expect(row.availability.reason, OfferingReason.nodeFull);
      expect(row.availability.detail, 'full');
    });
  });

  group('протокол: инбаунды выбранного узла', () {
    test('reality и обычный TLS это разные строки, а не одно слово vless', () {
      final slate = protocolSlateOf(_panel(selected: '1'), exitKey: '1');

      expect(slate.scope, ProtocolScope.singleExit);
      expect(slate.exitKey, '1');

      final reality = slate.rows.firstWhere(
        (r) =>
            r.key ==
            const ProtocolKey(
              protocol: 'vless',
              transport: 'tcp',
              security: 'reality',
            ),
      );
      final tls = slate.rows.firstWhere(
        (r) =>
            r.key ==
            const ProtocolKey(
              protocol: 'vless',
              transport: 'tcp',
              security: 'tls',
            ),
      );
      expect(reality.label, 'Stealth');
      expect(tls.label, 'Secure');
      expect(reality.key, isNot(tls.key));
      expect(reality.proxyNames, <String>['🇩🇪 Stealth ↪']);
    });

    test(
      'список отфильтрован узлом: у Канады нет grpc, который есть у Германии',
      () {
        const grpc = ProtocolKey(
          protocol: 'vless',
          transport: 'grpc',
          security: 'tls',
        );
        final de = protocolSlateOf(_panel(selected: '1'), exitKey: '1');
        final ca = protocolSlateOf(_panel(selected: '5'), exitKey: '5');

        expect(de.rows.length, 8);
        expect(ca.rows.length, 6);
        expect(de.rows.map((r) => r.key), contains(grpc));
        expect(ca.rows.map((r) => r.key), isNot(contains(grpc)));
      },
    );

    test(
      'без закреплённого узла область названа флотом, а не выдана за узел',
      () {
        final slate = protocolSlateOf(_panel());
        expect(slate.scope, ProtocolScope.wholeFleet);
        expect(slate.exitKey, isNull);
        // Объединение: у Германии восемь троек, Канада добавляет ноль новых.
        expect(slate.rows.length, 8);
        final grpc = slate.rows.firstWhere((r) => r.key.transport == 'grpc');
        expect(grpc.exitKeys, <String>['1']);
      },
    );

    test('naive виден строкой и выключен причиной панели, а не спрятан', () {
      final slate = protocolSlateOf(_panel(selected: '1'), exitKey: '1');
      final naive = slate.rows.firstWhere((r) => r.key.protocol == 'naive');

      expect(naive.isAvailable, isFalse);
      expect(
        naive.availability.reason,
        OfferingReason.inboundNotEmittedByClash,
      );
      expect(naive.availability.detail, 'protocol_not_emitted_by_clash');
      // Прокси в теле нет — связывать строку не с чем, и слой этого не скрывает.
      expect(naive.proxyNames, isEmpty);
    });

    test('панель не прочитала инбаунды: НЕИЗВЕСТНО с её же объяснением', () {
      final o = buildPanelOffering(servers: <Server>[_mute]);
      final exit = o.exitByKey('9')!;

      expect(exit.inbounds, isEmpty);
      expect(exit.inboundsKnown.status, OfferingStatus.unknown);
      expect(
        exit.inboundsKnown.reason,
        OfferingReason.panelDidNotReportInbounds,
      );
      expect(exit.inboundsKnown.detail, 'panel_could_not_read_inbounds');

      final slate = protocolSlateOf(o, exitKey: '9');
      expect(slate.known.status, OfferingStatus.unknown);
      // Молчание не разрешение: пустого «всё доступно» здесь нет.
      expect(slate.rows, isEmpty);
    });

    test('пустой массив инбаундов это НЕ то же самое, что их отсутствие', () {
      final empty = Server.fromJson(<String, dynamic>{
        'id': 3,
        'name': 'Node #3',
        'country_code': 'DE',
        'status': 'online',
        'inbounds': <Map<String, dynamic>>[],
      });
      final exit = buildPanelOffering(servers: <Server>[empty]).exitByKey('3')!;

      expect(exit.inbounds, isEmpty);
      // Панель ответила: включённых инбаундов у узла нет. Это факт, не молчание.
      expect(exit.inboundsKnown.status, OfferingStatus.available);
    });
  });

  group('входы: узлы, а не одна страна', () {
    test(
      'релэй приходит узлом с именем и списком выходов, из которых достижим',
      () {
        final relays = _panel().relays;
        final ru = relays.firstWhere((r) => r.panelNodeId == 2);

        expect(ru.label, 'RU relay');
        expect(ru.countryCode, 'RU');
        expect(ru.reachableFromExitKeys, <String>['1']);
        expect(ru.reachability.status, OfferingStatus.available);
      },
    );

    test('цепочки в конфиге нет: вход выключен С ПРИЧИНОЙ, но виден', () {
      final ru = _panel().relays.firstWhere((r) => r.panelNodeId == 2);

      expect(ru.isAvailable, isFalse);
      expect(ru.availability.reason, OfferingReason.relayNotChainedByGenerator);
      expect(ru.availability.origin.wire, 'GET /app/servers[].via_relay');
    });

    test(
      'страна из /relays без единой привязки остаётся строкой НЕИЗВЕСТНО',
      () {
        final o = buildPanelOffering(
          servers: <Server>[_canada],
          relayCountries: const <RelayCountryRow>[
            RelayCountryRow(
              countryCode: 'NL',
              countryName: 'Нидерланды',
              nodeCount: 3,
            ),
          ],
        );
        final nl = o.relays.single;

        expect(nl.panelNodeId, isNull);
        expect(nl.nodeCountInCountry, 3);
        expect(
          nl.reachability.reason,
          OfferingReason.panelReportsRelaysByCountryOnly,
        );
        expect(nl.availability.status, OfferingStatus.unknown);
      },
    );

    test('генератор, который цепочку строит, вход включает', () {
      final chained = Server.fromJson(<String, dynamic>{
        'id': 1,
        'name': 'Node #1',
        'country_code': 'DE',
        'status': 'online',
        'via_relay': <String, dynamic>{
          'node_id': 2,
          'name': 'RU relay',
          'country_code': 'RU',
          'chained_in_config': true,
        },
        'inbounds': <Map<String, dynamic>>[],
      });
      final o = buildPanelOffering(servers: <Server>[chained]);

      expect(o.relays.single.isAvailable, isTrue);
      expect(o.capabilities.relayChaining.isAvailable, isTrue);
    });
  });

  group('возможности', () {
    test('цепочка недоступна и причина названа, а не скрыта', () {
      final cap = _panel().capabilities.relayChaining;
      expect(cap.isAvailable, isFalse);
      expect(
        cap.availability.reason,
        OfferingReason.relayNotChainedByGenerator,
      );
      // Панель об этом ОТЧИТЫВАЕТСЯ — значит вывод проверяем.
      expect(cap.isVerifiable, isTrue);
    });

    test('блок рекламы и стриминг доступны, но подтвердить их нечем', () {
      final caps = _panel().capabilities;

      for (final cap in <CapabilityOffer>[caps.adBlock, caps.streaming]) {
        expect(cap.isAvailable, isTrue);
        expect(cap.availability.origin.source, OfferingSource.coreRegistry);
        // Ровно то, на что жаловался владелец: «непонятно, работают или нет».
        // Ответ честный — канала обратной связи не существует.
        expect(cap.isVerifiable, isFalse);
        expect(cap.verification.reason, OfferingReason.noFeedbackChannel);
      }
    });

    test('закрепление узла и протокола опирается на реальные ключи', () {
      final caps = _panel().capabilities;
      expect(caps.nodePin.isAvailable, isTrue);
      expect(caps.protocolPin.isAvailable, isTrue);

      // Узел, у которого имён прокси нет вовсе, закреплять нечем.
      final caps2 = buildPanelOffering(servers: <Server>[_mute]).capabilities;
      expect(caps2.protocolPin.availability.status, OfferingStatus.unknown);
    });

    test('без профиля всё недоступно с причиной, а не пусто и молча', () {
      final o = buildEmptyOffering();
      expect(o.exits, isEmpty);
      expect(
        o.capabilities.nodePin.availability.reason,
        OfferingReason.noProfile,
      );
      // Пресеты принадлежат ядру и существуют даже без подписки.
      expect(o.routePresets.length, 9);
    });
  });

  group('импортированная подписка теряет форму — и говорит об этом', () {
    test('машины восстановлены по адресу, а не по числу прокси', () {
      final o = buildImportedOffering(servers: _imported);

      expect(o.exits.length, 2);
      final de = o.exitByKey('de1.example.net')!;
      expect(de.countryCode, 'DE');
      expect(de.inbounds.length, 3);
      expect(o.exitByKey('ca1.example.net')!.inbounds.length, 1);
    });

    test('транспорт и TLS неизвестны: тройка не выдумывается', () {
      final o = buildImportedOffering(servers: _imported);
      final slate = protocolSlateOf(o, exitKey: 'de1.example.net');

      expect(slate.known.status, OfferingStatus.unknown);
      expect(slate.known.reason, OfferingReason.sourceDoesNotReportTransport);
      for (final row in slate.rows) {
        expect(row.key.isFullyQualified, isFalse);
        expect(row.key.transport, isEmpty);
        expect(row.key.security, isEmpty);
        // Строки при этом рабочие: прокси в теле есть, ядро их потребило.
        expect(row.isAvailable, isTrue);
      }
      expect(
        slate.rows.map((r) => r.key.protocol),
        containsAll(<String>['vless', 'hysteria2', 'ss']),
      );
    });

    test('имя прокси сохранено побайтово: им и закрепляется выбор', () {
      final o = buildImportedOffering(
        servers: _imported,
        selectedProxyName: '🇩🇪 Speed ↪',
      );
      expect(o.selectedExitKey, 'de1.example.net');
      final row = protocolSlateOf(
        o,
        exitKey: 'de1.example.net',
      ).rows.firstWhere((r) => r.key.protocol == 'hysteria2');
      expect(row.proxyNames, <String>['🇩🇪 Speed ↪']);
    });

    test('вход невыразим этим источником — контрол выключен, а не спрятан', () {
      final o = buildImportedOffering(servers: _imported);
      expect(o.relays, isEmpty);
      final cap = o.capabilities.relayChaining;
      expect(cap.isAvailable, isFalse);
      expect(
        cap.availability.reason,
        OfferingReason.relayChainingUnsupportedBySource,
      );
    });
  });

  group('маршруты приходят из реестра ядра', () {
    test('их девять, и это те же девять', () {
      expect(kCoreRoutePresets.map((p) => p.id).toList(), <String>[
        'ru-smart',
        'ru-full',
        'telegram-only',
        'ir-smart',
        'by-smart',
        'cn-smart',
        'streaming',
        'adblock',
        'global',
      ]);
    });

    test(
      'пресет на встроенных базах доступен, пресет с внешними — неизвестен',
      () {
        final offers = _panel().routePresets;

        final adblock = offers.firstWhere((p) => p.id == 'adblock');
        expect(adblock.availability.status, OfferingStatus.available);

        final ruSmart = offers.firstWhere((p) => p.id == 'ru-smart');
        expect(ruSmart.availability.status, OfferingStatus.unknown);
        expect(
          ruSmart.availability.reason,
          OfferingReason.rulesetMirrorUnverified,
        );
        expect(ruSmart.availability.detail, 'ru-blocked, ru-blocked-ip');
      },
    );

    test('рекламу режут ровно два пресета, стриминг уводит один', () {
      expect(
        kCoreRoutePresets.where((p) => p.blocksAds).map((p) => p.id),
        <String>['cn-smart', 'adblock'],
      );
      expect(
        kCoreRoutePresets.where((p) => p.routesStreaming).map((p) => p.id),
        <String>['streaming'],
      );
    });

    test('карта легаси-индексов совпадает со списком пикера', () {
      // Индекс это сохранённое значение `CoreConfig.route`: разъехавшись, карта
      // молча увела бы живого пользователя на другой маршрут.
      for (final entry in kLegacyRouteIndexByCoreId.entries) {
        final uiId = entry.key == 'ru-full' ? 'full' : entry.key;
        expect(RoutingMode.defaults[entry.value].id, uiId, reason: entry.key);
      }
      expect(RoutingMode.defaults.length, kCoreRoutePresets.length);
      expect(kLegacyRouteIndexByCoreId.length, kCoreRoutePresets.length);
    });
  });

  group('выдуманного больше нет', () {
    test('Relay.defaults не содержит ни одной страны', () {
      expect(Relay.defaults.every((r) => r.isOff || r.isAuto), isTrue);
      expect(Relay.defaults.map((r) => r.country).whereType<String>(), isEmpty);
    });

    test('пустая выдача панели не подменяется дефолтным набором стран', () {
      expect(Relay.fromCountries(const <Relay>[]), Relay.defaults);
    });
  });

  group('молчание источника это неизвестно, а не разрешение', () {
    test('панель без инбаундов не включает весь список протоколов ядра', () {
      // Легаси-инвентарь: узел, про протоколы которого источник ничего не
      // сказал. Раньше здесь каждая строка приходила `available` — «ничего не
      // исключено, значит можно всё».
      const inventory = ExitInventory(
        source: ExitInventorySource.panelRest,
        nodes: <ExitNode>[
          ExitNode(
            key: '1',
            name: 'Node #1',
            countryCode: 'DE',
            source: ExitInventorySource.panelRest,
          ),
        ],
      );
      final inv = buildProtocolInventory(ProtocolOption.defaults, inventory);

      expect(inv.verified, isFalse);
      expect(inv.unverified?.reason, ProtocolUnavailableReason.sourceSilent);
      expect(inv.unverified?.isUnknown, isTrue);

      final amnezia = inv.choices.firstWhere((c) => c.name == 'AmneziaWG');
      expect(amnezia.availability.status, OfferingStatus.unknown);
      // Не выключено: узлы рабочие, и запрет по молчанию отнял бы выбор.
      expect(amnezia.isAvailable, isTrue);
      // Но и не подтверждено.
      expect(amnezia.availability.isVerified, isFalse);

      // «Авто» это отказ от выбора, он валиден на любом источнике.
      expect(inv.choices.first.availability.status, OfferingStatus.available);
    });

    test('закреплённый узел сужает легаси-инвентарь до своих протоколов', () {
      const inventory = ExitInventory(
        source: ExitInventorySource.panelRest,
        selectedNodeKey: '5',
        nodes: <ExitNode>[
          ExitNode(
            key: '1',
            name: 'DE',
            countryCode: 'DE',
            source: ExitInventorySource.panelRest,
            protocol: 'vless+tcp+reality hysteria2+udp+tls',
          ),
          ExitNode(
            key: '5',
            name: 'CA',
            countryCode: 'CA',
            source: ExitInventorySource.panelRest,
            protocol: 'vless+ws+tls',
          ),
        ],
      );
      final inv = buildProtocolInventory(
        ProtocolOption.defaults,
        inventory,
        onlyNodeKey: '5',
      );

      expect(inv.scopedNodeKey, '5');
      final hy2 = inv.choices.firstWhere((c) => c.name == 'Hysteria2');
      // На выбранном узле его нет, хотя во флоте он есть.
      expect(hy2.isAvailable, isFalse);
      expect(hy2.availability.reason, ProtocolUnavailableReason.notInFleet);
    });

    test('инбаунды панели доезжают до легаси-инвентаря узла', () {
      final node = ExitNode.fromServer(_germany);
      expect(node.protocol, contains('vless+tcp+reality'));
      expect(node.protocol, contains('vless+ws+tls'));
      // `security: none` в токены не попадает: это отсутствие TLS, а не форма.
      expect(node.protocol, contains('shadowsocks+tcp'));
      // Невыпускаемый инбаунд в токены узла не идёт: его в теле нет.
      expect(node.protocol, isNot(contains('naive')));

      // Молчащая панель даёт пустую строку — то самое «неизвестно».
      expect(ExitNode.fromServer(_mute).protocol, isEmpty);
    });
  });
}
