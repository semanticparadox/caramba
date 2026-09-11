// Сцена «autotune»: «Подобрать лучший узел» — замер узлов, выбор лучшего и
// список кандидатов с вердиктами.
//
// Только с --dart-define=CARAMBA_DEMO=1; см. support/demo_render.dart.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/features/autotune/autotune_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
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
    latencyMs: 39,
    tcpMs: 27,
    verdict: ProbeVerdict.ok,
  ),
  ProbeResult(
    id: '🇩🇪 AmneziaWG',
    name: '🇩🇪 AmneziaWG',
    country: 'DE',
    latencyMs: 47,
    tcpMs: 27,
    verdict: ProbeVerdict.ok,
  ),
  ProbeResult(
    id: '🇳🇱 Stealth',
    name: '🇳🇱 Stealth',
    country: 'NL',
    latencyMs: 58,
    tcpMs: 44,
    verdict: ProbeVerdict.ok,
  ),
  ProbeResult(
    id: '🇳🇱 Speed',
    name: '🇳🇱 Speed',
    country: 'NL',
    latencyMs: 61,
    tcpMs: 44,
    verdict: ProbeVerdict.ok,
  ),
  ProbeResult(
    id: '🇫🇮 Stealth',
    name: '🇫🇮 Stealth',
    country: 'FI',
    latencyMs: -1,
    tcpMs: -1,
    verdict: ProbeVerdict.timeout,
  ),
];

void main() {
  testWidgets('autotune', (tester) async {
    await loadDemoFonts();
    demoPhone(tester);
    final rec = DemoRecorder(tester, 'autotune');

    final core = DemoCore()..probeGate = Completer<List<ProbeResult>>();
    final store = DemoProfilesStore(<ConnectionProfile>[
      demoRawProfile(selectedServerId: null),
    ], 'cp_demo');

    await tester.pumpWidget(
      rec.wrap(
        ProviderScope(
          overrides: <Override>[
            vpnConnectionProvider.overrideWithValue(core),
            connectionProfilesStoreProvider.overrideWithValue(store),
          ],
          child: MaterialApp.router(
            debugShowCheckedModeBanner: false,
            theme: demoTheme(),
            routerConfig: demoRouter(
              initial: '/settings/autotune',
              routes: <String, WidgetBuilder>{
                '/home': (_) => const SizedBox.shrink(),
                '/settings/autotune': (_) =>
                    const AutotuneScreen(fromSettings: true),
              },
            ),
          ),
        ),
      ),
    );
    await rec.settle();

    // Идёт замер: секунды считаются по настоящим часам.
    for (var s = 0; s < 4; s++) {
      await rec.animate(steps: 5, step: const Duration(milliseconds: 100));
      await rec.realWait(const Duration(milliseconds: 520));
    }

    // Ответ ядра приехал: выбор и список кандидатов.
    core.probeGate!.complete(_results);
    await rec.settle(pumps: 8);
    await tester.pump(const Duration(milliseconds: 200));
    await rec.shot(2200);

    // Листаем к списку кандидатов.
    await rec.dragAnimated(
      find.byType(SingleChildScrollView),
      const Offset(0, -560),
      steps: 10,
      holdMs: 2600,
    );

    rec.finish();
    await demoTearDownTree(tester);
  }, skip: !demoEnabled, timeout: const Timeout(Duration(minutes: 5)));
}
