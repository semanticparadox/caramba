// Одновременные правки настроек не теряются, 02-SPEC.md 7.8 и инвариант 16.
//
// Нотифаер CSM читает состояние профиля целиком и целиком его возвращает. Пока
// каждая мутация укладывается в один синхронный участок, это безопасно; как
// только между чтением и записью появляется await, вторая правка, начавшаяся в
// том же кадре, пишет поверх первой и первая исчезает вместе с очередью, в
// которой она стояла. Путь слияния подписанной директивы применяет больше
// одной настройки за раз, поэтому здесь это проверяется отдельно.

import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_capability.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
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

CsmProfileState _enrolled() => const CsmProfileState(
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

/// Стор с ЗАДЕРЖКОЙ на записи. Задержка и есть предмет проверки: она
/// растягивает окно между чтением состояния и его записью настолько, чтобы
/// вторая правка успела начаться внутри него.
class _SlowStore implements ConnectionProfilesStore {
  _SlowStore(this.profiles, this.activeId);

  List<ConnectionProfile> profiles;
  String? activeId;

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;

  @override
  Future<String?> readActiveId() async => activeId;

  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async {
    await Future<void>.delayed(const Duration(milliseconds: 5));
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
  /// Пока он не завершён, подписанный запрос висит в полёте. Это и есть окно,
  /// в котором пользователь отвечает на карточку.
  Completer<void>? inFlight;

  @override
  Future<String> csmRequestSettings({
    required Map<int, Object?> want,
    Map<int, String> sel = const <int, String>{},
    String accountJwt = '',
  }) async {
    sentWrites.add(Map<int, Object?>.from(want));
    await inFlight?.future;
    return '{}';
  }

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

Future<(ProviderContainer, _FakeCore, CsmNotifier)> _boot({
  CsmProfileState? csm,
}) async {
  final core = _FakeCore();
  final profile = ConnectionProfile(
    id: 'cp_1',
    type: ProfileType.rawSub,
    displayName: 'Моя подписка',
    source: 'https://sub.example/a',
    rawConfig: 'proxies: []',
    format: 'clash',
    csm: csm ?? _enrolled(),
  );
  final container = ProviderContainer(
    overrides: <Override>[
      vpnConnectionProvider.overrideWithValue(core),
      connectionProfilesStoreProvider.overrideWithValue(
        _SlowStore(<ConnectionProfile>[profile], 'cp_1'),
      ),
    ],
  );
  addTearDown(container.dispose);
  container.read(connectionProfilesProvider);
  for (var i = 0; i < 8; i++) {
    await Future<void>.delayed(Duration.zero);
  }
  return (container, core, container.read(csmNotifierProvider));
}

void main() {
  test('две правки в одном кадре обе доживают до состояния', () async {
    final (container, _, notifier) = await _boot();

    // Ни одна не ожидается по отдельности: ровно так их выдаёт слияние
    // подписанной директивы и ровно так они наезжают друг на друга.
    await Future.wait(<Future<void>>[
      notifier.setByUser(CsmSettingKey.mtu, const CsmUint(1280)),
      notifier.setByUser(CsmSettingKey.killSwitch, const CsmBoolean(true)),
      notifier.setByUser(CsmSettingKey.ipv6, const CsmBoolean(false)),
    ]);
    for (var i = 0; i < 32; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 1));
    }

    final settings = container.read(csmProfileStateProvider)!.settings;
    expect(settings.valueOf(CsmSettingKey.mtu), const CsmUint(1280));
    expect(settings.valueOf(CsmSettingKey.killSwitch), const CsmBoolean(true));
    expect(settings.valueOf(CsmSettingKey.ipv6), const CsmBoolean(false));
    expect(settings.isUserSet(CsmSettingKey.mtu), isTrue);
    expect(settings.isUserSet(CsmSettingKey.killSwitch), isTrue);
    expect(settings.isUserSet(CsmSettingKey.ipv6), isTrue);
  });

  test('ответ на карточку во время полёта записи не пропадает', () async {
    // Профиль с висящей карточкой по killSwitch: оператор предлагает снять
    // защиту, пользователь её удерживает.
    const card = CsmPendingChange(
      id: 'card_1',
      raisedMs: 1788307400000,
      items: <CsmCardItem>[
        CsmCardItem(
          key: CsmSettingKey.killSwitch,
          current: CsmBoolean(true),
          proposed: CsmBoolean(false),
          src: CsmProvenance.operator,
          trigger: CsmCardTrigger.narrowing,
        ),
      ],
    );
    final (container, core, notifier) = await _boot(
      csm: _enrolled().copyWith(pendingChanges: const <CsmPendingChange>[card]),
    );

    // Запись уходит и зависает в полёте.
    core.inFlight = Completer<void>();
    unawaited(
      notifier.setByUser(
        CsmSettingKey.killSwitch,
        const CsmBoolean(true),
        nowMs: 1788307500000,
      ),
    );
    for (var i = 0; i < 16; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 1));
    }
    expect(core.sentWrites, hasLength(1));

    // Пока она в полёте, пользователь отвечает «Оставить моё»: по тому же
    // ключу встаёт НОВАЯ запись, которая никуда не уходила.
    await notifier.keepCard('card_1', nowMs: 1788307600000);

    core.inFlight!.complete();
    for (var i = 0; i < 32; i++) {
      await Future<void>.delayed(const Duration(milliseconds: 1));
    }

    // Снимается ровно отправленное. Ответ пользователя остаётся в очереди и
    // уйдёт следующей отдачей.
    final queue = container.read(csmProfileStateProvider)!.writeQueue;
    expect(queue.entries, hasLength(1));
    expect(queue.entries.single.key, CsmSettingKey.killSwitch);
    expect(queue.entries.single.queuedMs, 1788307600000);
  });
}
