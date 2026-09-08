// Путь записи настроек CSM/1, от правки пользователя до подписанного запроса.
//
// Нормативно: 02-SPEC.md 7.5 (трёхсторонняя семантика), 7.8 (локально сразу,
// потом в очередь), 6.2 и 6.5 (возможности), 03-WIRE.md 13.6 (форма запроса и
// прообраз доказательства), инвариант 16 (сетевой отказ ничего не теряет).
//
// Весь round trip живёт в ядре: оно подписывает прообраз ключом устройства,
// ставит X-CSM-Proof и If-Match, идёт по лестнице и проверяет подписанный
// ответ. Здесь проверяется то, что решает слой Dart: ЧТО именно уходит, когда
// уходит, и что происходит с очередью, когда не уходит.

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/csm_write.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
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

/// Профиль после энроллмента, с проверенной непросроченной директивой, чей
/// `cap` предлагает запись настроек (бит 3).
CsmProfileState _enrolled({int cap = 1 << 3}) => CsmProfileState(
  pin: _pin,
  stage: CsmProfileStage.trusted,
  directive: const CsmDocumentRecord(
    docType: 3,
    version: 411,
    issuedSec: 1788307200,
    expiresSec: 1788400000,
    signerFingerprints: <String>['aa'],
    verifiedAtMs: _nowMs,
  ),
  directiveCapabilities: CsmCapabilitySet(cap),
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

Future<(ProviderContainer, _FakeCore, CsmNotifier)> _boot(
  CsmProfileState? csm,
) async {
  final core = _FakeCore();
  final profile = ConnectionProfile(
    id: 'cp_1',
    type: ProfileType.rawSub,
    displayName: 'Моя подписка',
    source: 'https://sub.example/a',
    rawConfig: 'proxies: []',
    format: 'clash',
    csm: csm,
  );
  final container = ProviderContainer(
    overrides: <Override>[
      vpnConnectionProvider.overrideWithValue(core),
      connectionProfilesStoreProvider.overrideWithValue(
        _Store(<ConnectionProfile>[profile], 'cp_1'),
      ),
    ],
  );
  addTearDown(container.dispose);
  // Профили читаются асинхронно; дождёмся первого чтения.
  container.read(connectionProfilesProvider);
  for (var i = 0; i < 8; i++) {
    await Future<void>.delayed(Duration.zero);
  }
  return (container, core, container.read(csmNotifierProvider));
}

void main() {
  test(
    'правка уходит одним подписанным запросом и снимается с очереди',
    () async {
      final (container, core, notifier) = await _boot(_enrolled());

      await notifier.setByUser(CsmSettingKey.protocol, const CsmText('auto'));
      // setByUser сам инициирует отдачу; дождёмся её.
      for (var i = 0; i < 8; i++) {
        await Future<void>.delayed(Duration.zero);
      }

      expect(core.sentWrites, hasLength(1));
      expect(core.sentWrites.single, <int, Object?>{1: 'auto'});
      expect(
        container.read(csmProfileStateProvider)!.writeQueue.isEmpty,
        isTrue,
      );
    },
  );

  test('типы значений те же, что у карты pol директивы', () async {
    final (_, core, notifier) = await _boot(_enrolled());

    // Очередь схлопывается по ключу, поэтому три РАЗНЫХ ключа: текст, целое,
    // булево. Значение не своего типа панель отвергла бы по разбору.
    await notifier.setByUser(CsmSettingKey.mtu, const CsmUint(1280));
    await notifier.setByUser(CsmSettingKey.killSwitch, const CsmBoolean(true));
    await notifier.setByUser(
      CsmSettingKey.dnsNameservers,
      const CsmTextList(<String>['https://1.1.1.1/dns-query']),
    );
    for (var i = 0; i < 16; i++) {
      await Future<void>.delayed(Duration.zero);
    }

    // Каждая правка инициирует свою отдачу, поэтому смотрим на объединение:
    // важно, каким ТИПОМ ушло значение, а не сколько запросов его унесло.
    final all = <int, Object?>{};
    for (final w in core.sentWrites) {
      all.addAll(w);
    }
    expect(all[CsmSettingKey.mtu.wire], 1280);
    expect(all[CsmSettingKey.killSwitch.wire], true);
    expect(all[CsmSettingKey.dnsNameservers.wire], <String>[
      'https://1.1.1.1/dns-query',
    ]);
  });

  test(
    'отказ сети не теряет очередь и не откатывает локальное значение',
    () async {
      final (container, core, notifier) = await _boot(_enrolled());
      core.failWritesWith = 'ладдер не дошёл';

      await notifier.setByUser(CsmSettingKey.protocol, const CsmText('auto'));
      for (var i = 0; i < 8; i++) {
        await Future<void>.delayed(Duration.zero);
      }

      final state = container.read(csmProfileStateProvider)!;
      // Инвариант 16: отказ сети не очищает ничего.
      expect(state.writeQueue.length, 1);
      expect(
        state.settings[CsmSettingKey.protocol]?.value,
        const CsmText('auto'),
      );
      expect(core.sentWrites, isEmpty);

      // Следующая попытка уходит той же очередью.
      core.failWritesWith = '';
      final outcome = await notifier.flushWrites(nowMs: _nowMs);
      expect(outcome, CsmWriteFlushOutcome.sent);
      expect(core.sentWrites.single, <int, Object?>{1: 'auto'});
    },
  );

  test('снятый бит 3: запрос не уходит вовсе', () async {
    // Оператор записи настроек не предлагает. Слать запрос всё равно значило бы
    // дать пользователю обещание, которого панель не давала.
    final (container, core, notifier) = await _boot(_enrolled(cap: 1 << 1));

    await notifier.setByUser(CsmSettingKey.protocol, const CsmText('auto'));
    for (var i = 0; i < 8; i++) {
      await Future<void>.delayed(Duration.zero);
    }

    expect(core.sentWrites, isEmpty);
    // Локально принято и стоит в очереди: бит может вернуться.
    expect(container.read(csmProfileStateProvider)!.writeQueue.length, 1);
    expect(
      await notifier.flushWrites(nowMs: _nowMs),
      CsmWriteFlushOutcome.notOffered,
    );
  });

  test('профиль без энроллмента: писать некуда', () async {
    final (_, core, notifier) = await _boot(null);
    expect(
      await notifier.flushWrites(nowMs: _nowMs),
      CsmWriteFlushOutcome.notEnrolled,
    );
    expect(core.sentWrites, isEmpty);
  });

  test('пустая очередь ничего не отправляет', () async {
    final (_, core, notifier) = await _boot(_enrolled());
    expect(
      await notifier.flushWrites(nowMs: _nowMs),
      CsmWriteFlushOutcome.idle,
    );
    expect(core.sentWrites, isEmpty);
  });

  test('сброс уходит текстовым сентинелом для любого типа ключа', () {
    // 02-SPEC.md 7.5: CBOR null запрещён строгим профилем разбора, формы
    // ["default"] не существует, поэтому сентинел один и он текстовый.
    final want = csmWantMapFromQueue(<CsmQueuedWrite>[
      const CsmQueuedWrite(
        key: CsmSettingKey.mtu,
        op: CsmWantReset(),
        queuedMs: _nowMs,
      ),
      const CsmQueuedWrite(
        key: CsmSettingKey.killSwitch,
        op: CsmWantReset(),
        queuedMs: _nowMs,
      ),
    ]);
    expect(want[CsmSettingKey.mtu.wire], 'default');
    expect(want[CsmSettingKey.killSwitch.wire], 'default');
  });
}
