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
import 'support/fake_csm_device.dart';

/// Ядро, поднятое в proxy-режиме на конкретном узле: ровно те поля ABI v2,
/// которые generic-ветка Home показывает вместо панельных.
class _FakeCore with FakeCsmDevice implements VpnConnection {
  @override
  final VpnStatus currentStatus;

  final TrafficStats _traffic;

  _FakeCore({
    required VpnStage stage,
    required TrafficStats traffic,
    String activeProxy = 'Amsterdam #2',
  }) : _traffic = traffic,
       currentStatus = VpnStatus(
         stage: stage,
         connectedSince: stage == VpnStage.connected
             ? DateTime.now().subtract(const Duration(minutes: 3, seconds: 5))
             : null,
         mode: TunnelMode.proxy,
         mixedPort: 7890,
         activeProxy: stage == VpnStage.connected ? activeProxy : null,
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

  /// Правду о стадии фейк знает сам: платформы за ним нет.
  @override
  Future<VpnStatus> refreshStatus() async => currentStatus;

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

/// Подписка живого вида: НЕСКОЛЬКО прокси на одной машине. Ровно так выглядит
/// подписка владельца — 13 строк на две машины, потому что каждый инбаунд
/// каждой ноды приезжает отдельным прокси.
const _multiInbound = <ImportedServer>[
  ImportedServer(
    id: 'de-reality',
    name: 'DE Stealth',
    type: 'vless',
    server: 'de.example',
    port: 443,
    country: 'DE',
  ),
  ImportedServer(
    id: 'de-tls',
    name: 'DE Secure',
    type: 'vless',
    server: 'de.example',
    port: 8443,
    country: 'DE',
  ),
  ImportedServer(
    id: 'de-hy2',
    name: 'DE Speed',
    type: 'hysteria2',
    server: 'de.example',
    port: 2096,
    country: 'DE',
  ),
  ImportedServer(
    id: 'ca-reality',
    name: 'CA Stealth',
    type: 'vless',
    server: 'ca.example',
    port: 443,
    country: 'CA',
  ),
  ImportedServer(
    id: 'ca-hy2',
    name: 'CA Speed',
    type: 'hysteria2',
    server: 'ca.example',
    port: 2096,
    country: 'CA',
  ),
];

Widget _guestHome({
  VpnStage stage = VpnStage.connected,
  TrafficStats traffic = const TrafficStats(
    downBps: 2048 * 1024,
    upBps: 64 * 1024,
    downTotal: 12 * 1024 * 1024,
    upTotal: 3 * 1024 * 1024,
  ),
  ConnectionProfile? profile,
  String activeProxy = 'Amsterdam #2',
}) {
  final p = profile ?? _profile;
  return ProviderScope(
    overrides: [
      vpnConnectionProvider.overrideWithValue(
        _FakeCore(stage: stage, traffic: traffic, activeProxy: activeProxy),
      ),
      connectionProfilesStoreProvider.overrideWithValue(
        _FakeProfilesStore(<ConnectionProfile>[p], p.id),
      ),
    ],
    child: MaterialApp(theme: AppTheme.dark(), home: const HomeScreen()),
  );
}

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
    // Имена стоят РАЗДЕЛЬНО, а не склеены через «·»: CRow режет значение
    // многоточием с конца, и склейка двух живых имён давала на устройстве
    // «🇨🇦 Stream via 🇷🇺 ·…» — обрубок без единого целого имени.
    expect(find.text('Сервер'), findsOneWidget);
    expect(find.text('Amsterdam #1'), findsOneWidget);
    expect(find.text('Amsterdam #2'), findsOneWidget);
    expect(find.text('Amsterdam #1 · Amsterdam #2'), findsNothing);

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

    // Relay остаётся ВИДИМЫМ. Раньше строки тут не было вовсе, и это ровно тот
    // отказ, ради которого правило написано: спрятанный переключатель
    // неотличим от «такой настройки не бывает», и пользователь ищет её в
    // обновлении приложения, которого не существует.
    //
    // Значение — то, что СЕЙЧАС в силе («Выкл»), а не слово «Недоступно»:
    // «Выкл» доступен при любом источнике, и подмена значения на «Недоступно»
    // была неправдой о самой строке. Причина недоступности ЦЕПОЧКИ приходит
    // отдельным баннером из возможности слоя предложения
    // (`Capabilities.relayChaining`), а не из догадки экрана.
    expect(find.text('Relay (вход)'), findsOneWidget);
    expect(find.text('Выкл'), findsOneWidget);
    expect(find.text('Недоступно'), findsNothing);
    expect(
      find.textContaining('цепочку через вход выразить не может'),
      findsOneWidget,
    );

    // Панельного здесь нет ничего.
    expect(find.byType(NotificationBell), findsNothing);
    expect(find.text('Трафик'), findsNothing);
    expect(find.text('Free'), findsNothing);
    expect(find.text('Задержка'), findsNothing);

    expect(tester.takeException(), isNull);
    // Гасим односекундный таймер сессии вместе с деревом.
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('«узлов» на Home считает МАШИНЫ, а не строки конфига', (
    tester,
  ) async {
    // Жалоба владельца дословно: «восемь серверов» там, где машина одна. Экран
    // серверов от неё избавлен, а Home повторял её на плашке «Подписка»:
    // `profile.serverCount` — это длина списка прокси, то есть по строке на
    // каждый инбаунд каждой машины. На живой подписке это 13 при двух машинах.
    //
    // Здесь пять прокси на двух хостах. Плашка обязана сказать «2» — то же
    // число и тем же словом, что экран серверов.
    _usePhoneView(tester);
    await tester.pumpWidget(
      _guestHome(
        profile: _profile.copyWith(
          servers: _multiInbound,
          selectedServerId: 'de-reality',
        ),
      ),
    );
    await tester.pump();
    await tester.pump();
    await tester.pump();

    expect(find.text('УЗЛОВ: 2'), findsOneWidget);
    expect(
      find.text('УЗЛОВ: 5'),
      findsNothing,
      reason: 'пять прокси это не пять узлов',
    );

    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  // Замер с устройства: строка «Сервер» показывала «🇨🇦 Stream via 🇷🇺» прямо
  // над строкой «Relay (вход): Выкл». Имя обещало вход, которого на этом пути
  // нет вовсе (`detour` теряется при переводе sing-box → clash в ядре), и
  // обещание попадало на ПЕРВЫЙ экран.
  testWidgets('строка «Сервер» говорит проводом, а не обещанием в имени', (
    tester,
  ) async {
    _usePhoneView(tester);
    // Узлы живой подписки `sub 34`: один и тот же провод под двумя именами.
    const live = <ImportedServer>[
      ImportedServer(
        id: '🇨🇦 Stream via 🇷🇺',
        name: '🇨🇦 Stream via 🇷🇺',
        type: 'vless',
        server: '158.69.213.88',
        port: 10400,
        country: 'CA',
      ),
      ImportedServer(
        id: '🇨🇦 Stream',
        name: '🇨🇦 Stream',
        type: 'vless',
        server: '158.69.213.88',
        port: 10400,
        country: 'CA',
      ),
    ];
    await tester.pumpWidget(
      _guestHome(
        profile: _profile.copyWith(
          servers: live,
          selectedServerId: '🇨🇦 Stream via 🇷🇺',
        ),
        activeProxy: '🇨🇦 Stream via 🇷🇺',
      ),
    );
    await tester.pump();
    await tester.pump();
    await tester.pump();

    // В строке — то, что НА ПРОВОДЕ.
    expect(find.text('🇨🇦 Stream'), findsWidgets);
    // Обещания на первом экране больше нет...
    expect(find.text('🇨🇦 Stream via 🇷🇺'), findsNothing);
    expect(find.textContaining('via 🇷🇺'), findsNothing);
    // ...но переименование не молчаливое: расхождение названо тут же, в строке.
    expect(find.text('ВХОД ТОЛЬКО В ИМЕНИ'), findsOneWidget);

    expect(tester.takeException(), isNull);
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
