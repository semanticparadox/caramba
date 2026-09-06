// Стадия туннеля при запуске обязана соответствовать действительности.
//
// ВОСПРОИЗВЕДЁННАЯ ПОЛОМКА (устройство, 18:57 и 19:04). Человек нажал «Назад»
// на внутреннем экране, приложение закрылось, через ~300 мс за ним ушёл и
// туннель: «Mihomo shutting down», tun0 исчез. Процесс приложения при этом
// остался в памяти вместе с нативной шиной статуса, а в ней — последний кадр
// «connected» с моментом подъёма. Следующий запуск получал этот кадр первым же
// событием: главный экран показывал «Защищено» и идущий таймер (03:35), значка
// VPN в статус-баре не было, а выходной адрес в браузере был домашним.
//
// Приложение выдумало соединение. Здесь проверяется, что оно больше не верит
// доставшемуся по наследству кадру, а СПРАШИВАЕТ платформу — при появлении
// экрана и при возвращении из фона — и принимает ответ.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/connect_dial.dart';
import 'support/fake_csm_device.dart';

/// Ядро, чей КЭШ разошёлся с действительностью.
///
/// [currentStatus] и первый кадр потока — то, что нативная шина отдаёт новому
/// движку Flutter при подписке. [truth] — то, что она отвечает на прямой вопрос
/// о стадии. В боевой сборке эти два совпадают ровно потому, что шина больше не
/// отдаёт «connected» без живого сеанса; тест держит их врозь, чтобы проверить
/// сторону приложения.
class _StaleCore with FakeCsmDevice implements VpnConnection {
  _StaleCore({required VpnStatus cached, required this.truth})
    : _last = cached,
      _ctrl = StreamController<VpnStatus>.broadcast();

  final VpnStatus truth;
  VpnStatus _last;
  final StreamController<VpnStatus> _ctrl;

  /// Сколько раз приложение переспросило платформу.
  int asks = 0;

  @override
  VpnStatus get currentStatus => _last;

  @override
  Stream<VpnStatus> get status async* {
    yield _last;
    yield* _ctrl.stream;
  }

  @override
  Stream<TrafficStats> get traffic => const Stream<TrafficStats>.empty();

  @override
  Future<VpnStatus> refreshStatus() async {
    asks++;
    _last = truth;
    _ctrl.add(truth);
    return truth;
  }

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
  Future<void> dispose() async => _ctrl.close();
}

class _FakeProfilesStore implements ConnectionProfilesStore {
  _FakeProfilesStore(this.profiles, this.activeId);

  List<ConnectionProfile> profiles;
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
    profiles = const [];
    activeId = null;
  }
}

final _profile = ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://sub.example/a',
  rawConfig: 'proxies: [{name: DE Stealth}]',
  format: 'clash',
  servers: const <ImportedServer>[
    ImportedServer(
      id: 'de-reality',
      name: 'DE Stealth',
      type: 'vless',
      server: 'de.example',
      port: 443,
      country: 'DE',
    ),
  ],
  serversUpdatedMs: DateTime.now().millisecondsSinceEpoch,
);

/// Тот самый кадр из шины: подключено, таймер идёт с 3 м 35 с назад.
final _staleConnected = VpnStatus(
  stage: VpnStage.connected,
  connectedSince: DateTime.now().subtract(
    const Duration(minutes: 3, seconds: 35),
  ),
  mode: TunnelMode.tun,
  activeProxy: 'DE Stealth',
);

Widget _home(_StaleCore core) => ProviderScope(
  overrides: [
    vpnConnectionProvider.overrideWithValue(core),
    connectionProfilesStoreProvider.overrideWithValue(
      _FakeProfilesStore(<ConnectionProfile>[_profile], _profile.id),
    ),
  ],
  child: MaterialApp(theme: AppTheme.dark(), home: const HomeScreen()),
);

void _usePhoneView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 1688)
    ..devicePixelRatio = 2
    ..viewPadding = const FakeViewPadding(top: 94)
    ..padding = const FakeViewPadding(top: 94);
  addTearDown(tester.view.reset);
}

ConnectDial _dial(WidgetTester tester) =>
    tester.widget<ConnectDial>(find.byType(ConnectDial));

Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 5; i++) {
    await tester.pump();
  }
}

void main() {
  testWidgets('пережившее туннель «Защищено» снимается при запуске', (
    tester,
  ) async {
    _usePhoneView(tester);
    final core = _StaleCore(
      cached: _staleConnected,
      truth: const VpnStatus.disconnected(),
    );
    await tester.pumpWidget(_home(core));
    await _settle(tester);

    expect(core.asks, greaterThan(0), reason: 'экран обязан переспросить');
    expect(
      find.text('Защищено'),
      findsNothing,
      reason:
          'туннеля нет — слово «Защищено» это утверждение, а не оформление',
    );
    expect(_dial(tester).stage, VpnStage.disconnected);
    expect(find.text('Нажмите, чтобы подключиться'), findsOneWidget);
  });

  testWidgets('живой туннель вопросом не сбивается', (tester) async {
    // Обратная сторона: переспрашивать нужно ровно затем, чтобы не врать в
    // опасную сторону. Врать в безопасную — тоже врать, и защищённого человека
    // приложение не имеет права объявлять отключённым.
    _usePhoneView(tester);
    final core = _StaleCore(cached: _staleConnected, truth: _staleConnected);
    await tester.pumpWidget(_home(core));
    await _settle(tester);

    expect(core.asks, greaterThan(0));
    expect(_dial(tester).stage, VpnStage.connected);
    expect(find.text('Защищено'), findsOneWidget);
  });

  testWidgets('возвращение из фона переспрашивает заново', (tester) async {
    // Пока приложение было в фоне, туннель мог упасть, а сообщить об этом было
    // некому: движка Flutter в этот момент могло не быть вовсе.
    _usePhoneView(tester);
    final core = _StaleCore(cached: _staleConnected, truth: _staleConnected);
    await tester.pumpWidget(_home(core));
    await _settle(tester);
    final atStart = core.asks;

    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await _settle(tester);

    expect(
      core.asks,
      greaterThan(atStart),
      reason: 'после возвращения приложение обязано спросить, а не помнить',
    );
  });
}
