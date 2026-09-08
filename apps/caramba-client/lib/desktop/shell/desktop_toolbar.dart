/// Верхняя полоса окна: заголовок раздела и кнопки заголовка там, где их
/// рисуем мы.
///
/// ТРИ ПЛАТФОРМЫ — ТРИ РАЗНЫЕ ПОЛОСЫ, и это не украшательство.
///   * macOS: системный заголовок скрыт, но трафик-лайты остались (их рисует
///     система в левом верхнем углу окна). Место под них освобождает ВЕРХНЯЯ
///     ПОЛОСА САЙДБАРА, а тулбар слева начинается с заголовка раздела;
///   * Windows: системного заголовка нет вовсе, и min/max/close обязаны быть
///     наши. Close при этом ведёт себя как красная кнопка — прячет окно, а не
///     убивает процесс с туннелем;
///   * Linux: заголовок системный, поверх него ни своих кнопок, ни
///     перетаскивания — менеджер окон делает это сам и лучше.
///
/// Перетаскивание и двойной клик отданы [DragToMoveArea] целиком: двойной клик
/// он и так разворачивает/сворачивает окно (на macOS это системный zoom).
/// Своя обработка того же жеста означала бы две гонящиеся друг за другом
/// команды окну.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:window_manager/window_manager.dart';

import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/desktop/shell/desktop_shortcuts.dart';
import 'package:caramba_client/desktop/window_service.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';

class DesktopToolbar extends ConsumerWidget {
  /// Заголовок раздела: подпись активной ветки шелла.
  final String title;

  const DesktopToolbar({required this.title, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;

    final bar = Container(
      height: DesktopTokens.toolbarHeight,
      decoration: BoxDecoration(
        border: Border(
          bottom: BorderSide(color: c.borderSubtle, width: AppBorders.hairline),
        ),
      ),
      child: Row(
        children: [
          const SizedBox(width: DesktopTokens.contentPad),
          Expanded(
            child: Text(
              title,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: AppType.titleLg.copyWith(color: c.textHi),
            ),
          ),
          if (isWindowsPlatform) const _WindowsCaptionButtons(),
        ],
      ),
    );

    return isLinuxPlatform ? bar : DragToMoveArea(child: bar);
  }
}

/// Кнопки заголовка Windows: свернуть, развернуть, закрыть.
///
/// Закрытие идёт тем же путём, что и системная красная кнопка
/// ([requestWindowClose]): по умолчанию окно прячется, туннель продолжает
/// работать. Иначе выход из приложения зависел бы от того, каким именно
/// способом закрыли окно.
class _WindowsCaptionButtons extends ConsumerStatefulWidget {
  const _WindowsCaptionButtons();

  @override
  ConsumerState<_WindowsCaptionButtons> createState() =>
      _WindowsCaptionButtonsState();
}

class _WindowsCaptionButtonsState
    extends ConsumerState<_WindowsCaptionButtons> {
  /// Развёрнутость окна спрашиваем у порта, а не у плагина: в тесте порт
  /// подменяется фейком, и полоса не уходит в метод-канал.
  bool _maximized = false;

  @override
  void initState() {
    super.initState();
    unawaited(_syncMaximized());
  }

  Future<void> _syncMaximized() async {
    final value = await ref.read(windowServiceProvider).port.isMaximized();
    if (!mounted || value == _maximized) return;
    setState(() => _maximized = value);
  }

  @override
  Widget build(BuildContext context) {
    final brightness = Theme.of(context).brightness;
    final port = ref.read(windowServiceProvider).port;

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        _slot(
          // Сворачивания в [WindowPort] нет: решения за ним не стоит, а
          // жест доходит до плагина только при нажатии — в тесте кнопки
          // Windows не рисуются вовсе.
          WindowCaptionButton.minimize(
            brightness: brightness,
            onPressed: () => unawaited(windowManager.minimize()),
          ),
        ),
        _slot(
          _maximized
              ? WindowCaptionButton.unmaximize(
                  brightness: brightness,
                  onPressed: () async {
                    await port.unmaximize();
                    await _syncMaximized();
                  },
                )
              : WindowCaptionButton.maximize(
                  brightness: brightness,
                  onPressed: () async {
                    await port.maximize();
                    await _syncMaximized();
                  },
                ),
        ),
        _slot(
          WindowCaptionButton.close(
            brightness: brightness,
            onPressed: () => unawaited(requestWindowClose(ref)),
          ),
        ),
      ],
    );
  }

  /// Кнопка плагина знает только свой минимум (46x32): высоту до полосы
  /// добираем сами, иначе зона нажатия не совпадает с видимой кнопкой.
  Widget _slot(Widget child) => SizedBox(
    width: DesktopTokens.winCaptionButtonWidth,
    height: DesktopTokens.toolbarHeight,
    child: child,
  );
}
