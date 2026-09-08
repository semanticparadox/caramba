/// Один вход для «листов», которые на десктопе обязаны стать диалогами.
///
/// ПОЧЕМУ АДАПТЕР, А НЕ ПРАВКА КАЖДОГО ВЫЗОВА. Нижний лист на десктопе плох не
/// эстетикой: он выезжает от нижней кромки окна 1120x720, занимает половину
/// экрана под три строки, не закрывается по Esc и не закрывается кликом мимо
/// так, как этого ждут от окна. Переписать пять мест по отдельности значит
/// получить пять разных диалогов; здесь одна геометрия на всех, а вызывающий
/// код остаётся мобильным по форме — меняется только то, чем это показано.
///
/// СИГНАТУРА ПОВТОРЯЕТ `showModalBottomSheet` НЕ СЛУЧАЙНО. Замена в местах
/// вызова обязана быть механической: те же именованные параметры с теми же
/// значениями по умолчанию, тот же `Future<T?>`, тот же `Navigator.of(ctx).pop`
/// внутри билдера. Иначе при замене легко потерять `isScrollControlled` и
/// получить лист, обрезающий содержимое ровно на половине.
library;

import 'package:flutter/material.dart';

import 'package:caramba_client/desktop/desktop_platform.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';

/// Показывает содержимое [builder] листом на мобильном и диалогом на десктопе.
///
/// Мобильная ветка — это ровно прежний `showModalBottomSheet` с фоном
/// `surface1`; [isScrollControlled], [showDragHandle] и [shape] проброшены,
/// чтобы поведение существующих вызовов не поехало ни на пиксель.
///
/// Десктопная ветка — центрированный диалог шириной не больше [maxWidth] и
/// высотой не больше 85% окна: дальше содержимое скроллит сам билдер, как и в
/// листе. `FocusScope(autofocus: true)` внутри обязателен — без фокуса в
/// поддереве диалога нажатие Esc не доходит до `DismissAction`, который ставит
/// `ModalRoute`, и окно перестаёт закрываться клавиатурой.
Future<T?> showAdaptiveSheet<T>(
  BuildContext context, {
  required WidgetBuilder builder,
  bool isScrollControlled = true,
  bool showDragHandle = true,
  ShapeBorder? shape,
  double maxWidth = DesktopTokens.dialogMaxWidth,

  /// Отступ от системных зон. Действует ТОЛЬКО на десктопной ветке.
  ///
  /// У `showModalBottomSheet` одноимённый параметр по умолчанию `false`, и все
  /// пять мигрирующих листов уже оборачивают своё содержимое в `SafeArea`
  /// сами. Пробрось мы значение и туда — получили бы двойной отступ снизу и
  /// изменившуюся максимальную высоту листа на телефоне, то есть ровно ту
  /// регрессию, которой эта задача не имеет права допустить.
  bool useSafeArea = true,
}) {
  final c = context.c;
  if (!isDesktopPlatform) {
    return showModalBottomSheet<T>(
      context: context,
      backgroundColor: c.surface1,
      isScrollControlled: isScrollControlled,
      showDragHandle: showDragHandle,
      shape: shape,
      builder: builder,
    );
  }

  return showDialog<T>(
    context: context,
    barrierColor: c.overlayScrim,
    useSafeArea: useSafeArea,
    builder: (ctx) => Dialog(
      backgroundColor: c.surface1,
      surfaceTintColor: Colors.transparent,
      shape: const RoundedRectangleBorder(borderRadius: AppRadius.r22),
      // 40 по кругу: диалог не липнет к тулбару и к нижней кромке даже в окне
      // минимального размера 960x640.
      insetPadding: const EdgeInsets.all(40),
      clipBehavior: Clip.antiAlias,
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: maxWidth,
          maxHeight: MediaQuery.sizeOf(ctx).height * 0.85,
        ),
        child: FocusScope(
          autofocus: true,
          // Ширина берётся максимальной из разрешённых, а не по содержимому:
          // иначе диалог с двумя короткими строками схлопывается в узкую
          // полоску, а соседний с длинным текстом растягивается на 520 —
          // и два листа одного приложения выглядят как из разных.
          child: SizedBox(
            width: double.infinity,
            // Прозрачный Material поверх фона диалога: `ListTile`, `InkWell` и
            // `Text` внутри билдеров рассчитывают на предка-Material, а свой
            // цвет он не навязывает.
            child: Material(color: Colors.transparent, child: builder(ctx)),
          ),
        ),
      ),
    ),
  );
}
