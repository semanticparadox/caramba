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

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/csm_settings.dart';
import 'package:caramba_client/state/csm_catalog_guard.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Все висящие карточки активного профиля, стопкой. Пусто, когда их нет.
///
/// Стопка одна на оба происхождения. Изменение настройки и сужение, пришедшее
/// в каталоге, это один и тот же вопрос к пользователю, и разводить их по
/// разным поверхностям значило бы сделать второе тише первого, тогда как
/// 04-THREAT-MODEL.md 7.3 шаг 5 называет второе самым явным нарушением
/// границы, за которую враждебный оператор не должен заходить.
class CsmPendingChangesSection extends ConsumerStatefulWidget {
  const CsmPendingChangesSection({super.key});

  @override
  ConsumerState<CsmPendingChangesSection> createState() =>
      _CsmPendingChangesSectionState();
}

class _CsmPendingChangesSectionState
    extends ConsumerState<CsmPendingChangesSection> {
  @override
  void initState() {
    super.initState();
    // Набор ресурсов сверяется там же, где карточки показываются: иначе
    // сужение, пришедшее в каталоге, доходит до пользователя тогда, когда он
    // сам откроет нужный экран, то есть никогда.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        unawaited(
          csmPumpCatalogGuard(
            connection: ref.read(vpnConnectionProvider),
            guard: ref.read(csmCatalogGuardProvider.notifier),
          ),
        );
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final cards = ref.watch(csmPendingChangesProvider);
    final catalogCards = ref.watch(csmCatalogChangesProvider);
    if (cards.isEmpty && catalogCards.isEmpty) {
      return const SizedBox.shrink();
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final card in catalogCards) ...[
          CsmCatalogChangeCard(card: card),
          const SizedBox(height: AppSpace.s3),
        ],
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

/// Заголовок строки изменения каталога. Только инертные значения из
/// подписанного каталога: имя провайдера, идентификатор маршрута, короткий
/// хеш. Ни одной строки, которую написал оператор (INV-10).
String csmCatalogChangeTitle(CsmCatalogChangeKind kind) => switch (kind) {
  CsmCatalogChangeKind.resourceAdded => 'Добавлен набор правил',
  CsmCatalogChangeKind.resourceRemoved => 'Убран набор правил',
  CsmCatalogChangeKind.resourceRenamed => 'Набор правил переименован',
  CsmCatalogChangeKind.resourceHashChanged =>
    'Содержимое набора правил заменено',
  CsmCatalogChangeKind.routeRulesChanged => 'У маршрута другой список правил',
};

/// Карточка сужения, пришедшего в каталоге (02-SPEC.md 7.7.1, INV-22).
///
/// Она поднимается БЕЗУСЛОВНО: хеш связывает байты, но не связывает того, кто
/// выбрал и путь, и хеш. Оператор вправе опубликовать набор правил, уводящий
/// названные домены в DIRECT, и хеш при этом сойдётся. Ничто в формате этого не
/// ограничивает, поэтому ограничивает эта карточка.
class CsmCatalogChangeCard extends ConsumerWidget {
  final CsmCatalogChange card;

  const CsmCatalogChangeCard({required this.card, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;

    // Ответ уходит СНАЧАЛА в ядро: страж ресурсов живёт там, и до ответа там
    // удерживается прежний набор. Ответ, оставшийся здесь, не откатил бы
    // ничего, а текст карточки обещает обратное.
    Future<void> answer(bool accept) => csmAnswerCatalogCard(
      connection: ref.read(vpnConnectionProvider),
      guard: ref.read(csmCatalogGuardProvider.notifier),
      cardId: card.id,
      accept: accept,
    );

    return Container(
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r16,
        border: Border.all(color: c.warning.withValues(alpha: 0.45)),
      ),
      padding: const EdgeInsets.all(AppSpace.s4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              LucideIcon(Lucide.shield, color: c.warning, size: 18),
              const SizedBox(width: AppSpace.s3),
              Expanded(
                child: Text(
                  'Оператор поменял правила маршрутизации',
                  style: AppType.titleMd.copyWith(color: c.textHi),
                ),
              ),
            ],
          ),
          const SizedBox(height: AppSpace.s2),
          Text(
            'Эти файлы решают, какой трафик идёт в туннель, а какой мимо него. '
            'Подпись оператора на них сходится, но что именно внутри, '
            'приложение проверить не может. Пока вы не ответите, действует '
            'прежний набор.',
            style: AppType.bodySm.copyWith(color: c.textMed),
          ),
          const SizedBox(height: AppSpace.s3),
          for (final row in card.rows) ...[
            _CatalogRow(row: row),
            if (row != card.rows.last) const SizedBox(height: AppSpace.s3),
          ],
          const SizedBox(height: AppSpace.s4),
          Row(
            children: [
              Expanded(
                child: GhostButton(
                  label: 'Оставить прежние',
                  icon: Lucide.check,
                  minHeight: 44,
                  onPressed: () => unawaited(answer(false)),
                ),
              ),
              const SizedBox(width: AppSpace.s2),
              Expanded(
                child: GhostButton(
                  label: 'Принять новые',
                  icon: Lucide.undo,
                  minHeight: 44,
                  onPressed: () => unawaited(answer(true)),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Одна изменившаяся запись: что это, как было, как стало.
class _CatalogRow extends StatelessWidget {
  final CsmCatalogChangeRow row;
  const _CatalogRow({required this.row});

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
            csmCatalogChangeTitle(row.kind),
            style: AppType.bodyMd.copyWith(color: c.textHi),
          ),
          const SizedBox(height: AppSpace.s2),
          _ValueLine(label: 'Название', value: row.name, color: c.textHi),
          if (row.previous.isNotEmpty) ...[
            const SizedBox(height: 2),
            _ValueLine(label: 'Было', value: row.previous, color: c.textMed),
          ],
          if (row.proposed.isNotEmpty) ...[
            const SizedBox(height: 2),
            _ValueLine(
              label: 'Оператор предлагает',
              value: row.proposed,
              color: c.warning,
            ),
          ],
        ],
      ),
    );
  }
}
