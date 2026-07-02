import 'package:flutter/material.dart';

import 'package:caramba_client/theme/colors.dart';

/// A [ThemeExtension] that carries the full exarobot [AppColors] palette
/// (and resolved shadow/glow tokens) through the widget tree, so widgets can
/// read brand tokens that the stock Material [ColorScheme] cannot express.
///
/// Usage: `context.tokens.accent`, `context.tokens.success`, etc.
@immutable
class AppTokens extends ThemeExtension<AppTokens> {
  final AppColors colors;

  /// Elevation shadows resolved for the active brightness.
  final List<BoxShadow> elevCard;
  final List<BoxShadow> elevRaised;
  final List<BoxShadow> elevSheet;
  final List<BoxShadow> glowAccent;
  final List<BoxShadow> glowConnected;

  const AppTokens({
    required this.colors,
    required this.elevCard,
    required this.elevRaised,
    required this.elevSheet,
    required this.glowAccent,
    required this.glowConnected,
  });

  factory AppTokens.dark() => const AppTokens(
    colors: AppColors.dark,
    elevCard: AppShadows.card,
    elevRaised: AppShadows.raised,
    elevSheet: AppShadows.sheet,
    glowAccent: AppShadows.glowAccent,
    glowConnected: AppShadows.glowConnected,
  );

  factory AppTokens.light() => const AppTokens(
    colors: AppColors.light,
    elevCard: AppShadows.cardLight,
    elevRaised: AppShadows.raisedLight,
    elevSheet: AppShadows.sheetLight,
    glowAccent: AppShadows.glowAccentLight,
    glowConnected: AppShadows.glowConnectedLight,
  );

  @override
  AppTokens copyWith({
    AppColors? colors,
    List<BoxShadow>? elevCard,
    List<BoxShadow>? elevRaised,
    List<BoxShadow>? elevSheet,
    List<BoxShadow>? glowAccent,
    List<BoxShadow>? glowConnected,
  }) => AppTokens(
    colors: colors ?? this.colors,
    elevCard: elevCard ?? this.elevCard,
    elevRaised: elevRaised ?? this.elevRaised,
    elevSheet: elevSheet ?? this.elevSheet,
    glowAccent: glowAccent ?? this.glowAccent,
    glowConnected: glowConnected ?? this.glowConnected,
  );

  @override
  AppTokens lerp(ThemeExtension<AppTokens>? other, double t) {
    // Tokens are discrete brand palettes; snap rather than interpolate.
    if (other is! AppTokens) return this;
    return t < 0.5 ? this : other;
  }
}

/// Ergonomic access to exarobot tokens from any [BuildContext].
extension AppTokensX on BuildContext {
  AppTokens get tokens =>
      Theme.of(this).extension<AppTokens>() ?? AppTokens.dark();

  /// Shorthand for the active palette.
  AppColors get c => tokens.colors;
}
