/// Строка меню macOS.
///
/// ЗАЧЕМ ОНА, ЕСЛИ ЕСТЬ ШОРТКАТЫ. На macOS строка меню — это не украшение, а
/// место, где ярлыки ОБЪЯВЛЕНЫ. Человек ищет «Настройки…» в меню приложения и
/// узнаёт оттуда, что это ⌘,; приложение без меню выглядит недоделанным
/// портом, чем E1 и выглядел. Дублировать те же сочетания в [Shortcuts] нельзя:
/// объявленное дважды сочетание срабатывает дважды — отсюда деление, описанное
/// в `desktop_shortcuts.dart`.
///
/// ПРОВЕРКА НА ЖИВОЙ СБОРКЕ СДЕЛАЛА ЭТО ПРАВИЛОМ, А НЕ ПРЕДПОЧТЕНИЕМ. На маке
/// ⌘, и ⌘⇧S (объявленные здесь) срабатывали, а ⌘1/⌘2/⌘3, живущие только в
/// [Shortcuts], не срабатывали вовсе: ⌘-события система сначала предлагает
/// строке меню, и до виджетов Flutter они доходят не всегда. Поэтому КАЖДОЕ
/// сочетание десктопной оболочки на macOS объявлено в этом файле, а
/// `desktop_shortcuts.dart` на маке не объявляет ни одного.
///
/// Пункты hide/quit не наши: их отдаёт система ([PlatformProvidedMenuItem]).
/// Quit при этом приходит в приложение запросом выхода, который ловит
/// `WindowService` через `AppLifecycleListener` — то есть туннель опускается
/// ДО того, как процесс исчезнет, и системный пункт делает ровно то же, что
/// наш «Выйти» в трее.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/desktop/shell/desktop_shortcuts.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/shell/app_shell.dart' show kShellDestinations;
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Цифровые клавиши по номеру ветки: ⌘1 — первая, ⌘2 — вторая, ⌘3 — третья.
const List<LogicalKeyboardKey> _digits = <LogicalKeyboardKey>[
  LogicalKeyboardKey.digit1,
  LogicalKeyboardKey.digit2,
  LogicalKeyboardKey.digit3,
  LogicalKeyboardKey.digit4,
  LogicalKeyboardKey.digit5,
  LogicalKeyboardKey.digit6,
  LogicalKeyboardKey.digit7,
  LogicalKeyboardKey.digit8,
  LogicalKeyboardKey.digit9,
];

class MacMenuBar extends ConsumerWidget {
  final Widget child;

  /// Переключение ветки шелла: «Настройки…» ведут на ту же вкладку, что и
  /// пункт сайдбара, а не открывают вторую копию экрана.
  final void Function(int index) onSelectTab;

  const MacMenuBar({required this.child, required this.onSelectTab, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final brand = ref.watch(activeBrandingProvider).displayName(kBrandName);
    final stage = ref.watch(vpnProvider).stage;
    final noConnections = ref.watch(desktopNoConnectionsProvider);

    return PlatformMenuBar(
      menus: <PlatformMenuItem>[
        PlatformMenu(
          label: brand,
          menus: <PlatformMenuItem>[
            const PlatformProvidedMenuItem(
              type: PlatformProvidedMenuItemType.about,
            ),
            PlatformMenuItemGroup(
              members: <PlatformMenuItem>[
                PlatformMenuItem(
                  label: DesktopStrings.menuSettings,
                  shortcut: const SingleActivator(
                    LogicalKeyboardKey.comma,
                    meta: true,
                  ),
                  onSelected: () =>
                      onSelectTab(DesktopShortcuts.settingsBranch),
                ),
              ],
            ),
            const PlatformMenuItemGroup(
              members: <PlatformMenuItem>[
                PlatformProvidedMenuItem(
                  type: PlatformProvidedMenuItemType.hide,
                ),
                PlatformProvidedMenuItem(
                  type: PlatformProvidedMenuItemType.hideOtherApplications,
                ),
                PlatformProvidedMenuItem(
                  type: PlatformProvidedMenuItemType.showAllApplications,
                ),
              ],
            ),
            const PlatformMenuItemGroup(
              members: <PlatformMenuItem>[
                PlatformProvidedMenuItem(
                  type: PlatformProvidedMenuItemType.quit,
                ),
              ],
            ),
          ],
        ),
        PlatformMenu(
          label: DesktopStrings.menuConnection,
          menus: <PlatformMenuItem>[
            PlatformMenuItem(
              label: _connectionLabel(stage, noConnections: noConnections),
              shortcut: const SingleActivator(
                LogicalKeyboardKey.keyC,
                meta: true,
                shift: true,
              ),
              onSelected: () => toggleConnection(context, ref),
            ),
            PlatformMenuItem(
              label: '${DesktopStrings.menuServers}…',
              shortcut: const SingleActivator(
                LogicalKeyboardKey.keyS,
                meta: true,
                shift: true,
              ),
              onSelected: () => context.go(AppRoute.servers),
            ),
            PlatformMenuItemGroup(
              members: <PlatformMenuItem>[
                PlatformMenuItem(
                  label: DesktopStrings.menuProbe,
                  shortcut: const SingleActivator(
                    LogicalKeyboardKey.keyR,
                    meta: true,
                  ),
                  // Замер принадлежит экрану серверов; на других он не делает
                  // ничего — решение одно на меню и на Ctrl+R других платформ.
                  onSelected: () => probeIfOnServers(context, ref),
                ),
              ],
            ),
          ],
        ),
        // Разделы шелла: подписи те же, что у пунктов сайдбара
        // ([kShellDestinations]), иначе одна вкладка называлась бы в меню и в
        // сайдбаре по-разному.
        PlatformMenu(
          label: DesktopStrings.menuView,
          menus: <PlatformMenuItem>[
            for (var i = 0; i < kShellDestinations.length && i < 9; i++)
              PlatformMenuItem(
                label: kShellDestinations[i].label,
                shortcut: SingleActivator(_digits[i], meta: true),
                onSelected: () => onSelectTab(i),
              ),
          ],
        ),
        PlatformMenu(
          label: DesktopStrings.menuWindow,
          menus: <PlatformMenuItem>[
            const PlatformProvidedMenuItem(
              type: PlatformProvidedMenuItemType.minimizeWindow,
            ),
            const PlatformProvidedMenuItem(
              type: PlatformProvidedMenuItemType.zoomWindow,
            ),
            PlatformMenuItemGroup(
              members: <PlatformMenuItem>[
                PlatformMenuItem(
                  label: DesktopStrings.menuCloseWindow,
                  shortcut: const SingleActivator(
                    LogicalKeyboardKey.keyW,
                    meta: true,
                  ),
                  // ⌘W — это НЕ выход: окно прячется, туннель продолжает
                  // работать. Тот же путь, что и у красной кнопки.
                  onSelected: () => unawaited(requestWindowClose(ref)),
                ),
              ],
            ),
          ],
        ),
      ],
      child: child,
    );
  }

  /// Пункт называет ДЕЙСТВИЕ, а не состояние: «Отключить» при поднятом
  /// туннеле, «Отмена подключения» пока он поднимается.
  static String _connectionLabel(
    VpnStage stage, {
    required bool noConnections,
  }) {
    if (noConnections) return DesktopStrings.trayAddConnection;
    return switch (stage) {
      VpnStage.connected => DesktopStrings.actionDisconnect,
      VpnStage.connecting ||
      VpnStage.reconnecting => DesktopStrings.trayCancelConnect,
      VpnStage.disconnected || VpnStage.error => DesktopStrings.actionConnect,
    };
  }
}
