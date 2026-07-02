import 'package:flutter/painting.dart';

/// exarobot color tokens (DESIGN.md §1).
///
/// Neutral monochrome base. There is no brand hue. Color is reserved for the
/// connection-status language only: green = connected, amber = connecting,
/// red = error. Everything else is neutral.
///
/// The `accent` token is the single high-contrast neutral used for the primary
/// button and selection emphasis: near-white on dark, near-black on light. It
/// is NOT a colored brand accent. (See ANTI-SLOP.md "the lila rule".)
///
/// [AppColors.dark] is the default; [AppColors.light] is the faithful sibling.
/// The token NAMES are stable so screens keep compiling; only the VALUES and
/// their meaning changed when the purple "Electric Iris" theme was dropped.
class AppColors {
  // ---- Backgrounds & surfaces
  final Color bgBase;
  final Color bgCanvas;
  final Color surface1;
  final Color surface2;
  final Color surface3;
  final Color surfaceInset;

  // ---- Borders
  final Color borderSubtle;
  final Color borderStrong;

  // ---- Accent (neutral high-contrast: primary button, selection emphasis).
  // Not a hue. accent == text.hi-grade neutral; subtle == a flat neutral fill.
  final Color accent;
  final Color accentVariant;
  final Color accentDeep;
  final Color accentSubtle;

  // ---- State (the ONLY colors in the app)
  final Color success; // connected (green)
  final Color successDeep;
  final Color successSubtle;
  final Color warning; // connecting (amber)
  final Color warningSubtle;
  final Color danger; // error (red)
  final Color dangerDeep;
  final Color dangerSubtle;
  final Color info; // neutral-informational; kept neutral, no blue brand

  // ---- Text
  final Color textHi;
  final Color textMed;
  final Color textLow;
  final Color textOnAccent;
  final Color textOnSuccess;

  // ---- Misc
  final Color overlayScrim;
  final Color shimmerBase;
  final Color shimmerHi;

  const AppColors({
    required this.bgBase,
    required this.bgCanvas,
    required this.surface1,
    required this.surface2,
    required this.surface3,
    required this.surfaceInset,
    required this.borderSubtle,
    required this.borderStrong,
    required this.accent,
    required this.accentVariant,
    required this.accentDeep,
    required this.accentSubtle,
    required this.success,
    required this.successDeep,
    required this.successSubtle,
    required this.warning,
    required this.warningSubtle,
    required this.danger,
    required this.dangerDeep,
    required this.dangerSubtle,
    required this.info,
    required this.textHi,
    required this.textMed,
    required this.textLow,
    required this.textOnAccent,
    required this.textOnSuccess,
    required this.overlayScrim,
    required this.shimmerBase,
    required this.shimmerHi,
  });

  /// Returns a copy with ONLY the four neutral-emphasis accent fields replaced
  /// by an operator [brand] hue. Every status field (success/warning/danger and
  /// their deep/subtle variants), every surface, border and text token is left
  /// EXACTLY as-is. This is the single sanctioned seam for runtime brand theming
  /// (P3 contract E): the connection-status language (green/amber/red) is never
  /// reachable from here, so a brand accent can never read as connection state.
  ///
  /// [brand] must already be a vetted, opaque color (clamped/rejected upstream
  /// by `parseBrandAccent` — purple/violet/indigo and status-like hues are gone
  /// before they reach this method). We derive the four accent fields from it:
  ///   * accent        = brand (primary button / selection emphasis)
  ///   * accentVariant = brand (hover/secondary; kept = accent, no gradient)
  ///   * accentDeep    = brand (pressed/inverse; kept = accent, no glow)
  ///   * accentSubtle  = brand @ ~8% alpha (selected-row/chip neutral fill)
  AppColors withBrandAccent(Color brand) {
    final subtle = brand.withValues(alpha: 0.08);
    return AppColors(
      bgBase: bgBase,
      bgCanvas: bgCanvas,
      surface1: surface1,
      surface2: surface2,
      surface3: surface3,
      surfaceInset: surfaceInset,
      borderSubtle: borderSubtle,
      borderStrong: borderStrong,
      // ---- ONLY these four change ----
      accent: brand,
      accentVariant: brand,
      accentDeep: brand,
      accentSubtle: subtle,
      // ---- status & everything else: untouched ----
      success: success,
      successDeep: successDeep,
      successSubtle: successSubtle,
      warning: warning,
      warningSubtle: warningSubtle,
      danger: danger,
      dangerDeep: dangerDeep,
      dangerSubtle: dangerSubtle,
      info: info,
      textHi: textHi,
      textMed: textMed,
      textLow: textLow,
      textOnAccent: _readableOn(brand),
      textOnSuccess: textOnSuccess,
      overlayScrim: overlayScrim,
      shimmerBase: shimmerBase,
      shimmerHi: shimmerHi,
    );
  }

  /// Picks near-black or near-white for text/icons sitting ON the brand accent,
  /// by relative luminance, so the primary button label stays legible whatever
  /// vetted hue the operator chose. Mirrors the contrast intent of the neutral
  /// [textOnAccent] tokens without ever touching a status color.
  static Color _readableOn(Color bg) {
    final lum = bg.computeLuminance();
    return lum > 0.5 ? const Color(0xFF0A0A0A) : const Color(0xFFFAFAFA);
  }

  /// Retained for source compatibility with screens that referenced it. There
  /// is no cyan in the de-slopped language; the connected ring is solid green.
  /// This now aliases [success] so any leftover reference stays on-palette.
  Color get cyanKiss => success;

  /// "Gradient" tokens are retained as names only (screens reference them) but
  /// are now flat single-color gradients: no purple to lilac, no glows on fills.
  /// Primary emphasis is a solid neutral; connected emphasis is solid green.
  LinearGradient get accentGradient => LinearGradient(
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
    colors: [accent, accent],
  );

  /// Connected emphasis: solid green (no cyan kiss).
  LinearGradient get connectedGradient => LinearGradient(
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
    colors: [success, success],
  );

  /// Dark theme. Neutral near-black base (#0A0A0A), surfaces step up in
  /// lightness. Matches demo/caramba-demo.html.
  static const AppColors dark = AppColors(
    bgBase: Color(0xFF0A0A0A),
    bgCanvas: Color(0xFF0C0C0C),
    surface1: Color(0xFF161616),
    surface2: Color(0xFF1E1E1E),
    surface3: Color(0xFF272727),
    surfaceInset: Color(0xFF0E0E0E),
    borderSubtle: Color(0xFF2A2A2A),
    borderStrong: Color(0xFF3D3D3D),
    // Neutral high-contrast "accent" = near-white. Primary button fill.
    accent: Color(0xFFFAFAFA),
    accentVariant: Color(0xFFEDEDED),
    accentDeep: Color(0xFFD4D4D4),
    accentSubtle: Color(0x14FAFAFA), // 8% neutral fill (selected rows/chips)
    // Status colors only.
    success: Color(0xFF30D158),
    successDeep: Color(0xFF248A3D),
    successSubtle: Color(0x1A30D158),
    warning: Color(0xFFFF9F0A),
    warningSubtle: Color(0x1AFF9F0A),
    danger: Color(0xFFFF453A),
    dangerDeep: Color(0xFFD70015),
    dangerSubtle: Color(0x1AFF453A),
    // "info" stays neutral: no second brand hue.
    info: Color(0xFFA0A0A0),
    textHi: Color(0xFFFAFAFA),
    textMed: Color(0xFFA0A0A0),
    // #888888: clears AA 4.5:1 on surface1 (#161616) for 13px card subtitles
    // (#7C7C7C was 4.34:1 there, a borderline fail), still clearly below textMed.
    textLow: Color(0xFF888888),
    textOnAccent: Color(0xFF0A0A0A), // dark text on the near-white accent
    textOnSuccess: Color(0xFF062A12),
    overlayScrim: Color(0x99000000), // ~60%
    shimmerBase: Color(0xFF161616),
    shimmerHi: Color(0xFF232323),
  );

  /// Light theme. Warm-neutral, not stark. Status colors deepened for AA
  /// contrast on white. Matches demo/caramba-demo.html.
  static const AppColors light = AppColors(
    bgBase: Color(0xFFFAFAFA),
    bgCanvas: Color(0xFFFFFFFF),
    surface1: Color(0xFFFFFFFF),
    surface2: Color(0xFFF4F4F4),
    surface3: Color(0xFFECECEC),
    surfaceInset: Color(0xFFF1F1F1),
    borderSubtle: Color(0xFFE5E5E5),
    borderStrong: Color(0xFFD2D2D2),
    // Neutral high-contrast "accent" = near-black. Primary button fill (invert).
    accent: Color(0xFF0A0A0A),
    accentVariant: Color(0xFF1F1F1F),
    accentDeep: Color(0xFF000000),
    accentSubtle: Color(0x0F0A0A0A), // ~6% neutral fill
    // Deepened to #177A41: holds WCAG AA (>=4.5:1) on white, since `success`
    // is used as actual text/border color (Tag label/border, referral value,
    // good-ping rows). The brighter #1E9E54 only cleared 3.46:1 and failed AA
    // for small green text/badges in light mode.
    success: Color(0xFF177A41),
    successDeep: Color(0xFF136636),
    successSubtle: Color(0x14177A41),
    warning: Color(0xFFA85D00),
    warningSubtle: Color(0x14A85D00),
    danger: Color(0xFFC8102E),
    dangerDeep: Color(0xFFA00B24),
    dangerSubtle: Color(0x14C8102E),
    info: Color(0xFF585858),
    textHi: Color(0xFF0A0A0A),
    textMed: Color(0xFF585858),
    textLow: Color(0xFF6E6E6E),
    textOnAccent: Color(0xFFFFFFFF), // white text on the near-black accent
    textOnSuccess: Color(0xFFFFFFFF),
    overlayScrim: Color(0x73000000), // ~45%
    shimmerBase: Color(0xFFECECEC),
    shimmerHi: Color(0xFFF6F6F6),
  );
}

/// Elevation / shadow tokens (DESIGN.md §3.3).
///
/// Flat, neutral, soft. No colored glows. The `glow*` tokens are retained by
/// name (screens reference them on the orb / primary CTA) but are now a faint
/// neutral lift instead of a purple/green halo, honoring the no-glow rule. A
/// connected orb communicates state through the green ring + check glyph, not a
/// glow.
abstract final class AppShadows {
  static const card = [
    BoxShadow(color: Color(0x40000000), blurRadius: 8, offset: Offset(0, 2)),
  ];
  static const raised = [
    BoxShadow(color: Color(0x59000000), blurRadius: 24, offset: Offset(0, 8)),
  ];
  static const sheet = [
    BoxShadow(color: Color(0x73000000), blurRadius: 48, offset: Offset(0, 16)),
  ];

  // Retained names; intentionally near-flat neutral (no colored glow).
  static const glowAccent = [
    BoxShadow(color: Color(0x14000000), blurRadius: 12, offset: Offset(0, 2)),
  ];
  static const glowConnected = [
    BoxShadow(color: Color(0x14000000), blurRadius: 12, offset: Offset(0, 2)),
  ];

  // Light-theme elevation.
  static const cardLight = [
    BoxShadow(color: Color(0x12000000), blurRadius: 3, offset: Offset(0, 1)),
  ];
  static const raisedLight = [
    BoxShadow(color: Color(0x1A000000), blurRadius: 18, offset: Offset(0, 6)),
  ];
  static const sheetLight = [
    BoxShadow(color: Color(0x24000000), blurRadius: 40, offset: Offset(0, 16)),
  ];
  static const glowAccentLight = [
    BoxShadow(color: Color(0x0F000000), blurRadius: 10, offset: Offset(0, 2)),
  ];
  static const glowConnectedLight = [
    BoxShadow(color: Color(0x0F000000), blurRadius: 10, offset: Offset(0, 2)),
  ];
}
