// Smoke test for the Caramba Connect app root.
//
// Pumps the real root widget ([CarambaApp]) inside a [ProviderScope] and
// asserts the app boots: the router mounts, the splash route renders and the
// dark hero theme is applied. Plugin method channels the boot path touches
// (secure storage for the session restore, app_links for deep links) are
// stubbed so the test stays hermetic and offline.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/main.dart';

/// Method channels touched while the app boots. Returning `null` for every
/// call models a clean install: no stored tokens, no inbound deep link.
const List<MethodChannel> _bootChannels = <MethodChannel>[
  MethodChannel('plugins.it_nomads.com/flutter_secure_storage'),
  MethodChannel('com.llfbandit.app_links/messages'),
];

void main() {
  late TestDefaultBinaryMessenger messenger;

  setUp(() {
    // Локальные настройки читаются на старте (тема, first-run, режим туннеля):
    // чистая установка = пустое хранилище.
    SharedPreferences.setMockInitialValues(<String, Object>{});
    messenger =
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    for (final channel in _bootChannels) {
      messenger.setMockMethodCallHandler(channel, (call) async => null);
    }
  });

  tearDown(() {
    for (final channel in _bootChannels) {
      messenger.setMockMethodCallHandler(channel, null);
    }
  });

  testWidgets('app root boots into the splash route with the dark theme', (
    tester,
  ) async {
    await tester.pumpWidget(const ProviderScope(child: CarambaApp()));
    await tester.pump();

    // The router mounted a MaterialApp and something rendered inside it.
    final app = tester.widget<MaterialApp>(find.byType(MaterialApp));
    expect(app.routerConfig, isNotNull);
    expect(app.themeMode, ThemeMode.dark);
    expect(app.darkTheme, isNotNull);
    expect(app.darkTheme!.brightness, Brightness.dark);
    expect(find.byType(Scaffold), findsWidgets);
    expect(tester.takeException(), isNull);
  });
}
