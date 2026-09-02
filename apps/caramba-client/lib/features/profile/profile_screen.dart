import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart' hide Family;

import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Профиль: подписки (free с недельной квота-баром, платные с метой),
/// устройства, рефералы, семейный доступ. Все данные — из `/api/v2/app/*`.
class ProfileScreen extends ConsumerWidget {
  const ProfileScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final user = ref.watch(currentUserProvider);
    final subsAsync = ref.watch(subscriptionsProvider);
    final devicesAsync = ref.watch(devicesProvider);
    final referralAsync = ref.watch(referralProvider);
    // Гейт партнёрской роли: вход в дашборд рендерим только когда панель
    // подтвердила is_partner. Обычные пользователи раздел не видят.
    final isPartner = ref.watch(isPartnerProvider);

    final handle = (user?.username != null && user!.username!.isNotEmpty)
        ? '@${user.username}'
        : (user?.displayName ?? 'Аккаунт');

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: RefreshIndicator(
          color: c.accent,
          backgroundColor: c.surface2,
          onRefresh: () async {
            ref.invalidate(subscriptionsProvider);
            ref.invalidate(devicesProvider);
            ref.invalidate(referralProvider);
            ref.invalidate(partnerProvider);
            // Ждём первую перезагрузку (ошибки проглатываем — секции покажут
            // своё состояние ошибки сами).
            await ref
                .read(subscriptionsProvider.future)
                .catchError((_) => <SubPlan>[]);
          },
          child: ListView(
            padding: const EdgeInsets.fromLTRB(
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s5,
              AppSpace.s20 + AppSpace.s6,
            ),
            children: [
              const ScreenHead('Профиль', trailing: NotificationBell()),
              Row(
                children: [
                  Container(
                    width: 52,
                    height: 52,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: c.surfaceInset,
                      border: Border.all(color: c.borderSubtle),
                    ),
                    alignment: Alignment.center,
                    child: LucideIcon(Lucide.user, color: c.textMed, size: 24),
                  ),
                  const SizedBox(width: AppSpace.s3),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          handle,
                          overflow: TextOverflow.ellipsis,
                          style: AppType.titleLg.copyWith(color: c.textHi),
                        ),
                        if (user?.email != null && user!.email!.isNotEmpty) ...[
                          const SizedBox(height: 2),
                          Text(
                            user.email!,
                            overflow: TextOverflow.ellipsis,
                            style: AppType.bodySm.copyWith(color: c.textMed),
                          ),
                        ],
                      ],
                    ),
                  ),
                ],
              ),

              // ---- Баланс кошелька (money-модель: пополняется рефералами).
              const SectionTitle('Баланс'),
              RowsGroup(
                children: [
                  CRow(
                    icon: Lucide.wallet,
                    label: 'На балансе',
                    value: _balanceLabel(user?.balanceCents ?? 0),
                    mono: true,
                    valueColor: (user?.balanceCents ?? 0) > 0
                        ? c.success
                        : c.textHi,
                  ),
                ],
              ),

              // ---- Подписки
              const SectionTitle('Подписки'),
              subsAsync.when(
                data: (subs) => subs.isEmpty
                    ? const InlineEmpty(message: 'Активных подписок нет')
                    : Column(
                        children: [
                          for (var i = 0; i < subs.length; i++)
                            _SubCard(sub: subs[i]),
                        ],
                      ),
                loading: () => const InlineLoading(),
                error: (_, __) => InlineError(
                  message: 'Не удалось загрузить подписки',
                  onRetry: () => ref.invalidate(subscriptionsProvider),
                ),
              ),
              const SizedBox(height: AppSpace.s2),
              GhostButton(
                label: 'Купить или продлить',
                icon: Lucide.creditCard,
                onPressed: () =>
                    openExternal(context, 'https://t.me/exarobot?start=plans'),
              ),

              // ---- Устройства
              devicesAsync.when(
                data: (devices) => _DevicesSection(devices: devices),
                loading: () => const Column(
                  children: [SectionTitle('Устройства'), InlineLoading()],
                ),
                error: (_, __) => Column(
                  children: [
                    const SectionTitle('Устройства'),
                    InlineError(
                      message: 'Не удалось загрузить устройства',
                      onRetry: () => ref.invalidate(devicesProvider),
                    ),
                  ],
                ),
              ),

              // ---- Рефералы
              const SectionTitle('Рефералы'),
              referralAsync.when(
                data: (r) => _ReferralSection(referral: r),
                loading: () => const InlineLoading(),
                error: (_, __) => InlineError(
                  message: 'Не удалось загрузить рефералов',
                  onRetry: () => ref.invalidate(referralProvider),
                ),
              ),

              // ---- Партнёрам (только при подтверждённой партнёрской роли)
              if (isPartner) ...[
                const SectionTitle('Партнёрам'),
                RowsGroup(
                  children: [
                    CRow(
                      icon: Lucide.trendingUp,
                      label: 'Партнёрский дашборд',
                      chevron: true,
                      onTap: () => context.go(AppRoute.partner),
                    ),
                  ],
                ),
              ],

              // ---- Поддержка
              const SectionTitle('Поддержка'),
              RowsGroup(
                children: [
                  CRow(
                    icon: Lucide.lifeBuoy,
                    label: 'Запросы в поддержку',
                    chevron: true,
                    onTap: () => context.go(AppRoute.tickets),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// Баланс кошелька из `users.balance_cents` (минорные единицы) -> мажорная
  /// строка. Делим на ту же логику, что и реферальная сводка.
  String _balanceLabel(int cents) => ReferralInfo.formatMinor(cents);
}

class _DevicesSection extends ConsumerWidget {
  final List<Device> devices;
  const _DevicesSection({required this.devices});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SectionTitle(
          'Устройства',
          trailing: Text(
            '${devices.length}',
            style: AppType.monoSm.copyWith(color: c.textLow),
          ),
        ),
        if (devices.isEmpty)
          const InlineEmpty(message: 'Подключённых устройств нет')
        else
          RowsGroup(
            children: [
              for (final d in devices)
                CRow(
                  icon: d.icon,
                  label: d.name,
                  value: d.lastSeenLabel,
                  valueColor: d.online ? c.success : null,
                  trailing: IconBtn(
                    Lucide.trash,
                    size: 36,
                    color: c.danger,
                    onTap: () async {
                      try {
                        await ref.read(devicesProvider.notifier).remove(d.id);
                        if (context.mounted) {
                          showCarambaToast(context, 'Устройство отключено');
                        }
                      } on ApiException catch (e) {
                        if (context.mounted) {
                          showCarambaToast(context, e.message);
                        }
                      }
                    },
                  ),
                ),
            ],
          ),
      ],
    );
  }
}

class _ReferralSection extends StatelessWidget {
  final ReferralInfo referral;
  const _ReferralSection({required this.referral});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        RowsGroup(
          children: [
            CRow(
              icon: Lucide.users,
              label: 'Ваш код',
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
              label: 'Всего начислено',
              value: referral.balanceEarnedLabel,
              mono: true,
              valueColor: referral.balanceEarnedCents > 0
                  ? c.success
                  : c.textMed,
            ),
            CRow(
              icon: Lucide.gift,
              label: 'Реферальная программа',
              chevron: true,
              onTap: () => context.go(AppRoute.referrals),
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
        const SizedBox(height: AppSpace.s3),
        Text(
          'Приведите друга: он получит ${referral.refereeDiscountPercent}% '
          'скидки на первую покупку, а вам начислится '
          '${referral.rewardPercent}% от его платежа на баланс.',
          style: AppType.bodySm.copyWith(color: c.textMed),
        ),
      ],
    );
  }
}

class _SubCard extends ConsumerWidget {
  final SubPlan sub;
  const _SubCard({required this.sub});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    return Container(
      margin: const EdgeInsets.only(bottom: AppSpace.s2),
      padding: const EdgeInsets.all(AppSpace.s4),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r16,
        border: Border.all(color: c.borderSubtle),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              IBox(sub.icon, size: 34),
              const SizedBox(width: AppSpace.s3),
              Expanded(
                child: Text(
                  sub.name,
                  overflow: TextOverflow.ellipsis,
                  style: AppType.bodyMd.copyWith(color: c.textHi),
                ),
              ),
              Tag(sub.isActive ? 'Активна' : sub.status, ok: sub.isActive),
            ],
          ),
          const SizedBox(height: AppSpace.s3),
          if (sub.kind == SubKind.free && sub.quotaGb > 0) ...[
            Text(
              '${_gb(sub.usedGb)} из ${_gb(sub.quotaGb)} ГБ в неделю',
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s3),
            QuotaMeter(fraction: sub.quotaFraction, low: sub.quotaLow),
          ] else
            Text(
              [sub.meta, sub.expiresLabel].where((s) => s != null).join(' · '),
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
          const SizedBox(height: AppSpace.s3),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                'Устройства',
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
              Text(
                '${sub.devUsed} из ${sub.devLimit}',
                style: AppType.monoSm.copyWith(color: c.textHi),
              ),
            ],
          ),
          const SizedBox(height: AppSpace.s2),
          Text(sub.poolLabel, style: AppType.bodySm.copyWith(color: c.textLow)),
          if (sub.shareable) ...[
            const SizedBox(height: AppSpace.s3),
            GhostButton(
              label: sub.freeSlots > 0
                  ? 'Поделиться доступом (${sub.freeSlots} своб.)'
                  : 'Семейный доступ',
              icon: Lucide.userPlus,
              minHeight: 42,
              onPressed: () => _openFamily(context, ref, sub),
            ),
          ],
        ],
      ),
    );
  }

  String _gb(double v) =>
      v == v.roundToDouble() ? v.toStringAsFixed(0) : v.toStringAsFixed(1);

  void _openFamily(BuildContext context, WidgetRef ref, SubPlan sub) {
    showModalBottomSheet<void>(
      context: context,
      backgroundColor: context.c.surface1,
      isScrollControlled: true,
      showDragHandle: true,
      builder: (ctx) => _FamilySheet(sub: sub),
    );
  }
}

/// Лист семейного доступа: участники из `/app/family`, приглашение через
/// `/app/family/invite` (deeplink в бота), удаление участника.
class _FamilySheet extends ConsumerWidget {
  final SubPlan sub;
  const _FamilySheet({required this.sub});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final familyAsync = ref.watch(familyProvider(sub.id));

    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(
          AppSpace.s5,
          AppSpace.s1,
          AppSpace.s5,
          AppSpace.s6,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Семейный доступ',
              style: AppType.titleLg.copyWith(color: c.textHi),
            ),
            const SizedBox(height: AppSpace.s1),
            Text(
              '${sub.name}: пригласите близких. Их устройства займут свободные слоты.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s4),
            familyAsync.when(
              data: (family) => _members(context, ref, family),
              loading: () => const InlineLoading(top: AppSpace.s4),
              error: (_, __) => InlineError(
                top: AppSpace.s4,
                message: 'Не удалось загрузить участников',
                onRetry: () => ref.invalidate(familyProvider(sub.id)),
              ),
            ),
            const SizedBox(height: AppSpace.s4),
            if (sub.freeSlots > 0)
              FilledButton.icon(
                onPressed: () => _invite(context, ref),
                icon: LucideIcon(
                  Lucide.userPlus,
                  color: c.textOnAccent,
                  size: 18,
                ),
                label: Text('Пригласить (${sub.freeSlots} своб.)'),
              )
            else
              Text(
                'Все слоты заняты. Уберите участника или повысьте тариф.',
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
          ],
        ),
      ),
    );
  }

  Widget _members(BuildContext context, WidgetRef ref, Family family) {
    final c = context.c;
    if (family.members.isEmpty) {
      return const InlineEmpty(
        top: AppSpace.s2,
        message: 'В семье пока только вы',
      );
    }
    return RowsGroup(
      children: [
        for (final m in family.members)
          CRow(
            icon: Lucide.user,
            label: m.displayName,
            value: m.hasActiveSub ? 'активна' : null,
            valueColor: m.hasActiveSub ? c.success : null,
            trailing: IconBtn(
              Lucide.trash,
              size: 36,
              color: c.danger,
              onTap: () async {
                try {
                  await ref
                      .read(apiClientProvider)
                      .removeFamilyMember(m.userId);
                  ref.invalidate(familyProvider(sub.id));
                  ref.invalidate(subscriptionsProvider);
                  if (context.mounted) {
                    showCarambaToast(context, 'Участник убран из тарифа');
                  }
                } on ApiException catch (e) {
                  if (context.mounted) showCarambaToast(context, e.message);
                }
              },
            ),
          ),
      ],
    );
  }

  Future<void> _invite(BuildContext context, WidgetRef ref) async {
    try {
      final invite = await ref
          .read(apiClientProvider)
          .inviteFamily(subscriptionId: sub.id);
      unawaited(Clipboard.setData(ClipboardData(text: invite.inviteLink)));
      ref.invalidate(familyProvider(sub.id));
      if (context.mounted) {
        Navigator.of(context).pop();
        showCarambaToast(context, 'Ссылка-приглашение скопирована');
        await openExternal(context, invite.inviteLink);
      }
    } on ApiException catch (e) {
      if (context.mounted) showCarambaToast(context, e.message);
    }
  }
}
