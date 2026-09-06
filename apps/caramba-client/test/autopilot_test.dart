/// Автопилот: что он выбирает, чего не выбирает и как называет свой выбор.
///
/// Тесты держатся за ПОВЕДЕНИЕ, а не за числа коэффициентов: «reality при
/// равной задержке выигрывает у голого TLS» переживёт правку 0.85 → 0.87, а
/// `expect(score, 100.3)` сломается на ней, ничего при этом не защитив.
library;

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/domain/autopilot/auto_pick.dart';
import 'package:caramba_client/domain/autopilot/autopilot_state.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/domain/offering/offering_builder.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

ProbeResult _p(
  String id, {
  int latency = 100,
  ProbeVerdict verdict = ProbeVerdict.ok,
  int tcp = -1,
  String country = '',
}) => ProbeResult(
  id: id,
  name: id,
  country: country,
  latencyMs: latency,
  tcpMs: tcp,
  verdict: verdict,
);

FleetFact _f(
  String proxy, {
  String exitKey = 'm1',
  String country = 'DE',
  String title = 'Frankfurt',
  String protocol = 'vless',
  String security = 'tls',
  String transport = 'tcp',
  double load = -1,
  bool wireChained = false,
}) => FleetFact(
  proxyName: proxy,
  exitKey: exitKey,
  countryCode: country,
  machineTitle: title,
  protocol: protocol,
  transport: transport,
  security: security,
  protocolLabel: '$protocol · $transport · $security',
  loadPct: load,
  wireChained: wireChained,
);

/// Запись прошлого выбора. Возраст задаётся в минутах, а не отметкой времени:
/// проверяется поведение «замер протух», а не арифметика дат.
AutoPickRecord _rec({
  String proxy = 'DE Stealth',
  int ageMinutes = 1,
  int fleet = 7,
  String exitKey = '',
  String country = '',
  String title = '',
}) => AutoPickRecord(
  proxyName: proxy,
  exitKey: exitKey,
  countryCode: country,
  machineTitle: title,
  latencyMs: 120,
  serversUpdatedMs: fleet,
  updatedMs: DateTime.now()
      .subtract(Duration(minutes: ageMinutes))
      .millisecondsSinceEpoch,
);

/// Узел, на котором ядро держит сессию.
AutoHolder _tunnelOn(
  String proxy, {
  String exitKey = '',
  String country = '',
  String title = '',
}) => AutoHolder(
  source: AutoHolderSource.tunnel,
  proxyName: proxy,
  exitKey: exitKey,
  countryCode: country,
  title: title,
  nodeTitle: proxy,
);

/// Узел, закреплённый в строке «Сервер», когда туннеля нет.
AutoHolder _pinnedTo(
  String proxy, {
  String exitKey = '',
  String country = '',
  String title = '',
}) => AutoHolder(
  source: AutoHolderSource.pin,
  proxyName: proxy,
  exitKey: exitKey,
  countryCode: country,
  title: title,
  nodeTitle: proxy,
);

void main() {
  group('автоподбор: кого выбирает', () {
    // ГЛАВНОЕ. Узел, сквозь который прошёл настоящий запрос, важнее узла,
    // который просто быстрый. До вердиктов ядра это было неотличимо: узел с
    // отвергнутым ключом отдавал время TCP и выглядел лучшим.
    test(
      'подтверждённый узел выигрывает у более быстрого неподтверждённого',
      () {
        final out = autoPick(
          results: [
            _p('fast', latency: 30, verdict: ProbeVerdict.tcpOnly, tcp: 30),
            _p('slow', latency: 200, verdict: ProbeVerdict.ok),
          ],
          facts: {'fast': _f('fast'), 'slow': _f('slow')},
        );
        expect(out.pick?.proxyName, 'slow');
        expect(out.pick?.confirmed, isTrue);
      },
    );

    test('мёртвые узлы в кандидаты не попадают вовсе', () {
      final out = autoPick(
        results: [
          _p('dead', latency: -1, verdict: ProbeVerdict.authRejected, tcp: 118),
          _p('live', latency: 250, verdict: ProbeVerdict.ok),
        ],
        facts: {'dead': _f('dead'), 'live': _f('live')},
      );
      expect(out.ranked.map((c) => c.name), ['live']);
    });

    // Форма подключения — часть выбора, а не украшение: reality маскируется
    // под чужой сайт, голый TLS на нестандартном порту заметен.
    test('при равной задержке reality выигрывает у tls', () {
      final out = autoPick(
        results: [_p('tls', latency: 100), _p('re', latency: 100)],
        facts: {
          'tls': _f('tls', security: 'tls'),
          're': _f('re', security: 'reality'),
        },
      );
      expect(out.pick?.proxyName, 're');
    });

    test('загруженная машина проигрывает свободной при равной задержке', () {
      final out = autoPick(
        results: [_p('busy', latency: 100), _p('free', latency: 100)],
        facts: {
          'busy': _f('busy', load: 90),
          'free': _f('free', load: 5, exitKey: 'm2'),
        },
      );
      expect(out.pick?.proxyName, 'free');
    });

    test('счёт не даёт форме перебить разницу между континентами', () {
      // 40 мс на reality против 400 мс на голом TLS: поправка ±15% не должна
      // выбрать дальний узел. Коэффициенты умеренны ровно для этого.
      final out = autoPick(
        results: [_p('near', latency: 40), _p('far', latency: 400)],
        facts: {
          'near': _f('near', security: 'none'),
          'far': _f('far', security: 'reality'),
        },
      );
      expect(out.pick?.proxyName, 'near');
    });
  });

  group('автоподбор: ограничения человека сильнее счёта', () {
    test('закреплённая страна отсекает чужие узлы', () {
      final out = autoPick(
        results: [_p('de', latency: 50), _p('ca', latency: 10)],
        facts: {
          'de': _f('de', country: 'DE'),
          'ca': _f('ca', country: 'CA', exitKey: 'm2'),
        },
        constraints: const AutoConstraints(pinnedCountry: 'DE'),
      );
      expect(out.pick?.proxyName, 'de');
    });

    test('узел без известной страны под закреплением страны не проходит', () {
      // «Наверное он немецкий» — та самая догадка, из-за которой на Home
      // стояла «Германия» над канадским выходом.
      final out = autoPick(
        results: [_p('unknown', latency: 10)],
        facts: {'unknown': _f('unknown', country: '')},
        constraints: const AutoConstraints(pinnedCountry: 'DE'),
      );
      expect(out.pick, isNull);
      expect(out.failure?.kind, AutoFailureKind.constrainedOut);
    });

    test('закреплённое семейство протокола отсекает остальные', () {
      final out = autoPick(
        results: [_p('v', latency: 100), _p('h', latency: 10)],
        facts: {
          'v': _f('v', protocol: 'vless'),
          'h': _f('h', protocol: 'hysteria2', exitKey: 'm2'),
        },
        constraints: const AutoConstraints(protocolFamily: 'vless'),
      );
      expect(out.pick?.proxyName, 'v');
    });
  });

  group('гистерезис: выбор не прыгает от шума измерения', () {
    AutoPickRecord prev(String proxy) => AutoPickRecord(
      proxyName: proxy,
      latencyMs: 100,
      updatedMs: DateTime.now().millisecondsSinceEpoch,
    );

    test('мелкий выигрыш нового лидера прошлый выбор не сменяет', () {
      final out = autoPick(
        results: [_p('old', latency: 100), _p('new', latency: 85)],
        facts: {
          'old': _f('old'),
          'new': _f('new', exitKey: 'm2'),
        },
        previous: prev('old'),
      );
      expect(out.pick?.proxyName, 'old');
      expect(out.pick?.reasonCode, 'kept_previous');
    });

    test('крупный выигрыш выбор сменяет', () {
      final out = autoPick(
        results: [_p('old', latency: 300), _p('new', latency: 60)],
        facts: {
          'old': _f('old'),
          'new': _f('new', exitKey: 'm2'),
        },
        previous: prev('old'),
      );
      expect(out.pick?.proxyName, 'new');
      expect(out.pick?.reasonCode, 'best_score');
    });

    test('переставший работать прошлый выбор не удерживается', () {
      final out = autoPick(
        results: [
          _p('old', latency: -1, verdict: ProbeVerdict.authRejected, tcp: 20),
          _p('new', latency: 95),
        ],
        facts: {
          'old': _f('old'),
          'new': _f('new', exitKey: 'm2'),
        },
        previous: prev('old'),
      );
      expect(out.pick?.proxyName, 'new');
    });
  });

  group('отказ называет класс, а не «попробуйте ещё раз»', () {
    test('все узлы отвергли ключ — виновата подписка, не сеть', () {
      final out = autoPick(
        results: [
          _p('a', latency: -1, verdict: ProbeVerdict.authRejected, tcp: 110),
          _p('b', latency: -1, verdict: ProbeVerdict.authRejected, tcp: 120),
        ],
        facts: {'a': _f('a'), 'b': _f('b')},
      );
      expect(out.failure?.kind, AutoFailureKind.keyRejected);
      // Повтор здесь вернёт ровно то же самое, и предлагать его нечестно.
      expect(out.failure?.retryable, isFalse);
    });

    test('таймауты при живом TCP — похоже на фильтрацию в этой сети', () {
      final out = autoPick(
        results: [
          _p('a', latency: -1, verdict: ProbeVerdict.timeout, tcp: 40),
          _p('b', latency: -1, verdict: ProbeVerdict.timeout, tcp: 45),
        ],
        facts: {'a': _f('a'), 'b': _f('b')},
      );
      expect(out.failure?.kind, AutoFailureKind.protocolsBlocked);
      expect(out.failure?.retryable, isTrue);
    });

    test('сертификаты и закрытые порты — сторона оператора', () {
      final out = autoPick(
        results: [
          _p('a', latency: -1, verdict: ProbeVerdict.tlsUntrusted, tcp: 30),
          _p('b', latency: -1, verdict: ProbeVerdict.portClosed),
          _p('c', latency: -1, verdict: ProbeVerdict.tlsUntrusted, tcp: 31),
        ],
        facts: {'a': _f('a'), 'b': _f('b'), 'c': _f('c')},
      );
      expect(out.failure?.kind, AutoFailureKind.operatorSide);
    });

    test('пустой ответ ядра — это отказ с именем, а не «ноль узлов»', () {
      final out = autoPick(results: const [], facts: const {});
      expect(out.failure?.kind, AutoFailureKind.noNodes);
    });

    test('прошлый выбор при отказе не теряется', () {
      final previous = AutoPickRecord(
        proxyName: 'old',
        latencyMs: 90,
        updatedMs: DateTime.now().millisecondsSinceEpoch,
      );
      final out = autoPick(
        results: [_p('a', latency: -1, verdict: ProbeVerdict.timeout, tcp: 5)],
        facts: {'a': _f('a')},
        previous: previous,
      );
      expect(out.pick, isNull);
      expect(out.previous?.proxyName, 'old');
    });
  });

  group('устаревание выбора — по событиям, без таймеров', () {
    test('свежий выбор при согласном туннеле не устарел', () {
      expect(
        autoStaleReasonOf(
          pick: _rec(),
          serversUpdatedMs: 7,
          holder: _tunnelOn('DE Stealth'),
        ),
        AutoStaleReason.none,
      );
    });

    test('ядро стоит на другом узле — выбор устарел немедленно', () {
      expect(
        autoStaleReasonOf(
          pick: _rec(),
          serversUpdatedMs: 7,
          holder: _tunnelOn('CA Speed'),
        ),
        AutoStaleReason.tunnelDisagrees,
      );
    });

    // Второй дефект того же класса: туннеля нет, а «Сервер» закреплён мимо
    // выбора. Строка «Автоподбор: Канада · 160 мс» при этом обещала узел, к
    // которому connect не пойдёт.
    test('туннеля нет, а закреплён другой узел — выбор не в силе', () {
      expect(
        autoStaleReasonOf(
          pick: _rec(),
          serversUpdatedMs: 7,
          holder: _pinnedTo('🇩🇪 Stream'),
        ),
        AutoStaleReason.pinDisagrees,
      );
    });

    test('закреплён ровно выбранный узел — расхождения нет', () {
      expect(
        autoStaleReasonOf(
          pick: _rec(),
          serversUpdatedMs: 7,
          holder: _pinnedTo('DE Stealth'),
        ),
        AutoStaleReason.none,
      );
    });

    // Панельный пин закрепляет МАШИНУ, а инбаунд внутри неё выбирает ядро.
    // Сравнивать такой пин с именем прокси нечем — общий ключ только машина.
    test('панельный пин сравнивается по машине, а не по имени прокси', () {
      expect(
        autoStaleReasonOf(
          pick: _rec(exitKey: '1'),
          serversUpdatedMs: 7,
          holder: const AutoHolder(source: AutoHolderSource.pin, exitKey: '1'),
        ),
        AutoStaleReason.none,
      );
      expect(
        autoStaleReasonOf(
          pick: _rec(exitKey: '1'),
          serversUpdatedMs: 7,
          holder: const AutoHolder(source: AutoHolderSource.pin, exitKey: '2'),
        ),
        AutoStaleReason.pinDisagrees,
      );
    });

    test('общего ключа нет — расхождение не утверждается', () {
      // Запись выбора без ключа машины и пин без имени прокси: сравнить нечем,
      // и молчание источника это не «другой узел».
      expect(
        autoStaleReasonOf(
          pick: _rec(),
          serversUpdatedMs: 7,
          holder: const AutoHolder(source: AutoHolderSource.pin, exitKey: '2'),
        ),
        AutoStaleReason.none,
      );
    });

    test('состав узлов обновился — выбор устарел', () {
      expect(
        autoStaleReasonOf(pick: _rec(), serversUpdatedMs: 9),
        AutoStaleReason.fleetChanged,
      );
    });

    test('замеру больше шести часов — устарел', () {
      expect(
        autoStaleReasonOf(pick: _rec(ageMinutes: 7 * 60), serversUpdatedMs: 7),
        AutoStaleReason.age,
      );
    });
  });

  group('баннер автоподбора не приписывает выбору чужую подпись', () {
    // Снято на устройстве: автоподбор выбрал Канаду, туннель поднят на
    // немецком узле, а баннер заканчивался словами «Сейчас в туннеле» —
    // подпись контрола «Сервер», описывающая АКТИВНЫЙ узел, стояла под
    // утверждением о ВЫБОРЕ.
    final canada = _rec(
      proxy: '🇨🇦 Secure',
      exitKey: '9',
      country: 'CA',
      title: 'Канада',
    );
    final germany = _tunnelOn(
      '🇩🇪 Stream',
      exitKey: '1',
      country: 'DE',
      title: 'Германия',
    );

    test('активен другой узел — выбор не назван активным', () {
      final text = autopilotBannerText(
        pick: canada,
        stale: AutoStaleReason.tunnelDisagrees,
        holder: germany,
      );
      expect(text, contains('Автоподбор выбрал Канада'));
      expect(text, isNot(contains('сейчас в туннеле.')));
    });

    test('расхождение показано словами, вместе со страной выхода', () {
      final text = autopilotBannerText(
        pick: canada,
        stale: AutoStaleReason.tunnelDisagrees,
        holder: germany,
      );
      expect(text, contains('ядро сейчас стоит на другом узле'));
      expect(text, contains('«🇩🇪 Stream»'));
      expect(text, contains('Страна выхода — Германия, а выбрана Канада.'));
      expect(text, contains('Пересчитается при переподключении.'));
    });

    // На импортированном пути заголовком машины становится НАЗВАНИЕ СТРАНЫ, и
    // без проверки фраза читалась «на другом узле — Германия («🇩🇪 Stream»).
    // Страна выхода — Германия»: одно и то же слово дважды подряд.
    test('имя узла не повторяет страну, названную следом', () {
      final text = autopilotBannerText(
        pick: canada,
        stale: AutoStaleReason.tunnelDisagrees,
        holder: germany,
      );
      expect('Германия'.allMatches(text).length, 1);
    });

    test('закреплён другой узел вне туннеля — сказано и это', () {
      final text = autopilotBannerText(
        pick: canada,
        stale: AutoStaleReason.pinDisagrees,
        holder: _pinnedTo(
          '🇩🇪 Stream',
          exitKey: '1',
          country: 'DE',
          title: 'Германия',
        ),
      );
      expect(text, contains('«Сервер» закреплён на другом узле'));
      expect(text, contains('Германия'));
    });

    test('совпадение не помечается как расхождение', () {
      final text = autopilotBannerText(
        pick: canada,
        stale: AutoStaleReason.none,
        holder: _tunnelOn(
          '🇨🇦 Secure',
          exitKey: '9',
          country: 'CA',
          title: 'Канада',
        ),
      );
      expect(text, contains('Автоподбор выбрал Канада'));
      expect(text, contains('сейчас в туннеле'));
      expect(text, isNot(contains('на другом')));
      expect(text, isNot(contains('Страна выхода')));
    });

    // Панельный пин ставит МАШИНУ, ядро внутри неё выбирает инбаунд: страна
    // выхода та же, и звать это «другим узлом Германия» при выборе «Германия»
    // значило бы предложить искать разницу там, где её не видно.
    test('другой вход той же машины назван входом, а не другим узлом', () {
      final text = autopilotBannerText(
        pick: _rec(
          proxy: '🇩🇪 Secure',
          exitKey: '1',
          country: 'DE',
          title: 'Германия',
        ),
        stale: AutoStaleReason.tunnelDisagrees,
        holder: _tunnelOn(
          '🇩🇪 Stream',
          exitKey: '1',
          country: 'DE',
          title: 'Германия',
        ),
      );
      expect(text, contains('на другом входе той же машины'));
      expect(text, isNot(contains('Страна выхода')));
    });

    test('устаревший замер объясняется возрастом, а не чужим узлом', () {
      final text = autopilotBannerText(
        pick: canada,
        stale: AutoStaleReason.age,
        holder: AutoHolder.none,
      );
      expect(text, contains('Замеру больше 6 часов'));
      expect(text, isNot(contains('на другом')));
    });
  });

  group('кто в силе: ядро, пин или никто', () {
    final facts = <String, FleetFact>{
      '🇨🇦 Secure': _f(
        '🇨🇦 Secure',
        exitKey: '9',
        country: 'CA',
        title: 'Канада',
      ),
      '🇩🇪 Stream': _f(
        '🇩🇪 Stream',
        exitKey: '1',
        country: 'DE',
        title: 'Германия',
      ),
    };

    ProviderContainer container({
      String? activeProxy,
      ConnectionProfile? profile,
    }) {
      final c = ProviderContainer(
        overrides: [
          fleetFactsProvider.overrideWithValue(facts),
          activeProxyProvider.overrideWithValue(activeProxy),
          activeConnectionProfileProvider.overrideWithValue(profile),
        ],
      );
      addTearDown(c.dispose);
      return c;
    }

    ConnectionProfile raw({String? server}) => ConnectionProfile(
      id: 'p1',
      type: ProfileType.rawSub,
      displayName: 'Подписка',
      source: 'https://example',
      selectedServerId: server,
    );

    test('живой туннель важнее пина: в силе то, что держит ядро', () {
      final holder = container(
        activeProxy: '🇩🇪 Stream',
        profile: raw(server: '🇨🇦 Secure'),
      ).read(autoHolderProvider);
      expect(holder.source, AutoHolderSource.tunnel);
      expect(holder.proxyName, '🇩🇪 Stream');
      expect(holder.countryCode, 'DE');
    });

    // Второй дефект: туннеля нет, и до этой правки не в силе было НИЧЕГО —
    // пин никто не читал, а значит «Автоподбор: Канада» стояло над немецким
    // назначением.
    test('туннеля нет — в силе пин импортированной подписки', () {
      final holder = container(
        profile: raw(server: '🇩🇪 Stream'),
      ).read(autoHolderProvider);
      expect(holder.source, AutoHolderSource.pin);
      expect(holder.proxyName, '🇩🇪 Stream');
      expect(holder.title, 'Германия');
    });

    test('пин снят — в силе никто, и расхождения не выдумывается', () {
      final holder = container(profile: raw()).read(autoHolderProvider);
      expect(holder.source, AutoHolderSource.none);
      expect(holder.disagreesWith(_rec(proxy: '🇨🇦 Secure')), isFalse);
    });

    // Панель пинит МАШИНУ; имя прокси внутри неё выбирает ядро, и выдумывать
    // его по первому подходящему факту нельзя.
    test('панельный пин называет машину, но не имя прокси', () {
      final holder = container(
        profile: const ConnectionProfile(
          id: 'p2',
          type: ProfileType.panelAccount,
          displayName: 'Панель',
          source: 'https://panel',
          selectedExitNodeId: 1,
        ),
      ).read(autoHolderProvider);
      expect(holder.source, AutoHolderSource.pin);
      expect(holder.exitKey, '1');
      expect(holder.proxyName, isEmpty);
      expect(holder.title, 'Германия');
    });
  });

  group('подпись «Авто» сообщает выбор', () {
    test('без выбора обещает, а не называет', () {
      const label = AutoLabel.unknown;
      expect(label.value, 'Авто');
      expect(label.subtitle, 'Выберется при подключении');
    });

    test('с выбором называет его и источник', () {
      const label = AutoLabel(
        choice: 'CA · Canada',
        source: 'Сейчас в туннеле',
      );
      expect(label.value, 'Авто · CA · Canada');
      expect(label.subtitle, 'Сейчас в туннеле');
    });

    test('устаревший выбор называет причину, а не источник', () {
      const label = AutoLabel(
        choice: 'CA · Canada',
        source: 'По замеру 2 ч назад',
        stale: AutoStaleReason.tunnelDisagrees,
      );
      expect(label.value, 'Авто · CA · Canada');
      expect(label.subtitle, contains('на другом узле'));
    });

    // Слово выбирается ПРИЧИНОЙ, а не фактом «что-то не так»: перезамер лечит
    // только две причины из четырёх, и отправлять человека замерять там, где
    // надо снять пин, — это и есть неточность, на которую пожаловался владелец.
    test('расхождение с держателем — «не в силе», а не «устарело»', () {
      for (final stale in <AutoStaleReason>[
        AutoStaleReason.tunnelDisagrees,
        AutoStaleReason.pinDisagrees,
      ]) {
        expect(AutoLabel(choice: 'CA', stale: stale).badge, 'не в силе');
      }
    });

    test('состарившийся замер и сменившийся флот — «устарело»', () {
      for (final stale in <AutoStaleReason>[
        AutoStaleReason.age,
        AutoStaleReason.fleetChanged,
      ]) {
        expect(AutoLabel(choice: 'CA', stale: stale).badge, 'устарело');
      }
    });

    test('выбор в силе — бейджа нет вовсе', () {
      expect(const AutoLabel(choice: 'CA').badge, isEmpty);
    });
  });

  // Ровно тот дефект, что сняли с устройства: пин 🇩🇪 Stream, выбор
  // автоподбора — Канада, а строка «Авто» на «Серверах» гласила
  // «Авто · DE · Германия — Сейчас в туннеле», пока Главная в ту же секунду
  // говорила «Автоподбор выбрал Канада… не в силе». Контрол автоподбора
  // подписывался ДЕРЖАТЕЛЕМ — узлом, за который он не отвечает.
  group('строка «Авто» называет выбор, а не держателя', () {
    final facts = <String, FleetFact>{
      '🇨🇦 Secure': _f(
        '🇨🇦 Secure',
        exitKey: '9',
        country: 'CA',
        title: 'Канада',
        protocol: 'hysteria2',
        transport: 'udp',
        security: 'tls',
      ),
      '🇩🇪 Stream': _f(
        '🇩🇪 Stream',
        exitKey: '1',
        country: 'DE',
        title: 'Германия',
      ),
    };

    /// Выбор автоподбора — Канада, ровно как на устройстве.
    AutoPickRecord canada({int ageMinutes = 12, int fleet = 7}) =>
        AutoPickRecord(
          proxyName: '🇨🇦 Secure',
          exitKey: '9',
          countryCode: 'CA',
          machineTitle: 'Канада',
          protocolLabel: 'hysteria2 · udp · tls',
          latencyMs: 118,
          confirmed: true,
          serversUpdatedMs: fleet,
          updatedMs: DateTime.now()
              .subtract(Duration(minutes: ageMinutes))
              .millisecondsSinceEpoch,
        );

    ProviderContainer container({
      String? activeProxy,
      String? pinnedProxy,
      AutoPickRecord? pick,
      int fleet = 7,
    }) {
      final c = ProviderContainer(
        overrides: [
          fleetFactsProvider.overrideWithValue(facts),
          activeProxyProvider.overrideWithValue(activeProxy),
          activeConnectionProfileProvider.overrideWithValue(
            ConnectionProfile(
              id: 'p1',
              type: ProfileType.rawSub,
              displayName: 'Подписка',
              source: 'https://example',
              selectedServerId: pinnedProxy,
              autoPick: pick,
              serversUpdatedMs: fleet,
            ),
          ),
        ],
      );
      addTearDown(c.dispose);
      return c;
    }

    test('живой туннель на чужом узле не переименовывает выбор', () {
      final label = container(
        activeProxy: '🇩🇪 Stream',
        pinnedProxy: '🇩🇪 Stream',
        pick: canada(),
      ).read(autoServerLabelProvider);
      expect(label.value, 'Авто · CA · Канада');
      expect(label.value, isNot(contains('Германия')));
      expect(label.badge, 'не в силе');
      expect(label.subtitle, isNot(contains('Сейчас в туннеле')));
      expect(label.subtitle, contains('на другом узле'));
    });

    // Та же половина фразы, что на Главной: два экрана обязаны описывать одно
    // состояние одинаково, иначе для смотрящего это два разных состояния.
    test('«Серверы» и Главная говорят про один и тот же выбор', () {
      final c = container(
        activeProxy: '🇩🇪 Stream',
        pinnedProxy: '🇩🇪 Stream',
        pick: canada(),
      );
      final label = c.read(autoServerLabelProvider);
      final banner = autopilotBannerText(
        pick: canada(),
        stale: c.read(autoStaleProvider),
        holder: c.read(autoHolderProvider),
      );
      expect(banner, contains('Автоподбор выбрал Канада'));
      expect(label.value, contains('Канада'));
      expect(banner, contains('Германия'));
      expect(label.subtitle, isNot(contains('Германия')));
    });

    test('туннеля нет, пин на чужом узле — тоже «не в силе»', () {
      final label = container(
        pinnedProxy: '🇩🇪 Stream',
        pick: canada(),
      ).read(autoServerLabelProvider);
      expect(label.value, 'Авто · CA · Канада');
      expect(label.badge, 'не в силе');
      expect(label.subtitle, contains('закреплён на другом узле'));
    });

    test('туннель стоит на выбранном узле — источник это туннель', () {
      final label = container(
        activeProxy: '🇨🇦 Secure',
        pinnedProxy: '🇨🇦 Secure',
        pick: canada(),
      ).read(autoServerLabelProvider);
      expect(label.value, 'Авто · CA · Канада');
      expect(label.source, 'Сейчас в туннеле');
      expect(label.badge, isEmpty);
    });

    // Без туннеля свидетельство одно — сам замер, и подпись обязана назвать
    // его возрастом, а не обещанием «сейчас в туннеле».
    test('туннеля нет и пин совпал — источником остаётся замер', () {
      final label = container(
        pinnedProxy: '🇨🇦 Secure',
        pick: canada(),
      ).read(autoServerLabelProvider);
      expect(label.badge, isEmpty);
      expect(label.source, contains('По замеру'));
    });

    test('состарившийся замер называется «устарело», а не «не в силе»', () {
      final label = container(
        pinnedProxy: '🇨🇦 Secure',
        pick: canada(ageMinutes: 7 * 60),
      ).read(autoServerLabelProvider);
      expect(label.badge, 'устарело');
    });

    test('сменившийся флот называется «устарело»', () {
      final label = container(
        pinnedProxy: '🇨🇦 Secure',
        pick: canada(fleet: 7),
        fleet: 9,
      ).read(autoServerLabelProvider);
      expect(label.badge, 'устарело');
    });

    // Регрессия: раньше живой туннель сам по себе давал строке заголовок, и
    // «Авто» называло узел, которого автоподбор не выбирал — потому что не
    // запускался вовсе.
    test('подбора не было — туннель не выдаётся за его выбор', () {
      final label = container(
        activeProxy: '🇩🇪 Stream',
        pinnedProxy: '🇩🇪 Stream',
      ).read(autoServerLabelProvider);
      expect(label.value, 'Авто');
      expect(label.subtitle, 'Выберется при подключении');
      expect(label.badge, isEmpty);
    });

    // Тот же дефект жил и в строке «Тип подключения»: она называла форму
    // инбаунда, поднятого ядром, выдавая её за выбор автоподбора.
    test('«Тип подключения» тоже называет выбор, а не провод', () {
      final label = container(
        activeProxy: '🇩🇪 Stream',
        pinnedProxy: '🇩🇪 Stream',
        pick: canada(),
      ).read(autoProtocolLabelProvider);
      expect(label.value, 'Авто · hysteria2 · udp · tls');
      expect(label.badge, 'не в силе');
    });

    test('«Тип подключения» без подбора молчит, а не берёт форму туннеля', () {
      final label = container(
        activeProxy: '🇩🇪 Stream',
      ).read(autoProtocolLabelProvider);
      expect(label.value, 'Авто');
      expect(label.hasChoice, isFalse);
    });
  });

  group('склейка замера с предложением', () {
    test('имена прокси импортированной подписки находят свои машины', () {
      // Ключ между замером и предложением ровно один — имя прокси в теле
      // конфига. Разойдись они, и автоподбор потерял бы страну и форму.
      final offering = buildImportedOffering(
        servers: const <ImportedServer>[
          ImportedServer(
            id: 'DE Stealth',
            name: 'DE Stealth',
            type: 'vless',
            server: 'de.example',
            port: 443,
            country: 'DE',
          ),
          ImportedServer(
            id: 'DE Speed',
            name: 'DE Speed',
            type: 'vless',
            server: 'de.example',
            port: 8443,
            country: 'DE',
          ),
        ],
        latencyByProxy: const <String, int>{},
      );
      final facts = fleetFactsOf(offering);
      expect(facts.keys, containsAll(<String>['DE Stealth', 'DE Speed']));
      // Две строки — ОДНА машина: у них общий адрес, и склеивать их в два
      // сервера экран серверов уже перестал.
      expect(facts['DE Stealth']!.exitKey, facts['DE Speed']!.exitKey);
      expect(facts['DE Stealth']!.countryCode, 'DE');
    });
  });

  // Имена в этих тестах взяты С ЖИВОЙ ПОДПИСКИ (панель v0.9.81, `sub 34`), а
  // не выдуманы: clash-тело называет relay-узлы `🇩🇪 Secure ↪` и не несёт ни
  // `dialer-proxy`, ни группы `type: relay`; sing-box-тело той же подписки
  // называет их `🇩🇪 Secure via 🇷🇺` и цепочку строит (`detour`), но переводчик
  // sing-box → clash в ядре `detour` не переносит. До приложения в обоих
  // случаях доезжает ИМЯ без цепочки.
  group('цепочка на проводе против цепочки в имени', () {
    // Таблица снята с ЖИВОЙ подписки целиком, а не сочинена: слева —
    // clash-тело (`?client=clash`), справа — sing-box-тело того же `/sub/`.
    // Оба генератора помечают вход по-своему, и признак обязан работать на
    // обоих, иначе он защищает фикстуру, а не человека.
    const liveClashNames = <String>[
      '🇩🇪 Stealth ↪',
      '🇩🇪 Secure ↪',
      '🇩🇪 Stream ↪',
      '🇩🇪 Speed ↪',
      '🇩🇪 WebSocket ↪',
      '🇩🇪 HTTP ↪',
      '🇩🇪 TUIC ↪',
    ];
    const livePlainNames = <String>[
      '🇨🇦 Stealth',
      '🇨🇦 Secure',
      '🇨🇦 Stream',
      '🇨🇦 Speed',
      '🇨🇦 WebSocket',
      '🇨🇦 HTTP',
      // Сам релэй-узел в теле тоже есть — как обычный выход. Он ничего не
      // обещает и переименовывать его нельзя.
      'relay 🇷🇺',
    ];
    const liveSingboxNames = <String>[
      '🇩🇪 Secure via 🇷🇺',
      '🇨🇦 Secure via 🇷🇺',
      '🇨🇦 WebSocket via 🇷🇺',
      '🇨🇦 TUIC via 🇷🇺',
    ];

    test('живые имена: суффикс узнаётся в обеих формах генератора', () {
      for (final n in <String>[...liveClashNames, ...liveSingboxNames]) {
        expect(nameClaimsChain(n), isTrue, reason: n);
      }
      for (final n in livePlainNames) {
        expect(nameClaimsChain(n), isFalse, reason: n);
      }
    });

    test('после снятия обещания живое имя остаётся именем', () {
      // Плоский близнец у каждого узла в теле есть, и снятое имя обязано
      // совпасть с ним: иначе на экране появилось бы третье имя того же узла.
      expect(nameWithoutChainClaim('🇨🇦 Secure via 🇷🇺'), '🇨🇦 Secure');
      expect(nameWithoutChainClaim('🇩🇪 WebSocket ↪'), '🇩🇪 WebSocket');
      for (final n in livePlainNames) {
        expect(nameWithoutChainClaim(n), n, reason: n);
      }
    });

    test('«via» внутри слова обещанием не считается', () {
      // Без границы по букве «Bolivia» объявили бы врущим узлом, и честное имя
      // потеряло бы половину себя.
      expect(nameClaimsChain('🇧🇴 Bolivia'), isFalse);
      expect(nameClaimsChain('Latvia Speed'), isFalse);
      expect(nameWithoutChainClaim('🇧🇴 Bolivia'), '🇧🇴 Bolivia');
    });

    test('обещание снимается с имени, а имя не выбрасывается', () {
      expect(nameWithoutChainClaim('🇨🇦 Secure via 🇷🇺'), '🇨🇦 Secure');
      expect(nameWithoutChainClaim('🇩🇪 Stream ↪'), '🇩🇪 Stream');
      // Резать нечего — возвращаем как есть, пустая строка хуже неточной.
      expect(nameWithoutChainClaim('↪'), '↪');
    });

    test('подтвердить цепочку может только источник, а не имя', () {
      expect(_f('🇨🇦 Secure via 🇷🇺').chainClaim, ChainClaim.labelOnly);
      expect(
        _f('🇨🇦 Secure via 🇷🇺', wireChained: true).chainClaim,
        ChainClaim.wired,
      );
      expect(_f('🇨🇦 Secure').chainClaim, ChainClaim.none);
    });

    test('честный вид: провод в заголовке, имя оператора рядом', () {
      final naming = _f('🇨🇦 Secure via 🇷🇺').naming;
      expect(naming.overPromises, isTrue);
      // Показываем то, что на проводе...
      expect(naming.title, '🇨🇦 Secure');
      // ...но имя оператора не теряем: под ним узел лежит в конфиге.
      expect(naming.operatorName, '🇨🇦 Secure via 🇷🇺');
      expect(naming.note, contains('🇨🇦 Secure via 🇷🇺'));
      expect(naming.note, contains('цепочки нет'));
    });

    test('подтверждённая цепочка имя не трогает', () {
      final naming = _f('🇨🇦 Secure via 🇷🇺', wireChained: true).naming;
      expect(naming.overPromises, isFalse);
      expect(naming.title, '🇨🇦 Secure via 🇷🇺');
      expect(naming.note, isEmpty);
      expect(naming.listNote, isEmpty);
    });

    test('при равенстве в пределах шума выигрывает узел без ярлыка', () {
      final out = autoPick(
        results: [
          _p('🇨🇦 Secure via 🇷🇺', latency: 100),
          _p('🇨🇦 Stealth', latency: 110),
        ],
        facts: {
          '🇨🇦 Secure via 🇷🇺': _f('🇨🇦 Secure via 🇷🇺', country: 'CA'),
          '🇨🇦 Stealth': _f('🇨🇦 Stealth', country: 'CA'),
        },
      );
      expect(out.pick?.proxyName, '🇨🇦 Stealth');
      expect(out.pick?.reasonCode, 'plain_over_labelled');
      // Ранжирование при этом не переписано: лидер по счёту остался первым, и
      // человек видит, кого именно обошли.
      expect(out.ranked.first.name, '🇨🇦 Secure via 🇷🇺');
    });

    test('за подпись качеством не платим: ощутимо быстрый ярлык побеждает', () {
      final out = autoPick(
        results: [
          _p('🇨🇦 Secure via 🇷🇺', latency: 100),
          _p('🇨🇦 Stealth', latency: 400),
        ],
        facts: {
          '🇨🇦 Secure via 🇷🇺': _f('🇨🇦 Secure via 🇷🇺', country: 'CA'),
          '🇨🇦 Stealth': _f('🇨🇦 Stealth', country: 'CA'),
        },
      );
      expect(out.pick?.proxyName, '🇨🇦 Secure via 🇷🇺');
      expect(out.pick?.reasonCode, 'best_score');
    });

    test('цепочка, подтверждённая источником, уступать не обязана', () {
      final out = autoPick(
        results: [
          _p('🇨🇦 Secure via 🇷🇺', latency: 100),
          _p('🇨🇦 Stealth', latency: 110),
        ],
        facts: {
          '🇨🇦 Secure via 🇷🇺': _f(
            '🇨🇦 Secure via 🇷🇺',
            country: 'CA',
            wireChained: true,
          ),
          '🇨🇦 Stealth': _f('🇨🇦 Stealth', country: 'CA'),
        },
      );
      expect(out.pick?.proxyName, '🇨🇦 Secure via 🇷🇺');
    });

    test('ради подписи не берём непроверенный узел', () {
      // Подтверждённость важнее имени: иначе автоподбор вернул бы тот самый
      // фолбэк «адрес жив — значит годится», который ядро перестало делать.
      final out = autoPick(
        results: [
          _p('🇨🇦 Secure via 🇷🇺', latency: 100),
          _p('🇨🇦 Stealth', latency: 105, verdict: ProbeVerdict.tcpOnly),
        ],
        facts: {
          '🇨🇦 Secure via 🇷🇺': _f('🇨🇦 Secure via 🇷🇺', country: 'CA'),
          '🇨🇦 Stealth': _f('🇨🇦 Stealth', country: 'CA'),
        },
      );
      expect(out.pick?.proxyName, '🇨🇦 Secure via 🇷🇺');
      expect(out.pick?.confirmed, isTrue);
    });

    // ЖИВОЙ ФЛОТ, а не фикстура. Таблица снята так: sing-box-тело `sub 34`
    // (`curl -H 'User-Agent: sing-box/1.9' /sub/3d9847b3-…`) пропущено через
    // САМ импортёр ядра (`subimport.Import`), и разобраны его прокси. Импортёр
    // теряет `detour`, поэтому каждый инбаунд приезжает ДВАЖДЫ одним и тем же
    // подключением: 14 пар, в каждой совпадают адрес, порт и тип, а расходятся
    // только имена. Ровно эту пару автоподбор и обязан различать.
    const liveWire = <({String name, String host, int port, String type})>[
      (name: '🇨🇦 Stealth', host: '158.69.213.88', port: 8443, type: 'vless'),
      (
        name: '🇨🇦 Speed',
        host: '158.69.213.88',
        port: 11474,
        type: 'hysteria2',
      ),
      (name: '🇨🇦 Stream', host: '158.69.213.88', port: 10400, type: 'vless'),
      (name: '🇨🇦 HTTP', host: '158.69.213.88', port: 13400, type: 'vless'),
      (name: '🇨🇦 Secure', host: '158.69.213.88', port: 14400, type: 'vless'),
      (
        name: '🇨🇦 WebSocket',
        host: '158.69.213.88',
        port: 12400,
        type: 'vless',
      ),
      (name: '🇩🇪 Stream', host: '85.215.196.151', port: 10400, type: 'vless'),
      (
        name: '🇩🇪 WebSocket',
        host: '85.215.196.151',
        port: 12400,
        type: 'vless',
      ),
      (name: '🇩🇪 Stealth', host: '85.215.196.151', port: 443, type: 'vless'),
      (name: '🇩🇪 TUIC', host: '85.215.196.151', port: 16400, type: 'tuic'),
      (name: '🇩🇪 Secure', host: '85.215.196.151', port: 14400, type: 'vless'),
      (
        name: '🇩🇪 Speed',
        host: '85.215.196.151',
        port: 11466,
        type: 'hysteria2',
      ),
      (name: '🇩🇪 HTTP', host: '85.215.196.151', port: 13400, type: 'vless'),
      (name: '🇩🇪 Naive', host: '85.215.196.151', port: 15400, type: 'http'),
    ];

    // Факт в форме ИМПОРТИРОВАННОГО пути: машина — это адрес, форму источник
    // не называет целиком (транспорт и TLS в теле остаются пустыми), порт есть.
    FleetFact live(
      String proxy,
      ({String name, String host, int port, String type}) w,
    ) => FleetFact(
      proxyName: proxy,
      exitKey: w.host,
      countryCode: proxy.startsWith('🇨🇦') ? 'CA' : 'DE',
      protocol: w.type,
      port: w.port,
    );

    test('живые близнецы: один провод под двумя именами узнаётся весь', () {
      for (final w in liveWire) {
        final labelled = live('${w.name} via 🇷🇺', w);
        final plain = live(w.name, w);
        expect(
          isPlainTwin(labelled: labelled, plain: plain),
          isTrue,
          reason: w.name,
        );
        // Обратной стороны у отношения нет: честное имя ничего не обещает, и
        // «опровергать» его нечем.
        expect(
          isPlainTwin(labelled: plain, plain: labelled),
          isFalse,
          reason: w.name,
        );
      }
    });

    test('живой релэй-узел близнеца ни у кого не отнимает', () {
      // `relay 🇷🇺` лежит в теле как обычный выход на своей машине и своём
      // порту. Он не обещает входа и близнецом ничьим не является — иначе
      // подбор начал бы подменять им чужие узлы.
      const relay = FleetFact(
        proxyName: 'relay 🇷🇺',
        exitKey: '141.98.191.214',
        countryCode: 'RU',
        protocol: 'hysteria2',
        port: 11464,
      );
      for (final w in liveWire) {
        expect(
          isPlainTwin(labelled: live('${w.name} via 🇷🇺', w), plain: relay),
          isFalse,
          reason: w.name,
        );
      }
    });

    test('разные инбаунды одной машины близнецами не считаются', () {
      // Живая пара с ОДНОЙ машины, но с разных портов: подменять один инбаунд
      // другим нельзя — это разные подключения, а не два имени одного.
      final stream = liveWire.firstWhere((w) => w.name == '🇨🇦 Stream');
      final secure = liveWire.firstWhere((w) => w.name == '🇨🇦 Secure');
      expect(
        isPlainTwin(
          labelled: live('🇨🇦 Stream via 🇷🇺', stream),
          plain: live('🇨🇦 Secure', secure),
        ),
        isFalse,
      );
    });

    test('близнец узнаётся и без порта — по имени без обещания', () {
      // Источник порта не называет (панель до v0.9.81 его в инбаунде не
      // отдавала). Тогда близнеца называет само имя, из которого убрано
      // обещание, — при той же машине и том же протоколе.
      final labelled = _f('🇨🇦 Stream via 🇷🇺', exitKey: 'ca', country: 'CA');
      final plain = _f('🇨🇦 Stream', exitKey: 'ca', country: 'CA');
      expect(isPlainTwin(labelled: labelled, plain: plain), isTrue);
      // Другая машина — не близнец, как бы ни совпало имя.
      expect(
        isPlainTwin(
          labelled: labelled,
          plain: _f('🇨🇦 Stream', exitKey: 'de', country: 'CA'),
        ),
        isFalse,
      );
    });

    // ЗАМЕР С УСТРОЙСТВА, число в число: 138 мс и 154 мс на одной паре имён.
    // Полоса ничьей сверяла их ОТНОШЕНИЕ — 154/138 = 1.116 при пороге ×1.1 — и
    // на шестнадцати миллисекундах выпускала вперёд имя с пустым обещанием.
    //
    // Порядок замеров проверяется В ОБЕ СТОРОНЫ намеренно: у одного провода
    // «кто быстрее» решает шум, и завтра он ляжет наоборот. Правило обязано
    // отвечать одинаково при любом раскладе — иначе оно снова окажется
    // правилом про миллисекунды.
    test('между двумя именами одного провода миллисекунды не решают', () {
      final stream = liveWire.firstWhere((w) => w.name == '🇨🇦 Stream');
      final facts = {
        '🇨🇦 Stream via 🇷🇺': live('🇨🇦 Stream via 🇷🇺', stream),
        '🇨🇦 Stream': live('🇨🇦 Stream', stream),
      };

      // Ярлык быстрее — ровно тот расклад, что дошёл до устройства.
      final out = autoPick(
        results: [
          _p('🇨🇦 Stream via 🇷🇺', latency: 138, country: 'CA'),
          _p('🇨🇦 Stream', latency: 154, country: 'CA'),
        ],
        facts: facts,
      );
      expect(out.pick?.proxyName, '🇨🇦 Stream');
      expect(out.pick?.reasonCode, 'plain_twin');

      // Ярлык медленнее — ответ обязан быть тем же.
      final flipped = autoPick(
        results: [
          _p('🇨🇦 Stream via 🇷🇺', latency: 154, country: 'CA'),
          _p('🇨🇦 Stream', latency: 138, country: 'CA'),
        ],
        facts: facts,
      );
      expect(flipped.pick?.proxyName, '🇨🇦 Stream');

      // Проигравший из списка не исчез: человек мерил его своими секундами.
      expect(out.ranked.map((c) => c.name), contains('🇨🇦 Stream via 🇷🇺'));
      // ...и строка списка называет ПРИЧИНУ фактом, а не порогом.
      final loser = out.ranked.firstWhere(
        (c) => c.name == '🇨🇦 Stream via 🇷🇺',
      );
      expect(loser.chainDisprovedByTwin, isTrue);
      expect(loser.listNote, contains('тот же узел и порт'));
      expect(loser.listNote, contains('🇨🇦 Stream'));
    });

    // ГЛАВНОЕ отличие нового правила от полосы ничьей: оно не зависит от того,
    // на сколько разошлись замеры. Тот же провод — значит выигрыш ярлыка это
    // шум канала, каким бы крупным он ни выглядел.
    test('ярлык не побеждает и тогда, когда его замер заметно лучше', () {
      final stream = liveWire.firstWhere((w) => w.name == '🇨🇦 Stream');
      final out = autoPick(
        results: [
          _p('🇨🇦 Stream via 🇷🇺', latency: 60, country: 'CA'),
          _p('🇨🇦 Stream', latency: 400, country: 'CA'),
        ],
        facts: {
          '🇨🇦 Stream via 🇷🇺': live('🇨🇦 Stream via 🇷🇺', stream),
          '🇨🇦 Stream': live('🇨🇦 Stream', stream),
        },
      );
      expect(out.pick?.proxyName, '🇨🇦 Stream');
      expect(out.pick?.reasonCode, 'plain_twin');
    });

    test('честного близнеца нет — ярлык остаётся законным выбором', () {
      // Так выглядит CLASH-тело той же живой подписки: у немецкой машины ВСЕ
      // инбаунды помечены `↪`, плоского близнеца нет ни у одного. Отнимать у
      // человека единственные рабочие узлы ради подписи нельзя.
      final de = liveWire.firstWhere((w) => w.name == '🇩🇪 Secure');
      final out = autoPick(
        results: [
          _p('🇩🇪 Secure ↪', latency: 90, country: 'DE'),
          _p('🇩🇪 Stealth ↪', latency: 300, country: 'DE'),
        ],
        facts: {
          '🇩🇪 Secure ↪': live('🇩🇪 Secure ↪', de),
          '🇩🇪 Stealth ↪': live(
            '🇩🇪 Stealth ↪',
            liveWire.firstWhere((w) => w.name == '🇩🇪 Stealth'),
          ),
        },
      );
      expect(out.pick?.proxyName, '🇩🇪 Secure ↪');
      expect(out.pick?.reasonCode, 'best_score');
    });

    test('близнец, не прошедший проверку, подмены не делает', () {
      // Провод общий, а ответил он по-разному. Чинить подпись ценой
      // подключения нельзя: человеку нужен работающий узел.
      final stream = liveWire.firstWhere((w) => w.name == '🇨🇦 Stream');
      final out = autoPick(
        results: [
          _p('🇨🇦 Stream via 🇷🇺', latency: 154, country: 'CA'),
          _p(
            '🇨🇦 Stream',
            latency: -1,
            verdict: ProbeVerdict.timeout,
            country: 'CA',
          ),
        ],
        facts: {
          '🇨🇦 Stream via 🇷🇺': live('🇨🇦 Stream via 🇷🇺', stream),
          '🇨🇦 Stream': live('🇨🇦 Stream', stream),
        },
      );
      expect(out.pick?.proxyName, '🇨🇦 Stream via 🇷🇺');
    });

    test('гистерезис не удерживает опровергнутое имя', () {
      // Без этого закрепившийся однажды ярлык не проиграл бы уже никогда:
      // удержание прошлого выбора сильнее разницы в счёте.
      final stream = liveWire.firstWhere((w) => w.name == '🇨🇦 Stream');
      final out = autoPick(
        results: [
          _p('🇨🇦 Stream via 🇷🇺', latency: 100, country: 'CA'),
          _p('🇨🇦 Stream', latency: 105, country: 'CA'),
        ],
        facts: {
          '🇨🇦 Stream via 🇷🇺': live('🇨🇦 Stream via 🇷🇺', stream),
          '🇨🇦 Stream': live('🇨🇦 Stream', stream),
        },
        previous: AutoPickRecord(
          proxyName: '🇨🇦 Stream via 🇷🇺',
          latencyMs: 100,
          confirmed: true,
          updatedMs: DateTime.now().millisecondsSinceEpoch,
        ),
      );
      expect(out.pick?.proxyName, '🇨🇦 Stream');
    });

    test('подтверждённая источником цепочка близнецом не отменяется', () {
      // Панель сказала `chained_in_config: true` — значит цепочка НА ПРОВОДЕ, и
      // это уже другое подключение, а не второе имя того же.
      final stream = liveWire.firstWhere((w) => w.name == '🇨🇦 Stream');
      final wired = FleetFact(
        proxyName: '🇨🇦 Stream via 🇷🇺',
        exitKey: stream.host,
        countryCode: 'CA',
        protocol: stream.type,
        port: stream.port,
        wireChained: true,
      );
      expect(
        isPlainTwin(labelled: wired, plain: live('🇨🇦 Stream', stream)),
        isFalse,
      );
      final out = autoPick(
        results: [
          _p('🇨🇦 Stream via 🇷🇺', latency: 100, country: 'CA'),
          _p('🇨🇦 Stream', latency: 105, country: 'CA'),
        ],
        facts: {
          '🇨🇦 Stream via 🇷🇺': wired,
          '🇨🇦 Stream': live('🇨🇦 Stream', stream),
        },
      );
      expect(out.pick?.proxyName, '🇨🇦 Stream via 🇷🇺');
    });

    // СКВОЗНАЯ проверка на живом теле: тот же путь, что на устройстве —
    // прокси подписки → предложение → факты флота → подбор. Ни одного
    // выдуманного поля: имена, адреса и порты взяты из вывода импортёра ядра.
    test('на живом флоте обещание не побеждает ни на одном узле', () {
      final servers = <ImportedServer>[
        for (final w in liveWire) ...<ImportedServer>[
          ImportedServer(
            id: w.name,
            name: w.name,
            type: w.type,
            server: w.host,
            port: w.port,
            country: w.name.startsWith('🇨🇦') ? 'CA' : 'DE',
          ),
          ImportedServer(
            id: '${w.name} via 🇷🇺',
            name: '${w.name} via 🇷🇺',
            type: w.type,
            server: w.host,
            port: w.port,
            country: w.name.startsWith('🇨🇦') ? 'CA' : 'DE',
          ),
        ],
        const ImportedServer(
          id: 'relay 🇷🇺',
          name: 'relay 🇷🇺',
          type: 'hysteria2',
          server: '141.98.191.214',
          port: 11464,
          country: 'RU',
        ),
      ];
      final facts = fleetFactsOf(buildImportedOffering(servers: servers));
      // Порт доезжает до фактов: без него близнецов не отличить.
      expect(facts['🇨🇦 Stream']!.port, 10400);
      expect(facts['🇨🇦 Stream via 🇷🇺']!.port, 10400);

      // Обещающим именам даём ЛУЧШИЕ замеры — так, как их выдал бы шум канала
      // в самый неудобный момент. Победить не должно ни одно.
      final results = <ProbeResult>[
        for (var i = 0; i < servers.length; i++)
          _p(
            servers[i].id,
            latency: nameClaimsChain(servers[i].id) ? 50 + i : 300 + i,
            country: servers[i].country,
          ),
      ];
      final out = autoPick(results: results, facts: facts);
      expect(nameClaimsChain(out.pick!.proxyName), isFalse);
      // И это не случайность одного прохода: каждая из 14 живых пар опознана.
      final disproved = out.ranked
          .where((c) => c.chainDisprovedByTwin)
          .map((c) => c.name)
          .toSet();
      expect(disproved.length, liveWire.length);
      for (final w in liveWire) {
        expect(disproved, contains('${w.name} via 🇷🇺'), reason: w.name);
      }
      // Релэй-узел ни у кого не близнец и в опровергнутые не попал.
      expect(disproved, isNot(contains('relay 🇷🇺')));
    });

    test('панель приносит признак цепочки в факты флота', () {
      // Форма строки — как у живого `/api/v2/app/servers`: узел 1 привязан к
      // релэю 2, и панель честно говорит, что генератор цепочку не строит.
      final server = Server.fromJson(<String, dynamic>{
        'id': 1,
        'name': 'Germany',
        'country_code': 'DE',
        'status': 'online',
        'via_relay': <String, dynamic>{
          'node_id': 2,
          'name': 'Russia',
          'country_code': 'RU',
          'chained_in_config': false,
        },
        'inbounds': <Map<String, dynamic>>[
          <String, dynamic>{
            'id': 12,
            'tag': 'tls-in',
            'protocol': 'vless',
            'network': 'tcp',
            'security': 'tls',
            'port': 8443,
            'label': 'Secure',
            'proxy_name': '🇩🇪 Secure ↪',
            'available': true,
          },
        ],
      });
      final facts = fleetFactsOf(buildPanelOffering(servers: <Server>[server]));
      final fact = facts['🇩🇪 Secure ↪']!;
      expect(fact.wireChained, isFalse);
      expect(fact.chainClaim, ChainClaim.labelOnly);
      expect(fact.naming.title, '🇩🇪 Secure');
    });

    test('подпись «Авто» приписывает расхождение, не пряча источник', () {
      const label = AutoLabel(
        choice: 'CA · 🇨🇦 Secure',
        source: 'Сейчас в туннеле',
        claimNote: 'вход обещан именем, но не построен',
      );
      expect(label.subtitle, contains('Сейчас в туннеле'));
      expect(label.subtitle, contains('вход обещан именем'));
    });
  });
}
