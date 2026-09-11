import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/desktop/desktop_bootstrap.dart';
import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_services_host.dart';
import 'package:caramba_client/desktop/launch_args.dart';
import 'package:caramba_client/router/app_router.dart';
import 'package:caramba_client/state/bootstrap_state.dart';
import 'package:caramba_client/state/csm_profile_binding.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/settings_state.dart';
import 'package:caramba_client/theme/app_theme.dart';

void main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();
  // Окно готовится ДО первого кадра: размер и позиция читаются с диска, и
  // окно, которое сначала открылось дефолтом, а потом прыгнуло на своё место,
  // человек читает как сбой. На мобильном шаг отсутствует целиком.
  // Аргументы командной строки нужны только здесь: по `--autostart`
  // (регистрация автозапуска на Windows и Linux) решается, показывать ли окно.
  if (isDesktopPlatform) await initDesktop(launch: LaunchArgs.parse(args));
  runApp(const ProviderScope(child: CarambaApp()));
}

/// Root application widget. Dark is the hero theme and the default; light is a
/// faithful sibling. Theme mode follows the user preference in [settingsProvider].
/// Мессенджер корня: отказ по deeplink приходит вне дерева виджетов (роутер
/// строится в провайдере, контекста там нет), а промолчать нельзя - молчаливый
/// отказ выглядит как сломанное приложение. Ключ даёт показать тост из любого
/// места, не таща BuildContext через слои.
final GlobalKey<ScaffoldMessengerState> rootMessengerKey =
    GlobalKey<ScaffoldMessengerState>();

class CarambaApp extends ConsumerWidget {
  const CarambaApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // Стартуем чтение локальных настроек с первого кадра: тема и решение про
    // онбординг зависят от него, а роутер держит сплеш, пока оно не готово.
    ref.watch(appBootProvider);
    // Привязка состояния CSM к активному профилю смотрится ровно здесь и ровно
    // один раз: без наблюдателя она не случается вовсе, а живущая на экране не
    // случается, пока экран закрыт. Смена профиля обязана выбросить историю
    // попыток, отпечаток каталога и факты о транспорте прежнего оператора и
    // переключить хранилище CSM в ядре (02-SPEC.md 1.2).
    ref.watch(csmProfileBindingProvider);
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
      scaffoldMessengerKey: rootMessengerKey,
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(brandAccent: brandAccent),
      darkTheme: AppTheme.dark(brandAccent: brandAccent),
      themeMode: themeMode,
      routerConfig: router,
      // Десктопные сервисы (окно, значок в строке меню, автозапуск) живут
      // ВНУТРИ MaterialApp: им нужен контекст с темой и с мессенджером -
      // автозапуск объясняет свой отказ тостом. На мобильном хост прозрачен и
      // отдаёт ребёнка как есть.
      builder: (context, child) => DesktopServicesHost(child: child!),
    );
  }
}
