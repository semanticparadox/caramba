// Home в generic-режиме (своя подписка, аккаунта панели нет).
//
// Ключевое свойство ветки — она НЕ ходит в панель: ни подписки, ни рекомендаций
// сервера, ни истории трафика, ни колокола уведомлений. Данные берутся с
// активного профиля подключения и из потока статистики ядра. Здесь проверяется
// именно состав экрана: панельные элементы отсутствуют, generic-строки есть.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/connect_dial.dart';

/// Ядро, поднятое в proxy-режиме на конкретном узле: ровно те поля ABI v2,
/// которые generic-ветка Home показывает вместо панельных.
class _FakeCore implements VpnConnection {
  @override
  final VpnStatus currentStatus;

  final TrafficStats _traffic;

  _FakeCore({required VpnStage stage, required TrafficStats traffic})
    : _traffic = traffic,
      currentStatus = VpnStatus(
        stage: stage,
        connectedSince: stage == VpnStage.connected
            ? DateTime.now().subtract(const Duration(minutes: 3, seconds: 5))
            : null,
        mode: TunnelMode.proxy,
        mixedPort: 7890,
        activeProxy: stage == VpnStage.connected ? 'Amsterdam #2' : null,
      );

  @override
  Stream<VpnStatus> get status => Stream<VpnStatus>.value(currentStatus);

  @override
  Stream<TrafficStats> get traffic => Stream<TrafficStats>.value(_traffic);

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

/// Профили из памяти: secure storage в тесте не поднимаем.
class _FakeProfilesStore implements ConnectionProfilesStore {
  List<ConnectionProfile> profiles;
  String? activeId;

  _FakeProfilesStore(this.profiles, this.activeId);

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
    profiles = const [];
    activeId = null;
  }
}

const _nodes = <ImportedServer>[
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
];

final _profile = ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://sub.example/a',
  rawConfig: 'proxies: []',
  format: 'clash',
  servers: _nodes,
  selectedServerId: 'nl-1',
  serversUpdatedMs: DateTime.now().millisecondsSinceEpoch,
);

Widget _guestHome({
  VpnStage stage = VpnStage.connected,
  TrafficStats traffic = const TrafficStats(
    downBps: 2048 * 1024,
    upBps: 64 * 1024,
    downTotal: 12 * 1024 * 1024,
    upTotal: 3 * 1024 * 1024,
  ),
}) => ProviderScope(
  overrides: [
    vpnConnectionProvider.overrideWithValue(
      _FakeCore(stage: stage, traffic: traffic),
    ),
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

void main() {
  testWidgets('generic Home показывает подписку, узел и живую статистику', (
    tester,
  ) async {
    _usePhoneView(tester);
    await tester.pumpWidget(_guestHome());
    // Профили читаются асинхронно: до них экран ещё панельный.
    await tester.pump();
    await tester.pump();
    await tester.pump();

    expect(find.byType(ConnectDial), findsOneWidget);

    // Подписка: имя профиля + число узлов, ведёт в список профилей.
    expect(find.text('Подписка'), findsOneWidget);
    expect(find.text('Моя подписка'), findsOneWidget);
    expect(find.text('УЗЛОВ: 2'), findsOneWidget);

    // Сервер: закреплённый узел плюс тот, на который встал селектор ядра.
    expect(find.text('Сервер'), findsOneWidget);
    expect(find.text('Amsterdam #1 · Amsterdam #2'), findsOneWidget);

    // Локальный инбаунд подписан под дайлом: в proxy-режиме его надо прописать
    // руками, значит его надо видеть.
    expect(find.text('Прокси 127.0.0.1:7890'), findsOneWidget);

    // Статистика: объёмы, мгновенные скорости, таймер сессии и режим захвата.
    expect(find.text('СКАЧАНО'), findsOneWidget);
    expect(find.text('ОТПРАВЛЕНО'), findsOneWidget);
    expect(find.text('ПРИЁМ'), findsOneWidget);
    expect(find.text('ОТДАЧА'), findsOneWidget);
    expect(find.text('СЕССИЯ'), findsOneWidget);
    expect(find.text('Прокси'), findsOneWidget); // ячейка «Режим»
    expect(find.text('12,0 МБ'), findsOneWidget);
    expect(find.text('2,0 МБ/с'), findsOneWidget);
    expect(find.text('64 КБ/с'), findsOneWidget);

    // Панельного здесь нет ничего.
    expect(find.byType(NotificationBell), findsNothing);
    expect(find.text('Relay (вход)'), findsNothing);
    expect(find.text('Трафик'), findsNothing);
    expect(find.text('Free'), findsNothing);
    expect(find.text('Задержка'), findsNothing);

    expect(tester.takeException(), isNull);
    // Гасим односекундный таймер сессии вместе с деревом.
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('без подключения generic Home зовёт нажать на дайл', (
    tester,
  ) async {
    _usePhoneView(tester);
    await tester.pumpWidget(_guestHome(stage: VpnStage.disconnected));
    await tester.pump();
    await tester.pump();
    await tester.pump();

    expect(find.text('Нажмите, чтобы подключиться'), findsOneWidget);
    // Адрес локального инбаунда ядро отдаёт вместе с режимом, а не только в
    // сессии: строка живёт, пока ядро в proxy-режиме.
    expect(find.text('Прокси 127.0.0.1:7890'), findsOneWidget);
    expect(find.text('00:00'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
