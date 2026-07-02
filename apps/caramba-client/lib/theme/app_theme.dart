import 'package:flutter/material.dart';

import 'package:caramba_client/theme/colors.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';

/// Builds the full exarobot [ThemeData] for a given palette + brightness,
/// translating the DESIGN.md tokens into Material 3 component themes.
class AppTheme {
  const AppTheme._();

  /// Dark hero theme. [brandAccent] (P3 contract E) is an OPTIONAL, already
  /// vetted operator accent: when non-null it replaces ONLY the four neutral
  /// accent tokens via [AppColors.withBrandAccent]; status colors are untouched.
  /// Null => the default neutral Caramba Connect look.
  static ThemeData dark({Color? brandAccent}) =>
      _build(_palette(AppColors.dark, brandAccent), Brightness.dark);

  /// Light sibling. Same [brandAccent] contract as [dark].
  static ThemeData light({Color? brandAccent}) =>
      _build(_palette(AppColors.light, brandAccent), Brightness.light);

  /// Applies the optional brand accent to a base palette. The accent has already
  /// been clamped/rejected upstream (purple/violet/indigo and status-like hues
  /// removed); here we only fold it into the neutral accent tokens.
  static AppColors _palette(AppColors base, Color? brandAccent) =>
      brandAccent == null ? base : base.withBrandAccent(brandAccent);

  static ThemeData _build(AppColors c, Brightness b) {
    // Tokens carry the SAME palette as the ColorScheme so context.tokens.accent
    // also reflects the brand override (status tokens stay the originals).
    final base = b == Brightness.dark ? AppTokens.dark() : AppTokens.light();
    final tokens = base.copyWith(colors: c);

    final scheme = ColorScheme(
      brightness: b,
      primary: c.accent,
      onPrimary: c.textOnAccent,
      primaryContainer: c.accentSubtle,
      onPrimaryContainer: c.accent,
      secondary: c.accentVariant,
      onSecondary: c.textOnAccent,
      tertiary: c.success,
      onTertiary: c.textOnSuccess,
      error: c.danger,
      onError: b == Brightness.dark ? c.textHi : const Color(0xFFFFFFFF),
      errorContainer: c.dangerSubtle,
      onErrorContainer: c.danger,
      surface: c.surface1,
      onSurface: c.textHi,
      surfaceContainerLowest: c.bgBase,
      surfaceContainerLow: c.bgCanvas,
      surfaceContainer: c.surface1,
      surfaceContainerHigh: c.surface2,
      surfaceContainerHighest: c.surface3,
      onSurfaceVariant: c.textMed,
      outline: c.borderStrong,
      outlineVariant: c.borderSubtle,
      scrim: c.overlayScrim,
      inverseSurface: c.textHi,
      onInverseSurface: c.bgBase,
      inversePrimary: c.accentDeep,
      shadow: const Color(0xFF000000),
    );

    final textTheme = TextTheme(
      displayLarge: AppType.display.copyWith(color: c.textHi),
      displayMedium: AppType.headline.copyWith(color: c.textHi),
      headlineLarge: AppType.headline.copyWith(color: c.textHi),
      headlineMedium: AppType.titleLg.copyWith(color: c.textHi),
      titleLarge: AppType.titleLg.copyWith(color: c.textHi),
      titleMedium: AppType.titleMd.copyWith(color: c.textHi),
      titleSmall: AppType.label.copyWith(color: c.textHi),
      bodyLarge: AppType.bodyLg.copyWith(color: c.textHi),
      bodyMedium: AppType.bodyMd.copyWith(color: c.textHi),
      bodySmall: AppType.bodySm.copyWith(color: c.textMed),
      labelLarge: AppType.label.copyWith(color: c.textHi),
      labelMedium: AppType.label.copyWith(color: c.textMed),
      labelSmall: AppType.caption.copyWith(color: c.textLow),
    );

    return ThemeData(
      useMaterial3: true,
      brightness: b,
      colorScheme: scheme,
      scaffoldBackgroundColor: c.bgCanvas,
      canvasColor: c.bgCanvas,
      splashFactory: InkSparkle.splashFactory,
      textTheme: textTheme,
      extensions: <ThemeExtension<dynamic>>[tokens],

      dividerTheme: DividerThemeData(
        color: c.borderSubtle,
        thickness: AppBorders.hairline,
        space: AppBorders.hairline,
      ),

      appBarTheme: AppBarTheme(
        backgroundColor: c.bgCanvas,
        surfaceTintColor: Colors.transparent,
        foregroundColor: c.textHi,
        elevation: 0,
        scrolledUnderElevation: 0,
        centerTitle: false,
        titleTextStyle: AppType.titleLg.copyWith(color: c.textHi),
      ),

      cardTheme: CardThemeData(
        color: c.surface1,
        surfaceTintColor: Colors.transparent,
        elevation: 0,
        margin: EdgeInsets.zero,
        shape: RoundedRectangleBorder(
          borderRadius: AppRadius.r16,
          side: BorderSide(color: c.borderSubtle, width: AppBorders.hairline),
        ),
      ),

      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          backgroundColor: c.accent, // neutral: white on dark / black on light
          foregroundColor: c.textOnAccent,
          disabledBackgroundColor: c.surface2,
          disabledForegroundColor: c.textLow,
          minimumSize: const Size.fromHeight(50),
          shape: const RoundedRectangleBorder(borderRadius: AppRadius.r14),
          textStyle: AppType.label,
        ),
      ),

      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          backgroundColor: c.surface2,
          foregroundColor: c.textHi,
          minimumSize: const Size.fromHeight(50),
          side: BorderSide(color: c.borderSubtle, width: AppBorders.hairline),
          shape: const RoundedRectangleBorder(borderRadius: AppRadius.r14),
          textStyle: AppType.label,
        ),
      ),

      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          foregroundColor: c.textHi,
          minimumSize: const Size.fromHeight(44),
          shape: const RoundedRectangleBorder(borderRadius: AppRadius.r12),
          textStyle: AppType.label,
        ),
      ),

      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(
          foregroundColor: c.textMed,
          minimumSize: const Size(44, 44),
        ),
      ),

      iconTheme: IconThemeData(color: c.textMed, size: 24),

      chipTheme: ChipThemeData(
        backgroundColor: c.surface2,
        selectedColor: c.accentSubtle,
        labelStyle: AppType.label.copyWith(color: c.textHi),
        secondaryLabelStyle: AppType.label.copyWith(color: c.accent),
        side: BorderSide(color: c.borderSubtle, width: AppBorders.hairline),
        shape: const StadiumBorder(),
        padding: const EdgeInsets.symmetric(
          horizontal: AppSpace.s3,
          vertical: AppSpace.s2,
        ),
      ),

      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: c.surface2,
        hintStyle: AppType.bodyMd.copyWith(color: c.textLow),
        labelStyle: AppType.bodyMd.copyWith(color: c.textMed),
        floatingLabelStyle: AppType.bodySm.copyWith(color: c.accent),
        contentPadding: const EdgeInsets.symmetric(
          horizontal: AppSpace.s4,
          vertical: AppSpace.s4,
        ),
        border: OutlineInputBorder(
          borderRadius: AppRadius.r16,
          borderSide: BorderSide(
            color: c.borderStrong,
            width: AppBorders.input,
          ),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: AppRadius.r16,
          borderSide: BorderSide(
            color: c.borderStrong,
            width: AppBorders.input,
          ),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: AppRadius.r16,
          borderSide: BorderSide(color: c.accent, width: AppBorders.input),
        ),
        errorBorder: OutlineInputBorder(
          borderRadius: AppRadius.r16,
          borderSide: BorderSide(color: c.danger, width: AppBorders.input),
        ),
        focusedErrorBorder: OutlineInputBorder(
          borderRadius: AppRadius.r16,
          borderSide: BorderSide(color: c.danger, width: AppBorders.input),
        ),
        errorStyle: AppType.bodySm.copyWith(color: c.danger),
      ),

      switchTheme: SwitchThemeData(
        thumbColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.selected) ? c.textOnAccent : c.textMed,
        ),
        trackColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.selected) ? c.accent : c.surface3,
        ),
        trackOutlineColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.selected)
              ? Colors.transparent
              : c.borderStrong,
        ),
      ),

      bottomSheetTheme: BottomSheetThemeData(
        backgroundColor: c.surface2,
        surfaceTintColor: Colors.transparent,
        modalBackgroundColor: c.surface2,
        shape: const RoundedRectangleBorder(borderRadius: AppRadius.sheetTop),
        showDragHandle: true,
        dragHandleColor: c.surface3,
        dragHandleSize: const Size(36, 4),
      ),

      dialogTheme: DialogThemeData(
        backgroundColor: c.surface2,
        surfaceTintColor: Colors.transparent,
        shape: const RoundedRectangleBorder(borderRadius: AppRadius.r22),
        titleTextStyle: AppType.titleLg.copyWith(color: c.textHi),
        contentTextStyle: AppType.bodyMd.copyWith(color: c.textMed),
      ),

      // Active item is just high-contrast text/icon, no pill (matches demo).
      navigationBarTheme: NavigationBarThemeData(
        backgroundColor: c.surface1,
        surfaceTintColor: Colors.transparent,
        indicatorColor: Colors.transparent,
        elevation: 0,
        height: 64,
        labelBehavior: NavigationDestinationLabelBehavior.alwaysShow,
        iconTheme: WidgetStateProperty.resolveWith(
          (s) => IconThemeData(
            color: s.contains(WidgetState.selected) ? c.textHi : c.textLow,
          ),
        ),
        labelTextStyle: WidgetStateProperty.resolveWith(
          (s) => AppType.caption.copyWith(
            color: s.contains(WidgetState.selected) ? c.textHi : c.textLow,
          ),
        ),
      ),

      navigationRailTheme: NavigationRailThemeData(
        backgroundColor: c.surface1,
        indicatorColor: Colors.transparent,
        selectedIconTheme: IconThemeData(color: c.textHi),
        unselectedIconTheme: IconThemeData(color: c.textLow),
        selectedLabelTextStyle: AppType.caption.copyWith(color: c.textHi),
        unselectedLabelTextStyle: AppType.caption.copyWith(color: c.textLow),
      ),

      listTileTheme: ListTileThemeData(
        iconColor: c.textMed,
        textColor: c.textHi,
        titleTextStyle: AppType.bodyLg.copyWith(color: c.textHi),
        subtitleTextStyle: AppType.bodySm.copyWith(color: c.textMed),
        shape: const RoundedRectangleBorder(borderRadius: AppRadius.r16),
      ),

      snackBarTheme: SnackBarThemeData(
        backgroundColor: c.surface3,
        contentTextStyle: AppType.bodyMd.copyWith(color: c.textHi),
        behavior: SnackBarBehavior.floating,
        shape: const RoundedRectangleBorder(borderRadius: AppRadius.r12),
      ),

      progressIndicatorTheme: ProgressIndicatorThemeData(
        color: c.accent,
        linearTrackColor: c.surface3,
        circularTrackColor: c.surface3,
      ),

      sliderTheme: SliderThemeData(
        activeTrackColor: c.accent,
        inactiveTrackColor: c.surface3,
        thumbColor: c.accent,
        overlayColor: c.accentSubtle,
      ),

      tooltipTheme: TooltipThemeData(
        decoration: BoxDecoration(
          color: c.surface3,
          borderRadius: AppRadius.r8,
        ),
        textStyle: AppType.bodySm.copyWith(color: c.textHi),
      ),
    );
  }
}
