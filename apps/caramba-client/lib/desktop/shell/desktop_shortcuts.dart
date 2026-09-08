/// Клавиатура десктопной оболочки: интенты, их обработчики и раскладка
/// сочетаний по платформам.
///
/// ЗАЧЕМ ОТДЕЛЬНЫЙ ФАЙЛ И ПОЧЕМУ ЗДЕСЬ ЖЕ ЖИВУТ ТРИ ОБЩИХ РЕШЕНИЯ.
/// Одно и то же действие на десктопе вызывается из трёх мест сразу: кнопкой
/// в сайдбаре, пунктом строки меню и сочетанием клавиш. Разъедутся они молча:
/// ⌘⇧C будет отключать туннель там, где кнопка ведёт на импорт подписки. Чтобы
/// решение принималось один раз, здесь лежат не только интенты, но и
/// [toggleConnection] (что делает «Подключить/Отключить»),
/// [desktopNoConnectionsProvider] (подключаться вообще есть куда?) и
/// [requestWindowClose] (что делает закрытие окна) — их зовут и сайдбар, и
/// тулбар, и строка меню.
///
/// РАСКЛАДКА РАЗНАЯ ПО ПЛАТФОРМАМ, И ЭТО НЕ ПРИДИРКА. На macOS ярлык обязан
/// стоять В МЕНЮ: человек ищет его там, а главное — система отдаёт ⌘-события
/// сначала строке меню, и до виджетов Flutter они доходят не всегда. На живой
/// сборке ⌘, и ⌘⇧S (пункты меню) работали, а ⌘1/⌘2/⌘3, объявленные только
/// здесь, не срабатывали ни разу. Поэтому на macOS этот файл не объявляет
/// сочетаний вовсе — все они в `mac_menu_bar.dart`, — а на Windows и Linux,
/// где строки меню нет, наоборот, все живут здесь.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_prefs.dart';
import 'package:caramba_client/desktop/window_service.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/probe_state.dart';
import 'package:caramba_client/state/vpn_state.dart';

/// Открыть настройки (⌘, / Ctrl+,). На macOS живёт в меню приложения.
class OpenSettingsIntent extends Intent {
  const OpenSettingsIntent();
}

/// Переключить ветку шелла (⌘1/2/3): индекс тот же, что у [kShellDestinations].
class SelectTabIntent extends Intent {
  final int index;

  const SelectTabIntent(this.index);
}

/// Открыть список серверов (⌘⇧S). Накладной маршрут, а не ветка.
class OpenServersIntent extends Intent {
  const OpenServersIntent();
}

/// Подключить или отключить туннель (⌘⇧C).
class ToggleConnectionIntent extends Intent {
  const ToggleConnectionIntent();
}

/// Закрыть окно (⌘W / Ctrl+W): по умолчанию прячет, а не завершает.
class CloseWindowIntent extends Intent {
  const CloseWindowIntent();
}

/// Выйти из приложения (Ctrl+Q). На macOS этим занимается пункт Quit строки
/// меню: он приходит системой в `AppLifecycleListener`, который слушает
/// [WindowService].
class QuitAppIntent extends Intent {
  const QuitAppIntent();
}

/// Замерить задержку (⌘R / F5). Работает только на экране серверов.
class ProbeIntent extends Intent {
  const ProbeIntent();
}

/// Подключаться некуда: ни одного профиля подключения, и панельной сессии нет.
///
/// Считается ровно так же, как на «Подключении» (`HomeScreen.noConnections`):
/// пока профили не прочитаны, состояние «пусто» не объявляется, иначе сайдбар
/// мигал бы «Нет подключений» на каждом холодном старте.
final desktopNoConnectionsProvider = Provider<bool>((ref) {
  final panelSession = ref.watch(authProvider).stage == AuthStage.authenticated;
  if (panelSession) return false;
  final profiles = ref.watch(connectionProfilesProvider);
  return !profiles.loading && profiles.profiles.isEmpty;
});

/// Единственное место, где решают, что делает «Подключить/Отключить».
///
/// Поднимать нечего, когда подключений нет вовсе: тогда осмысленное действие
/// одно — увести туда, где их добавляют. Тот же выбор сделан у дайла на
/// «Подключении», и разойтись им нельзя.
void toggleConnection(BuildContext context, WidgetRef ref) {
  if (ref.read(desktopNoConnectionsProvider)) {
    context.go(AppRoute.connectionImport);
    return;
  }
  unawaited(ref.read(vpnProvider.notifier).toggle());
}

/// Замер задержки: он принадлежит экрану серверов и только ему.
///
/// Одно решение на два вызова — ⌘R в строке меню macOS и Ctrl+R/F5 в
/// [Shortcuts]: иначе на одной платформе замер молча гонял бы ядро с
/// «Подключения», а на другой нет.
void probeIfOnServers(BuildContext context, WidgetRef ref) {
  if (topRouteLocation(context) != AppRoute.servers) return;
  unawaited(ref.read(probeRunProvider.notifier).measure());
}

/// Закрытие окна по нашей воле (⌘W, Ctrl+W, кнопка заголовка на Windows).
///
/// Повторяет решение системного закрытия, которое ловит [WindowService]:
/// настройка «При закрытии окна» одна на оба пути, иначе красная кнопка
/// прятала бы приложение, а ⌘W завершало.
Future<void> requestWindowClose(WidgetRef ref) {
  final service = ref.read(windowServiceProvider);
  if (ref.read(desktopPrefsProvider).closeToTray) return service.port.hide();
  return service.quitApplication();
}

/// Путь ВЕРХНЕЙ страницы стека: ветка шелла раскрывается до листа.
///
/// Повторяет `CarambaRouter._topLocation` (он приватный). Спрашивают его двое:
/// подсветка пункта «Серверы» в сайдбаре и ⌘R, который меряет задержку только
/// там, где список серверов открыт.
String? topRouteLocation(BuildContext context) {
  final delegate = GoRouter.of(context).routerDelegate;
  final current = delegate.currentConfiguration;
  if (current.isEmpty) return null;
  RouteMatchBase match = current.matches.last;
  while (match is ShellRouteMatch) {
    if (match.matches.isEmpty) return null;
    match = match.matches.last;
  }
  return match.matchedLocation;
}

/// Обёртка шелла: [Shortcuts] + [Actions] на весь десктопный интерфейс.
///
/// [onSelectTab] приходит снаружи: переключать ветки умеет только
/// `StatefulNavigationShell`, а он принадлежит шеллу.
class DesktopShortcuts extends ConsumerWidget {
  final Widget child;

  /// Переключение ветки по индексу [kShellDestinations].
  final void Function(int index) onSelectTab;

  const DesktopShortcuts({
    required this.child,
    required this.onSelectTab,
    super.key,
  });

  /// Индекс ветки настроек. Совпадает с порядком веток в таблице маршрутов.
  static const int settingsBranch = 2;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Shortcuts(
      shortcuts: _bindings(),
      child: Actions(
        actions: <Type, Action<Intent>>{
          SelectTabIntent: CallbackAction<SelectTabIntent>(
            onInvoke: (intent) {
              onSelectTab(intent.index);
              return null;
            },
          ),
          OpenSettingsIntent: CallbackAction<OpenSettingsIntent>(
            onInvoke: (_) {
              onSelectTab(settingsBranch);
              return null;
            },
          ),
          OpenServersIntent: CallbackAction<OpenServersIntent>(
            onInvoke: (_) {
              context.go(AppRoute.servers);
              return null;
            },
          ),
          ToggleConnectionIntent: CallbackAction<ToggleConnectionIntent>(
            onInvoke: (_) {
              toggleConnection(context, ref);
              return null;
            },
          ),
          CloseWindowIntent: CallbackAction<CloseWindowIntent>(
            onInvoke: (_) {
              unawaited(requestWindowClose(ref));
              return null;
            },
          ),
          QuitAppIntent: CallbackAction<QuitAppIntent>(
            onInvoke: (_) {
              unawaited(ref.read(windowServiceProvider).quitApplication());
              return null;
            },
          ),
          ProbeIntent: CallbackAction<ProbeIntent>(
            onInvoke: (_) {
              probeIfOnServers(context, ref);
              return null;
            },
          ),
        },
        child: child,
      ),
    );
  }

  /// Сочетания клавиш активной платформы.
  ///
  /// На macOS ⌘, ⌘W и Quit объявлены строкой меню (`MacMenuBar`) и здесь
  /// отсутствуют намеренно: объявленное дважды сочетание срабатывает дважды.
  Map<ShortcutActivator, Intent> _bindings() {
    // На macOS ни одного: всё объявлено строкой меню (`MacMenuBar`). Пустая
    // карта здесь — не забывчивость, а единственный способ, которым ⌘1/2/3
    // вообще доходят до приложения; см. заголовок файла.
    if (isMacOSPlatform) return const <ShortcutActivator, Intent>{};
    return const <ShortcutActivator, Intent>{
      SingleActivator(LogicalKeyboardKey.comma, control: true):
          OpenSettingsIntent(),
      SingleActivator(LogicalKeyboardKey.digit1, control: true):
          SelectTabIntent(0),
      SingleActivator(LogicalKeyboardKey.digit2, control: true):
          SelectTabIntent(1),
      SingleActivator(LogicalKeyboardKey.digit3, control: true):
          SelectTabIntent(2),
      SingleActivator(LogicalKeyboardKey.keyS, control: true, shift: true):
          OpenServersIntent(),
      SingleActivator(LogicalKeyboardKey.keyC, control: true, shift: true):
          ToggleConnectionIntent(),
      SingleActivator(LogicalKeyboardKey.keyW, control: true):
          CloseWindowIntent(),
      SingleActivator(LogicalKeyboardKey.keyQ, control: true): QuitAppIntent(),
      SingleActivator(LogicalKeyboardKey.keyR, control: true): ProbeIntent(),
      SingleActivator(LogicalKeyboardKey.f5): ProbeIntent(),
    };
  }
}
