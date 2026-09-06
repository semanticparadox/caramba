// VpnNotifier: что именно уходит в ядро перед поднятием туннеля.
//
// Раньше rawSub-путь хардкодил format: 'auto' и не умел закреплять узел, а
// CoreConfig до ядра не доезжал вовсе. Тест фиксирует контракт: политика и
// режим захвата отправляются ДО connect (они действуют со следующего Up), а
// rawSub несёт формат профиля и выбранный serverId.

import 'dart:async';

import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/state/access_guard.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_vpn/caramba_vpn.dart' show jsonEncodePolicy;
import 'support/fake_csm_device.dart';

/// Записывает вызовы вместо разговора с платформой.
class FakeVpnConnection with FakeCsmDevice implements VpnConnection {
  final _statusCtrl = StreamController<VpnStatus>.broadcast();

  /// Порядок вызовов: политика и режим обязаны предшествовать connect.
  final List<String> calls = <String>[];

  Map<String, Object?>? lastRawArgs;
  Server? lastServer;
  CorePolicy? lastPolicy;
  TunnelMode? lastMode;
  int? lastMixedPort;

  /// Заставляет setPolicy упасть, моделируя ядро до ABI v2.
  bool failPolicy = false;

  @override
  VpnStatus get currentStatus => _current;

  VpnStatus _current = const VpnStatus.disconnected();

  /// Что ответит ПЛАТФОРМА на вопрос о подлинной стадии. Отдельно от того, что
  /// приложение помнит: расхождение между этими двумя и есть проверяемый случай.
  VpnStatus platformTruth = const VpnStatus.disconnected();

  /// Сколько раз приложение спросило платформу.
  int platformAsks = 0;

  @override
  Stream<VpnStatus> get status => _statusCtrl.stream;

  @override
  Stream<TrafficStats> get traffic => const Stream<TrafficStats>.empty();

  @override
  Future<void> connect(Server server) async {
    calls.add('connect');
    lastServer = server;
  }

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {
    calls.add('connectRaw');
    lastRawArgs = <String, Object?>{
      'raw': raw,
      'format': format,
      'label': label,
      'serverId': serverId,
    };
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
  Future<void> setPolicy(CorePolicy policy) async {
    calls.add('setPolicy');
    if (failPolicy) throw StateError('no CarambaSetPolicy in this build');
    lastPolicy = policy;
  }

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {
    calls.add('setTunnelMode');
    lastMode = mode;
    lastMixedPort = mixedPort;
  }

  @override
  Future<void> disconnect() async => calls.add('disconnect');

  /// Как нативная сторона: ответ уходит в общий поток статуса, отдельного
  /// канала для него нет.
  @override
  Future<VpnStatus> refreshStatus() async {
    platformAsks++;
    calls.add('refreshStatus');
    emit(platformTruth);
    return platformTruth;
  }

  /// Стадия «пришла от ядра»: нотифаер обязан не только показать её, но и
  /// разбудить сторожа доступа.
  void emit(VpnStatus status) {
    _current = status;
    _statusCtrl.add(status);
  }

  @override
  Future<void> dispose() async => _statusCtrl.close();
}

ConnectionProfile rawProfile({
  String format = 'clash',
  String? selectedServerId,
  String rawConfig = 'proxies: []',
}) => ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Резерв',
  source: 'https://sub.example/x',
  rawConfig: rawConfig,
  format: format,
  selectedServerId: selectedServerId,
);

const _policy = CorePolicy(protocol: 'Hysteria2', preset: 'ru-smart');

VpnNotifier build(
  FakeVpnConnection conn, {
  ConnectionProfile? profile,
  Server? recommended,
  TunnelMode mode = TunnelMode.proxy,
  AccessGuard? guard,
}) => VpnNotifier(
  conn,
  () => recommended,
  () => profile,
  () => _policy,
  () => mode,
  guard: guard == null ? null : () => guard,
);

/// Сторож с заранее известным ответом.
AccessGuard guardAnswering(
  AccessVerdict verdict, {
  Duration first = const Duration(milliseconds: 5),
}) => AccessGuard(
  check: (_) async => verdict,
  first: first,
  every: const Duration(milliseconds: 5),
);

/// Отказ оператора: дневная норма исчерпана.
final _quotaRefusal = AccessVerdict.refused(
  const AccessState(
    mayConnect: false,
    kind: AccessKind.dailyQuota,
    usedBytes: 263 * 1024 * 1024,
    limitBytes: 200 * 1024 * 1024,
    period: 'day',
  ),
  statusCode: 403,
);

void main() {
  test('rawSub connect передаёт формат профиля и закреплённый узел', () async {
    final conn = FakeVpnConnection();
    final notifier = build(
      conn,
      profile: rawProfile(format: 'v2ray', selectedServerId: 'NL-01'),
    );

    expect(await notifier.connect(), isTrue);

    expect(conn.lastRawArgs, {
      'raw': 'proxies: []',
      'format': 'v2ray',
      'label': 'Резерв',
      'serverId': 'NL-01',
    });
  });

  test(
    'без закреплённого узла serverId уходит null (авто-выбор ядра)',
    () async {
      final conn = FakeVpnConnection();
      final notifier = build(conn, profile: rawProfile());

      await notifier.connect();

      expect(conn.lastRawArgs!['serverId'], isNull);
      expect(conn.lastRawArgs!['format'], 'clash');
    },
  );

  test('политика и режим захвата уходят ДО поднятия туннеля', () async {
    final conn = FakeVpnConnection();
    final notifier = build(conn, profile: rawProfile(), mode: TunnelMode.tun);

    await notifier.connect();

    expect(conn.calls, ['setTunnelMode', 'setPolicy', 'connectRaw']);
    expect(conn.lastPolicy?.protocol, 'Hysteria2');
    expect(conn.lastMode, TunnelMode.tun);
    expect(conn.lastMixedPort, 7890);
    // Отправленную политику запоминаем: по ней UI решает, нужен ли реконнект.
    expect(notifier.appliedPolicyJson, jsonEncodePolicy(_policy));
    expect(notifier.appliedTunnelMode, TunnelMode.tun);
  });

  test(
    'панельный путь тоже применяет политику и идёт к рекомендованному узлу',
    () async {
      final conn = FakeVpnConnection();
      const server = Server(id: 7, name: 'Node #7', countryCode: 'NL');
      final notifier = build(conn, recommended: server);

      expect(await notifier.connect(), isTrue);

      expect(conn.calls, ['setTunnelMode', 'setPolicy', 'connect']);
      expect(conn.lastServer?.id, 7);
    },
  );

  test('ядро без setPolicy не мешает подключиться', () async {
    final conn = FakeVpnConnection()..failPolicy = true;
    final notifier = build(conn, profile: rawProfile());

    expect(await notifier.connect(), isTrue);

    expect(conn.calls, contains('connectRaw'));
    // Не знаем, что применилось, значит и про реконнект не врём.
    expect(notifier.appliedPolicyJson, isNull);
  });

  test('пустой rawSub-профиль даёт ошибку, а не молчаливый connect', () async {
    final conn = FakeVpnConnection();
    final notifier = build(
      conn,
      profile: const ConnectionProfile(
        id: 'cp_2',
        type: ProfileType.rawSub,
        displayName: 'Пустой',
        source: '',
      ),
    );

    expect(await notifier.connect(), isFalse);
    expect(notifier.state.stage, VpnStage.error);
    expect(conn.calls, isEmpty);
  });

  test('без профиля и без сервера подключаться не к чему', () async {
    final conn = FakeVpnConnection();
    final notifier = build(conn);

    expect(await notifier.connect(), isFalse);
    expect(notifier.state.stage, VpnStage.error);
    expect(conn.calls, isEmpty);
  });

  group('право подключаться проверяется ДО поднятия туннеля', () {
    // ВОСПРОИЗВЕДЁННАЯ ПОЛОМКА. Профиль импортирован, пока подписка была
    // здорова; трафик кончился; ядро в raw-режиме не спрашивает никого и
    // поднимает туннель из кэша. Туннель при этом исправен — а через него не
    // проходит ничего, и экран говорит «Защищено» с идущим таймером.
    test('закрытая подписка не поднимает туннель на кэше конфигурации', () async {
      final conn = FakeVpnConnection();
      final guard = guardAnswering(_quotaRefusal);
      final notifier = build(
        conn,
        profile: rawProfile(rawConfig: 'proxies: [{name: DE-01}]'),
        guard: guard,
      );

      expect(await notifier.connect(), isFalse);

      // Ядра не касались вовсе: ни политики, ни режима, ни connectRaw.
      expect(conn.calls, isEmpty);
      expect(conn.lastRawArgs, isNull);
      // И это не зелёный щит.
      expect(notifier.state.stage, VpnStage.error);
      expect(notifier.state.isConnected, isFalse);
      // Причина названа, и код, по которому её узнали, остался уликой.
      expect(notifier.state.detail, contains('403'));
      expect(notifier.state.detail, contains('Дневной лимит израсходован'));
      // Экран берёт человеческий текст из состояния, которое записал сторож.
      expect(guard.state.refusal!.kind, AccessKind.dailyQuota);
      guard.dispose();
    });

    test('панельный путь закрытая подписка тоже не поднимает', () async {
      final conn = FakeVpnConnection();
      final guard = guardAnswering(_quotaRefusal);
      const server = Server(id: 7, name: 'Node #7', countryCode: 'NL');
      final notifier = build(conn, recommended: server, guard: guard);

      expect(await notifier.connect(), isFalse);

      expect(conn.calls, isEmpty);
      expect(notifier.state.stage, VpnStage.error);
      guard.dispose();
    });

    test('открытая подписка подключается как раньше', () async {
      final conn = FakeVpnConnection();
      final guard = guardAnswering(AccessVerdict.allowed());
      final notifier = build(conn, profile: rawProfile(), guard: guard);

      expect(await notifier.connect(), isTrue);

      expect(conn.calls, ['setTunnelMode', 'setPolicy', 'connectRaw']);
      guard.dispose();
    });

    test('молчащая сеть подключаться НЕ запрещает', () async {
      // Отказать по неответу значило бы запретить VPN ровно там, где сеть
      // плохая, то есть там, где он и нужен. Судить о туннеле будет сам
      // туннель.
      final conn = FakeVpnConnection();
      final guard = guardAnswering(AccessVerdict.unknown);
      final notifier = build(conn, profile: rawProfile(), guard: guard);

      expect(await notifier.connect(), isTrue);

      expect(conn.calls, contains('connectRaw'));
      expect(notifier.state.stage, isNot(VpnStage.error));
      guard.dispose();
    });

    test('живой туннель будит сторожа, оборванный — усыпляет', () async {
      var asked = 0;
      final conn = FakeVpnConnection();
      final guard = AccessGuard(
        check: (_) async {
          asked++;
          return AccessVerdict.unknown;
        },
        first: const Duration(milliseconds: 5),
        every: const Duration(milliseconds: 5),
      );
      build(conn, profile: rawProfile(), guard: guard);

      conn.emit(const VpnStatus(stage: VpnStage.connected));
      await Future<void>.delayed(const Duration(milliseconds: 40));
      expect(asked, greaterThan(0), reason: 'сессия обязана проверяться');

      conn.emit(const VpnStatus.disconnected());
      await Future<void>.delayed(const Duration(milliseconds: 20));
      final afterStop = asked;
      await Future<void>.delayed(const Duration(milliseconds: 20));

      expect(asked, afterStop, reason: 'вне сессии спрашивать некого и незачем');
      guard.dispose();
    });
  });

  group('правда о стадии', () {
    test('переспрос платформы снимает пережившее туннель «подключено»', () async {
      // Кадр из нативного кэша: он пережил и туннель, и прошлый запуск
      // приложения. Пока его не переспросили, приложение утверждает защиту.
      final conn = FakeVpnConnection();
      final notifier = build(conn, profile: rawProfile());
      conn.emit(
        VpnStatus(
          stage: VpnStage.connected,
          connectedSince: DateTime.now().subtract(
            const Duration(minutes: 3, seconds: 35),
          ),
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(notifier.state.stage, VpnStage.connected);

      // Платформа знает правду: сеанса нет.
      conn.platformTruth = const VpnStatus.disconnected();
      await notifier.refreshStage();
      await Future<void>.delayed(Duration.zero);

      expect(conn.platformAsks, 1);
      expect(notifier.state.stage, VpnStage.disconnected);
      expect(
        notifier.state.connectedSince,
        isNull,
        reason: 'таймер не имеет права идти от подъёма, которого больше нет',
      );
    });

    test('подтверждённый туннель переспросом не рвётся', () async {
      final conn = FakeVpnConnection();
      final notifier = build(conn, profile: rawProfile());
      const live = VpnStatus(stage: VpnStage.connected);
      conn.emit(live);
      conn.platformTruth = live;

      await notifier.refreshStage();
      await Future<void>.delayed(Duration.zero);

      expect(notifier.state.stage, VpnStage.connected);
    });
  });
}
