// Резервный путь ступени R5 через локальный Tor: подъём из ядра и экран.
//
// Две вещи, ради которых этот файл существует.
//
// 1. Резерв, о котором приложение молчит, хуже отсутствующего: пользователь
//    видит «ступень не настроена» и не знает, что делать. Поэтому «не нашли»,
//    «не искали», «занято вашим прокси» и «платформа не умеет» проверяются как
//    ЧЕТЫРЕ разные строки на экране, а не как одно пустое место.
// 2. Через Tor берётся только подписка. Сеанс VPN от этого анонимным не
//    становится, и текст на экране обязан это говорить. Утверждение проверяется
//    тестом ровно потому, что соблазн сократить его до слова «анонимно» будет
//    возникать при каждой правке.

import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/features/csm/transport_ladder_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/csm_ladder_sync.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';

import 'support/fake_core.dart';

// ------------------------------------------------------------- фикстуры

String _ladderJson({
  Map<String, Object?>? tor,
  String? delivered,
  List<Map<String, Object?>> history = const <Map<String, Object?>>[],
}) => jsonEncode(<String, Object?>{
  'rungs': <Object?>[],
  'history': history,
  if (tor != null) 'tor': tor,
  if (delivered != null) 'delivered': delivered,
});

class _MemoryStore implements ConnectionProfilesStore {
  _MemoryStore(this.profiles, this.activeId);

  List<ConnectionProfile> profiles;
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
    profiles = const <ConnectionProfile>[];
    activeId = null;
  }
}

const _profile = ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.panelAccount,
  displayName: 'Мой оператор',
  source: 'https://panel.example.net',
);

Widget _screen({
  CsmTorStatus tor = CsmTorStatus.none,
  CsmRung? delivered,
}) => ProviderScope(
  overrides: <Override>[
    connectionProfilesStoreProvider.overrideWithValue(
      _MemoryStore(<ConnectionProfile>[_profile], 'cp_1'),
    ),
    csmTorStatusProvider.overrideWith((ref) => tor),
    csmDeliveredRungProvider.overrideWith((ref) => delivered),
  ],
  child: MaterialApp(
    theme: AppTheme.dark(),
    home: const TransportLadderScreen(),
  ),
);

Future<void> _settle(WidgetTester tester) async {
  await tester.pump();
  await tester.pump();
  await tester.pump();
}

void _tall(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 12000)
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
}

(ProviderContainer, FakeVpnCore) _boot() {
  final core = FakeVpnCore();
  final container = ProviderContainer(
    overrides: <Override>[vpnConnectionProvider.overrideWithValue(core)],
  );
  addTearDown(container.dispose);
  return (container, core);
}

void main() {
  group('подъём состояния Tor из ядра', () {
    test('найденный Tor поднимается с адресом и моментом пробы', () async {
      final (container, core) = _boot();
      core.csmLadderJson = _ladderJson(
        tor: <String, Object?>{
          'state': 'ready',
          'addr': '127.0.0.1:9050',
          'detail': 'локальный SOCKS5 отвечает',
          'checked_at': 1788307500,
        },
        delivered: 'direct',
      );

      expect(
        await container.read(csmLadderSyncProvider).pump(),
        CsmLadderSyncOutcome.ok,
      );

      final tor = container.read(csmTorStatusProvider);
      expect(tor.state, CsmTorState.ready);
      expect(tor.addr, '127.0.0.1:9050');
      expect(tor.checkedAtSec, 1788307500);
      expect(container.read(csmDeliveredRungProvider), CsmRung.direct);
    });

    test('ненайденный Tor поднимается вместе с причиной', () async {
      final (container, core) = _boot();
      core.csmLadderJson = _ladderJson(
        tor: <String, Object?>{
          'state': 'absent',
          'detail': '127.0.0.1:9050: connection refused',
          'checked_at': 1788307500,
        },
      );

      await container.read(csmLadderSyncProvider).pump();
      final tor = container.read(csmTorStatusProvider);
      expect(tor.state, CsmTorState.absent);
      expect(tor.addr, isEmpty);
      expect(tor.detail, contains('connection refused'));
      // Ступень ещё ничего не приносила: это «неизвестно», а не R0.
      expect(container.read(csmDeliveredRungProvider), isNull);
    });

    test('ядро без блока tor читается как «не искали», а не «не нашли»', () async {
      final (container, core) = _boot();
      core.csmLadderJson = _ladderJson();

      await container.read(csmLadderSyncProvider).pump();
      expect(container.read(csmTorStatusProvider).state, CsmTorState.unknown);
    });

    test('имя ступени из ядра переводится, неизвестное даёт null', () {
      expect(csmRungFromCoreName('proxy'), CsmRung.userProxy);
      expect(csmRungFromCoreName('out_of_band'), CsmRung.outOfBand);
      // R7 собрана в ядре и не реализована. Подставить вместо неё что-нибудь
      // знакомое значило бы соврать про то, что принесло конфигурацию.
      expect(csmRungFromCoreName('onion'), isNull);
      expect(csmRungFromCoreName(''), isNull);
      expect(csmRungFromCoreName(null), isNull);
    });
  });

  group('экран транспортов говорит о резервном пути вслух', () {
    testWidgets('Tor не найден: строка отказа и его причина на экране', (
      tester,
    ) async {
      _tall(tester);
      await tester.pumpWidget(
        _screen(
          tor: const CsmTorStatus(
            state: CsmTorState.absent,
            detail: '127.0.0.1:9050: connection refused',
            checkedAtSec: 1788307500,
          ),
        ),
      );
      await _settle(tester);

      expect(find.text('Локальный Tor не найден'), findsOneWidget);
      expect(find.textContaining('connection refused'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    testWidgets('Tor найден: на экране именно тот адрес, куда пойдёт R5', (
      tester,
    ) async {
      _tall(tester);
      await tester.pumpWidget(
        _screen(
          tor: const CsmTorStatus(
            state: CsmTorState.ready,
            addr: '127.0.0.1:9050',
            checkedAtSec: 1788307500,
          ),
        ),
      );
      await _settle(tester);

      expect(
        find.text('Локальный Tor найден: 127.0.0.1:9050'),
        findsOneWidget,
      );
    });

    testWidgets('пока R5 выключена, экран говорит «не искали»', (tester) async {
      _tall(tester);
      await tester.pumpWidget(_screen());
      await _settle(tester);
      expect(find.textContaining('Локальный Tor не искали'), findsOneWidget);
      // «Не искали» и «не нашли» это разные утверждения, и второе здесь
      // появиться не имеет права.
      expect(find.text('Локальный Tor не найден'), findsNothing);
    });

    testWidgets('свой прокси пользователя занимает ступень и это сказано', (
      tester,
    ) async {
      _tall(tester);
      await tester.pumpWidget(
        _screen(tor: const CsmTorStatus(state: CsmTorState.superseded)),
      );
      await _settle(tester);
      expect(
        find.textContaining('Занято прокси, который вы ввели сами'),
        findsOneWidget,
      );
    });

    testWidgets('экран не обещает анонимности, которой этот путь не даёт', (
      tester,
    ) async {
      _tall(tester);
      await tester.pumpWidget(
        _screen(
          tor: const CsmTorStatus(
            state: CsmTorState.ready,
            addr: '127.0.0.1:9050',
          ),
        ),
      );
      await _settle(tester);

      expect(
        find.textContaining('сеанс VPN от этого анонимным НЕ становится'),
        findsOneWidget,
      );
      expect(
        find.textContaining('Через Tor берётся только подписка'),
        findsOneWidget,
      );
      // Ступень последняя из автоматических, и экран это говорит: иначе
      // пользователь читает резерв как «включи и станет лучше».
      expect(
        find.textContaining('пробуется последней из автоматических'),
        findsOneWidget,
      );
    });

    testWidgets('неизвестная доставившая ступень не выдумывается', (
      tester,
    ) async {
      _tall(tester);
      await tester.pumpWidget(_screen());
      await _settle(tester);
      expect(
        find.textContaining('Ни одна ступень пока не приносила'),
        findsOneWidget,
      );
    });

    testWidgets('доставившая ступень названа по номеру', (tester) async {
      _tall(tester);
      await tester.pumpWidget(_screen(delivered: CsmRung.userProxy));
      await _settle(tester);
      expect(find.textContaining('принесла ступень R5'), findsOneWidget);
    });
  });
}
