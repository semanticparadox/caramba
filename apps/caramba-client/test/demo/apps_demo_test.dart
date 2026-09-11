// Сцена «apps»: «Правила по приложениям» (Android) — режим «Только список
// через VPN», добавление Telegram и YouTube из списка установленных.
//
// Иконки приложений — демо-данные (PNG рисуются в тесте как данные списка,
// а не поверх UI). Только с --dart-define=CARAMBA_DEMO=1.

import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_vpn/caramba_vpn.dart' show InstalledApp;

import 'package:caramba_client/data/models/connection_profile.dart';
import 'package:caramba_client/data/models/split_app.dart';
import 'package:caramba_client/features/settings/app_rules_screen.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/core_config_state.dart';
import 'package:caramba_client/state/installed_apps_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/widgets/ui.dart';

import 'support/demo_fakes.dart';
import 'support/demo_render.dart';

/// Демо-иконка: цветной скруглённый квадрат с буквой (96×96 PNG).
Future<Uint8List> _iconPng(Color bg, String letter) async {
  const size = 96.0;
  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder);
  canvas.drawRRect(
    RRect.fromRectAndRadius(
      const Rect.fromLTWH(0, 0, size, size),
      const Radius.circular(22),
    ),
    Paint()..color = bg,
  );
  final pb = ui.ParagraphBuilder(
    ui.ParagraphStyle(
      textAlign: TextAlign.center,
      fontSize: 54,
      fontWeight: FontWeight.w700,
      fontFamily: 'Roboto',
    ),
  )
    ..pushStyle(ui.TextStyle(color: Colors.white))
    ..addText(letter);
  final p = pb.build()..layout(const ui.ParagraphConstraints(width: size));
  canvas.drawParagraph(p, Offset(0, (size - p.height) / 2));
  final image = await recorder.endRecording().toImage(96, 96);
  final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
  image.dispose();
  return bytes!.buffer.asUint8List();
}

void main() {
  testWidgets('apps', (tester) async {
    await loadDemoFonts();
    demoPhone(tester);
    final rec = DemoRecorder(tester, 'apps');

    late List<InstalledApp> installed;
    await tester.runAsync(() async {
      installed = <InstalledApp>[
        InstalledApp(
          packageName: 'org.telegram.messenger',
          label: 'Telegram',
          iconPng: await _iconPng(const Color(0xFF2AABEE), 'T'),
        ),
        InstalledApp(
          packageName: 'com.google.android.youtube',
          label: 'YouTube',
          iconPng: await _iconPng(const Color(0xFFFF0033), 'Y'),
        ),
        InstalledApp(
          packageName: 'com.android.chrome',
          label: 'Chrome',
          iconPng: await _iconPng(const Color(0xFF34A853), 'C'),
        ),
        InstalledApp(
          packageName: 'com.instagram.android',
          label: 'Instagram',
          iconPng: await _iconPng(const Color(0xFFD62976), 'I'),
        ),
        InstalledApp(
          packageName: 'com.whatsapp',
          label: 'WhatsApp',
          iconPng: await _iconPng(const Color(0xFF25D366), 'W'),
        ),
        InstalledApp(
          packageName: 'ru.yandex.taxi',
          label: 'Яндекс Go',
          iconPng: await _iconPng(const Color(0xFFFFCC00), 'Я'),
        ),
        InstalledApp(
          packageName: 'com.discord',
          label: 'Discord',
          iconPng: await _iconPng(const Color(0xFF5865F2), 'D'),
        ),
      ];
    });

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
            // Сайт в «Правилах по сайтам» уже есть: иначе экран честно
            // предупредит, что режим «только список» ядру не уходит.
            coreConfigProvider.overrideWith(
              (ref) => CoreConfigNotifier()
                ..hydrate(
                  const CoreConfig(
                    allowDomains: 'youtube.com, chatgpt.com',
                  ),
                ),
            ),
            installedAppsLoaderProvider.overrideWithValue(
              () async => installed,
            ),
          ],
          child: MaterialApp.router(
            debugShowCheckedModeBanner: false,
            theme: demoTheme(),
            routerConfig: demoRouter(
              initial: '/settings/app-rules',
              routes: <String, WidgetBuilder>{
                '/home': (_) => const SizedBox.shrink(),
                '/settings': (_) => const SizedBox.shrink(),
                '/settings/app-rules': (_) => const AppRulesScreen(),
              },
            ),
          ),
        ),
      ),
    );
    await rec.settle();
    await rec.shot(1300);

    // Режим «Только список через VPN».
    await rec.tapAnimated(find.text(SplitMode.onlySelected.appsTitle),
        upSteps: 3);
    await rec.shot(1300);

    // Пикер установленных приложений.
    final add = find.byKey(const ValueKey('app-rules-add'));
    await tester.ensureVisible(add);
    await tester.pump();
    await rec.tapAnimated(add, upSteps: 1);
    await rec.animate(steps: 6, step: const Duration(milliseconds: 60));
    await rec.settle(pumps: 4);
    await rec.shot(1100);

    await rec.tapAnimated(
      find.byKey(const ValueKey('app-rules-pick-org.telegram.messenger')),
      upSteps: 3,
    );
    await rec.shot(700);
    await rec.tapAnimated(
      find.byKey(const ValueKey('app-rules-pick-com.google.android.youtube')),
      upSteps: 3,
    );
    await rec.shot(1100);

    // Закрываем лист: выбранное в списке.
    await tester.tap(find.byType(IconBtn).last);
    await rec.animate(steps: 6, step: const Duration(milliseconds: 60));
    await rec.settle(pumps: 3);
    await rec.shot(2600);

    rec.finish();
    await demoTearDownTree(tester);
  }, skip: !demoEnabled, timeout: const Timeout(Duration(minutes: 5)));
}
