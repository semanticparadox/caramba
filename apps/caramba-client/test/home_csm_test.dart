// Главная с хромом CSM/1: INV-21 (возраст конфигурации) и INV-22 (карточка
// «Оставить или Вернуть») появляются на Главной, НЕ двигая дайл.
//
// Это не косметика. Атмосферный слой зарегистрирован на измеренную геометрию:
// домашняя станция чарта стоит на дайле, верх границы держится за низ шапки, а
// тихая линза за прямоугольник подписи. Карточка, вставленная выше дайла,
// сдвинула бы всё три якоря разом. Поэтому весь хром CSM живёт в блоке
// карточек ниже дайла, и тест сравнивает якорь с карточкой и без неё.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/atmosphere/atmosphere_layer.dart';
import 'package:caramba_client/data/connection_profiles_store.dart';
import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/features/csm/config_age_card.dart';
import 'package:caramba_client/features/csm/keep_or_revert_card.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/connect_dial.dart';
import 'support/fake_csm_device.dart';

const _nowMs = 1788307500 * 1000;

const _pin = CsmPin(
  pid: '226e8a20f699b964',
  linkPin: '49Q8M87PK6WP9QXG3T30',
  origin: CsmPinOrigin.outOfBand,
  establishedMs: 1788300000000,
);

class _FakeCore with FakeCsmDevice implements VpnConnection {
  @override
  final VpnStatus currentStatus = const VpnStatus(stage: VpnStage.disconnected);

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
  /// Правду о стадии фейк знает сам: платформы за ним нет.
  @override
  Future<VpnStatus> refreshStatus() async => currentStatus;

  @override
  Future<void> dispose() async {}
}

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

ConnectionProfile _profile(CsmProfileState? csm) => ConnectionProfile(
  id: 'cp_1',
  type: ProfileType.rawSub,
  displayName: 'Моя подписка',
  source: 'https://sub.example/a',
  rawConfig: 'proxies: []',
  format: 'clash',
  csm: csm,
);

Widget _home(CsmProfileState? csm) {
  final profiles = <ConnectionProfile>[_profile(csm)];
  return ProviderScope(
    overrides: <Override>[
      vpnConnectionProvider.overrideWithValue(_FakeCore()),
      connectionProfilesStoreProvider.overrideWithValue(
        _Store(profiles, 'cp_1'),
      ),
      csmClockProvider.overrideWithValue(
        () => DateTime.fromMillisecondsSinceEpoch(_nowMs),
      ),
    ],
    child: MaterialApp(theme: AppTheme.dark(), home: const HomeScreen()),
  );
}

void _phone(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 1688)
    ..devicePixelRatio = 2
    ..viewPadding = const FakeViewPadding(top: 94)
    ..padding = const FakeViewPadding(top: 94);
  addTearDown(tester.view.reset);
}

Future<AtmosphereAnchor> _pumpAnchor(
  WidgetTester tester,
  CsmProfileState? csm,
) async {
  await tester.pumpWidget(_home(csm));
  await tester.pump();
  await tester.pump();
  await tester.pump();
  return tester.widget<AtmosphereLayer>(find.byType(AtmosphereLayer)).anchor;
}

/// Профиль, живущий на кэше: карточка возраста конфигурации обязана появиться.
CsmProfileState _staleCsm() => const CsmProfileState(
  pin: _pin,
  stage: CsmProfileStage.trustedStale,
  directive: CsmDocumentRecord(
    docType: 0x03,
    version: 41,
    issuedSec: _nowMs ~/ 1000 - 7200,
    expiresSec: _nowMs ~/ 1000 - 3600,
    signerFingerprints: <String>['a1b2c3d4e5f60718'],
    verifiedAtMs: _nowMs - 3600 * 1000,
    viaRung: 0,
  ),
);

/// Тот же профиль плюс висящая карточка «Оставить или Вернуть».
CsmProfileState _cardCsm() => CsmProfileState(
  pin: _pin,
  stage: CsmProfileStage.trustedStale,
  directive: _staleCsm().directive,
  pendingChanges: const <CsmPendingChange>[
    CsmPendingChange(
      id: 'card_1',
      raisedMs: _nowMs - 60000,
      items: <CsmCardItem>[
        CsmCardItem(
          key: CsmSettingKey.killSwitch,
          current: CsmBoolean(true),
          proposed: CsmBoolean(false),
          src: CsmProvenance.operator,
          trigger: CsmCardTrigger.narrowing,
        ),
      ],
    ),
  ],
);

void main() {
  testWidgets('профиль без CSM: хрома нет и Главная не меняется', (
    tester,
  ) async {
    _phone(tester);
    await _pumpAnchor(tester, null);

    expect(find.byType(ConnectDial), findsOneWidget);
    expect(find.text('Работает на сохранённой конфигурации'), findsNothing);
    expect(find.byType(KeepOrRevertCard), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('возраст конфигурации виден на Главной, INV-21', (tester) async {
    _phone(tester);
    await _pumpAnchor(tester, _staleCsm());

    expect(find.byType(CsmConfigAgeCard), findsOneWidget);
    expect(find.text('Работает на сохранённой конфигурации'), findsOneWidget);
    expect(find.textContaining('R0 сохранённые документы'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('карточка «Оставить или Вернуть» видна на Главной, INV-22', (
    tester,
  ) async {
    _phone(tester);
    await _pumpAnchor(tester, _cardCsm());

    expect(find.byType(KeepOrRevertCard), findsOneWidget);
    expect(find.text('Оператор сузил вашу защиту'), findsOneWidget);
    expect(find.text('Оставить моё'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('хром CSM не двигает дайл: якорь атмосферы совпадает', (
    tester,
  ) async {
    _phone(tester);

    final bare = await _pumpAnchor(tester, null);
    await tester.pumpWidget(const SizedBox.shrink());

    final withAge = await _pumpAnchor(tester, _staleCsm());
    await tester.pumpWidget(const SizedBox.shrink());

    final withCard = await _pumpAnchor(tester, _cardCsm());
    await tester.pumpWidget(const SizedBox.shrink());

    // Домашняя станция чарта, прямоугольник подписи и низ шапки одинаковы во
    // всех трёх случаях: карточки живут ниже дайла и высота шапки постоянна.
    expect(withAge.dialCenter, bare.dialCenter);
    expect(withCard.dialCenter, bare.dialCenter);
    expect(withAge.labelRect, bare.labelRect);
    expect(withCard.labelRect, bare.labelRect);
    expect(withAge.headerBottom, bare.headerBottom);
    expect(withCard.headerBottom, bare.headerBottom);
  });
}
