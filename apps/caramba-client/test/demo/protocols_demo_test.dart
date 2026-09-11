// Сцена «protocols»: экран «Тип подключения» немецкой машины — Авто,
// VLESS Reality (Stealth), Hysteria2 (Speed), AmneziaWG; замер задержек и
// выбор AmneziaWG.
//
// Только с --dart-define=CARAMBA_DEMO=1; см. support/demo_render.dart.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/features/protocol/protocol_screen.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';
import 'package:caramba_client/vpn/vpn_models.dart';

import 'support/demo_fakes.dart';
import 'support/demo_render.dart';

const List<ProbeResult> _results = <ProbeResult>[
  ProbeResult(
    id: '🇩🇪 Stealth',
    name: '🇩🇪 Stealth',
    country: 'DE',
    latencyMs: 41,
    tcpMs: 27,
    verdict: ProbeVerdict.ok,
  ),
  ProbeResult(
    id: '🇩🇪 Speed',
    name: '🇩🇪 Speed',
    country: 'DE',
    latencyMs: 38,
    tcpMs: 27,
    verdict: ProbeVerdict.ok,
  ),
  ProbeResult(
    id: '🇩🇪 AmneziaWG',
    name: '🇩🇪 AmneziaWG',
    country: 'DE',
    latencyMs: 46,
    tcpMs: 27,
    verdict: ProbeVerdict.ok,
  ),
];

void main() {
  testWidgets('protocols', (tester) async {
    await loadDemoFonts();
    demoPhone(tester);
    final rec = DemoRecorder(tester, 'protocols');

    final core = DemoCore()..probeGate = Completer<List<ProbeResult>>();
    final store = DemoProfilesStore(<ConnectionProfile>[
      demoPanelProfile(),
    ], 'cp_panel');

    await tester.pumpWidget(
      rec.wrap(
        ProviderScope(
          overrides: <Override>[
            vpnConnectionProvider.overrideWithValue(core),
            connectionProfilesStoreProvider.overrideWithValue(store),
            serversProvider.overrideWith((ref) async => demoPanelServers),
            apiRelaysProvider.overrideWith((ref) async => demoPanelRelays),
          ],
          child: MaterialApp.router(
            debugShowCheckedModeBanner: false,
            theme: demoTheme(),
            routerConfig: demoRouter(
              initial: '/protocol',
              routes: <String, WidgetBuilder>{
                '/home': (_) => const SizedBox.shrink(),
                '/protocol': (_) => const ProtocolScreen(),
              },
            ),
          ),
        ),
      ),
    );
    await rec.settle();

    // Список инбаундов уже на экране, замер идёт.
    await rec.animate(steps: 6, step: const Duration(milliseconds: 120));

    // Числа приехали.
    core.probeGate!.complete(_results);
    await rec.settle(pumps: 8);
    await rec.shot(1800);

    // Выбираем AmneziaWG.
    final awg = find.textContaining('AmneziaWG').first;
    await tester.ensureVisible(awg);
    await tester.pump();
    // Тост и галочка; экран закрывается сам, поэтому кадры до закрытия.
    await rec.tapAnimated(awg, upSteps: 1);
    await rec.animate(steps: 2, step: const Duration(milliseconds: 70));
    await rec.shot(2400);

    rec.finish();
    await demoTearDownTree(tester);
  }, skip: !demoEnabled, timeout: const Timeout(Duration(minutes: 5)));
}
