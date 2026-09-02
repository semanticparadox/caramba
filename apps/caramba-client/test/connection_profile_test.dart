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
}
