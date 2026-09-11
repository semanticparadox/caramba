// Сцена «entry»: экран «Вход» — Авто / Без входа / Россия → релей «Москва».
//
// Только с --dart-define=CARAMBA_DEMO=1; см. support/demo_render.dart.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/features/servers/relay_screen.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/servers_state.dart';

import 'support/demo_fakes.dart';
import 'support/demo_render.dart';

void main() {
  testWidgets('entry', (tester) async {
    await loadDemoFonts();
    demoPhone(tester);
    final rec = DemoRecorder(tester, 'entry');

    final core = DemoCore();
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
            // «Авто» в силе: индекс 1 в списке записи.
            coreConfigProvider.overrideWith(
              (ref) =>
                  CoreConfigNotifier()..hydrate(const CoreConfig(relay: 1)),
            ),
          ],
          child: MaterialApp.router(
            debugShowCheckedModeBanner: false,
            theme: demoTheme(),
            routerConfig: demoRouter(
              initial: '/relay',
              routes: <String, WidgetBuilder>{
                '/home': (_) => const SizedBox.shrink(),
                '/relay': (_) => const RelayScreen(),
              },
            ),
          ),
        ),
      ),
    );
    await rec.settle();
    await rec.shot(1600);

    // Листаем к входам оператора.
    final msk = find.text('msk-1');
    await tester.ensureVisible(msk);
    await tester.pump();
    await rec.shot(1400);

    // Релей «Москва»: тост и галочка; экран закрывается сам через 300 мс,
    // поэтому кадры снимаются до закрытия.
    await rec.tapAnimated(msk, upSteps: 1);
    await rec.animate(steps: 2, step: const Duration(milliseconds: 90));
    await rec.shot(2600);

    rec.finish();
    await demoTearDownTree(tester);
  }, skip: !demoEnabled, timeout: const Timeout(Duration(minutes: 5)));
}
