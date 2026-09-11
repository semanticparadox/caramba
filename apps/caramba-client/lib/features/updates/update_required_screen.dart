/// «Нужно обновиться»: панель требует сборку новее установленной.
///
/// Экран заменяет приложение (роутер держит на нём любую локацию, см.
/// `resolveRedirect`), поэтому у него нет крестика и «Назад». Единственные
/// действия — скачать обновление и проверить ещё раз (панель могла опустить
/// минимум). Причина названа прямо: старой сборке панель отказывает не из
/// вредности, а потому что протокол или формат ответа уже другой.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/features/branding/brand_wordmark.dart';
import 'package:caramba_client/state/app_update_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

class UpdateRequiredScreen extends ConsumerWidget {
  const UpdateRequiredScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final s = ref.watch(appUpdateProvider);
    final notifier = ref.read(appUpdateProvider.notifier);
    final latest = s.latest;
    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s12,
          ),
          children: [
            BrandWordmark(
              height: 28,
              textStyle: AppType.titleMd.copyWith(color: c.textHi),
            ),
            const SizedBox(height: AppSpace.s8),
            LucideIcon(Lucide.refresh, color: c.accent, size: 40),
            const SizedBox(height: AppSpace.s4),
            Text(
              'Нужно обновиться',
              style: AppType.headline.copyWith(color: c.textHi),
            ),
            const SizedBox(height: AppSpace.s3),
            Text(
              'Установлена версия ${s.installed.label}, а панель работает '
              'только с версией ${latest?.label ?? 'новее'} и выше. '
              'Обновите приложение, чтобы подключаться.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            if (latest != null && latest.notes.trim().isNotEmpty) ...[
              const SectionTitle('Что нового'),
              Text(
                latest.notes.trim(),
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
            ],
            if (s.installMessage != null) ...[
              const SizedBox(height: AppSpace.s4),
              InlineBanner(
                tone: BannerTone.info,
                glyph: Lucide.alert,
                text: s.installMessage!,
              ),
            ],
            const SizedBox(height: AppSpace.s6),
            GhostButton(
              label: s.installing ? 'Скачивание' : 'Скачать обновление',
              icon: Lucide.refresh,
              onPressed: s.installing ? null : () => notifier.install(),
            ),
            const SizedBox(height: AppSpace.s3),
            QuietButton(
              label: s.checking ? 'Проверяем' : 'Проверить ещё раз',
              onPressed: s.checking ? null : () => notifier.check(),
            ),
          ],
        ),
      ),
    );
  }
}
