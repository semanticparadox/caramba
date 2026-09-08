// Home и вход: что экран УТВЕРЖДАЕТ против того, что уходит ядру.
//
// Два разных вранья с одним корнем — заголовок говорил про вход то, чего в
// конфиге нет:
//
//  1. подпись под дайлом в состоянии «подключено» собирала «Вход: <страна> ->
//     <узел>», то есть утверждала ЦЕПОЧКУ. Тело clash, которое читает ядро,
//     цепочку выразить не может (`dialer-proxy` в нём нет), на живом флоте
//     панель отдаёт `chained_in_config: false`, и баннер двумя строками ниже
//     это уже сообщал — а заголовок утверждал обратное;
//
//  2. индекс входа для показа КЛАМПИЛСЯ к длине списка, а кодировщик провода
//     на том же значении отдаёт пустую строку («входа не выбрано»). У
//     пользователя, чей сохранённый индекс достался от удалённых выдуманных
//     стран (Турция/Казахстан/Финляндия занимали индексы 2..4), экран называл
//     последнюю строку списка, а ядро не получало ничего. Расхождение было по
//     построению.
//
// Первое проверяется на [panelDialSubtitle] напрямую: дайл в состоянии
// «подключено» подставляет таймер сессии вместо подписи, так что через экран
// эта строка сейчас ненаблюдаема — вычислялась и выбрасывалась. Второе
// проверяется сравнением [effectiveRelayIndex] с самим кодировщиком провода:
// утверждение здесь именно «показанное и отправленное — одно и то же».

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/domain/offering/availability.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/features/servers/relay_screen.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/core_policy_mapping.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Вход, как его называет панель у выхода (`/app/servers[].via_relay`).
const _ruRelay = Relay(
  id: 'RU',
  name: 'Россия',
  desc: 'Вход через RU',
  country: 'RU',
);

const _panelWire = Provenance(
  OfferingSource.panelRest,
  '/app/servers[].via_relay.chained_in_config',
);

/// Живой флот: метка входа у узла есть, цепочки в теле нет.
const _chainImpossible = Availability.unavailable(
  OfferingReason.relayNotChainedByGenerator,
  _panelWire,
);

/// Источник про цепочку промолчал. Это не разрешение.
const _chainUnknown = Availability.unknown(
  OfferingReason.panelDidNotReportInbounds,
  _panelWire,
);

const _chainWorks = Availability.available(_panelWire);

/// Тот же список, что показывает пикер: 0 — Выкл, 1 — Авто, 2 — страна.
final _relays = Relay.fromCountries(<Relay>[
  Relay.fromApiJson(const <String, dynamic>{
    'country_code': 'RU',
    'country_name': 'Россия',
    'node_count': 1,
  }),
]);

void main() {
  group('подпись дайла: цепочка утверждается только подтверждённой', () {
    test('генератор цепочку не строит — стрелки нет', () {
      final s = panelDialSubtitle(
        stage: VpnStage.connected,
        relay: _ruRelay,
        chaining: _chainImpossible,
        serverName: 'Node #1 (Germany)',
        protocolName: 'Авто',
      );
      expect(
        s,
        isNot(contains('->')),
        reason: 'цепочки в конфиге нет — утверждать её нельзя',
      );
      expect(s, isNot(contains('Вход:')));
      expect(s, 'Node #1 (Germany)');
    });

    test('источник промолчал — стрелки тоже нет', () {
      // Молчание источника не запрет и не разрешение. Но подпись — это
      // УТВЕРЖДЕНИЕ, а неподтверждённое утверждение врёт ровно так же, как
      // опровергнутое. Право на стрелку даёт только `available`.
      final s = panelDialSubtitle(
        stage: VpnStage.connected,
        relay: _ruRelay,
        chaining: _chainUnknown,
        serverName: 'Node #1 (Germany)',
        protocolName: 'Авто',
      );
      expect(s, isNot(contains('->')));
      expect(s, 'Node #1 (Germany)');
    });

    test('цепочка подтверждена — вход назван, и это не запрет функции', () {
      final s = panelDialSubtitle(
        stage: VpnStage.connected,
        relay: _ruRelay,
        chaining: _chainWorks,
        serverName: 'Node #1 (Germany)',
        protocolName: 'Авто',
      );
      expect(s, 'Вход: Россия -> Node #1 (Germany)');
    });

    test(
      '«Выкл» и «Авто» цепочкой не являются даже при рабочей возможности',
      () {
        for (final r in Relay.defaults) {
          final s = panelDialSubtitle(
            stage: VpnStage.connected,
            relay: r,
            chaining: _chainWorks,
            serverName: 'Node #1 (Germany)',
            protocolName: 'Авто',
          );
          expect(s, 'Node #1 (Germany)', reason: r.name);
        }
      },
    );
  });

  group('показанный вход и отправленный — одно и то же', () {
    test('индекс вне списка приводится так же, как его приводит провод', () {
      // Ровно тот пользователь, о котором речь: сохранённый индекс 2..4 достался
      // от удалённых Турции/Казахстана/Финляндии, а список сегодня из трёх
      // строк. Кламп показал бы «Россия», ядро получило бы «».
      for (final stored in <int>[-1, 0, 1, 2, 3, 4, 7, 1 << 19]) {
        final shown = effectiveRelayIndex(stored, _relays);
        expect(
          shown,
          inInclusiveRange(0, _relays.length - 1),
          reason: 'stored=$stored',
        );

        final wireOfShown = corePolicyFrom(
          CoreConfig(relay: shown),
          _relays,
        ).relay;
        final wireActual = corePolicyFrom(
          CoreConfig(relay: stored),
          _relays,
        ).relay;
        expect(
          wireOfShown,
          wireActual,
          reason:
              'stored=$stored: экран называет ${_relays[shown].name}, а ядру '
              'уходит «$wireActual»',
        );
      }
    });

    test('пустой список нечем назвать, и это не нулевой индекс', () {
      expect(effectiveRelayIndex(0, const <Relay>[]), -1);
      expect(effectiveRelayIndex(5, const <Relay>[]), -1);
    });

    test('валидный индекс не двигается', () {
      expect(effectiveRelayIndex(2, _relays), 2);
      expect(corePolicyFrom(const CoreConfig(relay: 2), _relays).relay, 'RU');
    });
  });
}
