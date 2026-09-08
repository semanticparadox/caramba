// Главная не имеет права называть тип, по которому ядро не работает.
//
// Снято на устройстве: закреплён TUIC (собственный замер по нему говорит «Не
// проходит: адрес не отвечает»), после переподключения экран показывает
// «Защищено», сервер «🇩🇪 Stream» и строку «Тип подключения: TUIC», а в логе
// ядра — `match Match using CARAMBA[🇩🇪 Stream]`, то есть vless.
//
// Механика известна и менять её никто не собирается: `applyProtocol`
// (libs/caramba-core/profile/profile.go) собирает url-test группу по
// `m["type"] == protocolClashType[Policy.Protocol]` и ставит её первой в
// селекторе; когда все её узлы мертвы, селектор уходит на другой прокси —
// связь по другому протоколу лучше, чем никакой. Деградация правильная, но
// молчаливая, и назвать её обязан экран.
//
// Здесь три утверждения, и третье не менее важно первых двух: НЕЗНАНИЕ НЕ ЕСТЬ
// РАСХОЖДЕНИЕ. Строка, которая кричит о подмене всякий раз, когда ядро молчит,
// перестаёт значить что-либо ровно так же, как строка, которая молчит всегда.

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/domain/autopilot/auto_pick.dart' show FleetFact;
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/features/protocol/protocol_truth.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/widgets/lucide.dart';

const _options = ProtocolOption.defaults;

/// Индексы в [ProtocolOption.defaults] — то, что лежит в `CoreConfig.protocol`.
const int _auto = 0;
const int _reality = 2;
const int _tuic = 4;
const int _vless = 6;

/// Прокси, на котором ядро стояло в снятом случае.
const String _live = '🇩🇪 Stream';

/// Инбаунд немецкой машины, поднявшийся вместо TUIC.
const _vlessFact = FleetFact(
  proxyName: _live,
  machineTitle: 'Stream',
  countryCode: 'DE',
  protocol: 'vless',
  transport: 'tcp',
  security: 'reality',
  protocolLabel: 'vless · tcp · reality',
);

const _tuicFact = FleetFact(
  proxyName: _live,
  machineTitle: 'Stream',
  countryCode: 'DE',
  protocol: 'tuic',
  transport: 'udp',
  security: 'tls',
  protocolLabel: 'tuic · udp · tls',
);

/// Инбаунд, который ядру попросить нечем: в `protocolClashType` такой строки
/// нет вовсе.
const _naiveFact = FleetFact(
  proxyName: _live,
  protocol: 'naive',
  transport: 'http2',
  security: 'tls',
);

const _facts = <String, FleetFact>{_live: _vlessFact};

ProtocolTruth _truth({
  required int pinned,
  String? activeProxy,
  Map<String, FleetFact> facts = _facts,
  AutoLabel auto = AutoLabel.unknown,
}) =>
    protocolTruthOf(
      options: _options,
      pinned: pinned,
      activeProxy: activeProxy,
      facts: facts,
      auto: auto,
    );

void main() {
  group('расхождение закреплённого и фактического типа', () {
    test('закреплён TUIC, на проводе vless — строка называет провод', () {
      final t = _truth(pinned: _tuic, activeProxy: _live);
      expect(t.diverged, isTrue);
      // Фактический стоит первым: значение обрезается многоточием справа, и
      // обрезок обязан оставаться правдой.
      expect(t.value, 'VLESS вместо TUIC');
      expect(t.value.startsWith('VLESS'), isTrue);
    });

    test('закреплённый тип не потерян: он назван и в строке, и в объяснении',
        () {
      final t = _truth(pinned: _tuic, activeProxy: _live);
      expect(t.value, contains('TUIC'));
      expect(t.note, contains('TUIC'));
      expect(t.note, contains('VLESS'));
      // Объяснение обязано назвать ПРИЧИНУ, а не только факт: без неё подмена
      // читается как «настройка сама сбросилась».
      expect(t.note, contains('не отвечает'));
    });

    test('иконка берётся у того типа, который на проводе', () {
      // Молния TUIC рядом со словом «VLESS» — второе утверждение о протоколе,
      // и оно осталось бы ложным.
      final t = _truth(pinned: _tuic, activeProxy: _live);
      expect(t.glyph, Lucide.shield);
      expect(t.glyph, isNot(Lucide.zap));
    });

    test('ядро подняло протокол, которого нет в его же таблице', () {
      // Попросить `naive` нечем, но это не повод оставить «TUIC» над ним:
      // слово источника про провод — единственное, что тут известно.
      final t = _truth(
        pinned: _tuic,
        activeProxy: _live,
        facts: <String, FleetFact>{_live: _naiveFact},
      );
      expect(t.diverged, isTrue);
      expect(t.value, 'NaiveProxy вместо TUIC');
    });
  });

  group('совпадение не помечается', () {
    test('закреплён TUIC, на проводе tuic — ни пометки, ни объяснения', () {
      final t = _truth(
        pinned: _tuic,
        activeProxy: _live,
        facts: const <String, FleetFact>{_live: _tuicFact},
      );
      expect(t.diverged, isFalse);
      expect(t.value, 'TUIC');
      expect(t.note, isEmpty);
      expect(t.glyph, isNull);
    });

    test('Reality на TLS-инбаунде — не подмена: ядро отбирает по семейству',
        () {
      // `applyProtocol` сравнивает только `type:`; уточнения формы в отборе не
      // участвуют, и ядро ничего сверх семейства не обещало. Считать это
      // расхождением значило бы поднимать тревогу на каждом пине Reality.
      const tls = FleetFact(
        proxyName: _live,
        protocol: 'vless',
        transport: 'ws',
        security: 'tls',
      );
      final t = _truth(
        pinned: _reality,
        activeProxy: _live,
        facts: const <String, FleetFact>{_live: tls},
      );
      expect(t.diverged, isFalse);
      expect(t.value, 'VLESS · Reality');
    });

    test('голый VLESS на reality-инбаунде — тоже одно семейство', () {
      final t = _truth(pinned: _vless, activeProxy: _live);
      expect(t.diverged, isFalse);
      expect(t.value, 'VLESS');
    });

    test('«Авто» расходиться не с чем: это отказ от выбора', () {
      const auto = AutoLabel(
        choice: 'vless · tcp · reality',
        source: 'Сейчас в туннеле',
      );
      final t = _truth(pinned: _auto, activeProxy: _live, auto: auto);
      expect(t.diverged, isFalse);
      expect(t.value, 'Авто · vless · tcp · reality');
    });
  });

  group('отсутствие данных о фактическом типе — не расхождение', () {
    test('туннеля нет: строка называет настройку', () {
      final t = _truth(pinned: _tuic);
      expect(t.diverged, isFalse);
      expect(t.value, 'TUIC');
      expect(t.note, isEmpty);
    });

    test('ядро не назвало узел (пустая строка) — молчим', () {
      final t = _truth(pinned: _tuic, activeProxy: '  ');
      expect(t.diverged, isFalse);
      expect(t.value, 'TUIC');
    });

    test('узел назван, но предложение о нём ничего не знает', () {
      // Ровно тот случай, из-за которого автоподбор говорил «Ядро не вернуло
      // ни одного узла»: имя есть, фактов за ним нет. Подменой это назвать
      // нельзя — неизвестно, что подменили.
      final t = _truth(
        pinned: _tuic,
        activeProxy: 'какой-то другой прокси',
      );
      expect(t.diverged, isFalse);
      expect(t.value, 'TUIC');
      expect(t.note, isEmpty);
    });

    test('факт есть, а протокол в нём не назван', () {
      const mute = FleetFact(proxyName: _live, machineTitle: 'Stream');
      final t = _truth(
        pinned: _tuic,
        activeProxy: _live,
        facts: const <String, FleetFact>{_live: mute},
      );
      expect(t.diverged, isFalse);
      expect(t.value, 'TUIC');
    });
  });

  group('строка Главной собрана из живых источников', () {
    // Провайдер — пять строк `watch`, и ошибиться в них можно ровно один раз:
    // подставить закреплённую опцию вместо сведённого ответа. Тест держит
    // именно проводку.
    ProviderContainer container({
      required int pinned,
      String? activeProxy,
      Map<String, FleetFact> facts = _facts,
    }) {
      final c = ProviderContainer(
        overrides: <Override>[
          coreConfigProvider.overrideWith((ref) => CoreConfigNotifier()),
          activeProxyProvider.overrideWithValue(activeProxy),
          fleetFactsProvider.overrideWithValue(facts),
          autoProtocolLabelProvider.overrideWithValue(AutoLabel.unknown),
        ],
      );
      addTearDown(c.dispose);
      c.read(coreConfigProvider.notifier).setProtocol(pinned);
      return c;
    }

    test('живой туннель по vless под закреплённым TUIC', () {
      final c = container(pinned: _tuic, activeProxy: _live);
      final t = c.read(protocolTruthProvider);
      expect(t.diverged, isTrue);
      expect(t.value, 'VLESS вместо TUIC');
    });

    test('без туннеля та же проводка молчит', () {
      final c = container(pinned: _tuic);
      final t = c.read(protocolTruthProvider);
      expect(t.diverged, isFalse);
      expect(t.value, 'TUIC');
    });
  });

  group('момент закрепления типа, входы которого молчат', () {
    test('закрепить дают, но провал называют заранее', () {
      // Запрещать нельзя: замер — снимок ОДНОЙ сети в ОДИН момент, и сам экран
      // оговаривается, что часть чисел про адрес, а не про вход. Зато молчать
      // про деградацию тоже нельзя — ядро о ней не скажет.
      final t = protocolPinToast(
        label: 'TUIC',
        exact: false,
        noneAnswered: true,
      );
      expect(t, contains('TUIC'));
      expect(t, contains('не отвечает'));
      expect(t, contains('другим типом'));
    });

    test('про точность пина в этот момент молчим', () {
      // «Инбаунд закреплён» рядом с «ни один вход не отвечает» читается как
      // обещание, что закреплённое и поднимется.
      final t = protocolPinToast(
        label: 'TUIC',
        exact: true,
        noneAnswered: true,
      );
      expect(t, isNot(contains('закреплён')));
      expect(t, contains('не отвечает'));
    });

    test('входы отвечают — прежний текст без предупреждения', () {
      final exact = protocolPinToast(
        label: 'VLESS · tcp · reality',
        exact: true,
        noneAnswered: false,
      );
      final family = protocolPinToast(
        label: 'TUIC',
        exact: false,
        noneAnswered: false,
      );
      expect(
        exact,
        'Тип подключения: VLESS · tcp · reality — инбаунд закреплён',
      );
      expect(family, 'Тип подключения: TUIC — ядро закрепит семейство');
      expect(exact, isNot(contains('не отвечает')));
      expect(family, isNot(contains('не отвечает')));
    });
  });
}
