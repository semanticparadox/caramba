// Миграция и сериализация ConnectionProfile.
//
// Записи, сделанные до generic-режима, не несут `format`, `servers`,
// `selected_server_id`, `last_probe` и `servers_updated_ms`. Стор читает весь
// список одной JSON-строкой, поэтому одна такая запись не должна ронять разбор
// (иначе пользователь теряет ВСЕ профили разом).

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

void main() {
  group('ConnectionProfile миграция старого JSON', () {
    /// Запись ровно в том виде, в каком её писала версия до generic-режима.
    Map<String, dynamic> legacyJson() => <String, dynamic>{
      'id': 'cp_1',
      'type': 'rawSub',
      'display_name': 'Резерв',
      'source': 'https://sub.example/x',
      'panel_url': null,
      'subscription_uuid': null,
      'access_token': null,
      'raw_config': 'proxies: []',
      'branding_cache': null,
      'last_active_ms': 42,
    };

    test('новые поля читаются как значения по умолчанию', () {
      final p = ConnectionProfile.fromJson(legacyJson());

      expect(p.id, 'cp_1');
      expect(p.displayName, 'Резерв');
      expect(p.rawConfig, 'proxies: []');
      expect(p.lastActiveMs, 42);
      // Отсутствие format означает автодетект, а не пустую строку на проводе.
      expect(p.format, 'auto');
      expect(p.servers, isEmpty);
      expect(p.selectedServerId, isNull);
      expect(p.lastProbe, isNull);
      expect(p.serversUpdatedMs, 0);
    });

    test('пересохранение старой записи добавляет новые ключи', () {
      final json = ConnectionProfile.fromJson(legacyJson()).toJson();

      expect(json['format'], 'auto');
      expect(json['servers'], isEmpty);
      expect(json['selected_server_id'], isNull);
      expect(json['last_probe'], isNull);
      expect(json['servers_updated_ms'], 0);
    });

    test('мусор в новых полях не роняет разбор', () {
      final json = legacyJson()
        ..['format'] = 42
        ..['servers'] = 'not a list'
        ..['selected_server_id'] = ''
        ..['last_probe'] = 'broken'
        ..['servers_updated_ms'] = 'soon';

      final p = ConnectionProfile.fromJson(json);

      expect(p.format, 'auto');
      expect(p.servers, isEmpty);
      expect(p.selectedServerId, isNull);
      expect(p.lastProbe, isNull);
      expect(p.serversUpdatedMs, 0);
    });
  });

  group('ConnectionProfile round trip', () {
    const server = ImportedServer(
      id: 'NL-01',
      name: 'Amsterdam 01',
      type: 'vless',
      server: 'nl-01.example',
      port: 443,
      country: 'NL',
    );

    test('новые поля переживают запись и чтение', () {
      const original = ConnectionProfile(
        id: 'cp_2',
        type: ProfileType.rawSub,
        displayName: 'Основная',
        source: 'https://sub.example/y',
        rawConfig: 'proxies: []',
        format: 'clash',
        servers: [server],
        selectedServerId: 'NL-01',
        lastProbe: ProbeSnapshot(
          latencyMs: {'NL-01': 42, 'DE-02': -1},
          updatedMs: 1700000000000,
        ),
        serversUpdatedMs: 1700000000000,
      );

      final restored = ConnectionProfile.fromJson(original.toJson());

      expect(restored.format, 'clash');
      expect(restored.serverCount, 1);
      expect(restored.servers.single.id, 'NL-01');
      expect(restored.servers.single.port, 443);
      expect(restored.selectedServerId, 'NL-01');
      expect(restored.latencyOf('NL-01'), 42);
      // -1 это таймаут, он обязан сохраниться отличимым от «не мерили».
      expect(restored.latencyOf('DE-02'), -1);
      expect(restored.latencyOf('TR-03'), isNull);
      expect(restored.serversUpdatedMs, 1700000000000);
    });

    test('copyWith снимает пин отдельным флагом', () {
      const pinned = ConnectionProfile(
        id: 'cp_3',
        type: ProfileType.rawSub,
        displayName: 'x',
        source: 'x',
        selectedServerId: 'NL-01',
      );

      final cleared = pinned.copyWith(clearSelectedServer: true);
      expect(cleared.selectedServerId, isNull);
      // Без флага null-аргумент означает «не менять», как везде в copyWith.
      expect(pinned.copyWith().selectedServerId, 'NL-01');
    });

    test('ProbeSnapshot.fromResults индексирует по id узла', () {
      const results = <ProbeResult>[
        ProbeResult(id: 'NL-01', latencyMs: 42),
        ProbeResult(id: 'DE-02', latencyMs: -1),
        // Узел без id ядро прислать не должно, но пустой ключ сломал бы карту.
        ProbeResult(id: '', latencyMs: 10),
      ];
      final at = DateTime.fromMillisecondsSinceEpoch(1700000000000);

      final snapshot = ProbeSnapshot.fromResults(results, at: at);

      expect(snapshot.latencyMs, {'NL-01': 42, 'DE-02': -1});
      expect(snapshot.updatedMs, 1700000000000);
    });
  });

  group('снимок замера несёт вердикты, а не только числа', () {
    test('вердикты и TCP доезжают до снимка', () {
      const results = <ProbeResult>[
        ProbeResult(
          id: 'DE Stealth',
          latencyMs: -1,
          tcpMs: 118,
          verdict: ProbeVerdict.authRejected,
        ),
        ProbeResult(id: 'CA Speed', latencyMs: 179, verdict: ProbeVerdict.ok),
      ];
      final snap = ProbeSnapshot.fromResults(results);

      expect(snap.verdictOf('DE Stealth'), ProbeVerdict.authRejected);
      expect(snap.tcpMs['DE Stealth'], 118);
      expect(snap.workingCount, 1);
    });

    // РЕГРЕССИЯ, которую этот тест и стережёт: запись, сделанная сборкой до
    // вердиктов, обязана читаться как «не знаю». Прочитать её как «ok» значило
    // бы вернуть ровно ту ложь, ради которой вердикты и заведены, — только
    // через хранилище.
    test('старая запись без вердиктов читается как «не знаю»', () {
      final snap = ProbeSnapshot.fromJson(<String, dynamic>{
        'latency_ms': <String, dynamic>{'DE Stealth': 118},
        'updated_ms': 1700000000000,
      });
      expect(snap, isNotNull);
      expect(snap!.latencyMs['DE Stealth'], 118);
      expect(snap.verdictOf('DE Stealth'), ProbeVerdict.unknown);
      expect(snap.workingCount, 0);
    });

    test('незнакомый вердикт из будущей сборки не роняет разбор', () {
      final snap = ProbeSnapshot.fromJson(<String, dynamic>{
        'latency_ms': <String, dynamic>{'X': 10},
        'verdicts': <String, dynamic>{'X': 'quantum_tunnel_collapsed'},
        'updated_ms': 1,
      });
      expect(snap!.verdictOf('X'), ProbeVerdict.unknown);
    });

    test('снимок переживает круг через JSON', () {
      final snap = ProbeSnapshot.fromResults(const <ProbeResult>[
        ProbeResult(
          id: 'CA Speed',
          latencyMs: 179,
          tcpMs: 150,
          verdict: ProbeVerdict.ok,
        ),
      ]);
      final back = ProbeSnapshot.fromJson(snap.toJson())!;
      expect(back.verdictOf('CA Speed'), ProbeVerdict.ok);
      expect(back.tcpMs['CA Speed'], 150);
    });
  });

  group('выбор автоподбора переживает перезапуск', () {
    test('запись ходит через JSON без потерь', () {
      const pick = AutoPickRecord(
        proxyName: 'CA Speed',
        exitKey: '2',
        countryCode: 'CA',
        machineTitle: 'Canada',
        protocolLabel: 'vless · tcp · reality',
        latencyMs: 179,
        confirmed: true,
        checked: 13,
        working: 4,
        total: 13,
        updatedMs: 1700000000000,
        serversUpdatedMs: 42,
        reasonCode: 'best_score',
      );
      final back = AutoPickRecord.fromJson(pick.toJson())!;
      expect(back.proxyName, 'CA Speed');
      expect(back.confirmed, isTrue);
      expect(back.working, 4);
      expect(back.serversUpdatedMs, 42);
      expect(back.shortLabel, 'Canada');
    });

    test('запись без имени прокси читается как «подбора не было»', () {
      // Закрепить такой выбор нечем: имя прокси — единственный ключ, который
      // понимают и ядро, и предложение.
      expect(
        AutoPickRecord.fromJson(<String, dynamic>{'latency_ms': 10}),
        isNull,
      );
      expect(AutoPickRecord.fromJson(null), isNull);
    });

    test('профиль без auto_pick грузится, поле остаётся пустым', () {
      final p = ConnectionProfile.fromJson(<String, dynamic>{
        'id': 'cp_1',
        'type': 'rawSub',
        'display_name': 'Резерв',
        'source': 'x',
      });
      expect(p.autoPick, isNull);
      expect(p.verdictOf('anything'), ProbeVerdict.unknown);
    });

    test('сброс выбора — явный флаг, а не отсутствие аргумента', () {
      const pick = AutoPickRecord(
        proxyName: 'CA Speed',
        latencyMs: 179,
        updatedMs: 1,
      );
      const base = ConnectionProfile(
        id: 'cp_1',
        type: ProfileType.rawSub,
        displayName: 'x',
        source: 'x',
        autoPick: pick,
      );
      expect(base.copyWith().autoPick?.proxyName, 'CA Speed');
      expect(base.copyWith(clearAutoPick: true).autoPick, isNull);
    });
  });
}
