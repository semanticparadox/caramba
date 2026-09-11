/// Экран «Обновления»: своя версия, последняя у панели, «Проверить», «что
/// нового» и «Скачать». Накладной поверх настроек (`/settings/updates`).
///
/// В отличие от баннера показывает и отложенную кнопкой «Позже» версию, и
/// ошибку проверки: человек пришёл сюда сам и спросил, а на такой вопрос
/// молчать нельзя.
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/features/updates/update_installer.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/app_update_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Строка статуса под версиями: одно предложение о том, что известно.
String updatesStatusText(AppUpdateState s, {required bool hasPanel}) {
  if (!hasPanel) {
    return 'Панель не подключена: проверять обновления не у кого. '
        'Скачать свежую версию можно в боте командой /apk.';
  }
  if (s.checking) return 'Проверяем';
  if (s.error != null) return 'Не удалось проверить: ${s.error}';
  final latest = s.latest;
  if (latest == null) {
    return s.checkedAt == null
        ? 'Ещё не проверяли.'
        : 'Панель не сообщает версию клиента. Возможно, панель старше '
              'приложения.';
  }
  if (!s.installed.isKnown) return 'Версия приложения неизвестна.';
  if (latest.build > s.installed.build) {
    return 'Доступна версия ${latest.label}.';
  }
  return 'У вас последняя версия.';
}

class UpdatesScreen extends ConsumerWidget {
  const UpdatesScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final s = ref.watch(appUpdateProvider);
    final notifier = ref.read(appUpdateProvider.notifier);
    final hasPanel = ref.watch(apiClientProvider).hasPanel;
    final latest = s.latest;
    final newer = s.hasNewer;

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: ListView(
          padding: const EdgeInsets.fromLTRB(
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s5,
            AppSpace.s12,
          ),
          children: [
            ScreenHead(
              'Обновления',
              trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
            ),
            RowsGroup(
              children: [
                CRow(
                  icon: Lucide.alert,
                  label: 'Установлена',
                  value: s.installed.label,
                ),
                CRow(
                  icon: Lucide.refresh,
                  label: 'Последняя',
                  value: latest == null ? 'неизвестно' : latest.label,
                ),
                if (latest?.publishedAt != null)
                  CRow(
                    label: 'Опубликована',
                    value: formatUpdateDate(latest!.publishedAt),
                  ),
                if (latest?.size != null && latest!.size! > 0)
                  CRow(label: 'Размер', value: formatUpdateSize(latest.size)),
              ],
            ),
            const SizedBox(height: AppSpace.s4),
            Text(
              updatesStatusText(s, hasPanel: hasPanel),
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
            if (s.installMessage != null) ...[
              const SizedBox(height: AppSpace.s3),
              InlineBanner(
                tone: BannerTone.info,
                glyph: Lucide.alert,
                text: s.installMessage!,
              ),
            ],
            if (latest != null && latest.notes.trim().isNotEmpty) ...[
              const SectionTitle('Что нового'),
              Container(
                padding: const EdgeInsets.all(AppSpace.s4),
                decoration: BoxDecoration(
                  color: c.surface1,
                  borderRadius: AppRadius.r14,
                  border: Border.all(color: c.borderSubtle),
                ),
                child: Text(
                  latest.notes.trim(),
                  style: AppType.bodySm.copyWith(color: c.textHi),
                ),
              ),
            ],
            const SizedBox(height: AppSpace.s5),
            if (newer)
              GhostButton(
                label: s.installing ? 'Скачивание' : 'Скачать обновление',
                icon: Lucide.refresh,
                onPressed: s.installing ? null : () => notifier.install(),
              ),
            if (newer) const SizedBox(height: AppSpace.s3),
            QuietButton(
              label: s.checking ? 'Проверяем' : 'Проверить',
              onPressed: (s.checking || !hasPanel)
                  ? null
                  : () => notifier.check(),
            ),
          ],
        ),
      ),
    );
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.settings);
    }
  }
}
