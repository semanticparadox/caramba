// Признак десктопа читается из defaultTargetPlatform, а не из dart:io.
//
// Тест сторожит ровно ту ошибку, которая обесценила бы весь остальной набор:
// возьми `isDesktopPlatform` `Platform.isMacOS`, и все 975 тестов на этом Маке
// поехали бы по десктопным веткам, а мобильный шелл перестал бы проверяться,
// оставаясь при этом зелёным.

import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/desktop/desktop_platform.dart';

void main() {
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  test('без переопределения тест остаётся мобильным', () {
    // Хост тут macOS, а платформа теста android: именно поэтому существующие
    // тесты не видят десктопных веток.
    expect(defaultTargetPlatform, TargetPlatform.android);
    expect(isDesktopPlatform, isFalse);
    expect(isMacOSPlatform, isFalse);
    expect(isWindowsPlatform, isFalse);
    expect(isLinuxPlatform, isFalse);
  });

  for (final platform in const [
    TargetPlatform.macOS,
    TargetPlatform.windows,
    TargetPlatform.linux,
  ]) {
    test('$platform это десктоп', () {
      debugDefaultTargetPlatformOverride = platform;
      expect(isDesktopPlatform, isTrue);
    });
  }

  for (final platform in const [
    TargetPlatform.android,
    TargetPlatform.iOS,
    TargetPlatform.fuchsia,
  ]) {
    test('$platform это не десктоп', () {
      debugDefaultTargetPlatformOverride = platform;
      expect(isDesktopPlatform, isFalse);
    });
  }

  test('частные признаки платформ не пересекаются', () {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    expect(
      [isMacOSPlatform, isWindowsPlatform, isLinuxPlatform],
      [isTrue, isFalse, isFalse],
    );

    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    expect(
      [isMacOSPlatform, isWindowsPlatform, isLinuxPlatform],
      [isFalse, isTrue, isFalse],
    );

    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    expect(
      [isMacOSPlatform, isWindowsPlatform, isLinuxPlatform],
      [isFalse, isFalse, isTrue],
    );
  });

  test('переопределение снято между тестами', () {
    expect(debugDefaultTargetPlatformOverride, isNull);
  });
}
