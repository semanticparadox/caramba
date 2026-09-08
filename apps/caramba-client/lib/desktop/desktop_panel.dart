/// Поверхности накладных маршрутов на десктопе: панель справа и диалог по
/// центру (DESKTOP-SPEC.md, раздел 3).
///
/// ЗАЧЕМ ОТДЕЛЬНЫЙ ФАЙЛ. Рамка знает только про геометрию и поверхность, и
/// ничего про навигацию: её же оболочку переиспользует `desktopOverlayPage`,
/// и её же можно смонтировать в тесте без роутера. Обратное (рисовать рамку
/// внутри `createRoute`) сделало бы её непроверяемой в отрыве от навигатора.
///
/// Никакого блюра и свечения: на десктопе панель лежит поверх статичного
/// контента, а `BackdropFilter` на всю высоту окна стоит кадров ровно там, где
/// под ним живёт атмосфера Home.
library;

import 'package:flutter/widgets.dart';

import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';

/// Доля высоты окна, выше которой диалог не растёт: под ним обязана остаться
/// видимая полоса скрима, иначе диалог читается как подменённый экран.
const double kDesktopDialogMaxHeightFactor = 0.85;

/// Панель, выезжающая от правого края на всю высоту окна.
///
/// Ширину задаёт вызывающий (`DesktopTokens.panelWide` под таблицы и списки,
/// `DesktopTokens.panelNarrow` под формы в одну колонку) — рамка про смысл
/// содержимого не знает.
class DesktopPanelFrame extends StatelessWidget {
  const DesktopPanelFrame({
    required this.width,
    required this.child,
    super.key,
  });

  final double width;
  final Widget child;

  /// Скруглены только левые углы: правая грань панели совпадает с гранью окна.
  static const BorderRadius _radius = BorderRadius.horizontal(
    left: Radius.circular(AppRadius.lg),
  );

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Align(
      alignment: Alignment.centerRight,
      child: SizedBox(
        width: width,
        height: double.infinity,
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: c.surface1,
            borderRadius: _radius,
            // Тень уже разрешена по яркости темы, отдельной развилки не надо.
            boxShadow: context.tokens.elevSheet,
          ),
          child: ClipRRect(
            borderRadius: _radius,
            // Грань рисуется ПОВЕРХ содержимого и без своего радиуса: у
            // `Border` с одной стороной радиус запрещён (ассерт в BoxBorder),
            // а закругление даёт внешний ClipRRect — он же обрезает хайрлайн
            // по дуге углов.
            child: DecoratedBox(
              position: DecorationPosition.foreground,
              decoration: BoxDecoration(
                border: Border(
                  left: BorderSide(
                    color: c.borderStrong,
                    width: AppBorders.hairline,
                  ),
                ),
              ),
              // Фокус обязан УЙТИ внутрь панели: у мобильного листа его не
              // было, и Esc уходил в экран под ним, а не в модалку.
              child: FocusScope(autofocus: true, child: child),
            ),
          ),
        ),
      ),
    );
  }
}

/// Центрированный диалог: вход, энроллмент, ссылка подключения, автонастройка.
class DesktopDialogFrame extends StatelessWidget {
  const DesktopDialogFrame({
    required this.child,
    this.maxWidth = AppBreakpoints.dialogMaxWidth,
    super.key,
  });

  final Widget child;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final height = MediaQuery.sizeOf(context).height;
    return Center(
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: maxWidth,
          maxHeight: height * kDesktopDialogMaxHeightFactor,
        ),
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: c.surface1,
            borderRadius: AppRadius.r22,
            boxShadow: context.tokens.elevSheet,
          ),
          child: ClipRRect(
            borderRadius: AppRadius.r22,
            child: FocusScope(autofocus: true, child: child),
          ),
        ),
      ),
    );
  }
}
