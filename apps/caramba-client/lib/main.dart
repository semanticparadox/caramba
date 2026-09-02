import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/state/bootstrap_state.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/app_theme.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const ProviderScope(child: CarambaApp()));
}

/// Root application widget. Dark is the hero theme and the default; light is a
/// faithful sibling. Theme mode follows the user preference in [settingsProvider].
class CarambaApp extends ConsumerWidget {
  const CarambaApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // Стартуем чтение локальных настроек с первого кадра: тема и решение про
    // онбординг зависят от него, а роутер держит сплеш, пока оно не готово.
    ref.watch(appBootProvider);
    final themeMode = ref.watch(themeModeProvider);
    final router = ref.watch(routerProvider);
    // Брендинг активного инстанса панели (P3, contract E). Всегда валиден:
    // дефолт = вид Caramba Connect (enabled=false). Акцент уже отфильтрован
    // анти-слопом (purple/violet/indigo и статус-цвета отклонены) и правит
    // ТОЛЬКО нейтральные accent-токены; статус-цвета не трогаются.
    final branding = ref.watch(activeBrandingProvider);
    final brandAccent = branding.brandAccentColor;
    return MaterialApp.router(
      title: branding.displayName(kBrandName),
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(brandAccent: brandAccent),
      darkTheme: AppTheme.dark(brandAccent: brandAccent),
      themeMode: themeMode,
      routerConfig: router,
    );
  }
}
