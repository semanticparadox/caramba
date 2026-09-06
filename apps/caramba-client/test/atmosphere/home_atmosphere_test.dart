// Home builds with the atmosphere layer behind it in every connection state,
// and the layer is registered to the real layout rather than to constants.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/atmosphere/atmosphere_layer.dart';
import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/app_theme.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/connect_dial.dart';
import '../support/fake_csm_device.dart';

/// The smallest thing that satisfies the tunnel contract: it reports one fixed
/// status and never talks to a platform channel.
class FakeVpnConnection with FakeCsmDevice implements VpnConnection {
  @override
  final VpnStatus currentStatus;

  FakeVpnConnection(VpnStage stage)
    : currentStatus = VpnStatus(
        stage: stage,
        connectedSince: stage == VpnStage.connected
            ? DateTime(2026, 9, 2, 10)
            : null,
      );

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

Widget _home(VpnStage stage) => ProviderScope(
  overrides: [
    vpnConnectionProvider.overrideWithValue(FakeVpnConnection(stage)),
  ],
  child: MaterialApp(theme: AppTheme.dark(), home: const HomeScreen()),
);

/// A phone-sized surface with a status bar inset, so the layout the chart is
/// anchored to is the real one.
void _usePhoneView(WidgetTester tester) {
  tester.view
    ..physicalSize = const Size(780, 1688)
    ..devicePixelRatio = 2
    ..viewPadding = const FakeViewPadding(top: 94)
    ..padding = const FakeViewPadding(top: 94);
  addTearDown(tester.view.reset);
}

void main() {
  for (final stage in VpnStage.values) {
    testWidgets('Home builds with the atmosphere layer in $stage', (
      tester,
    ) async {
      _usePhoneView(tester);
      await tester.pumpWidget(_home(stage));
      await tester.pump();
      await tester.pump();

      expect(find.byType(AtmosphereLayer), findsOneWidget);
      expect(find.byType(ConnectDial), findsOneWidget);
      expect(tester.takeException(), isNull);

      // The layer is full bleed behind the content.
      final layerBox = tester.getRect(find.byType(AtmosphereLayer));
      expect(layerBox.size, const Size(390, 844));

      await tester.pump(const Duration(milliseconds: 400));
      // Tear the tree down so Home's one-second session timer is cancelled.
      await tester.pumpWidget(const SizedBox.shrink());
    });
  }

  testWidgets('the chart anchors to the measured dial and connect block', (
    tester,
  ) async {
    _usePhoneView(tester);
    await tester.pumpWidget(_home(VpnStage.disconnected));
    await tester.pump();
    await tester.pump();

    final anchor = tester
        .widget<AtmosphereLayer>(find.byType(AtmosphereLayer))
        .anchor;
    expect(anchor.dialCenter, isNotNull);
    expect(anchor.labelRect, isNotNull);
    expect(anchor.headerBottom, isNotNull);

    // The chart's home station sits on the dial, not near it.
    final dialCenter = tester.getRect(find.byType(ConnectDial)).topCenter;
    expect(anchor.dialCenter!.dx, closeTo(dialCenter.dx, 0.5));
    expect(anchor.dialCenter!.dy, closeTo(dialCenter.dy + 98, 0.5));

    // The connect block sits below the dial and above the first card.
    expect(anchor.labelRect!.top, greaterThan(anchor.dialCenter!.dy + 98));
    expect(anchor.headerBottom, lessThan(anchor.dialCenter!.dy - 98));
    await tester.pump(const Duration(milliseconds: 400));
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
