/// Возраст конфигурации и её источник, INV-21, плюс липкая ошибка INV-13.
///
/// Показывается на Главной, где уже живёт атмосферный слой. Карточка стоит
/// НИЖЕ дайла, в блоке карточек: слой зарегистрирован на измеренную геометрию
/// шапки и дайла, и любой сдвиг вверх ломает его инвариант, поэтому высота
/// шапки остаётся прежней (DESIGN.md 4.2, test/atmosphere).
///
/// «Работает на сохранённой конфигурации» это НЕ ошибка. Это нормальное
/// состояние устройства в заблокированной сети, и рисовать его красным значит
/// научить пользователя пугаться работающего продукта (02-SPEC.md 2.1
/// правило 2).
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/csm_profile.dart';
import 'package:caramba_client/features/csm/csm_labels.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/csm_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Карточка возраста конфигурации. Свернётся в ничто, когда профиль не
/// проходил энроллмент CSM либо конфигурация свежая и пришла напрямую.
class CsmConfigAgeCard extends ConsumerWidget {
  const CsmConfigAgeCard({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final age = ref.watch(csmConfigurationAgeProvider);
    final sticky = ref.watch(csmStickyErrorProvider);

    // Липкая ошибка идёт первой и не закрывается ничем: закреплённый корень и
    // отсутствие поля возможностей это отказ, а не откат в непроверяемый
    // режим (INV-13).
    if (sticky != null) {
      return _StickyErrorCard(error: sticky);
    }

    if (age.stage == CsmProfileStage.unenrolled) {
      return const SizedBox.shrink();
    }

    // Отказ подключаться это отдельное состояние и оно важнее возраста.
    if (age.stage.refusesToConnect) {
      return _Card(
        tone: BannerTone.danger,
        glyph: Lucide.alert,
        title: csmStageTitle(age.stage),
        body: age.stage == CsmProfileStage.compromised
            ? 'Доверие к этому оператору отозвано. Подключение на его данных '
                  'не поднимается. Профиль нужно завести заново.'
            : 'Окно офлайн-работы исчерпано: конфигурация слишком стара, '
                  'чтобы на ней подключаться. Нужен свежий документ по любой '
                  'ступени, включая принесённый вне полосы.',
        onTap: () => context.go(AppRoute.csmDocuments),
      );
    }

    final ageSec = age.ageSec;
    if (!age.runningOnCache && ageSec == null) {
      return const SizedBox.shrink();
    }

    final source = age.source;
    final sourceText = source == null
        ? 'источник неизвестен'
        : '${csmRungId(source)} ${csmRungTitle(source).toLowerCase()}';
    final ageText = ageSec == null
        ? 'ни разу не обновлялась'
        : 'проверена ${csmAgeText(ageSec)} назад';

    if (!age.runningOnCache) {
      // Свежая конфигурация: одна тихая строка, без плашки.
      return Padding(
        padding: const EdgeInsets.only(bottom: AppSpace.s4),
        child: InkWell(
          borderRadius: AppRadius.r12,
          onTap: () => context.go(AppRoute.csmDocuments),
          child: Padding(
            padding: const EdgeInsets.symmetric(
              vertical: AppSpace.s2,
              horizontal: AppSpace.s1,
            ),
            child: Row(
              children: [
                LucideIcon(Lucide.fileCheck, color: c.textLow, size: 16),
                const SizedBox(width: AppSpace.s2),
                Expanded(
                  child: Text(
                    'Конфигурация $ageText, источник $sourceText',
                    style: AppType.bodySm.copyWith(color: c.textLow),
                  ),
                ),
                LucideIcon(Lucide.chevronRight, color: c.textLow, size: 16),
              ],
            ),
          ),
        ),
      );
    }

    return _Card(
      tone: BannerTone.warning,
      glyph: Lucide.clock,
      title: 'Работает на сохранённой конфигурации',
      body:
          'Конфигурация $ageText, источник $sourceText. Подключение это не '
          'ломает: сохранённые документы проверены и остаются рабочими. Новые '
          'инструкции и новый статус на них не принимаются.',
      onTap: () => context.go(AppRoute.csmDocuments),
    );
  }
}

/// Липкая ошибка: на закреплённом профиле не пришло поле возможностей.
/// Недиссмиссабельна по определению, поэтому у неё нет крестика.
class _StickyErrorCard extends StatelessWidget {
  final CsmStickyError error;
  const _StickyErrorCard({required this.error});

  @override
  Widget build(BuildContext context) {
    return _Card(
      tone: BannerTone.danger,
      glyph: Lucide.shield,
      title: 'Документ пришёл без поля возможностей',
      body:
          'Профиль ${error.pid} уже закрепил корневой ключ, поэтому отката к '
          'непроверяемому режиму нет ни по какой причине. Отсутствие поля это '
          'ровно та подмена одного поля, ради которой правило и написано.',
      onTap: () => context.go(AppRoute.csmDocuments),
    );
  }
}

/// Общая форма карточки: тон, глиф, заголовок, текст и переход к документам.
class _Card extends StatelessWidget {
  final BannerTone tone;
  final String glyph;
  final String title;
  final String body;
  final VoidCallback onTap;

  const _Card({
    required this.tone,
    required this.glyph,
    required this.title,
    required this.body,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final (Color fg, Color bg) = switch (tone) {
      BannerTone.info => (c.textMed, c.surface1),
      BannerTone.warning => (c.warning, c.warningSubtle),
      BannerTone.danger => (c.danger, c.dangerSubtle),
    };
    return Padding(
      padding: const EdgeInsets.only(bottom: AppSpace.s4),
      child: Material(
        color: bg,
        borderRadius: AppRadius.r14,
        child: InkWell(
          onTap: onTap,
          borderRadius: AppRadius.r14,
          child: Container(
            padding: const EdgeInsets.all(AppSpace.s4),
            decoration: BoxDecoration(
              borderRadius: AppRadius.r14,
              border: Border.all(color: fg.withValues(alpha: 0.35)),
            ),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                LucideIcon(glyph, color: fg, size: 18),
                const SizedBox(width: AppSpace.s3),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        title,
                        style: AppType.bodyMd.copyWith(color: c.textHi),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        body,
                        style: AppType.bodySm.copyWith(color: c.textMed),
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: AppSpace.s2),
                LucideIcon(Lucide.chevronRight, color: c.textLow, size: 18),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
