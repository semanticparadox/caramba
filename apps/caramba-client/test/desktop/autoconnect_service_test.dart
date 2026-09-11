// Автоподключение при запуске.
//
// Тумблер «Автоподключение» до этого не читал никто. Здесь проверяется, что
// он исполняется: один раз за процесс, только когда есть к чему подключаться,
// и никогда, если выключен.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/desktop/autoconnect_service.dart';
import 'package:caramba_client/state/access_guard.dart';
import 'package:caramba_client/state/bootstrap_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

import '../support/fake_csm_device.dart';

/// Ядро, которое считает подъёмы и отвечает кадром «подключено».
class _Core with FakeCsmDevice implements VpnConnection {
  final StreamController<VpnStatus> _ctrl =
      StreamController<VpnStatus>.broadcast();
  VpnStatus _current = const VpnStatus.disconnected();
  int rawConnects = 0;

  @override
  VpnStatus get currentStatus => _current;

  @override
  Stream<VpnStatus> get status => _ctrl.stream;

  @override
  Stream<TrafficStats> get traffic => const Stream<TrafficStats>.empty();

  @override
  Future<void> connect(Server server) async {}

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {
    rawConnects++;
    _current = const VpnStatus(stage: VpnStage.connected);
    _ctrl.add(_current);
  }

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async => ImportResult.empty;

  @override
  Future<List<ProbeResult>> probe({
    Duration timeout = const Duration(seconds: 5),
  }) async => const <ProbeResult>[];

  @override
  Future<void> setPolicy(CorePolicy policy) async {}

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {}

  @override
  Future<void> disconnect() async {
    _current = const VpnStatus.disconnected();
    _ctrl.add(_current);
  }

  @override
  Future<VpnStatus> refreshStatus() async => _current;

  @override
  Future<void> dispose() async => _ctrl.close();
}

class _Store implements ConnectionProfilesStore {
  _Store(this.profiles);

  List<ConnectionProfile> profiles;

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;

  @override
  Future<String?> readActiveId() async => profiles.firstOrNull?.id;

  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async =>
      profiles = next;

  @override
  Future<void> writeActiveId(String? id) async {}

  @override
  Future<void> clear() async => profiles = const <ConnectionProfile>[];
}

const ConnectionProfile _raw = ConnectionProfile(
  id: 'cp_raw',
  type: ProfileType.rawSub,
  displayName: 'Подписка',
  source: 'https://example.test/sub',
  rawConfig: 'proxies: []',
);

Future<void> pump() => Future<void>.delayed(Duration.zero);

AutoConnectInput _input({
  bool bootReady = true,
  bool wanted = true,
  bool profilesLoading = false,
  bool hasProfile = true,
  bool profileIsRaw = true,
  bool hasRecommendedServer = false,
  VpnStage stage = VpnStage.disconnected,
}) => AutoConnectInput(
  bootReady: bootReady,
  wanted: wanted,
  profilesLoading: profilesLoading,
  hasProfile: hasProfile,
  profileIsRaw: profileIsRaw,
  hasRecommendedServer: hasRecommendedServer,
  stage: stage,
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('autoConnectReady', () {
    test('сырой профиль готов без сервера панели', () {
      expect(autoConnectReady(_input()), isTrue);
    });

    test('выключенный тумблер это никогда', () {
      expect(autoConnectReady(_input(wanted: false)), isFalse);
    });

    test('ждёт настройки и профили', () {
      expect(autoConnectReady(_input(bootReady: false)), isFalse);
      expect(autoConnectReady(_input(profilesLoading: true)), isFalse);
      expect(autoConnectReady(_input(hasProfile: false)), isFalse);
    });

    test('панельный профиль ждёт рекомендованный сервер', () {
      expect(autoConnectReady(_input(profileIsRaw: false)), isFalse);
      expect(
        autoConnectReady(
          _input(profileIsRaw: false, hasRecommendedServer: true),
        ),
        isTrue,
      );
    });

    test('поднятый или поднимающийся туннель не трогается', () {
      for (final stage in const <VpnStage>[
        VpnStage.connecting,
        VpnStage.connected,
        VpnStage.reconnecting,
        VpnStage.error,
      ]) {
        expect(autoConnectReady(_input(stage: stage)), isFalse);
      }
    });
  });

  group('AutoConnectService', () {
    test('подключается, когда вход становится готов, и только раз', () async {
      var input = _input(profilesLoading: true);
      final listeners = <VoidCallback>[];
      var connects = 0;
      final service = AutoConnectService(
        readInput: () => input,
        listenInput: (onChange) {
          listeners.add(onChange);
          return () => listeners.remove(onChange);
        },
        connect: () async => connects++,
      );

      service.start();
      expect(connects, 0, reason: 'профили ещё грузятся');

      input = _input();
      for (final l in listeners.toList()) {
        l();
      }
      expect(connects, 1);
      expect(service.fired, isTrue);
      expect(listeners, isEmpty, reason: 'выстрелив, подписку снимает');

      // Повторные движения входа второго подъёма не дают.
      service.start();
      expect(connects, 1);
    });

    test('через провайдеры: сырой профиль поднимается сам', () async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final core = _Core();
      final container = ProviderContainer(
        overrides: <Override>[
          vpnConnectionProvider.overrideWithValue(core),
          connectionProfilesStoreProvider.overrideWithValue(
            _Store(<ConnectionProfile>[_raw]),
          ),
          // Сеть в тесте не поднимаем: сторожу доступа отдаётся готовый
          // вердикт, как в home_access_truth_test.
          accessGuardProvider.overrideWith(
            (ref) => AccessGuard(
              check: (_) async => AccessVerdict.unknown,
              initial: AccessVerdict.unknown,
              first: const Duration(days: 1),
              every: const Duration(days: 1),
            ),
          ),
        ],
      );
      addTearDown(container.dispose);
      await container.read(appBootProvider.future);
      container.read(coreConfigProvider.notifier).setAutoConnect(true);

      container.read(autoConnectServiceProvider).start();
      // Профили читаются из стора своим оборотом.
      container.read(connectionProfilesProvider);
      for (var i = 0; i < 5; i++) {
        await pump();
      }

      expect(core.rawConnects, 1);
      expect(container.read(vpnProvider).stage, VpnStage.connected);

      // Человек отключился: автоподключение не возвращает туннель.
      await container.read(vpnProvider.notifier).disconnect();
      for (var i = 0; i < 3; i++) {
        await pump();
      }
      expect(core.rawConnects, 1);
    });

    test('через провайдеры: выключенный тумблер ничего не поднимает', () async {
      SharedPreferences.setMockInitialValues(<String, Object>{});
      final core = _Core();
      final container = ProviderContainer(
        overrides: <Override>[
          vpnConnectionProvider.overrideWithValue(core),
          connectionProfilesStoreProvider.overrideWithValue(
            _Store(<ConnectionProfile>[_raw]),
          ),
          accessGuardProvider.overrideWith(
            (ref) => AccessGuard(
              check: (_) async => AccessVerdict.unknown,
              initial: AccessVerdict.unknown,
              first: const Duration(days: 1),
              every: const Duration(days: 1),
            ),
          ),
        ],
      );
      addTearDown(container.dispose);
      await container.read(appBootProvider.future);

      container.read(autoConnectServiceProvider).start();
      container.read(connectionProfilesProvider);
      for (var i = 0; i < 5; i++) {
        await pump();
      }

      expect(core.rawConnects, 0);
    });
  });
}
