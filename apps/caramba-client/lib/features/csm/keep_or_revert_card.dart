/// Карточка «Оставить или Вернуть», INV-22.
///
/// Поднимается на ЛЮБОЕ изменение оператора, затронувшее настройку, которую
/// пользователь ставил явно, и на ЛЮБОЕ сужение его защиты, безусловно и
/// независимо от происхождения (02-SPEC.md 7.7).
///
/// Карточка обязана назвать настройку, старое значение, новое значение и
/// происхождение, и она НЕ ВПРАВЕ рендерить текст оператора на той же
/// поверхности (INV-10): иначе строка, которую написал оператор, стоит рядом с
/// вопросом «доверять ли оператору» и наследует его рамку.
///
/// Пока пользователь не ответил, значение оператора НЕ применено: клиент не
/// применяет то, о чём спрашивает. Карточка не истекает по таймеру, не
/// закрывается навигацией и не отвечается молчанием.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Все висящие карточки активного профиля, стопкой. Пусто, когда их нет.
class CsmPendingChangesSection extends ConsumerWidget {
  const CsmPendingChangesSection({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final cards = ref.watch(csmPendingChangesProvider);
    if (cards.isEmpty) {
      return const SizedBox.shrink();
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final card in cards) ...[
          KeepOrRevertCard(card: card),
          const SizedBox(height: AppSpace.s3),
        ],
      ],
    );
  }
}

class KeepOrRevertCard extends ConsumerWidget {
  final CsmPendingChange card;

  const KeepOrRevertCard({required this.card, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final narrowing = card.items.any(
      (i) => i.trigger == CsmCardTrigger.narrowing,
    );

    return Container(
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r16,
        border: Border.all(
          color: narrowing ? c.warning.withValues(alpha: 0.45) : c.borderStrong,
        ),
      ),
      padding: const EdgeInsets.all(AppSpace.s4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              LucideIcon(
                narrowing ? Lucide.shield : Lucide.sliders,
                color: narrowing ? c.warning : c.textMed,
                size: 18,
              ),
              const SizedBox(width: AppSpace.s3),
              Expanded(
                child: Text(
                  narrowing
                      ? 'Оператор сузил вашу защиту'
                      : (card.isMultiKey
                            ? 'Оператор поменял ваши настройки'
                            : 'Оператор поменял вашу настройку'),
                  style: AppType.titleMd.copyWith(color: c.textHi),
                ),
              ),
            ],
          ),
          const SizedBox(height: AppSpace.s2),
          Text(
            'Пока вы не ответите, действует ваше значение. Новое не применено.',
            style: AppType.bodySm.copyWith(color: c.textMed),
          ),
          const SizedBox(height: AppSpace.s3),
          for (final item in card.items) ...[
            _ItemRow(item: item),
            if (item != card.items.last) const SizedBox(height: AppSpace.s3),
          ],
          const SizedBox(height: AppSpace.s4),
          Row(
            children: [
              Expanded(
                child: GhostButton(
                  label: 'Оставить моё',
                  icon: Lucide.check,
                  minHeight: 44,
                  onPressed: () =>
                      ref.read(csmNotifierProvider).keepCard(card.id),
                ),
              ),
              const SizedBox(width: AppSpace.s2),
              Expanded(
                child: GhostButton(
                  label: 'Принять новое',
                  icon: Lucide.undo,
                  minHeight: 44,
                  onPressed: () =>
                      ref.read(csmNotifierProvider).revertCard(card.id),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Один затронутый ключ: настройка, старое значение, новое значение,
/// происхождение. Все четыре обязательны.
class _ItemRow extends StatelessWidget {
  final CsmCardItem item;
  const _ItemRow({required this.item});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      padding: const EdgeInsets.all(AppSpace.s3),
      decoration: BoxDecoration(
        color: c.surfaceInset,
        borderRadius: AppRadius.r12,
        border: Border.all(color: c.borderSubtle),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            csmSettingTitle(item.key),
            style: AppType.bodyMd.copyWith(color: c.textHi),
          ),
          const SizedBox(height: AppSpace.s2),
          _ValueLine(
            label: 'Сейчас',
            value: csmSettingValueText(item.key, item.current),
            color: c.textHi,
          ),
          const SizedBox(height: 2),
          _ValueLine(
            label: 'Оператор предлагает',
            value: csmSettingValueText(item.key, item.proposed),
            color: c.warning,
          ),
          const SizedBox(height: AppSpace.s2),
          Text(
            'Источник: ${csmProvenanceTitle(item.src)}. Повод: '
            '${item.trigger == CsmCardTrigger.narrowing ? 'сужение защиты' : 'перезапись вашего значения'}.',
            style: AppType.bodySm.copyWith(color: c.textLow),
          ),
        ],
      ),
    );
  }
}

class _ValueLine extends StatelessWidget {
  final String label;
  final String value;
  final Color color;

  const _ValueLine({
    required this.label,
    required this.value,
    required this.color,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 148,
          child: Text(label, style: AppType.bodySm.copyWith(color: c.textMed)),
        ),
        Expanded(
          child: Text(value, style: AppType.monoSm.copyWith(color: color)),
        ),
      ],
    );
  }
}
