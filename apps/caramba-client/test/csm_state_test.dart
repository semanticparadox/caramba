// Провайдеры CSM/1, за которыми смотрит UI, и мутации, которые он вызывает.
//
// Экраны не собирают состояние CSM сами: они читают провайдеры отсюда. Этот
// тест фиксирует их имена, типы и поведение, чтобы контракт с экранами не
// разъехался молча.

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/csm_state.dart';

/// Хранилище профилей в памяти: тесту не нужен платформенный keychain.
class _MemoryStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles = <ConnectionProfile>[];
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
    profiles = <ConnectionProfile>[];
    activeId = null;
  }
}

const _pin = CsmPin(
  pid: '226e8a20f699b964',
  linkPin: '49Q8M87PK6WP9QXG3T30',
  origin: CsmPinOrigin.outOfBand,
  establishedMs: 1000,
);

const _now = 1788307500;

ConnectionProfile _profile({CsmProfileState? csm}) => ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.panelAccount,
  displayName: 'Оператор',
  source: 'https://panel.example.net',
  csm: csm,
);

Future<ProviderContainer> _boot({CsmProfileState? csm}) async {
  final store = _MemoryStore()
    ..profiles = <ConnectionProfile>[_profile(csm: csm)]
    ..activeId = 'cp_1';
  final container = ProviderContainer(
    overrides: <Override>[
      connectionProfilesStoreProvider.overrideWithValue(store),
      csmClockProvider.overrideWithValue(
        () => DateTime.fromMillisecondsSinceEpoch(_now * 1000),
      ),
    ],
  );
  addTearDown(container.dispose);
  // Нотифаер грузится из стора асинхронно: поднимаем его сразу и дожидаемся
  // окончания загрузки, иначе первое чтение придётся на уже разобранный
  // контейнер.
  container.read(connectionProfilesProvider);
  for (var i = 0; i < 100; i++) {
    if (!container.read(connectionProfilesProvider).loading) {
      break;
    }
    await Future<void>.delayed(Duration.zero);
  }
  return container;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test(
    'профиль без CSM отдаёт пустые провайдеры и не выдумывает состояние',
    () async {
      final c = await _boot();

      expect(c.read(csmProfileStateProvider), isNull);
      expect(c.read(csmOperatorIdentityProvider), isNull);
      expect(c.read(csmCapabilitiesProvider), CsmCapabilitySet.none);
      expect(c.read(csmLadderProvider).rungs, isEmpty);
      expect(c.read(csmPendingChangesProvider), isEmpty);
      expect(c.read(csmStickyErrorProvider), isNull);
      expect(c.read(csmConfigurationAgeProvider).ageSec, isNull);
      expect(c.read(csmDocumentStateProvider).hasAnything, isFalse);
    },
  );

  test('личность оператора отдаёт всё, что требует INV-18', () async {
    final c = await _boot(
      csm: const CsmProfileState(
        pin: _pin,
        stage: CsmProfileStage.trusted,
        enrolledAtMs: 1788300000000,
        hardwareTier: CsmHardwareTier.secureEnclave,
        operatorName: 'Exa Networks',
      ),
    );

    final identity = c.read(csmOperatorIdentityProvider)!;
    expect(identity.displayName, 'Exa Networks');
    expect(identity.pid, '226e8a20f699b964');
    expect(identity.fingerprint, '49Q8-M87P-K6WP-9QXG-3T30');
    expect(identity.pinOrigin, CsmPinOrigin.outOfBand);
    expect(identity.enrolledAtMs, 1788300000000);
    expect(identity.pinEverChanged, isFalse);
    expect(identity.hardwareTier, CsmHardwareTier.secureEnclave);
    expect(identity.stage, CsmProfileStage.trusted);
  });

  test('возможности: свежая директива, потом пересечение с клиентом', () async {
    final c = await _boot(
      csm: const CsmProfileState(
        pin: _pin,
        stage: CsmProfileStage.trusted,
        catalogCapabilities: CsmCapabilitySet(0x00000001),
        directiveCapabilities: CsmCapabilitySet(0x00000fff),
        // За битами 0, 4, 5 и 6 стоит содержимое каталога: без этих фактов
        // 02-SPEC.md 6.2 требует считать их нулём, и провайдер так и делает.
        catalogContent: CsmCatalogContent(
          known: true,
          hasExits: true,
          hasMirrors: true,
          hasDoh: true,
          hasResources: true,
        ),
        directive: CsmDocumentRecord(
          docType: 3,
          version: 412,
          issuedSec: 1788307200,
          expiresSec: 1788310800,
          signerFingerprints: <String>[],
          verifiedAtMs: 1788307200000,
          viaRung: 1,
        ),
      ),
    );

    final caps = c.read(csmCapabilitiesProvider);
    // Директива свежая, значит побеждает она; биты 10 и 11 клиент не умеет.
    expect(caps.raw, 0x000003ff);
    expect(caps.has(CsmCapability.relayChaining), isTrue);
    expect(caps.has(CsmCapability.portHopping), isFalse);
    // Разногласие каталога и директивы попадает в хром проверки.
    expect(c.read(csmDocumentStateProvider).capabilitiesDisagree, isTrue);
  });

  test('лестница отдаёт все семь ступеней, R0 первой, с причинами', () async {
    final c = await _boot(
      csm: const CsmProfileState(
        pin: _pin,
        stage: CsmProfileStage.trusted,
        directiveCapabilities: CsmCapabilitySet(0x00000020),
        // Бит 5 подкреплён непустым списком doh в связанном каталоге; за
        // остальными содержимого нет, и они остаются нулём.
        catalogContent: CsmCatalogContent(known: true, hasDoh: true),
        directive: CsmDocumentRecord(
          docType: 3,
          version: 412,
          issuedSec: 1788307200,
          expiresSec: 1788310800,
          signerFingerprints: <String>[],
          verifiedAtMs: 1788307200000,
        ),
        ladder: CsmLadderPrefs(
          order: <int>[0, 1, 2, 3, 4, 5, 6],
          enabled: <int>[0, 3, 6],
        ),
      ),
    );

    final ladder = c.read(csmLadderProvider);
    // INV-17: список полный, ни одна ступень не спрятана.
    expect(ladder.rungs, hasLength(7));
    expect(ladder.rungs.first.rung, CsmRung.cached);
    expect(ladder.rungs.first.position, 0);

    CsmLadderRung of(CsmRung r) => ladder.rungs.firstWhere((e) => e.rung == r);

    // Выключенная пользователем ступень называет причину, а не исчезает.
    expect(of(CsmRung.direct).enabled, isFalse);
    expect(of(CsmRung.direct).reason, CsmUnavailableReason.userDisabled);
    // Оператор не отдал пул зеркал.
    expect(of(CsmRung.mirrors).reason, CsmUnavailableReason.userDisabled);
    // DoH включён пользователем и отдан оператором (бит 5).
    expect(of(CsmRung.doh).enabled, isTrue);
    expect(of(CsmRung.doh).reason, isNull);
    // R0 и R6 отключить нельзя.
    expect(of(CsmRung.cached).enabled, isTrue);
    expect(of(CsmRung.outOfBand).enabled, isTrue);
  });

  test('возраст конфигурации и источник, INV-21', () async {
    final c = await _boot(
      csm: const CsmProfileState(
        pin: _pin,
        stage: CsmProfileStage.trustedStale,
        directive: CsmDocumentRecord(
          docType: 3,
          version: 412,
          issuedSec: 1788300000,
          expiresSec: 1788303600,
          signerFingerprints: <String>[],
          verifiedAtMs: 1788300000 * 1000,
          viaRung: 2,
        ),
      ),
    );

    final age = c.read(csmConfigurationAgeProvider);
    expect(age.ageSec, _now - 1788300000);
    expect(age.source, CsmRung.mirrors);
    // trusted_stale это НЕ ошибка: это нормальное состояние устройства в
    // блокированной сети.
    expect(age.runningOnCache, isTrue);
    expect(age.stage.refusesToConnect, isFalse);
  });

  test(
    'липкое правило: пин есть, cap нет, ошибка недиссмиссабельная',
    () async {
      final c = await _boot(
        csm: const CsmProfileState(
          pin: _pin,
          stage: CsmProfileStage.trusted,
          missingCapability: true,
        ),
      );

      final error = c.read(csmStickyErrorProvider)!;
      expect(error.pid, '226e8a20f699b964');
      expect(error.dismissible, isFalse);
    },
  );

  test('пин закрепляется один раз и после этого неизменен', () async {
    final c = await _boot();
    final notifier = c.read(csmNotifierProvider);

    // Профиль ещё не закреплял корень: закрепить можно.
    expect(c.read(csmProfileStateProvider), isNull);

    await c
        .read(connectionProfilesProvider.notifier)
        .setCsm(
          'cp_1',
          const CsmProfileState(pin: _pin, stage: CsmProfileStage.pinning),
        );
    expect(c.read(csmProfileStateProvider)!.stage, CsmProfileStage.pinning);

    await notifier.anchor(
      pid: '226e8a20f699b964',
      keyDocument: const CsmDocumentRecord(
        docType: 1,
        version: 1,
        issuedSec: 1788307200,
        expiresSec: 1788912000,
        signerFingerprints: <String>['aa'],
        verifiedAtMs: 1788307200000,
      ),
      timeFloorSec: 1788307200,
    );

    final anchored = c.read(csmProfileStateProvider)!;
    expect(anchored.stage, CsmProfileStage.anchored);
    expect(anchored.stage.isPinned, isTrue);
    expect(anchored.timeFloorSec, 1788307200);

    // Временной пол монотонен и никогда не уменьшается.
    await notifier.anchor(
      pid: '226e8a20f699b964',
      keyDocument: anchored.keyDocument!,
      timeFloorSec: 1,
    );
    expect(c.read(csmProfileStateProvider)!.timeFloorSec, 1788307200);
  });

  test(
    'карточка: Оставить удерживает значение и ставит запись в очередь',
    () async {
      final c = await _boot(
        csm: CsmProfileState(
          pin: _pin,
          stage: CsmProfileStage.trusted,
          settings: CsmSettings.empty.setByUser(
            CsmSettingKey.killSwitch,
            const CsmBoolean(true),
          ),
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
        ),
      );

      expect(c.read(csmPendingChangesProvider), hasLength(1));
      await c.read(csmNotifierProvider).keepCard('card_1', nowMs: 2000);

      final csm = c.read(csmProfileStateProvider)!;
      expect(csm.pendingChanges, isEmpty);
      expect(
        (csm.settings.valueOf(CsmSettingKey.killSwitch)! as CsmBoolean).value,
        isTrue,
      );
      expect(csm.settings.isUserSet(CsmSettingKey.killSwitch), isTrue);
      // Запись, переутверждающая локальное значение, встала в очередь.
      expect(csm.writeQueue.length, 1);
      expect(csm.writeQueue.entries.single.key, CsmSettingKey.killSwitch);
    },
  );

  test(
    'карточка: Вернуть применяет значение оператора и снимает отметку',
    () async {
      final c = await _boot(
        csm: CsmProfileState(
          pin: _pin,
          stage: CsmProfileStage.trusted,
          settings: CsmSettings.empty.setByUser(
            CsmSettingKey.splitMode,
            const CsmText('bypass'),
          ),
          pendingChanges: const <CsmPendingChange>[
            CsmPendingChange(
              id: 'card_1',
              raisedMs: 900,
              items: <CsmCardItem>[
                CsmCardItem(
                  key: CsmSettingKey.splitMode,
                  current: CsmText('bypass'),
                  proposed: CsmText('off'),
                  src: CsmProvenance.operator,
                  trigger: CsmCardTrigger.narrowing,
                ),
              ],
            ),
          ],
        ),
      );

      await c.read(csmNotifierProvider).revertCard('card_1');

      final csm = c.read(csmProfileStateProvider)!;
      expect(csm.pendingChanges, isEmpty);
      expect(
        (csm.settings.valueOf(CsmSettingKey.splitMode)! as CsmText).value,
        'off',
      );
      expect(csm.settings.isUserSet(CsmSettingKey.splitMode), isFalse);
    },
  );

  test(
    'изменение пользователя принимается локально и встаёт в очередь',
    () async {
      final c = await _boot(
        csm: const CsmProfileState(pin: _pin, stage: CsmProfileStage.trusted),
      );

      await c
          .read(csmNotifierProvider)
          .setByUser(
            CsmSettingKey.protocol,
            const CsmText('TUIC'),
            nowMs: 2000,
          );

      final csm = c.read(csmProfileStateProvider)!;
      expect(
        (csm.settings.valueOf(CsmSettingKey.protocol)! as CsmText).value,
        'TUIC',
      );
      expect(csm.settings[CsmSettingKey.protocol]!.src, CsmProvenance.user);
      expect(csm.writeQueue.length, 1);

      // Значение вне словаря не принимается и очередь не растит.
      await c
          .read(csmNotifierProvider)
          .setByUser(CsmSettingKey.preset, const CsmText('full'), nowMs: 2001);
      expect(c.read(csmProfileStateProvider)!.writeQueue.length, 1);
      expect(
        c.read(csmProfileStateProvider)!.settings.valueOf(CsmSettingKey.preset),
        isNull,
      );
    },
  );

  test(
    'пользователь, тронувший лестницу, побеждает умолчания навсегда',
    () async {
      final c = await _boot(
        csm: const CsmProfileState(pin: _pin, stage: CsmProfileStage.trusted),
      );

      expect(c.read(csmLadderProvider).userTouched, isFalse);

      await c.read(csmNotifierProvider).setLadder(enabled: <int>[1]);

      final ladder = c.read(csmLadderProvider);
      expect(ladder.userTouched, isTrue);
      // R0 и R6 остаются включёнными даже после явного выключения всего.
      final mandatory = ladder.rungs
          .where((r) => r.rung.isMandatory)
          .every((r) => r.enabled);
      expect(mandatory, isTrue);
    },
  );
}
