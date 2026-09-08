// Home на десктопе: две колонки, кнопки по содержимому, статистика в три
// столбца.
//
// Платформа переопределяется через `debugDefaultTargetPlatformOverride` и
// возвращается в `null` в `tearDown`: без этого следующий тест в том же файле
// (и, что хуже, в соседнем) поехал бы по десктопной ветке. Ширина окна на выбор
// ветки НЕ влияет — она выбирается платформой, — но 1280x800 нужна, чтобы обе
// колонки поместились и дайл взял свои 232.
//
// Сторожим здесь ровно то, чем десктопная раскладка отличается от «мобильного
// экрана, растянутого на окно»: кнопка пустого состояния не во всю ширину,
// карточки живут в отдельной колонке, а шесть ячеек статистики стоят тремя
// столбцами, а не шестью строками.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/atmosphere/atmosphere_layer.dart';
import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/features/home/autopilot_button.dart';
import 'package:caramba_client/features/home/home_desktop_layout.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/connect_dial.dart';
import '../support/fake_csm_device.dart';

/// Ядро в proxy-режиме: те же поля, что читает generic-ветка Home.
class _FakeCore with FakeCsmDevice implements VpnConnection {
  @override
  final VpnStatus currentStatus;

  final TrafficStats _traffic;

  _FakeCore({
    required VpnStage stage,
    required TrafficStats traffic,
    String activeProxy = 'Amsterdam #2',
  })  : _traffic = traffic,
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
  }) async =>
      const ImportResult(servers: <ImportedServer>[]);

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

Widget _desktopHome({
  VpnStage stage = VpnStage.connected,
  TrafficStats traffic = const TrafficStats(
    downBps: 2048 * 1024,
    upBps: 64 * 1024,
    downTotal: 12 * 1024 * 1024,
    upTotal: 3 * 1024 * 1024,
  ),
  List<ConnectionProfile>? profiles,
}) {
  final stored = profiles ?? <ConnectionProfile>[_profile];
  return ProviderScope(
    overrides: [
      vpnConnectionProvider.overrideWithValue(
        _FakeCore(stage: stage, traffic: traffic),
      ),
      connectionProfilesStoreProvider.overrideWithValue(
        _FakeProfilesStore(stored, stored.isEmpty ? null : _profile.id),
      ),
    ],
    child: MaterialApp(theme: AppTheme.dark(), home: const HomeScreen()),
  );
}

/// Окно десктопа. Ветку выбирает платформа, размер — только то, поместятся ли
/// в него обе колонки.
void _useDesktopView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(1280, 800)
    ..devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

/// Тело теста под macOS-платформой.
///
/// Override снимается ВНУТРИ тела, а не в `tearDown`: проверка инвариантов
/// (`debugAssertAllFoundationVarsUnset`) отрабатывает раньше `tearDown`, и
/// оставленный до неё флаг валит тест. `tearDown` ниже остаётся страховкой на
/// случай броска исключения.
Future<void> _onMacOS(Future<void> Function() body) async {
  debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
  try {
    await body();
  } finally {
    debugDefaultTargetPlatformOverride = null;
  }
}

void main() {
  // Платформа глобальна: оставленный override увёл бы в десктопную ветку тесты
  // мобильного шелла.
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  testWidgets(
    'без подключений: пустое состояние с кнопкой по содержимому',
    (tester) async => _onMacOS(() async {
      _useDesktopView(tester);
      // The shell reserves 240 px for its sidebar.
      tester.view.physicalSize = const Size(1040, 800);
      await tester.pumpWidget(
        _desktopHome(stage: VpnStage.disconnected, profiles: const []),
      );
      await tester.pump();
      await tester.pump();
      await tester.pump();

      expect(find.byType(HomeDesktopLayout), findsOneWidget);
      // Дайл и атмосфера на месте: геометрия слоя держится на дайле, и убирать
      // его из пустого состояния нельзя ни на одной платформе.
      expect(find.byType(ConnectDial), findsOneWidget);
      expect(find.byType(AtmosphereLayer), findsOneWidget);

      expect(find.text('Подключений пока нет'), findsOneWidget);
      final add = find.widgetWithText(FilledButton, 'Добавить подключение');
      expect(add, findsOneWidget);
      // Ровно та поломка, ради которой раскладка и написана: на E1 эта кнопка
      // растягивалась во всю ширину окна.
      expect(tester.getSize(add).width, lessThan(400));
      expect(
        find.widgetWithText(OutlinedButton, 'Подключить панель'),
        findsOneWidget,
      );

      for (final label in ['Добавить подключение', 'Подключить панель']) {
        final paragraph = tester.renderObject<RenderParagraph>(
          find.descendant(
            of: find.text(label),
            matching: find.byType(RichText),
          ),
        );
        expect(paragraph.didExceedMaxLines, isFalse, reason: label);
      }

      // Подбирать не из чего, и списка карточек тоже нет.
      expect(find.byType(AutopilotButton), findsNothing);
      expect(find.text('Сервер'), findsNothing);
      expect(find.text('СКАЧАНО'), findsNothing);

      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    }),
  );

  testWidgets(
    'с профилем: строки в правой колонке, статистика в три столбца',
    (tester) async => _onMacOS(() async {
      _useDesktopView(tester);
      await tester.pumpWidget(_desktopHome());
      await tester.pump();
      await tester.pump();
      await tester.pump();

      expect(find.byType(HomeDesktopLayout), findsOneWidget);
      expect(find.byType(ConnectDial), findsOneWidget);
      expect(find.byType(AtmosphereLayer), findsOneWidget);

      // Те же строки, что и на мобильном: состав карточек десктоп не меняет.
      expect(find.text('Подписка'), findsOneWidget);
      expect(find.text('Сервер'), findsOneWidget);
      expect(find.text('Relay (вход)'), findsOneWidget);
      expect(find.text('Тип подключения'), findsOneWidget);

      // Шесть ячеек generic-статистики.
      const labels = [
        'СКАЧАНО',
        'ОТПРАВЛЕНО',
        'ПРИЁМ',
        'ОТДАЧА',
        'СЕССИЯ',
        'ЗАХВАТ',
      ];
      for (final label in labels) {
        expect(find.text(label), findsOneWidget, reason: label);
      }

      // Три столбца, а не два: ячейки одного ряда стоят на одной высоте, и
      // рядов ровно два. Проверяем координатами, а не числом виджетов —
      // «шесть ячеек» одинаково истинно и для 2x3, и для 3x2.
      final tops = <double, List<double>>{};
      for (final label in labels) {
        final r = tester.getTopLeft(find.text(label));
        tops.putIfAbsent(r.dy, () => <double>[]).add(r.dx);
      }
      expect(tops.length, 2, reason: 'два ряда по три ячейки');
      for (final row in tops.values) {
        expect(row.length, 3);
      }

      // Автоподбор ушёл в левую панель, под дайл: он действие над подключением,
      // а не карточка выбора.
      final autopilot = find.byType(AutopilotButton);
      expect(autopilot, findsOneWidget);
      expect(
        tester.getTopLeft(autopilot).dx,
        lessThan(tester.getTopLeft(find.text('Подписка')).dx),
      );

      // Адрес локального инбаунда подписан под дайлом и на десктопе.
      expect(find.text('Прокси 127.0.0.1:7890'), findsOneWidget);

      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    }),
  );
}
