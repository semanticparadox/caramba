// История попыток доходит из ядра до экрана, INV-17 и 02-SPEC.md 8.8.
//
// Лестницей ходит ядро, поэтому попытки записывает оно. Пока их никто не
// поднимал в состояние приложения, экран транспортов показывал пустой список,
// то есть утверждал, что попыток не было, тогда как их было сколько угодно.
// Здесь проверяется, что попытки поднимаются, что неудачная видна СО СВОЕЙ
// причиной, и что повторный подъём не удваивает историю.

import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/features/csm/attempt_history.dart';
import 'package:caramba_client/state/csm_ladder_sync.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';

import 'support/fake_core.dart';

Map<String, Object?> _attempt({
  required int rung,
  required String start,
  String host = '',
  String outcome = 'ok',
  String code = '',
  int status = 0,
  int bytes = 0,
  int millis = 0,
}) => <String, Object?>{
  'rung': rung,
  'host': host,
  'start': start,
  'outcome': outcome,
  'code': code,
  'status': status,
  'bytes': bytes,
  'millis': millis,
};

String _ladder({
  required List<Map<String, Object?>> history,
  List<Map<String, Object?>> rungs = const <Map<String, Object?>>[],
}) => jsonEncode(<String, Object?>{'rungs': rungs, 'history': history});

(ProviderContainer, FakeVpnCore) _boot() {
  final core = FakeVpnCore();
  final container = ProviderContainer(
    overrides: <Override>[vpnConnectionProvider.overrideWithValue(core)],
  );
  addTearDown(container.dispose);
  return (container, core);
}

void main() {
  test('попытки поднимаются из ядра, свежая первой', () async {
    final (container, core) = _boot();
    core.csmLadderJson = _ladder(
      history: <Map<String, Object?>>[
        _attempt(
          rung: 1,
          host: 'panel.example',
          start: '2026-09-03T10:00:00Z',
          outcome: 'network',
          millis: 12000,
        ),
        _attempt(
          rung: 2,
          host: 'mirror-a',
          start: '2026-09-03T10:00:12Z',
          status: 200,
          bytes: 4096,
          millis: 340,
        ),
      ],
    );

    final outcome = await container.read(csmLadderSyncProvider).pump();
    expect(outcome, CsmLadderSyncOutcome.ok);

    final history = container.read(csmAttemptHistoryProvider);
    expect(history, hasLength(2));
    // Самая свежая первой: экран читается сверху вниз.
    expect(history.first.rung, CsmRung.mirrors);
    expect(history.first.host, 'mirror-a');
    expect(history.first.outcome, CsmAttemptOutcome.ok);
    expect(history.first.bytes, 4096);
    expect(history.first.status, 200);
    expect(history.first.durationMs, 340);
    expect(history.last.rung, CsmRung.direct);
  });

  test('неудачная попытка видна со своей причиной, а не молча', () async {
    final (container, core) = _boot();
    core.csmLadderJson = _ladder(
      history: <Map<String, Object?>>[
        // Кадр приехал и НЕ проверился. Это событие безопасности, и оно не
        // приравнивается к пустому ответу: размер приехавшего тоже виден.
        _attempt(
          rung: 2,
          host: 'mirror-b',
          start: '2026-09-03T10:01:00Z',
          outcome: 'verify',
          code: 'E_SIG',
          status: 200,
          bytes: 812,
          millis: 210,
        ),
        // Отказ без кода из реестра: причиной становится сам исход, потому что
        // пустая причина это отказ, о котором приложение промолчало.
        _attempt(
          rung: 3,
          host: 'doh-a',
          start: '2026-09-03T10:01:05Z',
          outcome: 'network',
          millis: 12000,
        ),
      ],
    );

    await container.read(csmLadderSyncProvider).pump();

    final history = container.read(csmAttemptHistoryProvider);
    expect(history, hasLength(2));
    expect(history.last.outcome, CsmAttemptOutcome.failed);
    expect(history.last.errorCode, 'E_SIG');
    expect(history.last.bytes, 812);
    expect(history.first.outcome, CsmAttemptOutcome.failed);
    expect(history.first.errorCode, 'network');
  });

  test('повторный подъём не удваивает историю и дописывает хвост', () async {
    final (container, core) = _boot();
    final first = _attempt(
      rung: 1,
      host: 'panel.example',
      start: '2026-09-03T10:00:00Z',
      status: 200,
      bytes: 900,
    );
    core.csmLadderJson = _ladder(history: <Map<String, Object?>>[first]);
    final sync = container.read(csmLadderSyncProvider);

    await sync.pump();
    await sync.pump();
    expect(container.read(csmAttemptHistoryProvider), hasLength(1));

    core.csmLadderJson = _ladder(
      history: <Map<String, Object?>>[
        first,
        _attempt(
          rung: 2,
          host: 'mirror-a',
          start: '2026-09-03T10:00:30Z',
          outcome: 'http',
          status: 403,
          millis: 80,
        ),
      ],
    );
    await sync.pump();

    final history = container.read(csmAttemptHistoryProvider);
    expect(history, hasLength(2));
    expect(history.first.rung, CsmRung.mirrors);
    expect(history.first.errorCode, 'http');
    expect(history.first.status, 403);
  });

  test('ядро без ABI v3 отвечает пусто и ничего не выдумывается', () async {
    final (container, core) = _boot();
    core.csmLadderJson = '';

    expect(
      await container.read(csmLadderSyncProvider).pump(),
      CsmLadderSyncOutcome.unavailable,
    );
    expect(container.read(csmAttemptHistoryProvider), isEmpty);
  });

  test(
    'факты о транспорте берутся из причин ядра, а не из умолчаний',
    () async {
      final (container, core) = _boot();
      // Умолчания провайдера рисуют R4 как platform_unsupported, а R5 как
      // not_configured независимо от того, что настроено. Ядро говорит иначе.
      core.csmLadderJson = _ladder(
        history: const <Map<String, Object?>>[],
        rungs: <Map<String, Object?>>[
          <String, Object?>{'rung': 4, 'enabled': true, 'reason': ''},
          <String, Object?>{'rung': 5, 'enabled': true, 'reason': ''},
        ],
      );

      await container.read(csmLadderSyncProvider).pump();

      final facts = container.read(csmTransportFactsProvider);
      expect(facts.tunnelFetchSupported, isTrue);
      expect(facts.proxyConfigured, isTrue);
    },
  );

  test('ступень без пути НЕ объявляется доступной', () async {
    // Самый обычный случай: туннель ещё не поднят, и ядро ставит R4
    // not_configured. Вывод через отрицание одной причины
    // (reason != 'platform_unsupported') объявлял ступень доступной ровно
    // тогда, когда ядро в ней отказало, а экран при этом не рисовал никакой
    // причины вообще. То же с R5, где user_disabled читался как «прокси есть».
    final (container, core) = _boot();
    core.csmLadderJson = _ladder(
      history: const <Map<String, Object?>>[],
      rungs: <Map<String, Object?>>[
        <String, Object?>{
          'rung': 4,
          'enabled': false,
          'reason': 'not_configured',
        },
        <String, Object?>{
          'rung': 5,
          'enabled': false,
          'reason': 'not_configured',
        },
      ],
    );

    await container.read(csmLadderSyncProvider).pump();

    final facts = container.read(csmTransportFactsProvider);
    expect(facts.tunnelFetchSupported, isFalse);
    expect(facts.proxyConfigured, isFalse);
  });

  test('user_disabled это выключенный путь, а не отсутствующий', () async {
    // Пользователь выключил ступень сам. Путь у неё есть, и сказать
    // «эта сборка так не умеет» значило бы соврать про причину.
    final (container, core) = _boot();
    core.csmLadderJson = _ladder(
      history: const <Map<String, Object?>>[],
      rungs: <Map<String, Object?>>[
        <String, Object?>{
          'rung': 4,
          'enabled': false,
          'reason': 'user_disabled',
        },
        <String, Object?>{
          'rung': 5,
          'enabled': false,
          'reason': 'user_disabled',
        },
      ],
    );

    await container.read(csmLadderSyncProvider).pump();

    final facts = container.read(csmTransportFactsProvider);
    expect(facts.tunnelFetchSupported, isTrue);
    expect(facts.proxyConfigured, isTrue);
  });
}
