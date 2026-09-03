// Словарь настроек CSM/1, подключённый к существующим экранам.
//
// Правка настройки обязана уйти в ОБА конца: в [CoreConfig], откуда политика
// попадает в ядро на следующем `Up`, и в состояние CSM, откуда она уйдёт
// оператору очередью записи. Один конец без другого это ровно та
// однонаправленная синхронизация, из-за которой второе устройство навсегда
// показывает устаревший UI поверх правильного поведения.
//
// Значение вне закрытого словаря во второй конец не попадает вовсе (INV-11):
// `stack = auto` существует в UI и не существует на проводе.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/relay.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/features/protocol/protocol_screen.dart';
import 'package:caramba_client/features/servers/servers_screen.dart';
import 'package:caramba_client/features/settings/reconnect_banner.dart';
import 'package:caramba_client/features/settings/csm_settings_bridge.dart';
import 'package:caramba_client/features/settings/settings_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

const _pin = CsmPin(
  pid: '226e8a20f699b964',
  linkPin: '49Q8M87PK6WP9QXG3T30',
  origin: CsmPinOrigin.outOfBand,
  establishedMs: 1788300000000,
);

const _csm = CsmProfileState(pin: _pin, stage: CsmProfileStage.trusted);

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

class _FakeCore implements VpnConnection {
  _FakeCore({VpnStage stage = VpnStage.disconnected, String? activeProxy})
    : currentStatus = VpnStatus(
        stage: stage,
        connectedSince: stage == VpnStage.connected ? DateTime(2026) : null,
        mode: TunnelMode.proxy,
        mixedPort: 7890,
        activeProxy: activeProxy,
      );

  @override
  final VpnStatus currentStatus;

  @override
  Stream<VpnStatus> get status => Stream<VpnStatus>.value(currentStatus);

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

  @override
  Future<void> dispose() async {}
}

ConnectionProfile _profile(CsmProfileState? csm) => ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://sub.example/a',
  rawConfig: 'proxies: []',
  format: 'clash',
  csm: csm,
);

ProviderContainer _container(
  CsmProfileState? csm, {
  VpnConnection? core,
  List<ConnectionProfile>? seed,
}) {
  final profiles = seed ?? <ConnectionProfile>[_profile(csm)];
  final container = ProviderContainer(
    overrides: <Override>[
      // Первые кадры холодного старта проходят с ещё не прочитанным профилем,
      // и экран серверов успевает уйти в панельную ветку. Список панели в
      // тесте пустой и синхронный: сеть здесь не поднимаем.
      serversProvider.overrideWith((ref) async => const <Server>[]),
      vpnConnectionProvider.overrideWithValue(core ?? _FakeCore()),
      connectionProfilesStoreProvider.overrideWithValue(
        _Store(profiles, 'cp_1'),
      ),
    ],
  );
  addTearDown(container.dispose);
  return container;
}

/// Запись настройки асинхронна: она проходит через нотифаер профилей и
/// возвращается в провайдеры следующим кадром. Восемь кадров с запасом.
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.pump();
  }
}

void _phone(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 9000)
    ..devicePixelRatio = 2;
  addTearDown(tester.view.reset);
}

/// Кнопочный стенд: каждая кнопка дёргает один сеттер моста тем же путём,
/// каким его дёргает настоящий пикер.
class _Bench extends ConsumerWidget {
  const _Bench();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // Настоящий экран настроек смотрит на состояние CSM с первого кадра;
    // стенд обязан делать то же, иначе первая правка приходит в ещё не
    // прочитанный профиль и теряется.
    ref.watch(csmProfileStateProvider);
    return Scaffold(
      body: ListView(
        children: <Widget>[
          TextButton(
            onPressed: () => CsmSettingsBridge.setRoute(ref, 2),
            child: const Text('route'),
          ),
          TextButton(
            onPressed: () => CsmSettingsBridge.setRelay(ref, 0, Relay.defaults),
            child: const Text('relay-off'),
          ),
          TextButton(
            onPressed: () => CsmSettingsBridge.setRelay(ref, 1, Relay.defaults),
            child: const Text('relay-auto'),
          ),
          TextButton(
            onPressed: () => CsmSettingsBridge.setRelay(ref, 2, Relay.defaults),
            child: const Text('relay-country'),
          ),
          TextButton(
            onPressed: () => CsmSettingsBridge.setStack(ref, 0),
            child: const Text('stack-auto'),
          ),
          TextButton(
            onPressed: () => CsmSettingsBridge.setStack(ref, 2),
            child: const Text('stack-gvisor'),
          ),
          TextButton(
            onPressed: () => CsmSettingsBridge.setMtu(ref, 1),
            child: const Text('mtu'),
          ),
          TextButton(
            onPressed: () => CsmSettingsBridge.setDns(ref, 1),
            child: const Text('dns'),
          ),
          TextButton(
            onPressed: () => CsmSettingsBridge.setKillSwitch(ref, false),
            child: const Text('kill'),
          ),
          TextButton(
            onPressed: () =>
                CsmSettingsBridge.setSplitMode(ref, SplitMode.bypassSelected),
            child: const Text('split'),
          ),
        ],
      ),
    );
  }
}

Future<ProviderContainer> _pumpBench(
  WidgetTester tester, {
  required CsmProfileState? csm,
}) async {
  final container = _container(csm);
  await tester.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: const MaterialApp(home: _Bench()),
    ),
  );
  await _settle(tester);
  return container;
}

void main() {
  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  group('мост настроек пишет в оба конца', () {
    testWidgets('маршрутизация: UI зовёт full, провод ru-full', (tester) async {
      _phone(tester);
      final c = await _pumpBench(tester, csm: _csm);

      await tester.tap(find.text('route'));
      await _settle(tester);

      expect(c.read(coreConfigProvider).route, 2);
      final v = c.read(csmSettingsProvider).valueOf(CsmSettingKey.preset);
      expect((v! as CsmText).value, 'ru-full');
      expect(c.read(csmSettingsProvider).isUserSet(CsmSettingKey.preset), true);
    });

    testWidgets('relay: три состояния, а не два', (tester) async {
      _phone(tester);
      final c = await _pumpBench(tester, csm: _csm);

      // «Выкл» это явное «без релея», литерал `--`.
      await tester.tap(find.text('relay-off'));
      await _settle(tester);
      expect(
        (c.read(csmSettingsProvider).valueOf(CsmSettingKey.relay)! as CsmText)
            .value,
        kCsmNoRelay,
      );

      // «Авто» это «не выбрано, оператор решает», пустая строка.
      await tester.tap(find.text('relay-auto'));
      await _settle(tester);
      expect(
        (c.read(csmSettingsProvider).valueOf(CsmSettingKey.relay)! as CsmText)
            .value,
        '',
      );

      // Страна это её код.
      await tester.tap(find.text('relay-country'));
      await _settle(tester);
      final code =
          (c.read(csmSettingsProvider).valueOf(CsmSettingKey.relay)! as CsmText)
              .value;
      expect(code.length, 2);
      expect(code, code.toUpperCase());
    });

    testWidgets('stack: auto живёт в UI и не существует на проводе', (
      tester,
    ) async {
      _phone(tester);
      final c = await _pumpBench(tester, csm: _csm);

      await tester.tap(find.text('stack-auto'));
      await _settle(tester);
      expect(c.read(coreConfigProvider).stack, 0);
      // Вне закрытого словаря: во второй конец не ушло вовсе.
      expect(c.read(csmSettingsProvider).valueOf(CsmSettingKey.stack), isNull);

      await tester.tap(find.text('stack-gvisor'));
      await _settle(tester);
      expect(
        (c.read(csmSettingsProvider).valueOf(CsmSettingKey.stack)! as CsmText)
            .value,
        'gvisor',
      );
    });

    testWidgets('mtu, dns, kill-switch и режим раздельного туннелирования', (
      tester,
    ) async {
      _phone(tester);
      final c = await _pumpBench(tester, csm: _csm);

      // По одной правке за раз: пользователь тоже жмёт по одной, а две
      // записи в один кадр читают одно и то же состояние и затирают друг
      // друга.
      await tester.tap(find.text('mtu'));
      await _settle(tester);
      await tester.tap(find.text('dns'));
      await _settle(tester);
      await tester.tap(find.text('kill'));
      await _settle(tester);
      await tester.tap(find.text('split'));
      await _settle(tester);

      final s = c.read(csmSettingsProvider);
      expect((s.valueOf(CsmSettingKey.mtu)! as CsmUint).value, 1280);
      expect(
        (s.valueOf(CsmSettingKey.dnsNameservers)! as CsmTextList).value.first,
        startsWith('https://'),
      );
      expect(
        (s.valueOf(CsmSettingKey.dnsFallback)! as CsmTextList).value.first,
        startsWith('tls://'),
      );
      expect((s.valueOf(CsmSettingKey.killSwitch)! as CsmBoolean).value, false);
      expect((s.valueOf(CsmSettingKey.splitMode)! as CsmText).value, 'bypass');

      // Список приложений раздельного туннелирования ключа не имеет и
      // появиться в состоянии не может (INV-15).
      expect(s.entries.keys.length, 5);
    });

    testWidgets('профиль без CSM: пишется только конфигурация ядра', (
      tester,
    ) async {
      _phone(tester);
      final c = await _pumpBench(tester, csm: null);

      await tester.tap(find.text('route'));
      await _settle(tester);
      await tester.tap(find.text('kill'));
      await _settle(tester);

      expect(c.read(coreConfigProvider).route, 2);
      expect(c.read(coreConfigProvider).killSwitch, false);
      expect(c.read(csmSettingsProvider).entries, isEmpty);
    });
  });

  group('экран протокола', () {
    testWidgets('выбор протокола уходит и ядру, и оператору', (tester) async {
      _phone(tester);
      final container = _container(_csm);
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: const ProtocolScreen(),
          ),
        ),
      );
      await _settle(tester);

      await tester.tap(find.text('VLESS · Reality'));
      await _settle(tester);

      expect(container.read(coreConfigProvider).protocol, 2);
      final v = container
          .read(csmSettingsProvider)
          .valueOf(CsmSettingKey.protocol);
      expect((v! as CsmText).value, 'VLESS-Reality');

      // Экран сам закрывается через 300 мс; снимаем дерево заранее, чтобы
      // отложенный переход не искал роутер, которого в тесте нет.
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump(const Duration(milliseconds: 400));
    });

    testWidgets('происхождение значения оператора названо до перевыбора', (
      tester,
    ) async {
      _phone(tester);
      final container = _container(
        const CsmProfileState(
          pin: _pin,
          stage: CsmProfileStage.trusted,
          settings: CsmSettings(
            entries: <CsmSettingKey, CsmSettingEntry>{
              CsmSettingKey.protocol: CsmSettingEntry(
                value: CsmText('Hysteria2'),
                src: CsmProvenance.operator,
              ),
            },
          ),
        ),
      );
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: const ProtocolScreen(),
          ),
        ),
      );
      await _settle(tester);

      expect(
        find.textContaining('Текущее значение поставил оператор'),
        findsOneWidget,
      );
    });
  });

  group('экран серверов', () {
    ConnectionProfile subProfile(String? selectedId) => ConnectionProfile(
      id: 'cp_1',
      type: ProfileType.rawSub,
      displayName: 'Моя подписка',
      source: 'https://sub.example/a',
      rawConfig: 'proxies: []',
      format: 'clash',
      servers: const <ImportedServer>[
        ImportedServer(
          id: 'nl-1',
          name: 'Amsterdam #1',
          type: 'vless',
          server: 'a.example',
          port: 443,
          country: 'NL',
        ),
        ImportedServer(
          id: 'nl-2',
          name: 'Amsterdam #2',
          type: 'vless',
          server: 'b.example',
          port: 443,
          country: 'NL',
        ),
      ],
      selectedServerId: selectedId,
    );

    Future<void> pumpServers(
      WidgetTester tester, {
      required String? selectedId,
      required String? activeProxy,
      VpnStage stage = VpnStage.connected,
    }) async {
      final container = _container(
        null,
        core: _FakeCore(stage: stage, activeProxy: activeProxy),
        seed: <ConnectionProfile>[subProfile(selectedId)],
      );
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: const ServersScreen(),
          ),
        ),
      );
      await _settle(tester);
    }

    testWidgets('смена узла выхода при поднятом туннеле поднимает баннер', (
      tester,
    ) async {
      _phone(tester);
      await pumpServers(
        tester,
        selectedId: 'nl-1',
        activeProxy: 'Amsterdam #2',
      );

      expect(find.byType(ReconnectBanner), findsOneWidget);
      expect(
        find.text('Новые настройки применятся после переподключения.'),
        findsOneWidget,
      );
      await tester.pumpWidget(const SizedBox.shrink());
    });

    testWidgets('туннель уже на закреплённом узле: баннера нет', (
      tester,
    ) async {
      _phone(tester);
      await pumpServers(
        tester,
        selectedId: 'nl-1',
        activeProxy: 'Amsterdam #1',
      );

      expect(find.byType(ReconnectBanner), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
    });

    testWidgets('вне сессии баннера нет никогда', (tester) async {
      _phone(tester);
      await pumpServers(
        tester,
        selectedId: 'nl-1',
        activeProxy: null,
        stage: VpnStage.disconnected,
      );

      expect(find.byType(ReconnectBanner), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  });

  group('экран настроек', () {
    testWidgets('раздел проверки ведёт на все четыре экрана INV-17..INV-20', (
      tester,
    ) async {
      _phone(tester);
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(
        const MethodChannel('plugins.it_nomads.com/flutter_secure_storage'),
        (call) async => null,
      );
      addTearDown(
        () => messenger.setMockMethodCallHandler(
          const MethodChannel('plugins.it_nomads.com/flutter_secure_storage'),
          null,
        ),
      );

      final container = _container(_csm);
      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            theme: AppTheme.dark(),
            home: const SettingsScreen(),
          ),
        ),
      );
      await _settle(tester);

      expect(find.text('ПРОВЕРКА И ПРОЗРАЧНОСТЬ'), findsOneWidget);
      expect(find.text('Оператор'), findsOneWidget);
      expect(find.text('Документы'), findsOneWidget);
      expect(find.text('Транспорт'), findsOneWidget);
      expect(find.text('Что мы отправляем'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  });
}
