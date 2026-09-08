// Значок в строке меню: что он показывает и куда ведут его пункты.
//
// Проверяется ровно то, ради чего значок и существует: при спрятанном окне он
// единственное, что от приложения видно, и его картинка обязана отвечать на
// вопрос «работает или нет» без открытия меню, а каждый пункт обязан доехать до
// настоящего действия, а не до соседнего.
//
// Плагин `tray_manager` в тестах не поднимается: всё идёт через TrayPort, сюда
// подставлен FakeTrayPort из самого порта.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/desktop/ports/tray_port.dart';
import 'package:caramba_client/desktop/tray_menu_model.dart';
import 'package:caramba_client/desktop/tray_service.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

import '../support/fake_csm_device.dart';

/// Хранилище профилей в памяти: тесту не нужен платформенный keychain.
class _MemoryStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles = <ConnectionProfile>[];
  String? activeId;

  @override
  Future<List<ConnectionProfile>> readProfiles() async => profiles;

  @override
  Future<String?> readActiveId() async => activeId;

  @override
  Future<void> writeProfiles(List<ConnectionProfile> next) async =>
      profiles = next;

  @override
  Future<void> writeActiveId(String? id) async => activeId = id;

  @override
  Future<void> clear() async {
    profiles = <ConnectionProfile>[];
    activeId = null;
  }
}

/// Ядро-заглушка: отдаёт ровно те кадры, которые ему велели отдать.
class _FakeCore with FakeCsmDevice implements VpnConnection {
  final StreamController<VpnStatus> _ctrl =
      StreamController<VpnStatus>.broadcast();

  VpnStatus _current = const VpnStatus.disconnected();

  int disconnects = 0;

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
  }) async {}

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
    disconnects++;
    emit(const VpnStatus.disconnected());
  }

  @override
  Future<VpnStatus> refreshStatus() async => _current;

  @override
  Future<void> dispose() async => _ctrl.close();

  void emit(VpnStatus status) {
    _current = status;
    _ctrl.add(status);
  }
}

/// Прокрутить очередь, чтобы кадры статуса и `unawaited`-вызовы доехали.
Future<void> pump() => Future<void>.delayed(Duration.zero);

/// Импортированная подписка: две машины, у одной свой замер.
///
/// Импорт, а не панель, намеренно: у него нет ни одного похода в сеть, и
/// снимок для меню собирается из того, что уже лежит в профиле.
ConnectionProfile _rawProfile() => const ConnectionProfile(
  id: 'cp_raw',
  type: ProfileType.rawSub,
  displayName: 'Импорт',
  source: 'https://sub.example/x',
  servers: <ImportedServer>[
    ImportedServer(
      id: 'nl-vless',
      name: 'NL vless',
      type: 'vless',
      server: 'a.example',
      port: 443,
      country: 'NL',
    ),
    ImportedServer(
      id: 'de-vless',
      name: 'DE vless',
      type: 'vless',
      server: 'b.example',
      port: 443,
      country: 'DE',
    ),
  ],
  lastProbe: ProbeSnapshot(
    latencyMs: <String, int>{'nl-vless': 42, 'de-vless': 120},
    updatedMs: 1,
  ),
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late FakeTrayPort port;
  late _FakeCore core;
  late ProviderContainer container;
  late List<String> actions;

  /// Поднимает контейнер с профилями и фейковым ядром.
  Future<void> boot({List<ConnectionProfile> profiles = const []}) async {
    final store = _MemoryStore()
      ..profiles = profiles
      ..activeId = profiles.isEmpty ? null : profiles.first.id;
    container = ProviderContainer(
      overrides: <Override>[
        connectionProfilesStoreProvider.overrideWithValue(store),
        vpnConnectionProvider.overrideWithValue(core),
      ],
    );
    addTearDown(container.dispose);
    // Нотифаер грузится из стора асинхронно: без ожидания первое чтение
    // придётся на пустой список и «подключений нет» окажется случайной правдой.
    container.read(connectionProfilesProvider);
    for (var i = 0; i < 100; i++) {
      if (!container.read(connectionProfilesProvider).loading) break;
      await pump();
    }
  }

  /// Сервис на фейковом значке. Действия записываем строкой: проверяется, что
  /// клик доехал до НУЖНОГО действия, а не что оно сработало.
  TrayService build({
    TargetPlatform platform = TargetPlatform.macOS,
    Duration debounce = const Duration(milliseconds: 10),
  }) {
    final service = TrayService(
      port: port,
      platform: platform,
      readInput: () => container.read(trayMenuInputProvider),
      listenChanges: (onChange) {
        final sub = container.listen<TrayMenuInput>(
          trayMenuInputProvider,
          (_, __) => onChange(),
        );
        return sub.close;
      },
      toggleConnection: () => container.read(vpnProvider.notifier).toggle(),
      selectExit: (key) async => actions.add('selectExit($key)'),
      selectAuto: () async => actions.add('selectAuto'),
      activateProfile: (id) async => actions.add('activateProfile($id)'),
      showWindow: () async => actions.add('show'),
      toggleWindow: () async => actions.add('toggleWindow'),
      openRoute: (route) async => actions.add('open($route)'),
      copyText: (text) async => actions.add('copy($text)'),
      quitApplication: () async => actions.add('quit'),
      rebuildDebounce: debounce,
    );
    addTearDown(service.dispose);
    return service;
  }

  setUp(() {
    port = FakeTrayPort();
    core = _FakeCore();
    actions = <String>[];
  });

  group('значок', () {
    test('стартует с картинкой и подсказкой до всякого окна', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build().start();

      expect(port.iconPath, 'assets/tray/mac/idle.png');
      expect(port.tooltip, 'Caramba Connect · Отключено');
      expect(port.menu, isNotNull);
      expect(
        port.calls.first,
        'setIcon(assets/tray/mac/idle.png)',
        reason: 'пустая строка меню читается как «не запустилось»',
      );
    });

    test('на macOS картинка отдаётся системе шаблоном', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build(platform: TargetPlatform.macOS).start();

      expect(port.iconIsTemplate, isTrue);
    });

    test('на Windows шаблона нет: цвет несёт сама картинка', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build(platform: TargetPlatform.windows).start();

      expect(port.iconIsTemplate, isFalse);
      expect(port.iconPath, 'assets/tray/win/idle.ico');
    });

    test('стадия меняет картинку', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      final service = build();
      await service.start();

      core.emit(const VpnStatus(stage: VpnStage.connecting));
      await pump();
      await service.refresh();
      expect(port.iconPath, 'assets/tray/mac/busy.png');

      core.emit(const VpnStatus(stage: VpnStage.connected));
      await pump();
      await service.refresh();
      expect(port.iconPath, 'assets/tray/mac/connected.png');

      core.emit(const VpnStatus(stage: VpnStage.error));
      await pump();
      await service.refresh();
      expect(port.iconPath, 'assets/tray/mac/error.png');
    });

    test('картинка не переставляется, пока состояние то же', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      final service = build();
      await service.start();

      core.emit(const VpnStatus(stage: VpnStage.connecting));
      await pump();
      await service.refresh();
      core.emit(const VpnStatus(stage: VpnStage.reconnecting));
      await pump();
      await service.refresh();

      // Обе стадии рисуются одним файлом, и второй проход канала не нужен.
      expect(
        port.calls.where((c) => c.startsWith('setIcon')).length,
        2,
        reason: 'первый заход плюс переход в busy, и всё',
      );
    });

    test('кадры ядра доезжают до меню сами, одной пересборкой', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build(debounce: const Duration(milliseconds: 15)).start();
      final before = port.calls.where((c) => c == 'setContextMenu').length;

      // Один жест шевелит несколько источников подряд: пауза обязана собрать
      // их в одну пересборку.
      core.emit(const VpnStatus(stage: VpnStage.connecting));
      core.emit(const VpnStatus(stage: VpnStage.connected));
      await Future<void>.delayed(const Duration(milliseconds: 60));

      final after = port.calls.where((c) => c == 'setContextMenu').length;
      expect(after - before, 1);
      expect(port.iconPath, 'assets/tray/mac/connected.png');
    });

    test('dispose убирает значок из строки меню', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      final service = build();
      await service.start();

      await service.dispose();

      expect(port.destroyed, isTrue);
      expect(port.listener, isNull);
    });
  });

  group('клики по значку', () {
    test('на macOS левый клик открывает меню', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build(platform: TargetPlatform.macOS).start();

      port.tapLeft();

      expect(port.popUps, 1);
      expect(actions, isEmpty);
    });

    test('на Windows левый клик показывает или прячет окно', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build(platform: TargetPlatform.windows).start();

      port.tapLeft();
      port.tapRight();

      expect(actions, <String>['toggleWindow']);
      expect(port.popUps, 1, reason: 'меню открывает правый клик');
    });

    test('на Linux меню показывает сама панель', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build(platform: TargetPlatform.linux).start();

      port.tapLeft();
      port.tapRight();

      expect(port.popUps, 0);
      expect(actions, isEmpty);
    });
  });

  group('пункты меню', () {
    test('главное действие доезжает до ядра', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build().start();
      core.emit(const VpnStatus(stage: VpnStage.connected));
      await pump();

      port.tapItem(TrayKeys.toggle);
      await pump();

      expect(core.disconnects, 1);
    });

    test('узел уходит в выбор со своим ключом', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build().start();

      port.tapItem(TrayKeys.server('nl-vless'));
      port.tapItem(TrayKeys.serverAuto);

      expect(actions, <String>['selectExit(nl-vless)', 'selectAuto']);
    });

    test('профиль уходит в активацию со своим id', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build().start();

      port.tapItem(TrayKeys.profile('cp_raw'));

      expect(actions, <String>['activateProfile(cp_raw)']);
    });

    test('окно, настройки и серверы открывают своё', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build().start();

      port.tapItem(TrayKeys.openWindow);
      port.tapItem(TrayKeys.settings);
      port.tapItem(TrayKeys.allServers);
      port.tapItem(TrayKeys.addConnection);

      expect(actions, <String>[
        'show',
        'open(${AppRoute.settings})',
        'open(${AppRoute.servers})',
        'open(${AppRoute.connectionImport})',
      ]);
    });

    test('адрес прокси кладётся в буфер', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build().start();
      core.emit(
        const VpnStatus(
          stage: VpnStage.connected,
          mode: TunnelMode.proxy,
          mixedPort: 7890,
        ),
      );
      await pump();

      port.tapItem(TrayKeys.proxy);
      await pump();

      expect(actions, <String>['copy(127.0.0.1:7890)']);
    });

    test('вне сессии копировать нечего', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build().start();

      port.tapItem(TrayKeys.proxy);
      await pump();

      expect(actions, isEmpty);
    });

    test('выход уходит в общий путь выхода', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build().start();

      port.tapItem(TrayKeys.quit);

      expect(actions, <String>['quit']);
    });

    test('заголовок состояния кликом ничего не запускает', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      await build().start();

      port.tapItem(TrayKeys.servers);
      port.tapItem('что-то чужое');

      expect(actions, isEmpty);
      expect(core.disconnects, 0);
    });
  });

  group('снимок состояния для меню', () {
    test('импорт даёт машины с ключом узла и своей задержкой', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);

      final input = container.read(trayMenuInputProvider);

      expect(input.noConnections, isFalse);
      expect(input.stage, VpnStage.disconnected);
      expect(input.autoSelected, isTrue);
      expect(
        input.exits.map((e) => e.key),
        containsAll(<String>['nl-vless', 'de-vless']),
      );
      final nl = input.exits.firstWhere((e) => e.key == 'nl-vless');
      expect(nl.code, 'NL');
      expect(nl.latencyMs, 42);
      expect(nl.available, isTrue);
    });

    test('без профилей говорит «подключений нет»', () async {
      await boot();

      final input = container.read(trayMenuInputProvider);

      expect(input.noConnections, isTrue);
      expect(input.exits, isEmpty);
      expect(input.profiles, isEmpty);
    });

    test('единственный профиль назван активным', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);

      final input = container.read(trayMenuInputProvider);

      expect(input.profiles, hasLength(1));
      expect(input.profiles.single.id, 'cp_raw');
      expect(input.profiles.single.active, isTrue);
    });

    test('прокси приезжает только в proxy-режиме', () async {
      await boot(profiles: <ConnectionProfile>[_rawProfile()]);
      core.emit(
        const VpnStatus(
          stage: VpnStage.connected,
          mode: TunnelMode.proxy,
          mixedPort: 7890,
        ),
      );
      await pump();

      expect(
        container.read(trayMenuInputProvider).proxyEndpoint,
        '127.0.0.1:7890',
      );

      core.emit(const VpnStatus(stage: VpnStage.connected));
      await pump();

      expect(container.read(trayMenuInputProvider).proxyEndpoint, isNull);
    });
  });
}
