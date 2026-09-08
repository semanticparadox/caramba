// «Защищено» опирается на признак, которого приложение не может себе выдумать.
//
// ПРЕДЫСТОРИЯ. Стадия туннеля целиком принадлежала Go-ядру: оно объявляло себя
// подключённым, приложение это повторяло, и сверить утверждение было не с чем —
// весь путь от mihomo до слова «Защищено» состоял из высказываний одного
// источника. Ядро ошибалось: `executor.Shutdown()` у mihomo глобален на процесс,
// и закрытие служебного ядра валило чужой живой туннель, оставляя поднявший его
// движок в StateConnected. Проверка на устройстве показала и второй вид того же
// класса: подъём БЕЗ TUN-адаптера (в журнале нет «[TUN] Tun adapter listening»,
// у tun3 rx_bytes=0 и растущий tx_drop) — экран уверенно показывал «Защищено
// 03:37» при полностью мёртвой сети.
//
// Теперь под щитом лежит наблюдение платформы ([TunnelWitness]): сеть с
// транспортом VPN — ровно то, чем определяется значок в статус-баре, — плюс
// локальный TUN-интерфейс.
//
// ВТОРАЯ ПОЛОВИНА, КОТОРАЯ ВАЖНЕЕ ПЕРВОЙ. Ошибка в другую сторону
// («Отключено» на живом туннеле) опаснее исходного дефекта: человек полезет
// включать защиту заново, оборвёт работающий туннель и в промежутке выпустит
// трафик открытым. Поэтому вето срабатывает ТОЛЬКО на положительный ответ
// «VPN-транспорта нет», и половина проверок ниже — про молчание.

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
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/connect_dial.dart';
import 'support/fake_csm_device.dart';

/// Ядро, которое говорит про себя ровно то, что ему велели.
class _Core with FakeCsmDevice implements VpnConnection {
  _Core(this._last) : _ctrl = StreamController<VpnStatus>.broadcast();

  VpnStatus _last;
  final StreamController<VpnStatus> _ctrl;

  @override
  VpnStatus get currentStatus => _last;

  @override
  Stream<VpnStatus> get status async* {
    yield _last;
    yield* _ctrl.stream;
  }

  @override
  Stream<TrafficStats> get traffic => const Stream<TrafficStats>.empty();

  /// Новый кадр от «ядра» — тот же путь, которым приходят живые события.
  void emit(VpnStatus s) {
    _last = s;
    _ctrl.add(s);
  }

  @override
  Future<VpnStatus> refreshStatus() async {
    _ctrl.add(_last);
    return _last;
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

/// Кадр «подключено» с идущим таймером — тот самый, что рисовал щит.
VpnStatus _connected(TunnelWitness witness) => VpnStatus(
  stage: VpnStage.connected,
  connectedSince: DateTime.now().subtract(const Duration(minutes: 3)),
  mode: TunnelMode.tun,
  activeProxy: 'DE Stealth',
  witness: witness,
);

Widget _home(_Core core) => ProviderScope(
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
  group('вето: система говорит «VPN нет»', () {
    test('«подключено» с наблюдаемым отсутствием защитой не считается', () {
      final vetoed = witnessedStatus(_connected(TunnelWitness.absent));
      expect(vetoed.stage, VpnStage.error);
      expect(vetoed.detail, VpnFailureReason.noVpnTransport);
      expect(
        vetoed.connectedSince,
        isNull,
        reason: 'таймер сессии над несуществующей защитой — та же неправда',
      );
    });

    test('«переподключаемся» тоже утверждает защиту и тоже снимается', () {
      // reconnecting держит щит на экране ровно так же, как connected.
      const s = VpnStatus(
        stage: VpnStage.reconnecting,
        witness: TunnelWitness.absent,
      );
      expect(witnessedStatus(s).stage, VpnStage.error);
    });

    testWidgets('Home не говорит «Защищено», когда VPN-транспорта нет', (
      tester,
    ) async {
      _usePhoneView(tester);
      final core = _Core(_connected(TunnelWitness.absent));
      await tester.pumpWidget(_home(core));
      await _settle(tester);

      expect(
        find.text('Защищено'),
        findsNothing,
        reason:
            'значка VPN в статус-баре нет, транспорта нет — слово «Защищено» '
            'это утверждение, а не оформление',
      );
      expect(_dial(tester).stage, VpnStage.error);
      expect(find.text('Система не видит VPN-подключения'), findsOneWidget);
    });

    testWidgets('щит снимается и на кадре, пришедшем посреди сессии', (
      tester,
    ) async {
      // Туннель был жив и умер под приложением: сервис остался, ядро продолжает
      // отвечать «подключено», а транспорт исчез.
      _usePhoneView(tester);
      final core = _Core(_connected(TunnelWitness.present));
      await tester.pumpWidget(_home(core));
      await _settle(tester);
      expect(find.text('Защищено'), findsOneWidget);

      core.emit(_connected(TunnelWitness.absent));
      await _settle(tester);

      expect(find.text('Защищено'), findsNothing);
      expect(_dial(tester).stage, VpnStage.error);
    });
  });

  group('молчание — не приговор', () {
    test('«не знаю» щит не гасит', () {
      final s = _connected(TunnelWitness.unknown);
      expect(witnessedStatus(s).stage, VpnStage.connected);
      expect(witnessedStatus(s).connectedSince, s.connectedSince);
    });

    test('наблюдаемое присутствие ничего не меняет', () {
      final s = _connected(TunnelWitness.present);
      expect(witnessedStatus(s).stage, VpnStage.connected);
    });

    test('снимок без наблюдения проходит нетронутым', () {
      // Мост без наблюдения (iOS, desktop, FFI-путь, мок, сборка старее этой
      // правки) не присылает поля вовсе. Гасить по его молчанию значило бы
      // выключить защиту на всех платформах разом.
      const s = VpnStatus(stage: VpnStage.connected);
      expect(s.witness, TunnelWitness.unknown);
      expect(witnessedStatus(s).stage, VpnStage.connected);
    });

    testWidgets('Home держит щит, пока признак недоступен', (tester) async {
      _usePhoneView(tester);
      final core = _Core(_connected(TunnelWitness.unknown));
      await tester.pumpWidget(_home(core));
      await _settle(tester);

      expect(
        find.text('Защищено'),
        findsOneWidget,
        reason:
            'объявить защищённого человека отключённым хуже, чем не заметить '
            'разрыв: он полезет включать заново и оборвёт живой туннель',
      );
      expect(_dial(tester).stage, VpnStage.connected);
    });

    testWidgets('Home держит щит и при наблюдаемом присутствии', (
      tester,
    ) async {
      _usePhoneView(tester);
      final core = _Core(_connected(TunnelWitness.present));
      await tester.pumpWidget(_home(core));
      await _settle(tester);

      expect(find.text('Защищено'), findsOneWidget);
      expect(_dial(tester).stage, VpnStage.connected);
    });

    test('стадии, которые ничего не обещают, вето не касается', () {
      for (final stage in const [
        VpnStage.disconnected,
        VpnStage.connecting,
        VpnStage.error,
      ]) {
        final s = VpnStatus(stage: stage, witness: TunnelWitness.absent);
        expect(
          witnessedStatus(s).stage,
          stage,
          reason:
              '$stage защиты не утверждает — переписывать её нечем и незачем',
        );
      }
    });
  });

  group('названные причины читаются как поломка, а не как «сеть подвела»', () {
    test('обе причины клиента переведены', () {
      expect(
        clientFailureText(VpnFailureReason.noVpnTransport),
        'Система не видит VPN-подключения',
      );
      expect(
        clientFailureText(VpnFailureReason.tunNotServiced),
        'Туннель поднялся без адаптера и опущен',
      );
    });

    test('чужая причина остаётся общему переводчику', () {
      expect(clientFailureText(null), isNull);
      expect(clientFailureText('transport: код состояния 403'), isNull);
    });

    test('подпись под дайлом называет причину, а не советует чинить сеть', () {
      expect(
        dialErrorLabel(detail: VpnFailureReason.noVpnTransport),
        'Система не видит VPN-подключения',
      );
      expect(
        dialErrorLabel(detail: VpnFailureReason.tunNotServiced),
        'Туннель поднялся без адаптера и опущен',
      );
    });
  });
}
