// Сцена «connect»: Главная без подключения → нажатие на дайл → «Подключение…»
// → «Защищено» с узлом, типом подключения и растущим трафиком.
//
// Только с --dart-define=CARAMBA_DEMO=1; см. support/demo_render.dart.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/features/home/home_screen.dart';
import 'package:caramba_client/state/access_guard.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';
import 'package:caramba_client/widgets/connect_dial.dart';

import 'support/demo_fakes.dart';
import 'support/demo_render.dart';

/// Здоровый отчёт ядра о поднятом туннеле: пресет «Россия (умный)», список
/// рекламы приехал файлом, база GEOSITE проверена.
const String _healthyRoute = '''
{"known":true,"tunnel_up":true,"source":"preset","rules":14,
 "preset":{"preset_id":"ru-smart","preset_name":"Россия (умный)","emoji":"🇷🇺",
   "country":"RU","final_action":"DIRECT","rules":14,"dropped_rules":0,
   "sources":[{"name":"ads","state":"file","rules":148000,"kept_rules":148000},
              {"name":"ru-blocked","state":"file","rules":9120,"kept_rules":9120}]},
 "geosite":{"required":true,"state":"verified"},
 "relay":{"state":"not_requested","dialer_proxy_seen":false}}
''';

void main() {
  testWidgets('connect', (tester) async {
    await loadDemoFonts();
    demoPhone(tester);
    final rec = DemoRecorder(tester, 'connect');

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
            accessGuardProvider.overrideWith(
              (ref) => AccessGuard(
                check: (_) async => AccessVerdict.unknown,
                first: const Duration(days: 1),
                every: const Duration(days: 1),
              ),
            ),
          ],
          child: MaterialApp(
            debugShowCheckedModeBanner: false,
            theme: demoTheme(),
            home: const HomeScreen(),
          ),
        ),
      ),
    );
    await rec.settle();
    await tester.pump(const Duration(milliseconds: 400));
    await rec.shot(1400);

    // Нажатие на дайл.
    final dial = find.byType(ConnectDial);
    final gesture = await tester.startGesture(tester.getCenter(dial));
    await tester.pump(const Duration(milliseconds: 90));
    await rec.shot(120);
    await gesture.up();
    await tester.pump(const Duration(milliseconds: 40));
    await rec.shot(120);

    // «Подключение…»: дуга крутится.
    await rec.animate(steps: 14, step: const Duration(milliseconds: 90));

    // Подключено: узел, тип подключения, таймер и трафик растут.
    final server = rawProfileServer('Caramba Connect');
    core.routeReportJson = _healthyRoute;
    var down = 0;
    var up = 0;
    Future<void> second(int s) async {
      core.emit(
        VpnStatus(
          stage: VpnStage.connected,
          server: server,
          connectedSince: DateTime.now().subtract(Duration(seconds: s)),
          mode: TunnelMode.tun,
          activeProxy: '🇩🇪 Stealth',
        ),
      );
      final downBps =
          2 * 1024 * 1024 + ((s * 2654435761) & 0x7fffffff) % (9 * 1024 * 1024);
      final upBps = 180 * 1024 + ((s * 40503) & 0xfffff) % (700 * 1024);
      down += downBps;
      up += upBps;
      core.pushTraffic(
        TrafficStats(
          downBps: downBps,
          upBps: upBps,
          downTotal: down,
          upTotal: up,
        ),
      );
      await tester.pump(const Duration(milliseconds: 500));
      await rec.shot(s == 0 ? 900 : 650);
      await tester.pump(const Duration(milliseconds: 500));
    }

    for (var s = 0; s <= 5; s++) {
      await second(s);
    }
    // Листаем к ячейкам статистики: скачано, отправлено, скорость, сессия.
    await rec.dragAnimated(
      find.byType(ListView),
      const Offset(0, -470),
      steps: 8,
      holdMs: 400,
    );
    for (var s = 6; s <= 11; s++) {
      await second(s);
    }
    await rec.shot(2200);

    rec.finish();
    await demoTearDownTree(tester);
  }, skip: !demoEnabled, timeout: const Timeout(Duration(minutes: 5)));
}
