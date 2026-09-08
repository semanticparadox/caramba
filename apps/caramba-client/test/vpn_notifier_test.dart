// VpnNotifier: что именно уходит в ядро перед поднятием туннеля.
//
// Раньше rawSub-путь хардкодил format: 'auto' и не умел закреплять узел, а
// CoreConfig до ядра не доезжал вовсе. Тест фиксирует контракт: политика и
// режим захвата отправляются ДО connect (они действуют со следующего Up), а
// rawSub несёт формат профиля и выбранный serverId.
//
// Сюда же переехали два свойства, снятые с устройства пятым за сессию случаем
// «Защищено» над мёртвым туннелем: подъём НЕ начинается раньше подтверждённой
// остановки ядра, и неудачное применение настроек не глушит признак
// «настройки изменились» навсегда.

import 'dart:async';

import 'package:flutter/services.dart'
    show MissingPluginException, PlatformException;
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

  /// Заставляет setPolicy упасть отказом ЗДЕСЬ И СЕЙЧАС: мост есть, вызов не
  /// прошёл (ядро в разборке, платформа ответила ошибкой).
  bool failPolicy = false;

  /// Заставляет setPolicy упасть отсутствием моста — сборка без этого вызова.
  bool policyBridgeMissing = false;

  /// Отвечает ли `disconnect` кадром об остановке, как отвечает живой мост
  /// (Android печатает `disconnected` следующей строкой после возврата из
  /// `core.down()`). `false` — ядро «зависло в разборке»: команда ушла, кадра
  /// нет, и это ровно тот случай, ради которого ожидание и появилось.
  bool haltsOnDisconnect = true;

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
    if (policyBridgeMissing) throw MissingPluginException('setPolicy');
    if (failPolicy) throw PlatformException(code: 'set_policy_failed');
    lastPolicy = policy;
  }

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {
    calls.add('setTunnelMode');
    lastMode = mode;
    lastMixedPort = mixedPort;
  }

  @override
  Future<void> disconnect() async {
    calls.add('disconnect');
    // Как живой мост: команда возвращает управление сразу, а кадр об остановке
    // приезжает потоком статуса отдельным оборотом цикла событий. Именно этот
    // зазор и был дырой, в которую попадал подъём.
    if (haltsOnDisconnect) emit(const VpnStatus.disconnected());
  }

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
  Duration stopLimit = kStopConfirmationLimit,
}) => VpnNotifier(
  conn,
  () => recommended,
  () => profile,
  () => _policy,
  () => mode,
  guard: guard == null ? null : () => guard,
  stopLimit: stopLimit,
);

/// Прокрутить очередь микрозадач, чтобы кадры статуса дошли до нотифаера.
Future<void> pump() => Future<void>.delayed(Duration.zero);

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

  test('сборка без setPolicy не мешает подключиться', () async {
    final conn = FakeVpnConnection()..policyBridgeMissing = true;
    final notifier = build(conn, profile: rawProfile());

    expect(await notifier.connect(), isTrue);

    expect(conn.calls, contains('connectRaw'));
    // Не знаем, что применилось, значит и про реконнект не врём.
    expect(notifier.appliedPolicyJson, isNull);
    // И не зовём переподключаться: моста нет, переподключение ничего не даст.
    expect(notifier.corePreferencesStale, isFalse);
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
    test(
      'закрытая подписка не поднимает туннель на кэше конфигурации',
      () async {
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
      },
    );

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

      expect(
        asked,
        afterStop,
        reason: 'вне сессии спрашивать некого и незачем',
      );
      guard.dispose();
    });
  });

  group('правда о стадии', () {
    test(
      'переспрос платформы снимает пережившее туннель «подключено»',
      () async {
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
      },
    );

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

  group('подъём ждёт ПОДТВЕРЖДЁННОЙ остановки', () {
    // ВОСПРОИЗВЕДЁННАЯ ПОЛОМКА. Смена типа подключения завела
    // автопереподключение, `reconnect()` делал disconnect+connect встык, и
    // подъём приходил в ещё не закончившуюся разборку mihomo: «Initial
    // configuration complete, total time: 1ms», ни одной строки «[TUN] Tun
    // adapter listening», netd — «interface tun6 not assigned to any netId».
    // Экран при этом говорил «Защищено 01:31», трафика не было вовсе.
    test('пока кадра об остановке нет, ядра подъёмом не касаются', () async {
      final conn = FakeVpnConnection()..haltsOnDisconnect = false;
      final notifier = build(conn, profile: rawProfile());
      conn.emit(const VpnStatus(stage: VpnStage.connected));
      await pump();

      final pending = notifier.reconnect();
      await pump();
      await pump();

      // Команда «опустить» ушла — и на этом всё.
      expect(conn.calls, <String>['disconnect']);

      // Ядро доложило, что оно остановлено.
      conn.emit(const VpnStatus.disconnected());

      expect(await pending, isTrue);
      expect(conn.calls, <String>[
        'disconnect',
        'setTunnelMode',
        'setPolicy',
        'connectRaw',
      ]);
    });

    test('остановка не подтвердилась — остаёмся в названном отказе', () async {
      final conn = FakeVpnConnection()..haltsOnDisconnect = false;
      final notifier = build(
        conn,
        profile: rawProfile(),
        stopLimit: const Duration(milliseconds: 30),
      );
      conn.emit(const VpnStatus(stage: VpnStage.connected));
      await pump();

      expect(await notifier.reconnect(), isFalse);

      // Ни политики, ни режима, ни подъёма: поднимать в незнание — это и есть
      // зелёный щит над мёртвым туннелем.
      expect(conn.calls, <String>['disconnect']);
      expect(notifier.state.stage, VpnStage.error);
      expect(notifier.state.detail, VpnFailureReason.stopNotConfirmed);
      expect(notifier.state.isConnected, isFalse);
    });

    test('откат на прежнюю комбинацию ждёт остановки так же', () async {
      final conn = FakeVpnConnection();
      const server = Server(id: 7, name: 'Node #7', countryCode: 'NL');
      final notifier = build(conn, recommended: server);

      await notifier.connect();
      conn.emit(const VpnStatus(stage: VpnStage.connected));
      await pump();
      expect(notifier.lastGood, isNotNull);

      conn.calls.clear();
      conn.haltsOnDisconnect = false;
      final pending = notifier.restoreLastGood();
      await pump();
      await pump();
      expect(conn.calls, <String>['disconnect']);

      conn.emit(const VpnStatus.disconnected());
      expect(await pending, isTrue);
      expect(conn.calls.last, 'connect');
    });

    test('остановленное ядро не опускают повторно и не ждут', () async {
      // Обычное подключение с холодного старта: подтверждать нечего, и цена
      // ожидания обязана остаться нулевой.
      final conn = FakeVpnConnection();
      final notifier = build(
        conn,
        profile: rawProfile(),
        // Сорвался бы порядок — тест не «упал бы», а завис бы на десять секунд.
        stopLimit: const Duration(seconds: 10),
      );

      final watch = Stopwatch()..start();
      expect(await notifier.connect(), isTrue);
      watch.stop();

      expect(conn.calls, <String>['setTunnelMode', 'setPolicy', 'connectRaw']);
      expect(
        watch.elapsed,
        lessThan(const Duration(milliseconds: 200)),
        reason: 'успешный путь не имеет права ждать того, чего не ждёт',
      );
    });

    test('ошибка ядра — тоже конец сеанса и тоже подтверждение', () async {
      final conn = FakeVpnConnection()..haltsOnDisconnect = false;
      final notifier = build(conn, profile: rawProfile());
      conn.emit(const VpnStatus(stage: VpnStage.connected));
      await pump();

      final pending = notifier.reconnect();
      await pump();
      conn.emit(const VpnStatus(stage: VpnStage.error, detail: 'core down'));

      expect(await pending, isTrue);
      expect(conn.calls, contains('connectRaw'));
    });
  });

  group('провал применения настроек виден и не теряет состояние', () {
    // ВОСПРОИЗВЕДЁННАЯ ПОЛОМКА. После автопереподключения тумблер рекламы
    // перестал поднимать баннер «Переподключить» — ни в настройках, ни на
    // Главной; реклама не резалась, пока человек не сделал disconnect/connect
    // руками. setPolicy на останавливающемся ядре падал, applied обнулялось, и
    // признак расхождения молчал НАВСЕГДА.
    test('отказ моста не обнуляет применённое и поднимает признак', () async {
      final conn = FakeVpnConnection();
      final notifier = build(conn, profile: rawProfile());

      // Первый подъём удался: применённое известно.
      await notifier.connect();
      expect(notifier.appliedPolicy, isNotNull);
      final applied = notifier.appliedPolicyJson;
      conn.emit(const VpnStatus(stage: VpnStage.connected));
      await pump();

      // Второй — с отказом setPolicy.
      conn.failPolicy = true;
      await notifier.reconnect();

      expect(notifier.corePreferencesStale, isTrue);
      expect(
        notifier.appliedPolicyJson,
        applied,
        reason:
            'ядро осталось на том, что в нём было; забыть это — потерять '
            'состояние',
      );
    });

    test('удачное применение снимает признак', () async {
      final conn = FakeVpnConnection()..failPolicy = true;
      final notifier = build(conn, profile: rawProfile());

      await notifier.connect();
      expect(notifier.corePreferencesStale, isTrue);

      conn.failPolicy = false;
      conn.emit(const VpnStatus(stage: VpnStage.connected));
      await pump();
      await notifier.reconnect();

      expect(notifier.corePreferencesStale, isFalse);
      expect(notifier.appliedPolicy, isNotNull);
    });

    test('отказ setTunnelMode не роняет подключение исключением', () async {
      // Раньше здесь не было ловли вовсе: отказ улетал наружу из connect, и
      // баннер автоматики оставался со словом «Переподключаюсь» навсегда.
      final conn = _ModeRefusingConnection();
      final notifier = build(conn, profile: rawProfile());

      expect(await notifier.connect(), isTrue);
      expect(conn.calls, contains('connectRaw'));
      expect(notifier.corePreferencesStale, isTrue);
    });

    group('решение о баннере', () {
      const applied = CorePolicy(protocol: 'Hysteria2', preset: 'ru-smart');
      // Различие ИМЕННО в настроечной половине: `preset` и `protocol`
      // относятся к пути и применяются сами, поэтому отпечаток их не видит.
      const changed = CorePolicy(
        protocol: 'Hysteria2',
        preset: 'ru-smart',
        adblock: true,
      );

      test('провал говорит «переподключитесь» даже без применённого', () {
        expect(
          settingsAwaitReconnect(
            connected: true,
            preferencesStale: true,
            applied: null,
            appliedMode: null,
            current: () => applied,
            currentMode: () => TunnelMode.tun,
          ),
          isTrue,
        );
      });

      test('сборка без моста настроек молчит и не зовёт по кругу', () {
        expect(
          settingsAwaitReconnect(
            connected: true,
            preferencesStale: false,
            applied: null,
            appliedMode: null,
            current: () => applied,
            currentMode: () => TunnelMode.tun,
          ),
          isFalse,
        );
      });

      test('расхождение настроечной половины по-прежнему видно', () {
        expect(
          settingsAwaitReconnect(
            connected: true,
            preferencesStale: false,
            applied: applied,
            appliedMode: TunnelMode.tun,
            current: () => changed,
            currentMode: () => TunnelMode.tun,
          ),
          isTrue,
        );
      });

      test('вне сессии применять нечего', () {
        expect(
          settingsAwaitReconnect(
            connected: false,
            preferencesStale: true,
            applied: applied,
            appliedMode: TunnelMode.tun,
            current: () => changed,
            currentMode: () => TunnelMode.proxy,
          ),
          isFalse,
        );
      });
    });
  });
}

/// Мост, у которого отказывает именно `setTunnelMode`.
class _ModeRefusingConnection extends FakeVpnConnection {
  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {
    calls.add('setTunnelMode');
    throw PlatformException(code: 'set_tunnel_mode_failed');
  }
}
