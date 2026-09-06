// Экраны проверки CSM/1: INV-17 (лестница), INV-18 (личность оператора),
// INV-19 (состояние документов), INV-20 (что мы отправляем), INV-21 (возраст
// конфигурации), INV-22 (карточка «Оставить или Вернуть»).
//
// Каждый экран проверяется в трёх состояниях, которых требует DESIGN.md 5.9:
// загрузка, пусто, наполнено. Различие между «ещё читаем с диска» и «ничего не
// закреплено» здесь не косметическое: пользователь, пришедший проверять
// оператора, обязан их отличать, иначе пустой экран читается как «проверка
// провалилась».

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/features/csm/attempt_history.dart';
import 'package:caramba_client/features/csm/config_age_card.dart';
import 'package:caramba_client/features/csm/documents_screen.dart';
import 'package:caramba_client/features/csm/keep_or_revert_card.dart';
import 'package:caramba_client/features/csm/operator_identity_screen.dart';
import 'package:caramba_client/features/csm/transport_ladder_screen.dart';
import 'package:caramba_client/features/csm/what_we_send_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/widgets/ui.dart';

// ------------------------------------------------------------- фикстуры

const _pin = CsmPin(
  pid: '226e8a20f699b964',
  linkPin: '49Q8M87PK6WP9QXG3T30',
  origin: CsmPinOrigin.outOfBand,
  establishedMs: 1788300000000,
);

/// 02-09-2026 12:05 UTC, ровный якорь для дат в утверждениях.
const _nowSec = 1788307500;
const _nowMs = _nowSec * 1000;

CsmDocumentRecord _doc({
  required int docType,
  int version = 7,
  int issuedSec = _nowSec - 3600,
  int expiresSec = _nowSec + 3600,
  String verdict = 'ok',
  List<String> signers = const <String>['a1b2c3d4e5f60718'],
  String scope = '',
  String digest = '',
  int? viaRung,
}) => CsmDocumentRecord(
  docType: docType,
  version: version,
  issuedSec: issuedSec,
  expiresSec: expiresSec,
  signerFingerprints: signers,
  verifiedAtMs: _nowMs - 600 * 1000,
  scope: scope,
  frameDigest: digest,
  verdict: verdict,
  viaRung: viaRung,
);

/// Хранилище профилей в памяти. [gate] держит чтение незавершённым, пока тест
/// смотрит на состояние загрузки.
class _MemoryStore implements ConnectionProfilesStore {
  _MemoryStore(this.profiles, this.activeId, {this.gate});

  List<ConnectionProfile> profiles;
  String? activeId;
  final Completer<void>? gate;

  @override
  Future<List<ConnectionProfile>> readProfiles() async {
    if (gate != null) await gate!.future;
    return profiles;
  }

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

ConnectionProfile _profile(CsmProfileState? csm) => ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.panelAccount,
  displayName: 'Мой оператор',
  source: 'https://panel.example.net',
  csm: csm,
);

Widget _wrap(Widget screen, {CsmProfileState? csm, Completer<void>? gate}) =>
    ProviderScope(
      overrides: <Override>[
        connectionProfilesStoreProvider.overrideWithValue(
          _MemoryStore(<ConnectionProfile>[_profile(csm)], 'cp_1', gate: gate),
        ),
        csmClockProvider.overrideWithValue(
          () => DateTime.fromMillisecondsSinceEpoch(_nowMs),
        ),
      ],
      child: MaterialApp(theme: AppTheme.dark(), home: screen),
    );

/// Профили читаются асинхронно: три кадра доводят провайдер до готовности.
Future<void> _settle(WidgetTester tester) async {
  await tester.pump();
  await tester.pump();
  await tester.pump();
}

void _phone(WidgetTester tester) {
  tester.view
    // Экраны проверки длинные, а ListView строит только видимое. Высокое
    // окно даёт тесту увидеть весь экран без прокрутки.
    ..physicalSize = const Size(780, 9000)
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
}

void main() {
  // ------------------------------------------------------------- INV-18

  group('личность оператора, INV-18', () {
    testWidgets('состояние загрузки: скелет, а не пустой экран', (
      tester,
    ) async {
      _phone(tester);
      final gate = Completer<void>();
      await tester.pumpWidget(
        _wrap(const OperatorIdentityScreen(), gate: gate),
      );
      await tester.pump();

      expect(find.byType(SkeletonRows), findsWidgets);
      expect(find.text('Корневой ключ не закреплён'), findsNothing);

      gate.complete();
      await _settle(tester);
      await tester.pumpWidget(const SizedBox.shrink());
    });

    testWidgets('профиль без CSM: пустое состояние с призывом подключить', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(_wrap(const OperatorIdentityScreen()));
      await _settle(tester);

      expect(find.byType(ScreenEmpty), findsOneWidget);
      expect(find.text('Корневой ключ не закреплён'), findsOneWidget);
      expect(find.text('Подключить оператора'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    testWidgets('всё, что требует INV-18, на экране', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const OperatorIdentityScreen(),
          csm: const CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trusted,
            enrolledAtMs: 1788300000000,
            hardwareTier: CsmHardwareTier.secureEnclave,
            operatorName: 'Exa Networks',
          ),
        ),
      );
      await _settle(tester);

      // Имя оператора, отпечаток группами по четыре, дата, происхождение пина
      // и «менялся ли».
      expect(find.text('Exa Networks'), findsOneWidget);
      expect(find.text('49Q8-M87P-K6WP-9QXG-3T30'), findsOneWidget);
      expect(find.text('226e8a20f699b964'), findsOneWidget);
      expect(find.text('Вне полосы'), findsOneWidget);
      expect(find.text('нет, ни разу'), findsOneWidget);
      expect(find.textContaining('.2026'), findsWidgets);
      expect(find.text('Secure Enclave'), findsOneWidget);
      expect(find.text('Проверено'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    testWidgets('пин в приложении называется слабым, история показывается', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const OperatorIdentityScreen(),
          csm: const CsmProfileState(
            pin: CsmPin(
              pid: '226e8a20f699b964',
              linkPin: '49Q8M87PK6WP9QXG3T30',
              origin: CsmPinOrigin.inApp,
              establishedMs: 1788300000000,
            ),
            stage: CsmProfileStage.trusted,
            enrolledAtMs: 1788300000000,
            pinHistory: <CsmPinHistoryEntry>[
              CsmPinHistoryEntry(
                pin: CsmPin(
                  pid: 'ffffffffffffffff',
                  linkPin: 'AAAABBBBCCCCDDDDEEEE',
                  origin: CsmPinOrigin.outOfBand,
                  establishedMs: 1700000000000,
                ),
                retiredMs: 1780000000000,
              ),
            ],
          ),
        ),
      );
      await _settle(tester);

      expect(find.text('В приложении'), findsOneWidget);
      expect(find.text('да'), findsOneWidget);
      expect(find.text('История отпечатка'.toUpperCase()), findsOneWidget);
      expect(find.text('AAAA-BBBB-CCCC-DDDD-EEEE'), findsOneWidget);
      expect(
        find.textContaining(
          'переживает захват канала хуже',
          findRichText: true,
        ),
        findsNothing,
      );
      expect(tester.takeException(), isNull);
    });
  });

  // ------------------------------------------------------------- INV-19

  group('состояние документов, INV-19', () {
    testWidgets('состояние загрузки', (tester) async {
      _phone(tester);
      final gate = Completer<void>();
      await tester.pumpWidget(_wrap(const CsmDocumentsScreen(), gate: gate));
      await tester.pump();

      expect(find.byType(SkeletonRows), findsWidgets);
      gate.complete();
      await _settle(tester);
      await tester.pumpWidget(const SizedBox.shrink());
    });

    testWidgets('пусто: проверенных документов нет', (tester) async {
      _phone(tester);
      await tester.pumpWidget(_wrap(const CsmDocumentsScreen()));
      await _settle(tester);

      expect(find.byType(ScreenEmpty), findsOneWidget);
      expect(find.text('Проверенных документов нет'), findsOneWidget);
    });

    testWidgets('по каждому документу видны все поля INV-19', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const CsmDocumentsScreen(),
          csm: CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trusted,
            locator: 'LOC0LOC1LOC2LOC3LOC4LOC5',
            deviceThumbprint: '0011223344556677',
            timeFloorSec: _nowSec - 86400,
            catalogCapabilities: const CsmCapabilitySet(0x30),
            directiveCapabilities: const CsmCapabilitySet(0x30),
            // За битами 4 и 5 стоят непустые mir и doh связанного каталога:
            // без этого факта 02-SPEC.md 6.2 требует читать их нулём, и
            // возможности не показались бы вовсе.
            catalogContent: const CsmCatalogContent(
              known: true,
              hasMirrors: true,
              hasDoh: true,
            ),
            keyDocument: _doc(docType: 0x01, version: 3),
            catalog: _doc(
              docType: 0x02,
              version: 12,
              digest: 'aa'.padRight(8, 'b'),
              scope: 'cat-1',
            ),
            directive: _doc(docType: 0x03, version: 41, viaRung: 1),
          ),
        ),
      );
      await _settle(tester);

      expect(find.text('Ключевой документ'), findsOneWidget);
      expect(find.text('Каталог'), findsOneWidget);
      expect(find.text('Директива'), findsOneWidget);

      // Версии, подписант, вердикт, ступень.
      expect(find.text('3'), findsOneWidget);
      expect(find.text('12'), findsOneWidget);
      expect(find.text('41'), findsOneWidget);
      expect(find.text('ПРОВЕРЕНО'), findsNWidgets(3));
      expect(find.text('a1b2c3d4e5f60718'), findsNWidgets(3));
      expect(find.text('R1 Прямой HTTPS к оператору'), findsOneWidget);
      expect(find.text('с диска'), findsNWidgets(2));

      // Разобранные поля профиля и возможности.
      expect(find.text('LOC0LOC1LOC2LOC3LOC4LOC5'), findsOneWidget);
      expect(find.text('0011223344556677'), findsOneWidget);
      expect(find.text('Пул зеркал'), findsOneWidget);
      expect(find.text('Точки DoH'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    testWidgets('отказ проверки назван кодом и классом строки', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const CsmDocumentsScreen(),
          csm: CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trustedStale,
            keyDocument: _doc(docType: 0x01),
            directive: _doc(
              docType: 0x03,
              verdict: 'E_VERIFY_SIG',
              signers: const <String>[],
            ),
          ),
        ),
      );
      await _settle(tester);

      expect(find.text('E_VERIFY_SIG'), findsOneWidget);
      expect(find.text('Подлинность: Подпись не сошлась'), findsOneWidget);
      expect(
        find.text('нет: подпись не сошлась ни с одним авторизованным ключом'),
        findsOneWidget,
      );
    });

    testWidgets('отозванное доверие: весь экран это ошибка с подробностями', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const CsmDocumentsScreen(),
          csm: const CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.compromised,
          ),
        ),
      );
      await _settle(tester);

      expect(find.byType(ScreenError), findsOneWidget);
      expect(
        find.textContaining('Доверие к этому оператору отозвано'),
        findsOneWidget,
      );
      // Сырые подробности лежат под кнопкой, а не в лицо.
      expect(find.text('Подробности'), findsOneWidget);
      expect(find.textContaining('stage compromised'), findsNothing);

      await tester.tap(find.text('Подробности'));
      await tester.pump();
      expect(find.textContaining('stage compromised'), findsOneWidget);
      expect(find.textContaining('pid 226e8a20f699b964'), findsOneWidget);
    });

    testWidgets('три строки хрома 8.8.2 показываются по условию', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const CsmDocumentsScreen(),
          csm: CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trustedStale,
            fleetRootAnchored: false,
            catalogCapabilities: const CsmCapabilitySet(0x10),
            directiveCapabilities: const CsmCapabilitySet(0x30),
            // Просроченный ключевой документ ОСТАЁТСЯ якорем: экран говорит
            // его возраст, а не объявляет профиль сломанным.
            keyDocument: _doc(docType: 0x01, expiresSec: _nowSec - 7200),
            directive: _doc(docType: 0x02, verdict: 'E_VERIFY_REVOKED'),
          ),
        ),
      );
      await _settle(tester);

      expect(find.textContaining('fleet not root-anchored'), findsOneWidget);
      expect(
        find.textContaining('Возможности каталога и директивы разошлись'),
        findsOneWidget,
      );
      expect(find.textContaining('всё ещё остаётся якорем'), findsOneWidget);
      expect(find.textContaining('заменил ключ подписи'), findsOneWidget);
      // Просрочка не читается как разрыв.
      expect(find.textContaining('Подключаться это не мешает'), findsOneWidget);
    });
  });

  // ------------------------------------------------------------- INV-17

  group('транспортная лестница, INV-17', () {
    testWidgets('все семь ступеней видны даже без энроллмента', (tester) async {
      _phone(tester);
      await tester.pumpWidget(_wrap(const TransportLadderScreen()));
      await _settle(tester);

      for (final rung in CsmRung.values) {
        expect(find.text('R${rung.id}'), findsOneWidget);
      }
      expect(find.byType(Switch), findsNWidgets(7));
      expect(find.text('Попыток ещё не было'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    testWidgets(
      'недоступная ступень видна и выключена с причиной, а не спрятана',
      (tester) async {
        _phone(tester);
        await tester.pumpWidget(
          _wrap(
            const TransportLadderScreen(),
            csm: const CsmProfileState(
              pin: _pin,
              stage: CsmProfileStage.trusted,
              // Оператор не объявил ни зеркал, ни DoH: R2 и R3 недоступны.
              directiveCapabilities: CsmCapabilitySet(0),
              catalogCapabilities: CsmCapabilitySet(0),
              ladder: CsmLadderPrefs(
                order: <int>[0, 1, 2, 3, 4, 5, 6],
                enabled: <int>[0, 1, 2, 3, 6],
              ),
            ),
          ),
        );
        await _settle(tester);

        // Семь строк на экране, ни одна не выброшена.
        for (final rung in CsmRung.values) {
          expect(find.text('R${rung.id}'), findsOneWidget);
        }
        expect(find.text('Оператор её не предлагает'), findsNWidgets(2));
        expect(find.text('Выключена вами'), findsNWidgets(2));
        expect(find.text('Платформа этого не поддерживает'), findsNothing);

        // Обязательные ступени помечены и их переключатель заблокирован.
        expect(find.text('ВСЕГДА'), findsNWidgets(2));
        final switches = tester
            .widgetList<Switch>(find.byType(Switch))
            .toList();
        expect(switches.first.onChanged, isNull);
        expect(switches.last.onChanged, isNull);
      },
    );

    testWidgets('переключатель ступени пишет в лестницу профиля', (
      tester,
    ) async {
      _phone(tester);
      final profiles = <ConnectionProfile>[
        _profile(
          const CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trusted,
            directiveCapabilities: CsmCapabilitySet(0x30),
            ladder: CsmLadderPrefs(
              order: <int>[0, 1, 2, 3, 4, 5, 6],
              enabled: <int>[0, 1, 2, 3, 6],
            ),
          ),
        ),
      ];
      final store = _MemoryStore(profiles, 'cp_1');
      final container = ProviderContainer(
        overrides: <Override>[
          connectionProfilesStoreProvider.overrideWithValue(store),
          csmClockProvider.overrideWithValue(
            () => DateTime.fromMillisecondsSinceEpoch(_nowMs),
          ),
        ],
      );
      addTearDown(container.dispose);
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: const TransportLadderScreen(),
          ),
        ),
      );
      await _settle(tester);

      // R1 (прямой HTTPS) включён; выключаем его.
      final r1Switch = find.byType(Switch).at(1);
      await tester.tap(r1Switch);
      await _settle(tester);

      final ladder = container.read(csmProfileStateProvider)!.ladder;
      expect(ladder.enabled.contains(1), isFalse);
      expect(ladder.userTouched, isTrue);
      // R0 и R6 остаются включёнными всегда.
      expect(ladder.enabled.contains(0), isTrue);
      expect(ladder.enabled.contains(6), isTrue);
    });

    testWidgets('живая история попыток: ступень, хост, исход, код', (
      tester,
    ) async {
      _phone(tester);
      final container = ProviderContainer(
        overrides: <Override>[
          connectionProfilesStoreProvider.overrideWithValue(
            _MemoryStore(<ConnectionProfile>[_profile(null)], 'cp_1'),
          ),
          csmClockProvider.overrideWithValue(
            () => DateTime.fromMillisecondsSinceEpoch(_nowMs),
          ),
        ],
      );
      addTearDown(container.dispose);
      // Ходок по лестнице пишет сюда; экран только читает.
      container.read(csmAttemptHistoryProvider.notifier)
        ..record(
          const CsmAttempt(
            rung: CsmRung.direct,
            host: 'panel.example.net',
            startedMs: _nowMs - 30000,
            outcome: CsmAttemptOutcome.failed,
            errorCode: 'E_PARSE_MAGIC',
          ),
        )
        ..record(
          const CsmAttempt(
            rung: CsmRung.mirrors,
            host: 'зеркало 2',
            startedMs: _nowMs - 20000,
            outcome: CsmAttemptOutcome.ok,
          ),
        );
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: const TransportLadderScreen(),
          ),
        ),
      );
      await _settle(tester);

      expect(find.text('Попыток ещё не было'), findsNothing);
      expect(find.text('R1 · panel.example.net'), findsOneWidget);
      expect(find.text('E_PARSE_MAGIC'), findsOneWidget);
      expect(find.text('R2 · зеркало 2'), findsOneWidget);
      expect(find.text('успех'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  });

  // ------------------------------------------------------------- INV-20

  group('что мы отправляем, INV-20', () {
    testWidgets('состояние загрузки', (tester) async {
      _phone(tester);
      final gate = Completer<void>();
      await tester.pumpWidget(_wrap(const WhatWeSendScreen(), gate: gate));
      await tester.pump();

      expect(find.byType(SkeletonRows), findsWidgets);
      gate.complete();
      await _settle(tester);
      await tester.pumpWidget(const SizedBox.shrink());
    });

    testWidgets('без энроллмента состав запросов всё равно перечислен', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(_wrap(const WhatWeSendScreen()));
      await _settle(tester);

      expect(find.text('GET /sub/m1/{loc}'.toUpperCase()), findsOneWidget);
      expect(
        find.textContaining('Профиль не проходил энроллмент CSM'),
        findsOneWidget,
      );
      expect(find.text('Скопировать весь список'), findsOneWidget);
    });

    testWidgets('перечислены поля запросов и то, что не уходит никогда', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const WhatWeSendScreen(),
          csm: CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trusted,
            locator: 'LOC0LOC1LOC2LOC3LOC4LOC5',
            deviceThumbprint: '0011223344556677',
            directive: _doc(docType: 0x03, version: 41),
            keyDocument: _doc(docType: 0x01, version: 3),
          ),
        ),
      );
      await _settle(tester);

      // Поля запроса директивы: локатор, nonce, версия, отпечаток устройства.
      expect(find.text('loc'), findsWidgets);
      expect(find.text('n'), findsOneWidget);
      expect(find.text('v'), findsWidgets);
      expect(find.text('d'), findsOneWidget);
      expect(find.text('41'), findsWidgets);
      expect(find.text('3'), findsWidgets);
      expect(find.text('0011223344556677'), findsWidgets);

      // Список «никогда» на месте: INV-15 первым пунктом.
      expect(
        find.text(
          'Список приложений раздельного туннелирования, в обе стороны',
        ),
        findsOneWidget,
      );
      expect(
        find.text('Какая ступень транспорта принесла запрос'),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    });

    testWidgets('кнопка копирования кладёт весь список в буфер', (
      tester,
    ) async {
      _phone(tester);
      final copied = <String>[];
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        (call) async {
          if (call.method == 'Clipboard.setData') {
            copied.add((call.arguments as Map)['text'] as String);
          }
          return null;
        },
      );
      addTearDown(
        () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
          SystemChannels.platform,
          null,
        ),
      );

      await tester.pumpWidget(
        _wrap(
          const WhatWeSendScreen(),
          csm: const CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trusted,
            locator: 'LOC0LOC1LOC2LOC3LOC4LOC5',
          ),
        ),
      );
      await _settle(tester);

      await tester.tap(find.text('Скопировать весь список'));
      await tester.pump();

      expect(copied, hasLength(1));
      expect(copied.single, contains('GET /sub/m1/{loc}'));
      expect(copied.single, contains('Не уходит никогда:'));
      expect(copied.single, contains('49Q8-M87P-K6WP-9QXG-3T30'));
      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  // ------------------------------------------------------------- INV-21

  group('возраст конфигурации, INV-21', () {
    testWidgets('без энроллмента карточки нет вовсе', (tester) async {
      _phone(tester);
      await tester.pumpWidget(_wrap(const Scaffold(body: CsmConfigAgeCard())));
      await _settle(tester);

      expect(find.byType(InkWell), findsNothing);
      expect(tester.takeException(), isNull);
    });

    testWidgets('работа на кэше это не ошибка, а названное состояние', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const Scaffold(body: CsmConfigAgeCard()),
          csm: CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trustedStale,
            directive: _doc(docType: 0x03, viaRung: 0),
          ),
        ),
      );
      await _settle(tester);

      expect(find.text('Работает на сохранённой конфигурации'), findsOneWidget);
      expect(find.textContaining('проверена 10 минут назад'), findsOneWidget);
      expect(find.textContaining('R0 сохранённые документы'), findsOneWidget);
      expect(find.textContaining('Подключение это не ломает'), findsOneWidget);
    });

    testWidgets('свежая конфигурация: одна тихая строка с источником', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const Scaffold(body: CsmConfigAgeCard()),
          csm: CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trusted,
            directive: _doc(docType: 0x03, viaRung: 1),
          ),
        ),
      );
      await _settle(tester);

      expect(
        find.textContaining('Конфигурация проверена 10 минут назад'),
        findsOneWidget,
      );
      expect(find.text('Работает на сохранённой конфигурации'), findsNothing);
    });

    testWidgets('липкая ошибка INV-13 недиссмиссабельна и стоит первой', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const Scaffold(body: CsmConfigAgeCard()),
          csm: const CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.trusted,
            missingCapability: true,
          ),
        ),
      );
      await _settle(tester);

      expect(
        find.text('Документ пришёл без поля возможностей'),
        findsOneWidget,
      );
      expect(find.textContaining('отката'), findsOneWidget);
      // Закрыть её нечем: крестика на карточке нет.
      expect(find.byType(IconBtn), findsNothing);
    });

    testWidgets('отказ подключаться назван отдельно от возраста', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const Scaffold(body: CsmConfigAgeCard()),
          csm: const CsmProfileState(
            pin: _pin,
            stage: CsmProfileStage.graceExhausted,
          ),
        ),
      );
      await _settle(tester);

      expect(find.text('Окно офлайн-работы исчерпано'), findsOneWidget);
      expect(find.textContaining('вне полосы'), findsOneWidget);
    });
  });

  // ------------------------------------------------------------- INV-22

  group('карточка «Оставить или Вернуть», INV-22', () {
    CsmProfileState withCard({
      required CsmCardTrigger trigger,
      bool multi = false,
    }) => CsmProfileState(
      pin: _pin,
      stage: CsmProfileStage.trusted,
      settings: const CsmSettings(
        entries: <CsmSettingKey, CsmSettingEntry>{
          CsmSettingKey.killSwitch: CsmSettingEntry(
            value: CsmBoolean(true),
            src: CsmProvenance.user,
            userSet: true,
          ),
        },
      ),
      pendingChanges: <CsmPendingChange>[
        CsmPendingChange(
          id: 'card_1',
          raisedMs: _nowMs - 60000,
          items: <CsmCardItem>[
            CsmCardItem(
              key: CsmSettingKey.killSwitch,
              current: const CsmBoolean(true),
              proposed: const CsmBoolean(false),
              src: CsmProvenance.operator,
              trigger: trigger,
            ),
            if (multi)
              CsmCardItem(
                key: CsmSettingKey.protocol,
                current: const CsmText('AmneziaWG'),
                proposed: const CsmText('Shadowsocks'),
                src: CsmProvenance.operator,
                trigger: trigger,
              ),
          ],
        ),
      ],
    );

    testWidgets('карточек нет: секция сворачивается в ничто', (tester) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(const Scaffold(body: CsmPendingChangesSection())),
      );
      await _settle(tester);

      expect(find.byType(KeepOrRevertCard), findsNothing);
    });

    testWidgets('карточка называет настройку, оба значения и происхождение', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const Scaffold(body: CsmPendingChangesSection()),
          csm: withCard(trigger: CsmCardTrigger.narrowing),
        ),
      );
      await _settle(tester);

      expect(find.text('Оператор сузил вашу защиту'), findsOneWidget);
      expect(find.text('Kill-switch'), findsOneWidget);
      expect(find.text('Сейчас'), findsOneWidget);
      expect(find.text('включено'), findsOneWidget);
      expect(find.text('Оператор предлагает'), findsOneWidget);
      expect(find.text('выключено'), findsOneWidget);
      expect(
        find.text('Источник: оператор. Повод: сужение защиты.'),
        findsOneWidget,
      );
      expect(
        find.text(
          'Пока вы не ответите, действует ваше значение. Новое не применено.',
        ),
        findsOneWidget,
      );
      expect(find.text('Оставить моё'), findsOneWidget);
      expect(find.text('Принять новое'), findsOneWidget);
    });

    testWidgets('многоключевая карточка перечисляет все затронутые ключи', (
      tester,
    ) async {
      _phone(tester);
      await tester.pumpWidget(
        _wrap(
          const Scaffold(body: CsmPendingChangesSection()),
          csm: withCard(
            trigger: CsmCardTrigger.operatorOverwroteUserSet,
            multi: true,
          ),
        ),
      );
      await _settle(tester);

      expect(find.text('Оператор поменял ваши настройки'), findsOneWidget);
      expect(find.text('Kill-switch'), findsOneWidget);
      expect(find.text('Тип подключения'), findsOneWidget);
      expect(find.text('AmneziaWG'), findsOneWidget);
      expect(find.text('Shadowsocks'), findsOneWidget);
    });

    testWidgets('«Оставить моё» держит значение и снимает карточку', (
      tester,
    ) async {
      _phone(tester);
      final profiles = <ConnectionProfile>[
        _profile(withCard(trigger: CsmCardTrigger.narrowing)),
      ];
      final store = _MemoryStore(profiles, 'cp_1');
      final container = ProviderContainer(
        overrides: <Override>[
          connectionProfilesStoreProvider.overrideWithValue(store),
          csmClockProvider.overrideWithValue(
            () => DateTime.fromMillisecondsSinceEpoch(_nowMs),
          ),
        ],
      );
      addTearDown(container.dispose);
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: const MaterialApp(
            home: Scaffold(body: CsmPendingChangesSection()),
          ),
        ),
      );
      await _settle(tester);

      await tester.tap(find.text('Оставить моё'));
      await _settle(tester);

      final csm = container.read(csmProfileStateProvider)!;
      expect(csm.pendingChanges, isEmpty);
      expect(csm.settings.isUserSet(CsmSettingKey.killSwitch), isTrue);
      expect(
        (csm.settings.valueOf(CsmSettingKey.killSwitch)! as CsmBoolean).value,
        isTrue,
      );
      // Переутверждение встало в очередь записи, а не потерялось.
      expect(csm.writeQueue.length, 1);
      expect(find.byType(KeepOrRevertCard), findsNothing);
    });

    testWidgets('«Принять новое» применяет значение оператора', (tester) async {
      _phone(tester);
      final profiles = <ConnectionProfile>[
        _profile(withCard(trigger: CsmCardTrigger.narrowing)),
      ];
      final store = _MemoryStore(profiles, 'cp_1');
      final container = ProviderContainer(
        overrides: <Override>[
          connectionProfilesStoreProvider.overrideWithValue(store),
          csmClockProvider.overrideWithValue(
            () => DateTime.fromMillisecondsSinceEpoch(_nowMs),
          ),
        ],
      );
      addTearDown(container.dispose);
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: const MaterialApp(
            home: Scaffold(body: CsmPendingChangesSection()),
          ),
        ),
      );
      await _settle(tester);

      await tester.tap(find.text('Принять новое'));
      await _settle(tester);

      final csm = container.read(csmProfileStateProvider)!;
      expect(csm.pendingChanges, isEmpty);
      expect(csm.settings.isUserSet(CsmSettingKey.killSwitch), isFalse);
      expect(
        (csm.settings.valueOf(CsmSettingKey.killSwitch)! as CsmBoolean).value,
        isFalse,
      );
    });
  });
}
