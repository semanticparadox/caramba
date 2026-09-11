/// Хозяин десктопных сервисов: окно, значок в строке меню, автозапуск,
/// автоподключение.
///
/// ЗАЧЕМ ОТДЕЛЬНЫЙ ВИДЖЕТ. Все три сервиса собраны провайдерами, но ни один из
/// них себя не запускает: `WindowService.attach()`, `TrayService.start()` и
/// `AutostartService.start()` — действия с побочным эффектом в системе
/// (подписка на события окна, значок в панели, запись в Login Items), и
/// провайдер, делающий это при сборке, срабатывал бы от любого случайного
/// чтения — в том числе из теста. Поэтому запуск живёт в одном месте дерева,
/// которое существует ровно один раз за процесс.
///
/// ПОЧЕМУ В `builder:` У `MaterialApp.router`, А НЕ ВОКРУГ НЕГО. Сервисам
/// нужен контекст С ТЕМОЙ и с `ScaffoldMessenger` (автозапуск показывает отказ
/// тостом), а он появляется только внутри `MaterialApp`. Снаружи хост получил
/// бы голое дерево и молчащий мессенджер.
///
/// На мобильной платформе виджет прозрачен: отдаёт ребёнка и не читает ни
/// одного десктопного провайдера. Поэтому существующие тесты (включая
/// `widget_test`, который поднимает настоящий [CarambaApp]) его не замечают.
library;

import 'dart:async';

import 'package:app_links/app_links.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/desktop/autoconnect_service.dart';
import 'package:caramba_client/desktop/autostart_service.dart';
import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/tray_service.dart';
import 'package:caramba_client/desktop/window_service.dart';

class DesktopServicesHost extends ConsumerStatefulWidget {
  final Widget child;

  const DesktopServicesHost({required this.child, super.key});

  @override
  ConsumerState<DesktopServicesHost> createState() =>
      _DesktopServicesHostState();
}

class _DesktopServicesHostState extends ConsumerState<DesktopServicesHost> {
  StreamSubscription<Uri>? _links;

  @override
  void initState() {
    super.initState();
    if (!isDesktopPlatform) return;
    // После первого кадра, а не в `initState`: `TrayService.start()` уходит в
    // плагин, а `AutostartService` умеет показать тост — и тому и другому
    // нужен уже смонтированный `MaterialApp`.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      _startServices();
    });
  }

  void _startServices() {
    // Окно первым: `attach()` вешает слушателя красной кнопки и
    // `AppLifecycleListener` на ⌘Q. Пока его нет, выход убивает процесс, не
    // опустив туннель.
    final window = ref.read(windowServiceProvider);
    window.attach();

    // Значок сразу за окном. Читать провайдер обязательно, даже если бы
    // значок был не нужен: при сборке он назначает `windowService.beforeExit`,
    // и без этого чтения значок остался бы в строке меню после выхода.
    //
    // Окно к этому моменту уже показано (`initDesktop()` сделал это до
    // `runApp`), поэтому значок появляется на доли секунды позже окна, а не
    // раньше. Поменять порядок можно только ценой пустого экрана на старте:
    // до первого кадра ни темы, ни провайдеров ещё нет.
    unawaited(ref.read(trayServiceProvider).start());

    // Автозапуск последним: он ничего не рисует и все свои отказы гасит внутри
    // (система может не уметь автозапуск вовсе — macOS 12).
    unawaited(ref.read(autostartServiceProvider).start());

    // Автоподключение: ждёт готовности профилей и серверов подпиской и
    // поднимает туннель один раз, если тумблер включён.
    ref.read(autoConnectServiceProvider).start();

    // Диплинк при спрятанном окне. Навигацией занимается `DeepLinkHandler` в
    // роутере; здесь нужно ровно одно — ПОКАЗАТЬ окно, иначе переход случится
    // там, где его никто не видит, и ссылка выглядит проглоченной.
    // `AppLinks` — синглтон с broadcast-потоком, так что вторая подписка
    // первой не мешает.
    _links = AppLinks().uriLinkStream.listen(
      (_) => unawaited(window.show()),
      onError: (Object _) {
        // Сбойный URI — забота разборщика ссылок, а не окна.
      },
    );
  }

  @override
  void dispose() {
    unawaited(_links?.cancel());
    _links = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (isDesktopPlatform) {
      // Watch, а не read: сервисы обязаны пережить любую пересборку дерева.
      // Отпустив их, мы потеряли бы слушателя окна и значок вместе с ним.
      ref.watch(windowServiceProvider);
      ref.watch(trayServiceProvider);
      ref.watch(autostartServiceProvider);
      ref.watch(autoConnectServiceProvider);
    }
    return widget.child;
  }
}
