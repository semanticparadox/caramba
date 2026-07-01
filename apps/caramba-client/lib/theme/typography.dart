import 'package:flutter/widgets.dart';

/// exarobot type scale (DESIGN.md §2).
///
/// System stack only: SF Pro on Apple platforms, Segoe UI / Roboto elsewhere,
/// via the platform default plus explicit fallbacks. No webfont download, no
/// Plus Jakarta Sans (the old slop theme used google_fonts). Mono is the system
/// monospace (SF Mono / Menlo / Consolas) with JetBrains Mono as a fallback if
/// bundled, used only for technical data (latency, bytes, codes, prices).
///
/// Tabular figures are enabled on [display] and all `mono*` styles so changing
/// numbers do not jitter. Pass a text color from `AppColors` at call sites.
abstract final class AppType {
  static const _tabular = [FontFeature.tabularFigures()];

  /// System sans fallback chain (mirrors the demo `--sans`). Passing `null`
  /// for `fontFamily` lets Flutter pick the platform UI font; the fallbacks
  /// cover platforms whose default is not SF Pro.
  static const List<String> sansFallback = <String>[
    'SF Pro Text',
    'SF Pro Display',
    '.SF UI Text',
    'Segoe UI',
    'Roboto',
    'system-ui',
  ];

  /// System monospace fallback chain (mirrors the demo `--mono`).
  static const List<String> monoFallback = <String>[
    'SF Mono',
    'ui-monospace',
    'JetBrains Mono',
    'Menlo',
    'Consolas',
    'monospace',
  ];

  static TextStyle _sans(
    double size,
    double height,
    FontWeight w,
    double tracking,
  ) =>
      TextStyle(
        fontFamilyFallback: sansFallback,
        fontSize: size,
        height: height / size,
        fontWeight: w,
        letterSpacing: tracking,
      );

  static TextStyle _mono(double size, double height, FontWeight w) =>
      TextStyle(
        fontFamily: 'SF Mono',
        fontFamilyFallback: monoFallback,
        fontSize: size,
        height: height / size,
        fontWeight: w,
        fontFeatures: _tabular,
      );

  // Scale (px): t1 27, t2 20, t3 15, t4 13, t5 11 (demo). Line-heights tuned
  // for calm reading. Tracking negative on large display/title text only.
  static TextStyle get display =>
      _sans(34, 40, FontWeight.w700, -0.5).copyWith(fontFeatures: _tabular);
  static TextStyle get headline => _sans(27, 33, FontWeight.w700, -0.5);
  static TextStyle get titleLg => _sans(20, 26, FontWeight.w600, -0.2);
  static TextStyle get titleMd => _sans(17, 23, FontWeight.w600, -0.1);
  static TextStyle get bodyLg => _sans(16, 24, FontWeight.w500, 0);
  static TextStyle get bodyMd => _sans(15, 22, FontWeight.w500, 0);
  static TextStyle get bodySm => _sans(13, 18, FontWeight.w500, 0);
  static TextStyle get label => _sans(15, 18, FontWeight.w600, 0);
  static TextStyle get caption => _sans(11, 14, FontWeight.w600, 0.6);

  static TextStyle get monoMd => _mono(13, 18, FontWeight.w600);
  static TextStyle get monoSm => _mono(11, 15, FontWeight.w600);
}
