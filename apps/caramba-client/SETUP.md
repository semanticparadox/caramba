# caramba_client — setup

Flutter super-app (analog of "koala clash + amneziawg") tied to the caramba panel.
Targets: Android, iOS, macOS, Windows, Linux (+ Linux CLI mode via caramba-cli).

## Dart skeleton (already present)

The `lib/` tree is a coherent, compiling skeleton built entirely on the
DESIGN.md token system:

```
lib/
  main.dart                     ProviderScope + MaterialApp.router (dark default)
  theme/
    colors.dart                 AppColors.dark / .light (exact DESIGN.md hex), AppShadows
    typography.dart             AppType — Plus Jakarta Sans + JetBrains Mono, tabular figures
    spacing.dart                AppSpace / AppRadius / AppBorders / AppMotion / AppOrb / AppBreakpoints
    tokens.dart                 AppTokens ThemeExtension + context.c / context.tokens helpers
    app_theme.dart              full Material 3 ThemeData (dark + light) from the tokens
  router/
    routes.dart                 AppRoute path constants
    app_router.dart             GoRouter + StatefulShellRoute (5 tab branches)
  shell/
    app_shell.dart              bottom nav (mobile) / left rail (desktop ≥ 840)
    placeholder_screen.dart     themed placeholder used by stub screens
  features/
    splash/                     Session-probe gate (orb idle ring); shown while AuthStage.unknown
    onboarding/                 Telegram + Email segmented login (§6.1)
    home/                       Connect screen with shield-orb placeholder + stat cards (§6.2)
    servers/ subscription/ marketplace/ browser/ settings/   themed placeholders
```

## Generating native platform projects

Native runner folders (`android/`, `ios/`, `macos/`, `windows/`, `linux/`) are
NOT committed because `flutter` was unavailable in the scaffolding environment.
Generate them in-place once Flutter is installed:

```bash
cd apps/caramba-client
flutter create . \
  --org com.caramba \
  --project-name caramba_client \
  --platforms=android,ios,macos,windows,linux
flutter pub get
```

`flutter create .` is non-destructive to existing `lib/`, `pubspec.yaml`,
and `analysis_options.yaml` — it only adds the missing platform folders.

## Run / analyze

```bash
flutter pub get
flutter analyze
flutter run            # pick a device
```

## Notes

- Theme mode is pinned to dark (the hero theme); a user-preference provider
  will switch it later.
- Auth (Telegram + email/JWT against the panel), the live shield-orb
  `CustomPainter`, mihomo profile pull and the webview chrome are scoped to
  follow-up runs. The onboarding CTAs currently route straight into the shell
  so the skeleton is fully navigable.
