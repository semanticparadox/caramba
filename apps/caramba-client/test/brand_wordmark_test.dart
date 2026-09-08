import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/data/models/branding.dart';
import 'package:caramba_client/features/branding/brand_wordmark.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  Future<void> pump(WidgetTester tester, Branding branding) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [activeBrandingProvider.overrideWithValue(branding)],
        child: const MaterialApp(
          home: Scaffold(body: SizedBox(width: 240, child: BrandWordmark())),
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets(
    'default product shows bundled ship and product name',
    (tester) async {
      await pump(tester, Branding.fallback);
      expect(find.text('Caramba Connect'), findsOneWidget);
      expect(find.byType(Image), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
    skip: kBrandName != 'Caramba Connect',
  );

  testWidgets('runtime tenant name does not inherit the Caramba ship',
      (tester) async {
    await pump(tester, const Branding(enabled: true, brandName: 'Example VPN'));
    expect(find.text('Example VPN'), findsOneWidget);
    expect(find.byType(Image), findsNothing);
  });

  testWidgets('product aliases retain the ship regardless of case',
      (tester) async {
    await pump(tester, const Branding(enabled: true, brandName: 'cArAmBa'));
    expect(find.text('cArAmBa'), findsOneWidget);
    expect(find.byType(Image), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  // Run this file with --dart-define=CARAMBA_BRAND_NAME=Custom to exercise the
  // real build-time configuration rather than a test-only widget parameter.
  testWidgets(
    'build-time tenant name does not inherit the Caramba ship',
    (tester) async {
      await pump(tester, Branding.fallback);
      expect(find.text('Custom'), findsOneWidget);
      expect(find.byType(Image), findsNothing);
    },
    skip: kBrandName != 'Custom',
  );
}
