/// Десктопная оболочка: сайдбар слева, тулбар сверху, контент в центре.
///
/// ПОЧЕМУ ОТДЕЛЬНЫЙ ШЕЛЛ, А НЕ ВЕТКА В [AppShell]. Мобильный шелл выбирает вид
/// по ШИРИНЕ окна и держит две раскладки: нижнюю навигацию и рейл. Десктоп
/// выбирается ПЛАТФОРМОЙ ([isDesktopPlatform]) и приносит с собой то, чего у
/// мобильного нет вовсе: полосу заголовка окна, состояние туннеля вне первой
/// вкладки, клавиатуру и строку меню. Втащив это в `app_shell.dart`, мы
/// поставили бы под угрозу 975 существующих тестов ради экономии одного файла;
/// здесь же ни одна строка мобильного шелла не тронута.
///
/// КОНТРАКТ ТОТ ЖЕ, ЧТО У [AppShell]: на вход [StatefulNavigationShell], ветки
/// и их порядок берутся из [kShellDestinations] — того же списка, который
/// сверяет `shell_tabs_test`. Сайдбар показывает ЧЕТЫРЕ пункта при трёх
/// ветках: «Серверы» — накладной маршрут, см. `desktop_sidebar.dart`.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/desktop/shell/desktop_shortcuts.dart';
import 'package:caramba_client/desktop/shell/desktop_sidebar.dart';
import 'package:caramba_client/desktop/shell/desktop_toolbar.dart';
import 'package:caramba_client/desktop/shell/mac_menu_bar.dart';
import 'package:caramba_client/shell/app_shell.dart' show kShellDestinations;
import 'package:caramba_client/theme/tokens.dart';

class DesktopShell extends ConsumerWidget {
  final StatefulNavigationShell navigationShell;

  const DesktopShell({required this.navigationShell, super.key});

  int get _currentIndex => navigationShell.currentIndex;

  /// Повторный выбор активной ветки возвращает её на начальный экран — ровно
  /// как в [AppShell]. Одно поведение на две платформы: расхождение здесь
  /// человек читает как поломку.
  void _go(int index) =>
      navigationShell.goBranch(index, initialLocation: index == _currentIndex);

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;

    final scaffold = Scaffold(
      backgroundColor: c.bgCanvas,
      body: DesktopShortcuts(
        onSelectTab: _go,
        // Фокус нужен самой клавиатуре: [Shortcuts] ловит нажатия только по
        // пути от текущего фокуса вверх, а на свежем окне фокуса нет ни у
        // кого.
        child: FocusScope(
          autofocus: true,
          child: Row(
            children: [
              DesktopSidebar(currentBranch: _currentIndex, onSelectBranch: _go),
              Expanded(
                child: Column(
                  children: [
                    DesktopToolbar(title: _sectionTitle()),
                    Expanded(
                      child: Center(
                        child: ConstrainedBox(
                          constraints: const BoxConstraints(
                            maxWidth: DesktopTokens.contentMaxWidth,
                          ),
                          // Стек веток принадлежит навигации: контент уезжает
                          // сюда как есть, со своим `IndexedStack` внутри.
                          child: navigationShell,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );

    // Строка меню существует только на macOS. На Windows и Linux
    // [PlatformMenuBar] не рисует ничего, но заводить его там незачем: те же
    // сочетания там объявлены в [DesktopShortcuts].
    if (!isMacOSPlatform) return scaffold;
    return MacMenuBar(onSelectTab: _go, child: scaffold);
  }

  /// Заголовок раздела в тулбаре. Берётся из подписей мобильного шелла: два
  /// названия одной вкладки — это две вкладки в глазах человека.
  String _sectionTitle() {
    final i = _currentIndex;
    if (i < 0 || i >= kShellDestinations.length) return '';
    return kShellDestinations[i].label;
  }
}
