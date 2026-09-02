import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/brand.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/features/profile/panel_required.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Реферальная программа. Денежная модель: за оплату приглашённого реферер
/// получает процент на внутренний баланс; приглашённый получает скидку на свою
/// первую платную покупку. Данные — авторитетный `/api/v2/app/referrals`.
class ReferralsScreen extends ConsumerWidget {
  const ReferralsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    // Раздел живёт у аккаунта панели: в generic-режиме показываем, что нужно
    // сделать, чтобы он заработал, вместо 401 за каждым провайдером.
    if (ref.watch(authProvider).stage != AuthStage.authenticated) {
      return const PanelRequiredScreen(title: 'Рефералы');
    }
    final async = ref.watch(referralProvider);

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: RefreshIndicator(
          color: c.accent,
          backgroundColor: c.surface2,
          onRefresh: () async {
            ref.invalidate(referralProvider);
            await ref
                .read(referralProvider.future)
                .catchError((_) => const ReferralInfo(code: '', invited: 0));
          },
          child: ListView(
            padding: const EdgeInsets.fromLTRB(
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s12,
            ),
            children: [
              ScreenHead(
                'Рефералы',
                trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
              ),
              async.when(
                data: (r) => _ReferralBody(referral: r),
                loading: () => const InlineLoading(top: AppSpace.s16),
                error: (_, __) => InlineError(
                  top: AppSpace.s16,
                  message: 'Не удалось загрузить реферальную программу',
                  onRetry: () => ref.invalidate(referralProvider),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _close(BuildContext context) {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.profile);
    }
  }
}

class _ReferralBody extends StatelessWidget {
  final ReferralInfo referral;
  const _ReferralBody({required this.referral});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          'Приглашайте друзей в $kBrandName. Когда приглашённый оплатит подписку, '
          'вам начислится ${referral.rewardPercent}% от его платежа на баланс, '
          'а он получит ${referral.refereeDiscountPercent}% скидки на первую покупку.',
          style: AppType.bodyMd.copyWith(color: c.textMed),
        ),

        // ---- Баланс / всего начислено (минорные единицы, mono-значения).
        const SectionTitle('Баланс'),
        Row(
          children: [
            Expanded(
              child: _StatCard(
                icon: Lucide.wallet,
                label: 'Баланс',
                value: referral.balanceLabel,
                accent: referral.balanceCents > 0,
              ),
            ),
            const SizedBox(width: AppSpace.s2),
            Expanded(
              child: _StatCard(
                icon: Lucide.trendingUp,
                label: 'Всего начислено',
                value: referral.balanceEarnedLabel,
                accent: false,
              ),
            ),
          ],
        ),

        // ---- Код + ссылка.
        const SectionTitle('Ваш код'),
        RowsGroup(
          children: [
            CRow(
              icon: Lucide.users,
              label: 'Код',
              value: referral.code.isEmpty ? '·' : referral.code,
              mono: true,
              valueColor: c.textHi,
            ),
            CRow(
              label: 'Приглашено',
              value: '${referral.invited}',
              mono: true,
              valueColor: c.textHi,
            ),
            CRow(
              icon: Lucide.percent,
              label: 'Ваше вознаграждение',
              value: '${referral.rewardPercent}%',
              mono: true,
              valueColor: c.textHi,
            ),
            CRow(
              icon: Lucide.badgePercent,
              label: 'Скидка приглашённому',
              value: '${referral.refereeDiscountPercent}%',
              mono: true,
              valueColor: c.textHi,
            ),
          ],
        ),
        const SizedBox(height: AppSpace.s3),
        GhostButton(
          label: 'Скопировать ссылку',
          icon: Lucide.copy,
          onPressed: () {
            Clipboard.setData(ClipboardData(text: referral.inviteLink));
            showCarambaToast(context, 'Ссылка скопирована');
          },
        ),
        const SizedBox(height: AppSpace.s2),
        GhostButton(
          label: 'Открыть в Telegram',
          icon: Lucide.externalLink,
          onPressed: () => openExternal(context, referral.inviteLink),
        ),

        // ---- Список приглашённых.
        SectionTitle(
          'Приглашённые',
          trailing: Text(
            '${referral.referrals.length}',
            style: AppType.monoSm.copyWith(color: c.textLow),
          ),
        ),
        if (referral.referrals.isEmpty)
          const InlineEmpty(
            top: AppSpace.s4,
            message: 'Вы ещё никого не пригласили. Поделитесь ссылкой выше.',
          )
        else
          RowsGroup(
            children: [
              for (final e in referral.referrals) _ReferralRow(entry: e),
            ],
          ),
      ],
    );
  }
}

/// Карточка-метрика баланса: mono-значение крупно, подпись под ним.
class _StatCard extends StatelessWidget {
  final String icon;
  final String label;
  final String value;
  final bool accent;
  const _StatCard({
    required this.icon,
    required this.label,
    required this.value,
    required this.accent,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      padding: const EdgeInsets.all(AppSpace.s4),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r16,
        border: Border.all(color: c.borderSubtle),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          LucideIcon(icon, color: c.textMed, size: 18),
          const SizedBox(height: AppSpace.s3),
          // Денежное значение — mono (anti-slop: mono только для числовых данных).
          Text(
            value,
            style: AppType.monoMd.copyWith(
              fontSize: 26,
              height: 1.1,
              color: accent ? c.success : c.textHi,
            ),
          ),
          const SizedBox(height: 4),
          Text(label, style: AppType.bodySm.copyWith(color: c.textMed)),
        ],
      ),
    );
  }
}

/// Строка приглашённого: маскированный username, статус-тег, начислено (mono).
class _ReferralRow extends StatelessWidget {
  final ReferralEntry entry;
  const _ReferralRow({required this.entry});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final earned = entry.earnedLabel;
    return CRow(
      icon: Lucide.user,
      label: entry.displayName,
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Tag(entry.purchased ? 'оплатил' : 'регистрация', ok: entry.purchased),
          if (entry.earnedCents > 0) ...[
            const SizedBox(width: AppSpace.s3),
            Text('+$earned', style: AppType.monoMd.copyWith(color: c.success)),
          ],
        ],
      ),
    );
  }
}
