// Сцена «rules»: Настройки → «Правила трафика»: смена режима «Российский
// режим» → «Полный обход», переключатель «Блокировать рекламу и трекеры»,
// экран «Правила по сайтам» со своим доменом и готовым набором.
//
// Только с --dart-define=CARAMBA_DEMO=1; см. support/demo_render.dart.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/protocol.dart';
import 'package:caramba_client/features/settings/settings_screen.dart';
import 'package:caramba_client/features/settings/site_rules_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/widgets/ui.dart';

import 'support/demo_fakes.dart';
import 'support/demo_render.dart';

void main() {
  testWidgets('rules', (tester) async {
    await loadDemoFonts();
    demoPhone(tester);
    final rec = DemoRecorder(tester, 'rules');

    final ruSmart = RoutingMode.defaults.indexWhere((m) => m.id == 'ru-smart');
    final core = DemoCore();
    final store = DemoProfilesStore(<ConnectionProfile>[
      demoRawProfile(),
    ], 'cp_demo');

    await tester.pumpWidget(
      rec.wrap(
        ProviderScope(
          overrides: <Override>[
            vpnConnectionProvider.overrideWithValue(core),
            connectionProfilesStoreProvider.overrideWithValue(store),
            coreConfigProvider.overrideWith(
              (ref) => CoreConfigNotifier()
                ..hydrate(CoreConfig(route: ruSmart, routeChosen: true)),
            ),
          ],
          child: MaterialApp.router(
            debugShowCheckedModeBanner: false,
            theme: demoTheme(),
            routerConfig: demoRouter(
              initial: '/settings',
              routes: <String, WidgetBuilder>{
                '/home': (_) => const SizedBox.shrink(),
                '/settings': (_) => const SettingsScreen(),
                '/settings/site-rules': (_) => const SiteRulesScreen(),
              },
            ),
          ),
        ),
      ),
    );
    await rec.settle();
    await rec.shot(900);

    // Раздел «Правила трафика» виден сразу под «Подключением».
    await rec.shot(1200);

    // Режим: лист выбора → «Полный обход».
    await rec.tapAnimated(find.text('Режим').first, upSteps: 1);
    await rec.animate(steps: 5, step: const Duration(milliseconds: 60));
    await rec.shot(1200);
    await rec.tapAnimated(find.text('Полный обход').last, upSteps: 1);
    await rec.animate(steps: 5, step: const Duration(milliseconds: 60));
    await tester.pump(const Duration(milliseconds: 200));
    await rec.shot(1400);

    // Блок рекламы и трекеров.
    await rec.tapAnimated(find.byType(Switch).first, upSteps: 4);
    await rec.shot(1400);

    // Правила по сайтам.
    await rec.tapAnimated(find.text('Правила по сайтам').first, upSteps: 1);
    await rec.animate(steps: 5, step: const Duration(milliseconds: 60));
    await rec.settle(pumps: 3);
    await rec.shot(1300);

    await rec.tapAnimated(find.text('Только выбранные сайты'), upSteps: 3);
    await rec.shot(1000);

    // Свои сайты: листаем к полю и набираем домены.
    await rec.dragAnimated(
      find.byType(ListView).last,
      const Offset(0, -360),
      steps: 6,
      holdMs: 500,
    );
    final field = find.byKey(
      const ValueKey('site-rules-allow-domains-field'),
      skipOffstage: false,
    );
    await rec.typeAnimated(field, 'youtube.com, chatgpt.com', delayMs: 70);

    // Готовый набор: Telegram.
    await rec.dragAnimated(
      find.byType(ListView).last,
      const Offset(0, -420),
      steps: 6,
      holdMs: 700,
    );
    final tgRow = find
        .ancestor(of: find.text('Telegram').first, matching: find.byType(CRow))
        .first;
    final sw = find.descendant(of: tgRow, matching: find.byType(Switch)).first;
    await rec.tapAnimated(sw, upSteps: 4);
    await rec.shot(2400);

    rec.finish();
    await demoTearDownTree(tester);
  }, skip: !demoEnabled, timeout: const Timeout(Duration(minutes: 5)));
}
