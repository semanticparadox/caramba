// CoreConfig (индексы UI) -> CorePolicy (строки ABI v2).
//
// Это единственный шов между пикерами и ядром: ошибка здесь тихо отправляет
// ядру не тот пресет или не ту страну relay, и туннель поднимается «не так»,
// не падая. Поэтому проверяем и значения, и правило «Авто = не переопределять».

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';

/// Входы ровно в той форме, в какой их отдаёт панель: `GET /app/relays`
/// плюс два псевдо-варианта, которые дописывает клиент. Раньше здесь стоял
/// `Relay.defaults` с выдуманными Турцией, Казахстаном и Финляндией — тест
/// проверял перевод индекса в страну по списку, которого у оператора нет.
/// Индексы сохранены: 0 — Выкл, 1 — Авто, 2 — страна.
final _panelRelays = Relay.fromCountries(<Relay>[
  Relay.fromApiJson(const <String, dynamic>{
    'country_code': 'TR',
    'country_name': 'Турция',
    'node_count': 2,
  }),
]);

void main() {
  group('corePolicyFrom', () {
    test('дефолтная конфигурация означает «ядро решает само»', () {
      final policy = corePolicyFrom(const CoreConfig(), _panelRelays);

      // Протокол это единственное поле, где «Авто» на проводе имеет имя.
      expect(policy.protocol, 'auto');
      // Умолчание — ГЛОБАЛЬНЫЙ пресет, а не российский.
      //
      // Здесь стояло `ru-smart`, и это фиксировало живой баг: конфигурация «по
      // умолчанию» означала «пользователь в России». У `ru-smart` финальное
      // действие DIRECT, поэтому американец с поднятым туннелем гнал почти
      // весь трафик мимо него. Страну теперь приносит панель
      // (`client_country`), и `adoptUserCountry` вернёт `ru-smart` тому, кто
      // действительно в РФ; до этого момента врать про место нельзя.
      expect(policy.preset, 'global');
      expect(policy.relay, '');
      expect(policy.stack, isNull);
      expect(policy.mtu, isNull);
      expect(policy.dns, isNull);
      expect(policy.fakeIp, isTrue);
      expect(policy.ipv6, isFalse);
      expect(policy.killSwitch, isTrue);
      expect(policy.split?.mode, 'off');
    });

    test('индексы переводятся в идентификаторы контракта', () {
      final policy = corePolicyFrom(
        const CoreConfig(
          protocol: 2, // VLESS-Reality
          route: 3, // streaming
          relay: 2, // TR в _panelRelays
          stack: 2, // gvisor
          dns: 1, // cloudflare
          mtu: 2, // 1420
          ipv6: true,
          fakeIp: false,
          killSwitch: false,
        ),
        _panelRelays,
      );

      expect(policy.protocol, 'VLESS-Reality');
      expect(policy.preset, 'streaming');
      expect(policy.relay, 'TR');
      expect(policy.stack, 'gvisor');
      expect(policy.mtu, 1420);
      expect(policy.dns?.nameservers, ['https://1.1.1.1/dns-query']);
      expect(policy.dns?.fallback, ['tls://1.1.1.1:853']);
      expect(policy.ipv6, isTrue);
      expect(policy.fakeIp, isFalse);
      expect(policy.killSwitch, isFalse);
    });

    test('пресет full переименован в ru-full, как его знает ядро', () {
      final policy = corePolicyFrom(const CoreConfig(route: 2), _panelRelays);
      expect(policy.preset, 'ru-full');
    });

    test('relay «Выкл» и «Авто» дают пустую строку', () {
      expect(
        corePolicyFrom(const CoreConfig(relay: 0), _panelRelays).relay,
        '',
      );
      expect(
        corePolicyFrom(const CoreConfig(relay: 1), _panelRelays).relay,
        '',
      );
    });

    test('индекс relay вне списка панели не роняет маппинг', () {
      final policy = corePolicyFrom(const CoreConfig(relay: 99), _panelRelays);
      expect(policy.relay, '');
    });

    test('split-режимы переводятся в allow/bypass, домены разбираются', () {
      final allow = corePolicyFrom(
        const CoreConfig(
          splitMode: SplitMode.onlySelected,
          splitApps: {'com.b', 'com.a'},
          bypassDomains: 'example.com, bank.ru\nmail.local\n\n',
        ),
        _panelRelays,
      );
      expect(allow.split?.mode, 'allow');
      // Порядок приложений детерминирован: иначе JSON политики «дрожит» и
      // баннер «нужно переподключение» загорается на ровном месте.
      expect(allow.split?.apps, ['com.a', 'com.b']);
      expect(allow.split?.bypassDomains, [
        'example.com',
        'bank.ru',
        'mail.local',
      ]);

      final bypass = corePolicyFrom(
        const CoreConfig(splitMode: SplitMode.bypassSelected),
        _panelRelays,
      );
      expect(bypass.split?.mode, 'bypass');
    });

    test('в режиме off приложения и домены не отправляются', () {
      final policy = corePolicyFrom(
        const CoreConfig(splitApps: {'com.a'}, bypassDomains: 'example.com'),
        _panelRelays,
      );
      expect(policy.split?.mode, 'off');
      expect(policy.split?.apps, isEmpty);
      expect(policy.split?.bypassDomains, isEmpty);
    });

    test('toJson не пишет ключи, которые ядро не должно менять', () {
      final json = corePolicyFrom(const CoreConfig(), _panelRelays).toJson();
      expect(json.containsKey('stack'), isFalse);
      expect(json.containsKey('mtu'), isFalse);
      expect(json.containsKey('dns'), isFalse);
      expect(json['protocol'], 'auto');
    });
  });

  group('CoreConfig.bypassDomainList', () {
    test('терпит запятые, точки с запятой и переводы строк', () {
      const cfg = CoreConfig(bypassDomains: ' a.com ,, b.com;\n c.com \r\n');
      expect(cfg.bypassDomainList, ['a.com', 'b.com', 'c.com']);
    });

    test('пустая строка даёт пустой список', () {
      expect(const CoreConfig().bypassDomainList, isEmpty);
    });
  });
}
