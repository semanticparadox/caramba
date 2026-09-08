// Швы между слоем Dart и ядром: регистрация, обновление, лестница, ответ на
// карточку каталога и переключение хранилища при смене профиля.
//
// Нормативно: 02-SPEC.md 1.2 (хранилище на профиль), 7.7.1 и
// 04-THREAT-MODEL.md 7.3 шаг 5 (смена набора ресурсов), 8.1 и 8.3 (лестница),
// 8.9 (управляющий слой не открывает своих сокетов), 9 (регистрация),
// INV-16, INV-17, INV-22.
//
// Каждый тест здесь про одно: доходит ли действие пользователя ДО ЯДРА.
// Действие, оставшееся в слое Dart, меняет только картинку, и именно этим
// раньше отличались экраны транспортов и карточка каталога от того, что они
// обещают на экране.

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/features/csm/attempt_history.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/csm_catalog_guard.dart';
import 'package:caramba_client/state/csm_enrollment_bridge.dart';
import 'package:caramba_client/state/csm_ladder_sync.dart';
import 'package:caramba_client/state/csm_profile_binding.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

import 'support/fake_csm_device.dart';

const _pin = CsmPin(
  pid: '226e8a20f699b964',
  linkPin: '49Q8M87PK6WP9QXG3T30',
  origin: CsmPinOrigin.outOfBand,
  establishedMs: 1788300000000,
);

const int _nowMs = 1788307500000;

CsmProfileState _pinned() =>
    const CsmProfileState(pin: _pin, stage: CsmProfileStage.pinning);

CsmProfileState _trusted() => const CsmProfileState(
  pin: _pin,
  stage: CsmProfileStage.trusted,
  directive: CsmDocumentRecord(
    docType: 3,
    version: 411,
    issuedSec: 1788307200,
    expiresSec: 1788400000,
    signerFingerprints: <String>['aa'],
    verifiedAtMs: _nowMs,
  ),
  directiveCapabilities: CsmCapabilitySet(1 << 3),
);

class _Store implements ConnectionProfilesStore {
  _Store(this.profiles, this.activeId);

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

class _FakeCore with FakeCsmDevice implements VpnConnection {
  @override
  final VpnStatus currentStatus = const VpnStatus(stage: VpnStage.disconnected);

  @override
  Stream<VpnStatus> get status => Stream<VpnStatus>.value(currentStatus);

  @override
  Stream<TrafficStats> get traffic => const Stream<TrafficStats>.empty();

  @override
  Future<void> connect(Object server) async {}

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {}

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async => const ImportResult(servers: <ImportedServer>[]);

  @override
  Future<List<ProbeResult>> probe({Duration timeout = Duration.zero}) async =>
      const <ProbeResult>[];

  @override
  Future<void> setPolicy(CorePolicy policy) async {}

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 0}) async {}

  @override
  Future<void> disconnect() async {}
  /// Правду о стадии фейк знает сам: платформы за ним нет.
  @override
  Future<VpnStatus> refreshStatus() async => currentStatus;

  @override
  Future<void> dispose() async {}
}

Future<void> _settle() async {
  for (var i = 0; i < 8; i++) {
    await Future<void>.delayed(Duration.zero);
  }
}

Future<(ProviderContainer, _FakeCore)> _boot({
  CsmProfileState? csm,
  List<ConnectionProfile>? profiles,
  String activeId = 'cp_1',
}) async {
  final core = _FakeCore();
  final list =
      profiles ??
      <ConnectionProfile>[
        ConnectionProfile(
          id: 'cp_1',
          type: ProfileType.rawSub,
          displayName: 'Моя подписка',
          source: 'https://sub.example/a',
          rawConfig: 'proxies: []',
          format: 'clash',
          csm: csm,
        ),
      ];
  final container = ProviderContainer(
    overrides: <Override>[
      vpnConnectionProvider.overrideWithValue(core),
      connectionProfilesStoreProvider.overrideWithValue(_Store(list, activeId)),
    ],
  );
  addTearDown(container.dispose);
  container.read(connectionProfilesProvider);
  await _settle();
  return (container, core);
}

void main() {
  test('порядок ступеней доходит до ядра, а не остаётся картинкой', () async {
    final (container, core) = await _boot(csm: _trusted());

    final outcome = await container
        .read(csmNotifierProvider)
        .setLadder(order: <int>[0, 2, 1, 3, 4, 5, 6]);

    expect(outcome, CsmLadderApplyOutcome.applied);
    // Лестницей ходит ядро, и порядок оно берёт из своего хранилища. Запись,
    // не дошедшая сюда, оставила бы выборку на прежнем порядке.
    expect(core.lastLadderOrder, <int>[0, 2, 1, 3, 4, 5, 6]);
    expect(core.lastLadderEnabled[0], isTrue);
    expect(core.lastLadderEnabled[6], isTrue);
  });

  test('профиль без энроллмента не делает вид, что применил порядок', () async {
    final (container, core) = await _boot();

    final outcome = await container
        .read(csmNotifierProvider)
        .setLadder(order: <int>[0, 1, 2]);

    expect(outcome, CsmLadderApplyOutcome.notEnrolled);
    expect(core.lastLadderOrder, isEmpty);
  });

  test('ответ на карточку каталога уходит В ЯДРО до снятия карточки', () async {
    final (container, core) = await _boot(csm: _trusted());
    final guard = container.read(csmCatalogGuardProvider.notifier);
    guard.observe(
      const CsmCatalogFingerprint(
        resources: <CsmResourceRef>[
          CsmResourceRef(kind: 'rs', name: 'ru-banks', hash: 'aa'),
        ],
      ),
      nowMs: _nowMs,
    );
    final raised = guard.observe(
      const CsmCatalogFingerprint(
        resources: <CsmResourceRef>[
          CsmResourceRef(kind: 'rs', name: 'ru-banks', hash: 'bb'),
        ],
      ),
      nowMs: _nowMs + 1000,
    );
    expect(raised, isTrue);
    final card = container.read(csmCatalogChangesProvider).single;

    final ok = await csmAnswerCatalogCard(
      connection: core,
      guard: guard,
      cardId: card.id,
      accept: false,
    );

    expect(ok, isTrue);
    // Прежний набор удерживает ЯДРО, и до этого вызова кнопка "оставить
    // прежние" не откатывала ничего.
    expect(core.catalogAnswers, <bool>[false]);
    expect(container.read(csmCatalogChangesProvider), isEmpty);
  });

  test('отказ ядра НЕ снимает карточку', () async {
    final (container, core) = await _boot(csm: _trusted());
    final guard = container.read(csmCatalogGuardProvider.notifier);
    guard.observe(
      const CsmCatalogFingerprint(
        resources: <CsmResourceRef>[
          CsmResourceRef(kind: 'rs', name: 'ru-banks', hash: 'aa'),
        ],
      ),
      nowMs: _nowMs,
    );
    guard.observe(
      const CsmCatalogFingerprint(
        resources: <CsmResourceRef>[
          CsmResourceRef(kind: 'rs', name: 'ru-banks', hash: 'bb'),
        ],
      ),
      nowMs: _nowMs + 1000,
    );
    final card = container.read(csmCatalogChangesProvider).single;
    core.failCatalogAnswerWith = 'мост недоступен';

    final ok = await csmAnswerCatalogCard(
      connection: core,
      guard: guard,
      cardId: card.id,
      accept: true,
    );

    expect(ok, isFalse);
    // Пользователь, у которого вопрос исчез, считает, что ответил. Ответ никуда
    // не дошёл, поэтому карточка остаётся.
    expect(container.read(csmCatalogChangesProvider), hasLength(1));
  });

  test(
    'регистрация через ядро закрепляет профиль на проверенном ключе',
    () async {
      final (container, core) = await _boot(csm: _pinned());
      core.csmEnrollJson =
          '{"enrolled":true,"pid":"226e8a20f699b964","time_floor":1788307200,'
          '"key":{"present":true,"ver":7,"iat":1788300000,"exp":1788999999,'
          '"signers":["aa","bb"]}}';

      final result = await csmEnrollAndAnchor(
        connection: core,
        notifier: container.read(csmNotifierProvider),
        origin: 'https://panel.example',
        code: 'ABCDEFGH123456789012',
        linkPin: _pin.linkPin,
        nowMs: _nowMs,
      );
      await _settle();

      expect(result.outcome, CsmAnchorOutcome.anchored);
      expect(core.enrollCalls.single['origin'], 'https://panel.example');
      final csm = container.read(csmProfileStateProvider);
      expect(csm?.stage, CsmProfileStage.anchored);
      expect(csm?.pin.pid, '226e8a20f699b964');
      expect(csm?.timeFloorSec, 1788307200);
      expect(csm?.keyDocument?.version, 7);
      expect(csm?.keyDocument?.signerFingerprints, <String>['aa', 'bb']);
    },
  );

  test('снимок без ключевого документа НЕ закрепляет профиль', () async {
    final (container, core) = await _boot(csm: _pinned());
    // Закрепить профиль на pid без документа, которым он посчитан, значит
    // объявить проверенным то, что не проверялось.
    core.csmEnrollJson = '{"enrolled":true,"pid":"226e8a20f699b964"}';

    final result = await csmEnrollAndAnchor(
      connection: core,
      notifier: container.read(csmNotifierProvider),
      origin: 'https://panel.example',
      nowMs: _nowMs,
    );
    await _settle();

    expect(result.outcome, CsmAnchorOutcome.refused);
    expect(
      container.read(csmProfileStateProvider)?.stage,
      CsmProfileStage.pinning,
    );
  });

  test('сборка без ABI v3 отвечает пусто, а не отказом регистрации', () async {
    final (container, core) = await _boot(csm: _pinned());
    core.csmEnrollJson = '';

    final result = await csmEnrollAndAnchor(
      connection: core,
      notifier: container.read(csmNotifierProvider),
      origin: 'https://panel.example',
    );

    expect(result.outcome, CsmAnchorOutcome.unavailable);
  });

  test('смена профиля выбрасывает историю и переключает хранилище', () async {
    final profiles = <ConnectionProfile>[
      ConnectionProfile(
        id: 'cp_a',
        type: ProfileType.rawSub,
        displayName: 'A',
        source: 'https://a.example',
        rawConfig: 'proxies: []',
        format: 'clash',
        csm: _trusted(),
      ),
      const ConnectionProfile(
        id: 'cp_b',
        type: ProfileType.rawSub,
        displayName: 'B',
        source: 'https://b.example',
        rawConfig: 'proxies: []',
        format: 'clash',
      ),
    ];
    final (container, core) = await _boot(profiles: profiles, activeId: 'cp_a');

    // Наблюдатель привязки живёт в корне приложения; в тесте его роль играет
    // это чтение.
    container.read(csmProfileBindingProvider);
    await _settle();
    expect(core.selectedCsmProfile, 'cp_a');

    container
        .read(csmAttemptHistoryProvider.notifier)
        .record(
          const CsmAttempt(
            rung: CsmRung.mirrors,
            host: 'mirror-1',
            startedMs: _nowMs,
            outcome: CsmAttemptOutcome.ok,
          ),
        );
    container.read(csmTransportFactsProvider.notifier).state =
        const CsmTransportFacts(tunnelFetchSupported: true);
    expect(container.read(csmAttemptHistoryProvider), hasLength(1));

    await container.read(connectionProfilesProvider.notifier).activate('cp_b');
    container.read(csmProfileBindingProvider);
    await _settle();

    // История ЛОКАЛЬНА и хранится на профиль (02-SPEC.md 7.10): попытки
    // оператора A на экране оператора B это раскрытие между тенантами.
    expect(container.read(csmAttemptHistoryProvider), isEmpty);
    expect(
      container.read(csmTransportFactsProvider).tunnelFetchSupported,
      isFalse,
    );
    expect(core.selectedCsmProfile, 'cp_b');
    expect(container.read(csmLadderSyncProvider), isNotNull);
  });

  test('ключ хранилища ядра вычищается до безопасного имени каталога', () {
    // Ключ уходит в путь на стороне ядра. Значение ".." увело бы хранилище
    // личности устройства на уровень выше рабочего каталога.
    expect(csmCoreProfileKey('cp_A-1'), 'cp_a-1');
    expect(csmCoreProfileKey('../etc'), '---etc');
    expect(csmCoreProfileKey(''), '');
    expect(csmCoreProfileKey(null), '');
    expect(csmCoreProfileKey('x' * 100).length, 64);
  });
}
