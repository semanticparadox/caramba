import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';

/// Раздел-заголовок (демо `h2`): uppercase, разрядка, text.low.
class SectionTitle extends StatelessWidget {
  final String text;
  final Widget? trailing;
  final EdgeInsets? padding;
  const SectionTitle(this.text, {this.trailing, this.padding, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: padding ??
          const EdgeInsets.fromLTRB(0, AppSpace.s6, 0, AppSpace.s3),
      child: Row(
        children: [
          Text(
            text.toUpperCase(),
            style: AppType.caption.copyWith(
              color: c.textLow,
              letterSpacing: 1.0,
            ),
          ),
          if (trailing != null) ...[const SizedBox(width: AppSpace.s2), trailing!],
        ],
      ),
    );
  }
}

/// Группа строк-настроек (демо `.rows`): surface1, border, скруглённый блок
/// с hairline-разделителями между [CRow].
class RowsGroup extends StatelessWidget {
  final List<Widget> children;
  final EdgeInsets? margin;
  const RowsGroup({required this.children, this.margin, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final rows = <Widget>[];
    for (var i = 0; i < children.length; i++) {
      if (i > 0) rows.add(Divider(height: 1, thickness: 1, color: c.borderSubtle));
      rows.add(children[i]);
    }
    return Container(
      margin: margin,
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r16,
        border: Border.all(color: c.borderSubtle),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(children: rows),
    );
  }
}

/// Строка внутри [RowsGroup] (демо `.crow`): иконка-лейбл-значение-шеврон.
class CRow extends StatelessWidget {
  final String? icon; // Lucide glyph
  final String label;
  final String? value;
  final Color? valueColor;
  final bool mono;
  final bool chevron;
  final Widget? trailing;
  final VoidCallback? onTap;

  const CRow({
    required this.label,
    this.icon,
    this.value,
    this.valueColor,
    this.mono = false,
    this.chevron = false,
    this.trailing,
    this.onTap,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return InkWell(
      onTap: onTap,
      child: Container(
        constraints: const BoxConstraints(minHeight: 54),
        padding: const EdgeInsets.symmetric(horizontal: AppSpace.s4),
        child: Row(
          children: [
            if (icon != null) ...[
              LucideIcon(icon!, color: c.textMed, size: 20),
              const SizedBox(width: AppSpace.s3 + 2),
            ],
            Expanded(
              child: Text(
                label,
                style: AppType.bodyMd.copyWith(color: c.textHi),
              ),
            ),
            if (trailing != null) trailing!,
            if (value != null)
              Flexible(
                child: Text(
                  value!,
                  overflow: TextOverflow.ellipsis,
                  textAlign: TextAlign.right,
                  style: (mono ? AppType.monoMd : AppType.bodyMd)
                      .copyWith(color: valueColor ?? c.textMed),
                ),
              ),
            if (chevron) ...[
              const SizedBox(width: AppSpace.s2),
              LucideIcon(Lucide.chevronRight, color: c.textLow, size: 18),
            ],
          ],
        ),
      ),
    );
  }
}

/// Карта-кнопка большого пункта списка (демо `.item`): иконка/код, заголовок,
/// подпись, опц. трейлинг, выделение слева полоской при [selected].
class ListItemCard extends StatelessWidget {
  final Widget leading;
  final String title;
  final String? subtitle;
  final Widget? trailing;
  final bool selected;
  final bool locked;
  final VoidCallback? onTap;
  final List<Widget> titleBadges;

  const ListItemCard({
    required this.leading,
    required this.title,
    this.subtitle,
    this.trailing,
    this.selected = false,
    this.locked = false,
    this.onTap,
    this.titleBadges = const [],
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    // Icon-less rows (relay/core picker pass a zero-width placeholder) collapse
    // the leading gutter so their titles left-align with the row edge instead
    // of inheriting the iconed indent.
    final l = leading;
    final hasLeading = !(l is SizedBox && (l.width ?? 0) == 0);
    return Opacity(
      opacity: locked ? 0.55 : 1,
      child: Container(
        margin: const EdgeInsets.only(bottom: AppSpace.s2),
        decoration: BoxDecoration(
          color: selected ? c.surface2 : c.surface1,
          borderRadius: AppRadius.r14,
          border: Border.all(
            color: selected ? c.borderStrong : c.borderSubtle,
          ),
        ),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: locked ? null : onTap,
          child: IntrinsicHeight(
            child: Row(
              children: [
                if (selected)
                  Container(
                    width: 3,
                    margin: const EdgeInsets.symmetric(vertical: 11),
                    decoration: BoxDecoration(
                      color: c.textHi,
                      borderRadius: BorderRadius.circular(3),
                    ),
                  )
                else
                  const SizedBox(width: 3),
                if (hasLeading) ...[
                  Padding(
                    padding: const EdgeInsets.fromLTRB(13, 10, 0, 10),
                    child: leading,
                  ),
                  const SizedBox(width: AppSpace.s3 + 2),
                ] else
                  const SizedBox(width: 13),
                Expanded(
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 11),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        Row(
                          children: [
                            Flexible(
                              child: Text(
                                title,
                                overflow: TextOverflow.ellipsis,
                                style: AppType.bodyMd
                                    .copyWith(color: c.textHi),
                              ),
                            ),
                            for (final b in titleBadges) ...[
                              const SizedBox(width: AppSpace.s2),
                              b,
                            ],
                          ],
                        ),
                        if (subtitle != null) ...[
                          const SizedBox(height: 2),
                          Text(
                            subtitle!,
                            style: AppType.bodySm.copyWith(color: c.textMed),
                          ),
                        ],
                      ],
                    ),
                  ),
                ),
                if (trailing != null) ...[
                  const SizedBox(width: AppSpace.s2),
                  Padding(
                    padding: const EdgeInsets.only(right: AppSpace.s4),
                    child: trailing!,
                  ),
                ],
                if (selected && trailing == null)
                  Padding(
                    padding: const EdgeInsets.only(right: AppSpace.s4),
                    child: LucideIcon(Lucide.check, color: c.textHi, size: 18),
                  )
                else if (trailing == null)
                  const SizedBox(width: AppSpace.s4),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Квадратная иконка-«ibox» (демо): inset-фон, border, текст.med иконка.
class IBox extends StatelessWidget {
  final String glyph;
  final double size;
  final Color? color;
  const IBox(this.glyph, {this.size = 40, this.color, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: c.surfaceInset,
        borderRadius: BorderRadius.circular(size * 0.275),
        border: Border.all(color: c.borderSubtle),
      ),
      alignment: Alignment.center,
      child: LucideIcon(glyph, color: color ?? c.textMed, size: size * 0.5),
    );
  }
}

/// Мини-код (демо `.code`): mono-плашка на 2-буквенный код страны.
class CodeChip extends StatelessWidget {
  final String text;
  const CodeChip(this.text, {super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 7),
      constraints: const BoxConstraints(minWidth: 40),
      decoration: BoxDecoration(
        color: c.surfaceInset,
        borderRadius: AppRadius.r8,
        border: Border.all(color: c.borderSubtle),
      ),
      alignment: Alignment.center,
      child: Text(
        text,
        style: AppType.monoMd.copyWith(color: c.textHi, letterSpacing: 0.5),
      ),
    );
  }
}

/// Тег-плашка (демо `.tag`): mono uppercase в обводке. [ok] = зелёный.
class Tag extends StatelessWidget {
  final String text;
  final bool ok;
  const Tag(this.text, {this.ok = false, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final color = ok ? c.success : c.textMed;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(5),
        border: Border.all(color: ok ? c.success : c.borderStrong),
      ),
      child: Text(
        text.toUpperCase(),
        style: AppType.monoSm.copyWith(color: color, letterSpacing: 0.6),
      ),
    );
  }
}

/// Сплошной 6px квота-бар (демо `.quota`): text.hi заливка, amber при low.
class QuotaMeter extends StatelessWidget {
  final double fraction; // 0..1
  final bool low;
  const QuotaMeter({required this.fraction, this.low = false, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return ClipRRect(
      borderRadius: BorderRadius.circular(3),
      child: LinearProgressIndicator(
        value: fraction.clamp(0, 1),
        minHeight: 6,
        backgroundColor: c.surfaceInset,
        valueColor: AlwaysStoppedAnimation(low ? c.warning : c.textHi),
      ),
    );
  }
}

/// Сигнальные «палочки» (демо `.bars`): 3 столбика, активные — text.med.
class SignalBars extends StatelessWidget {
  final int level; // 1..3
  const SignalBars({required this.level, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Row(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.end,
      children: [
        for (var i = 1; i <= 3; i++) ...[
          if (i > 1) const SizedBox(width: 2),
          Container(
            width: 3,
            height: 9,
            decoration: BoxDecoration(
              color: i <= level ? c.textMed : c.borderStrong,
              borderRadius: BorderRadius.circular(1),
            ),
          ),
        ],
      ],
    );
  }
}

/// Ghost-кнопка (демо `.btn.ghost`): surface2, border, опц. иконка.
class GhostButton extends StatelessWidget {
  final String label;
  final String? icon;
  final VoidCallback? onPressed;
  final double minHeight;
  const GhostButton({
    required this.label,
    this.icon,
    this.onPressed,
    this.minHeight = 50,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return SizedBox(
      width: double.infinity,
      child: OutlinedButton(
        onPressed: onPressed,
        style: OutlinedButton.styleFrom(
          minimumSize: Size.fromHeight(minHeight),
        ),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          mainAxisSize: MainAxisSize.min,
          children: [
            if (icon != null) ...[
              LucideIcon(icon!, color: c.textHi, size: 18),
              const SizedBox(width: AppSpace.s2),
            ],
            Flexible(
              child: Text(label, overflow: TextOverflow.ellipsis),
            ),
          ],
        ),
      ),
    );
  }
}

/// Тихая (destructive) кнопка (демо `.btn.quiet`): прозрачная, danger-текст.
class QuietButton extends StatelessWidget {
  final String label;
  final VoidCallback? onPressed;
  final Color? color;
  const QuietButton({required this.label, this.onPressed, this.color, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return SizedBox(
      width: double.infinity,
      child: TextButton(
        onPressed: onPressed,
        style: TextButton.styleFrom(
          foregroundColor: color ?? c.danger,
          minimumSize: const Size.fromHeight(50),
        ),
        child: Text(label),
      ),
    );
  }
}

/// Круглая icon-button-плашка (демо `.iconbtn`): surface1, border, 44x44.
class IconBtn extends StatelessWidget {
  final String glyph;
  final VoidCallback? onTap;
  final Color? color;
  final double size;
  const IconBtn(this.glyph, {this.onTap, this.color, this.size = 44, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Material(
      color: c.surface1,
      borderRadius: AppRadius.r12,
      child: InkWell(
        onTap: onTap,
        borderRadius: AppRadius.r12,
        child: Container(
          width: size,
          height: size,
          decoration: BoxDecoration(
            borderRadius: AppRadius.r12,
            border: Border.all(color: c.borderSubtle),
          ),
          alignment: Alignment.center,
          child: LucideIcon(glyph, color: color ?? c.textMed, size: 20),
        ),
      ),
    );
  }
}

/// Заголовок экрана (демо `.head`): h1 + опц. трейлинг (крестик/чип).
class ScreenHead extends StatelessWidget {
  final String title;
  final Widget? trailing;
  const ScreenHead(this.title, {this.trailing, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.only(bottom: AppSpace.s5),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Expanded(
            child: Text(
              title,
              style: AppType.headline.copyWith(color: c.textHi),
            ),
          ),
          if (trailing != null) trailing!,
        ],
      ),
    );
  }
}

/// Тост (демо `.toast`): surface3, border, shield-иконка слева.
void showCarambaToast(BuildContext context, String message) {
  final c = context.c;
  final messenger = ScaffoldMessenger.of(context);
  messenger.clearSnackBars();
  messenger.showSnackBar(
    SnackBar(
      duration: const Duration(milliseconds: 2400),
      backgroundColor: c.surface3,
      content: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          LucideIcon(Lucide.shield, color: c.textHi, size: 18),
          const SizedBox(width: AppSpace.s3),
          Flexible(
            child: Text(
              message,
              style: AppType.bodyMd.copyWith(color: c.textHi),
            ),
          ),
        ],
      ),
    ),
  );
}

/// Открывает внешнюю ссылку (deeplink в бота / страница оплаты). При неуспехе
/// показывает тост. Используется для purchase pay_url и share-ссылок.
Future<void> openExternal(BuildContext context, String url) async {
  final uri = Uri.tryParse(url);
  if (uri == null) {
    if (context.mounted) showCarambaToast(context, 'Ссылка недоступна');
    return;
  }
  final ok = await launchUrl(uri, mode: LaunchMode.externalApplication);
  if (!ok && context.mounted) {
    showCarambaToast(context, 'Не удалось открыть ссылку');
  }
}

/// Inline-спиннер для секций списка (загрузка устройств/рефералов/семьи).
class InlineLoading extends StatelessWidget {
  final double top;
  const InlineLoading({this.top = AppSpace.s8, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: EdgeInsets.only(top: top, bottom: top),
      child: Center(
        child: SizedBox(
          width: 22,
          height: 22,
          child: CircularProgressIndicator(strokeWidth: 2, color: c.textHi),
        ),
      ),
    );
  }
}

/// Inline-ошибка с кнопкой повтора (плоский текст, без em-dash).
class InlineError extends StatelessWidget {
  final String message;
  final VoidCallback onRetry;
  final double top;
  const InlineError({
    required this.message,
    required this.onRetry,
    this.top = AppSpace.s8,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: EdgeInsets.only(top: top),
      child: Column(
        children: [
          LucideIcon(Lucide.alert, color: c.textMed, size: 26),
          const SizedBox(height: AppSpace.s3),
          Text(message,
              textAlign: TextAlign.center,
              style: AppType.bodyMd.copyWith(color: c.textMed)),
          const SizedBox(height: AppSpace.s4),
          GhostButton(label: 'Повторить', icon: Lucide.refresh, onPressed: onRetry),
        ],
      ),
    );
  }
}

/// Inline-пустое состояние секции (плоский текст).
class InlineEmpty extends StatelessWidget {
  final String message;
  final double top;
  const InlineEmpty({required this.message, this.top = AppSpace.s8, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: EdgeInsets.only(top: top, bottom: top),
      child: Center(
        child: Text(message,
            textAlign: TextAlign.center,
            style: AppType.bodyMd.copyWith(color: c.textMed)),
      ),
    );
  }
}

/// Нижний лист-пикер (демо `.sheet`): заголовок, подпись, список опций
/// [ListItemCard] с галочкой на выбранном. Возвращает выбранный индекс.
Future<int?> showPickerSheet({
  required BuildContext context,
  required String title,
  required String subtitle,
  required List<({String name, String desc, String? icon})> options,
  required int selected,
}) {
  final c = context.c;
  return showModalBottomSheet<int>(
    context: context,
    backgroundColor: c.surface1,
    isScrollControlled: true,
    showDragHandle: true,
    builder: (ctx) {
      return SafeArea(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s1,
            AppSpace.s5,
            AppSpace.s6,
          ),
          child: ConstrainedBox(
            constraints: BoxConstraints(
              maxHeight: MediaQuery.sizeOf(ctx).height * 0.78,
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title, style: AppType.titleLg.copyWith(color: c.textHi)),
                const SizedBox(height: AppSpace.s1),
                Text(subtitle,
                    style: AppType.bodyMd.copyWith(color: c.textMed)),
                const SizedBox(height: AppSpace.s3),
                Flexible(
                  child: ListView.builder(
                    shrinkWrap: true,
                    itemCount: options.length,
                    itemBuilder: (_, i) {
                      final o = options[i];
                      return ListItemCard(
                        leading: o.icon != null
                            ? IBox(o.icon!)
                            : const SizedBox(width: 0, height: 40),
                        title: o.name,
                        subtitle: o.desc,
                        selected: i == selected,
                        onTap: () => Navigator.of(ctx).pop(i),
                      );
                    },
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    },
  );
}
