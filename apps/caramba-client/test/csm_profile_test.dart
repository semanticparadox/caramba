// Миграция профиля, закрепление пина и липкое правило INV-13.
//
// Стор читает весь список профилей одной JSON-строкой, поэтому запись, сделанная
// до CSM, обязана грузиться без миграции: одна такая запись не должна ронять
// разбор (иначе пользователь теряет ВСЕ профили разом).

import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_enrollment.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/csm_write.dart';
import 'package:caramba_client/data/csm_high_water_store.dart';

/// Запись ровно в том виде, в каком её писала версия до CSM.
Map<String, dynamic> _preCsmJson() => <String, dynamic>{
  'id': 'cp_1',
  'type': 'panelAccount',
  'display_name': 'Оператор',
  'source': 'https://panel.example.net',
  'panel_url': 'https://panel.example.net',
  'subscription_uuid': 'ec1c1b0e-0000-4000-8000-000000000001',
  'access_token': 'jwt',
  'raw_config': null,
  'format': 'auto',
  'servers': <Object?>[],
  'selected_server_id': null,
  'last_probe': null,
  'servers_updated_ms': 0,
  'branding_cache': null,
  'last_active_ms': 42,
};

CsmPin _pin({CsmPinOrigin origin = CsmPinOrigin.outOfBand}) => CsmPin(
  pid: '226e8a20f699b964',
  linkPin: '49Q8M87PK6WP9QXG3T30',
  origin: origin,
  establishedMs: 1000,
);

void main() {
  group('миграция профиля со старого JSON', () {
    test('запись без ключа csm грузится как «корень не закреплён»', () {
      final p = ConnectionProfile.fromJson(_preCsmJson());

      expect(p.id, 'cp_1');
      expect(p.lastActiveMs, 42);
      // Отсутствие CSM это не пустое состояние CSM: закреплять нечего.
      expect(p.csm, isNull);
      expect(p.isCsmPinned, isFalse);
      // Липкое правило на таком профиле не действует: legacy-путь ему открыт.
      expect(csmStickyRuleBlocksLegacy(p.csm), isFalse);
      expect(csmHardCapabilityError(p.csm), isFalse);
    });

    test(
      'пересохранение старой записи добавляет ключ csm со значением null',
      () {
        final json = ConnectionProfile.fromJson(_preCsmJson()).toJson();

        expect(json.containsKey('csm'), isTrue);
        expect(json['csm'], isNull);
      },
    );

    test('отсутствующий ключ csm это профиль без CSM, и только он', () {
      final p = ConnectionProfile.fromJson(_preCsmJson());
      expect(p.csm, isNull);
    });

    test('испорченная запись csm НЕ читается как отсутствующая', () {
      // 02-SPEC.md 8.8.3: обнулившееся хранилище неотличимо от отката, поэтому
      // испорченная запись обязана остаться ЗАКРЕПЛЁННОЙ и непроверяемой, а не
      // превратиться в "профиль никогда не закреплял корневой ключ". Иначе
      // липкое правило INV-13 снимается одним испорченным байтом на диске.
      for (final junk in <Object?>[
        'broken',
        42,
        <Object?>[],
        <String, Object?>{},
        <String, Object?>{'pin': 'not a map'},
        <String, Object?>{
          'pin': <String, Object?>{'pid': '', 'link_pin': ''},
        },
        // Пин читается, а всё остальное испорчено: отметки, пол, стадия и
        // липкий флаг отзыва обнулиться не имеют права.
        <String, Object?>{
          'pin': <String, Object?>{'pid': 'aa', 'link_pin': 'bb'},
          'hwm': <String, Object?>{'3|LOC': 'not a number'},
        },
        <String, Object?>{
          'pin': <String, Object?>{'pid': 'aa', 'link_pin': 'bb'},
          'time_floor': 'not a number',
        },
        <String, Object?>{
          'pin': <String, Object?>{'pid': 'aa', 'link_pin': 'bb'},
          'revoked': 'yes',
        },
      ]) {
        final json = _preCsmJson()..['csm'] = junk;
        final p = ConnectionProfile.fromJson(json);
        final csm = p.csm;
        expect(csm, isNotNull, reason: 'мусор $junk прочитался как отсутствие');
        expect(csm!.storeInconsistent, isTrue, reason: 'мусор $junk');
        expect(csm.stage.isPinned, isTrue, reason: 'мусор $junk');
        expect(
          csmStickyRuleBlocksLegacy(csm),
          isTrue,
          reason: 'липкое правило снялось на мусоре $junk',
        );
      }
    });

    test('полное состояние CSM переживает круг через JSON', () {
      final source = CsmProfileState(
        pin: _pin(),
        stage: CsmProfileStage.trusted,
        pinHistory: <CsmPinHistoryEntry>[
          CsmPinHistoryEntry(
            pin: _pin(origin: CsmPinOrigin.inApp),
            retiredMs: 500,
            note: 're-enrolled',
          ),
        ],
        highWaterMarks: <String, int>{
          csmHighWaterKey(1, ''): 1,
          csmHighWaterKey(3, 'EA3B8SKCY6VBWASE7AM1X48Y'): 412,
        },
        timeFloorSec: 1788307200,
        catalogCapabilities: const CsmCapabilitySet(0x0000000f),
        directiveCapabilities: const CsmCapabilitySet(0x00000fff),
        keyDocument: const CsmDocumentRecord(
          docType: 1,
          version: 1,
          issuedSec: 1788307200,
          expiresSec: 1788912000,
          signerFingerprints: <String>['aabbccddeeff001122334455'],
          verifiedAtMs: 1788307300000,
        ),
        directive: const CsmDocumentRecord(
          docType: 3,
          version: 412,
          issuedSec: 1788307200,
          expiresSec: 1788310800,
          signerFingerprints: <String>['0011223344556677889900aa'],
          verifiedAtMs: 1788307300000,
          scope: 'EA3B8SKCY6VBWASE7AM1X48Y',
          viaRung: 2,
        ),
        locator: 'EA3B8SKCY6VBWASE7AM1X48Y',
        deviceThumbprint: '4f0f22569564aab09a2d1a75c132d955',
        hardwareTier: CsmHardwareTier.secureEnclave,
        agreementKeyGeneration: 3,
        enrolledAtMs: 1788307000000,
        revoked: true,
        offlineGraceSec: 86400,
        settings: CsmSettings.empty
            .setByUser(CsmSettingKey.killSwitch, const CsmBoolean(true))
            .setDefault(CsmSettingKey.preset, const CsmText('ru-smart')),
        pendingChanges: const <CsmPendingChange>[
          CsmPendingChange(
            id: 'card_1',
            raisedMs: 900,
            items: <CsmCardItem>[
              CsmCardItem(
                key: CsmSettingKey.killSwitch,
                current: CsmBoolean(true),
                proposed: CsmBoolean(false),
                src: CsmProvenance.operator,
                trigger: CsmCardTrigger.narrowing,
              ),
            ],
          ),
        ],
        writeQueue: const CsmWriteQueue(<CsmQueuedWrite>[
          CsmQueuedWrite(
            key: CsmSettingKey.protocol,
            op: CsmWantSet(CsmText('auto')),
            queuedMs: 950,
          ),
        ]),
        ladder: const CsmLadderPrefs(
          order: <int>[0, 3, 2, 1, 6],
          enabled: <int>[0, 1, 3, 6],
          userTouched: true,
        ),
        operatorName: 'Exa Networks',
      );

      final profile = ConnectionProfile.fromJson(
        jsonDecode(
              jsonEncode(
                ConnectionProfile.fromJson(
                  _preCsmJson(),
                ).copyWith(csm: source).toJson(),
              ),
            )
            as Map<String, dynamic>,
      );
      final csm = profile.csm!;

      expect(csm.pin.pid, source.pin.pid);
      expect(csm.pin.origin, CsmPinOrigin.outOfBand);
      expect(csm.stage, CsmProfileStage.trusted);
      expect(csm.pinHistory.single.note, 're-enrolled');
      expect(
        csm.highWaterMarks[csmHighWaterKey(3, 'EA3B8SKCY6VBWASE7AM1X48Y')],
        412,
      );
      expect(csm.timeFloorSec, 1788307200);
      expect(csm.directiveCapabilities, const CsmCapabilitySet(0x00000fff));
      expect(csm.directive!.version, 412);
      expect(csm.directive!.viaRung, 2);
      // Липкий флаг отзыва обязан переживать перезапуск: клиент, потерявший
      // его, переподключается на отозванной подписке.
      expect(csm.revoked, isTrue);
      expect(csm.hardwareTier, CsmHardwareTier.secureEnclave);
      expect(csm.settings.isUserSet(CsmSettingKey.killSwitch), isTrue);
      expect(csm.settings[CsmSettingKey.preset]!.src, CsmProvenance.byDefault);
      expect(
        csm.pendingChanges.single.items.single.trigger,
        CsmCardTrigger.narrowing,
      );
      expect(csm.writeQueue.length, 1);
      expect(csm.ladder.userTouched, isTrue);
      expect(csm.operatorName, 'Exa Networks');
      // Запись без ключа fleet_anchored не показывает тревоги, которой не было.
      expect(csm.fleetRootAnchored, isTrue);
    });

    test('запись без ключей лестницы падает на умолчания и чинит R0/R6', () {
      final csm = CsmProfileState.fromJson(<String, Object?>{
        'pin': _pin().toJson(),
        'ladder': <String, Object?>{
          // Оператор, снявший все сетевые ступени, всё равно не может снять
          // R0 и R6: их отключить нельзя никогда.
          'enabled': <int>[1],
        },
      })!;

      expect(csm.ladder.enabled, containsAll(<int>[0, 6]));
      expect(csm.ladder.order, kCsmDefaultLadderOrder);
      expect(csm.ladder.isEnabled(CsmRung.cached), isTrue);
      expect(csm.ladder.isEnabled(CsmRung.outOfBand), isTrue);
      expect(csm.ladder.isEnabled(CsmRung.mirrors), isFalse);
      expect(
        CsmProfileState.fromJson(<String, Object?>{
          'pin': _pin().toJson(),
          'fleet_anchored': false,
        })!.fleetRootAnchored,
        isFalse,
        reason: 'флот без записи tiers обязан сохраняться как таковой',
      );
      // R0 первый всегда, что бы ни говорил lad.ord.
      expect(csm.ladder.effectiveOrder.first, CsmRung.cached);
    });
  });

  group('установка пина', () {
    test('отпечаток рендерится группами по четыре', () {
      expect(_pin().fingerprint, '49Q8-M87P-K6WP-9QXG-3T30');
    });

    test('ссылка энроллмента с k= заводит профиль в pinning', () {
      final link = CsmEnrollLink.tryParse(
        'carambaconnect://enroll'
        '?panel=https://panel.example.net'
        '&code=49Q8-M87P-KQZ3-WFDG-ZTJX'
        '&k=49Q8M87PK6WP9QXG3T30',
      );

      expect(link, isNotNull);
      expect(link!.origin, 'https://panel.example.net');
      expect(link.code, '49Q8M87PKQZ3WFDGZTJX');
      expect(link.linkPin, '49Q8M87PK6WP9QXG3T30');
      expect(link.codeMatchesPin, isTrue);
      // Пин, приехавший в ссылке, установлен «в приложении», а не вне полосы,
      // и экран личности оператора обязан это говорить (INV-18).
      expect(link.pinOrigin, CsmPinOrigin.inApp);

      final csm = csmProfileFromLink(link, nowMs: 5)!;
      expect(csm.stage, CsmProfileStage.pinning);
      expect(csm.pin.linkPin, '49Q8M87PK6WP9QXG3T30');
      // pid выводится из корневого ключа, которого у ссылки нет.
      expect(csm.pin.pid, isEmpty);
    });

    test('ссылка без k= не заводит профиль CSM: закреплять нечего', () {
      final link = CsmEnrollLink.tryParse(
        'carambaconnect://enroll?panel=https://panel.example.net'
        '&code=49Q8M87PKQZ3WFDGZTJX',
      )!;

      expect(link.linkPin, isNull);
      expect(csmProfileFromLink(link, nowMs: 5), isNull);
    });

    test('код, не сворачивающий пин из той же ссылки, профиль не заводит', () {
      final link = CsmEnrollLink.tryParse(
        'carambaconnect://enroll?panel=https://panel.example.net'
        '&code=ZZZZZZZZKQZ3WFDGZTJX&k=49Q8M87PK6WP9QXG3T30',
      )!;

      expect(link.codeMatchesPin, isFalse);
      expect(csmProfileFromLink(link, nowMs: 5), isNull);
    });

    test('http отвергается, .onion остаётся единственным исключением', () {
      // INV-8: `http://` не принимается ни для одной выборки клиента.
      expect(csmNormalizeOrigin('http://panel.example.net'), isNull);
      expect(
        CsmEnrollLink.tryParse(
          'carambaconnect://enroll?panel=http://panel.example.net&code=49Q8M87PKQZ3WFDGZTJX',
        ),
        isNull,
      );
      expect(
        csmNormalizeOrigin('https://panel.example.net'),
        'https://panel.example.net',
      );
      // Луковые адреса самоаутентичны, поэтому TLS для них не требуется.
      expect(
        csmNormalizeOrigin('http://abcdefghij234567.onion'),
        'http://abcdefghij234567.onion',
      );
    });

    test('код нормализуется без дефисов, регистра и с фолдингом I/L/O', () {
      expect(
        csmNormalizeEnrollCode('49q8-m87p-kqz3-wfdg-ztjx'),
        '49Q8M87PKQZ3WFDGZTJX',
      );
      // I и L это 1, O это 0 (03-WIRE.md 4.1).
      expect(
        csmNormalizeEnrollCode('I9Q8M87PKQZ3WFDGZTJX')?.startsWith('19'),
        isTrue,
      );
      expect(
        csmNormalizeEnrollCode('O9Q8M87PKQZ3WFDGZTJX')?.startsWith('09'),
        isTrue,
      );
      // U вне алфавита Crockford.
      expect(csmNormalizeEnrollCode('U9Q8M87PKQZ3WFDGZTJX'), isNull);
      // Длина ровно 20 символов.
      expect(csmNormalizeEnrollCode('49Q8M87PKQZ3WFDGZTJ'), isNull);
    });

    test('часы вне окна BUILD_EPOCH запрещают энроллмент', () {
      const buildEpoch = 1788307200;

      expect(csmClockPlausible(buildEpoch, buildEpoch: buildEpoch), isTrue);
      expect(
        csmClockPlausible(buildEpoch + 86400, buildEpoch: buildEpoch),
        isTrue,
      );
      // До сборки часы идти не могут: это заводской сброс.
      expect(
        csmClockPlausible(buildEpoch - 1, buildEpoch: buildEpoch),
        isFalse,
      );
      // Десять лет это потолок окна.
      expect(
        csmClockPlausible(
          buildEpoch + kCsmClockPlausibilityWindowSec + 1,
          buildEpoch: buildEpoch,
        ),
        isFalse,
      );
    });
  });

  group('липкое правило INV-13', () {
    test(
      'закреплённый профиль не откатывается в legacy ни по какой причине',
      () {
        for (final stage in CsmProfileStage.values) {
          final csm = CsmProfileState(pin: _pin(), stage: stage);
          final pinned =
              stage != CsmProfileStage.unenrolled &&
              stage != CsmProfileStage.pinning;
          expect(
            csmStickyRuleBlocksLegacy(csm),
            pinned,
            reason: 'состояние ${stage.wire}',
          );
        }
      },
    );

    test('отсутствие cap на закреплённом профиле это жёсткая ошибка', () {
      final anchored = CsmProfileState(
        pin: _pin(),
        stage: CsmProfileStage.trusted,
      );
      expect(csmHardCapabilityError(anchored), isFalse);

      final degraded = csmMarkMissingCapability(anchored);
      expect(degraded.missingCapability, isTrue);
      expect(csmHardCapabilityError(degraded), isTrue);
      // Отката к «считаем, что можно всё» нет: legacy остаётся закрытым.
      expect(csmStickyRuleBlocksLegacy(degraded), isTrue);
    });

    test('на профиле в pinning отсутствие cap ещё не жёсткая ошибка', () {
      final pinning = csmMarkMissingCapability(
        CsmProfileState(pin: _pin(), stage: CsmProfileStage.pinning),
      );
      expect(csmHardCapabilityError(pinning), isFalse);
    });

    test('состояние, отказывающее в подключении, ровно два', () {
      final refusing = CsmProfileStage.values
          .where((s) => s.refusesToConnect)
          .toSet();
      expect(refusing, <CsmProfileStage>{
        CsmProfileStage.graceExhausted,
        CsmProfileStage.compromised,
      });
      // Просрочка НЕ отключает: grace всё ещё подключается (INV-16).
      expect(CsmProfileStage.grace.refusesToConnect, isFalse);
      expect(CsmProfileStage.trustedStale.refusesToConnect, isFalse);
    });
  });

  group('разрешение возможностей и возраст конфигурации', () {
    CsmProfileState withDocs({
      required int directiveExp,
      int? catCap,
      int? dirCap,
    }) => CsmProfileState(
      pin: _pin(),
      stage: CsmProfileStage.trusted,
      catalogCapabilities: catCap == null ? null : CsmCapabilitySet(catCap),
      directiveCapabilities: dirCap == null ? null : CsmCapabilitySet(dirCap),
      catalog: const CsmDocumentRecord(
        docType: 2,
        version: 6,
        issuedSec: 1788000000,
        expiresSec: 1790000000,
        signerFingerprints: <String>[],
        verifiedAtMs: 1788000000000,
      ),
      directive: CsmDocumentRecord(
        docType: 3,
        version: 412,
        issuedSec: 1788307200,
        expiresSec: directiveExp,
        signerFingerprints: const <String>[],
        verifiedAtMs: 1788307200000,
        viaRung: 1,
      ),
    );

    test('свежая директива побеждает каталог', () {
      final csm = withDocs(
        directiveExp: 1788310800,
        catCap: 0x0000000f,
        dirCap: 0x00000001,
      );

      expect(
        csm.resolvedOperatorCapabilities(1788307500),
        const CsmCapabilitySet(0x00000001),
      );
      expect(csm.capabilitiesDisagree, isTrue);
    });

    test('просроченная директива уступает каталогу', () {
      final csm = withDocs(
        directiveExp: 1788310800,
        catCap: 0x0000000f,
        dirCap: 0x00000001,
      );

      // Директива просрочена: возможностью оператора становится каталог, и
      // это ровно случай глубокого офлайна (02-SPEC.md 6.5).
      expect(
        csm.resolvedOperatorCapabilities(1788400000),
        const CsmCapabilitySet(0x0000000f),
      );
    });

    test('возраст конфигурации считается от проверки директивы', () {
      final csm = withDocs(directiveExp: 1788310800, dirCap: 1);

      expect(csm.configurationAgeSec(1788307200000 + 7200 * 1000), 7200);
      // Часы, ушедшие назад, не дают отрицательного возраста.
      expect(csm.configurationAgeSec(1788307100000), 0);
    });

    test('без документов возраст неизвестен, а не ноль', () {
      final csm = CsmProfileState(pin: _pin());
      expect(csm.configurationAgeSec(1788307200000), isNull);
    });
  });

  group('отметки максимума версий', () {
    test('одна карта на профиль, монотонная, только вперёд', () {
      final store = CsmProfileHighWaterStore(
        marks: <String, int>{csmHighWaterKey(3, 'LOC'): 412},
      );

      expect(store.mark(3, 'LOC'), 412);
      // Область разная: каталог не делит отметку с директивой.
      expect(store.mark(2, 'LOC'), 0);

      store.advance(3, 'LOC', 411, Uint8List.fromList(<int>[1]));
      expect(store.mark(3, 'LOC'), 412, reason: 'откат назад запрещён');

      store.advance(3, 'LOC', 413, Uint8List.fromList(<int>[2]));
      expect(store.mark(3, 'LOC'), 413);
      expect(store.storedFrame(3, 'LOC'), <int>[2]);
      expect(store.snapshot()[csmHighWaterKey(3, 'LOC')], 413);
    });
  });
}
