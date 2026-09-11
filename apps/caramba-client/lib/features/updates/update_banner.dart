/// Баннер «Доступна версия X» на «Подключении».
///
/// Пуст, пока обновляться не на что: виджет сам решает, показываться ли, чтобы
/// у Home не было ещё одного условия. Обязательное обновление сюда не
/// попадает: его показывает экран «Нужно обновиться» через роутер, и баннер
/// с кнопкой «Позже» рядом с ним был бы ложью.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/app_update_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';

/// Текст баннера: версия и первая строка «что нового», если оно есть.
String updateBannerText(AppVersionInfo latest) {
  final notes = latest.notes.trim();
  if (notes.isEmpty) return 'Доступна версия ${latest.label}.';
  final firstLine = notes.split('\n').first.trim();
  return 'Доступна версия ${latest.label}. Что нового: $firstLine';
}

class UpdateBanner extends ConsumerWidget {
  const UpdateBanner({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final update = ref.watch(appUpdateProvider);
    final latest = update.latest;
    if (latest == null || update.verdict != UpdateVerdict.available) {
      return const SizedBox.shrink();
    }
    final c = context.c;
    final notifier = ref.read(appUpdateProvider.notifier);
    return Padding(
      padding: const EdgeInsets.only(bottom: AppSpace.s4),
      child: Container(
        padding: const EdgeInsets.all(AppSpace.s4),
        decoration: BoxDecoration(
          color: c.surface1,
          borderRadius: AppRadius.r14,
          border: Border.all(color: c.borderSubtle),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                LucideIcon(Lucide.refresh, color: c.accent, size: 18),
                const SizedBox(width: AppSpace.s3),
                Expanded(
                  child: Text(
                    updateBannerText(latest),
                    style: AppType.bodySm.copyWith(color: c.textMed),
                  ),
                ),
              ],
            ),
            if (update.installMessage != null) ...[
              const SizedBox(height: AppSpace.s2),
              Text(
                update.installMessage!,
                style: AppType.bodySm.copyWith(color: c.textLow),
              ),
            ],
            const SizedBox(height: AppSpace.s2),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                TextButton(
                  style: TextButton.styleFrom(
                    foregroundColor: c.textMed,
                    minimumSize: const Size(0, 40),
                    padding: const EdgeInsets.symmetric(
                      horizontal: AppSpace.s3,
                    ),
                  ),
                  onPressed: () => notifier.dismiss(),
                  child: const Text('Позже'),
                ),
                TextButton(
                  style: TextButton.styleFrom(
                    foregroundColor: c.textMed,
                    minimumSize: const Size(0, 40),
                    padding: const EdgeInsets.symmetric(
                      horizontal: AppSpace.s3,
                    ),
                  ),
                  onPressed: () => context.go(AppRoute.updates),
                  child: const Text('Подробнее'),
                ),
                TextButton(
                  style: TextButton.styleFrom(
                    foregroundColor: c.textHi,
                    minimumSize: const Size(0, 40),
                    padding: const EdgeInsets.symmetric(
                      horizontal: AppSpace.s3,
                    ),
                  ),
                  onPressed: update.installing
                      ? null
                      : () => notifier.install(),
                  child: Text(update.installing ? 'Скачивание' : 'Скачать'),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
