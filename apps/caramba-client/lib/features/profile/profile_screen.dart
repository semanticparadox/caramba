import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart' hide Family;

import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/data/models/subscription.dart'
    show AccessState, formatBytesRu;
import 'package:caramba_client/desktop/adaptive_sheet.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/features/profile/panel_required.dart';
import 'package:caramba_client/features/servers/access_card.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/device_identity.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Профиль: подписки (free с недельной квота-баром, платные с метой),
/// устройства, рефералы, семейный доступ. Все данные — из `/api/v2/app/*`.
///
/// Весь экран панельный. В generic-режиме (своя подписка, аккаунта панели нет)
/// показывать нечего, а протянутые провайдеры ушли бы в 401 — поэтому без
/// сессии рендерим пустое состояние с приглашением подключить панель.
class ProfileScreen extends ConsumerWidget {
  const ProfileScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    if (ref.watch(authProvider).stage != AuthStage.authenticated) {
      return const PanelRequiredScreen(title: 'Профиль');
    }
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
                            SubscriptionCard(sub: subs[i]),
                        ],
                      ),
                loading: () => const InlineLoading(),
                error: (_, __) => InlineError(
                  message: 'Не удалось загрузить подписки',
                  onRetry: () => ref.invalidate(subscriptionsProvider),
                ),
              ),
              const SizedBox(height: AppSpace.s2),
              // Раньше эта кнопка звала openExternal со ссылкой из брендинга —
              // и у оператора, который бота не опубликовал, отдавала пустую
              // строку: нажатие показывало «Ссылка недоступна» и всё. Теперь
              // она ведёт на витрину тарифов, а решение «оплатить здесь или
              // уйти в Telegram» принимается там, где для него есть данные.
              GhostButton(
                label: 'Купить или продлить',
                icon: Lucide.creditCard,
                onPressed: () => context.push(AppRoute.plans),
              ),

              // ---- Устройства
              devicesAsync.when(
                data: (devices) => ProfileDevicesSection(devices: devices),
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

/// Устройства аккаунта: имя, платформа, последняя активность, отметка «это
/// устройство», переименование и отвязка.
///
/// ПУБЛИЧНАЯ и общая с десктопом (`profile_desktop.dart`) намеренно. Раньше
/// секция была приватной, и десктоп держал её дословную копию: два списка
/// устройств с двумя наборами тостов расходятся при первой же правке, а список
/// этот — единственное место, где человек управляет привязками. Раскладка
/// строки одинакова на обеих платформах, различается только колонка, в которой
/// секция стоит.
class ProfileDevicesSection extends ConsumerWidget {
  final List<Device> devices;
  const ProfileDevicesSection({required this.devices, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    // Своё устройство: панель помечает его `is_current` по заголовку запроса,
    // но со старой панелью поля нет вовсе — тогда сверяем идентификатор сами.
    final mine = ref.watch(deviceIdentityProvider).valueOrNull;
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
          for (final d in devices)
            DeviceCard(device: d, isCurrent: isCurrentDevice(d, mine)),
      ],
    );
  }
}

/// Это ли устройство, с которого человек сейчас смотрит на список.
///
/// Два источника, и оба нужны: `is_current` панели работает и для сторонних
/// клиентов (там панель узнаёт лизу по отпечатку), а сверка идентификатора —
/// единственное, что работает с панелью, которая новых полей ещё не отдаёт.
bool isCurrentDevice(Device device, DeviceIdentity? mine) {
  if (device.isCurrent) return true;
  if (mine == null || !mine.isKnown) return false;
  return device.clientDeviceId.isNotEmpty &&
      device.clientDeviceId == mine.clientDeviceId;
}

/// Карточка одного устройства. Публичная ради теста, монтирующего её без всего
/// стека профиля (см. `SubscriptionCard` рядом — та же причина).
class DeviceCard extends ConsumerWidget {
  final Device device;
  final bool isCurrent;

  const DeviceCard({required this.device, this.isCurrent = false, super.key});

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
              IBox(device.icon, size: 34),
              const SizedBox(width: AppSpace.s3),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      device.name,
                      overflow: TextOverflow.ellipsis,
                      style: AppType.bodyMd.copyWith(color: c.textHi),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      device.metaLabel,
                      overflow: TextOverflow.ellipsis,
                      style: AppType.bodySm.copyWith(
                        color: device.online ? c.success : c.textMed,
                      ),
                    ),
                  ],
                ),
              ),
              if (isCurrent) ...[
                const SizedBox(width: AppSpace.s2),
                const Tag('Это устройство', ok: true),
              ],
            ],
          ),
          const SizedBox(height: AppSpace.s3),
          Row(
            children: [
              Expanded(
                child: GhostButton(
                  label: 'Переименовать',
                  icon: Lucide.user,
                  minHeight: 42,
                  onPressed: () => _rename(context, ref),
                ),
              ),
              const SizedBox(width: AppSpace.s2),
              Expanded(
                child: GhostButton(
                  label: 'Отвязать',
                  icon: Lucide.trash,
                  minHeight: 42,
                  onPressed: () => _unbind(context, ref),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Future<void> _rename(BuildContext context, WidgetRef ref) async {
    final name = await showAdaptiveSheet<String>(
      context,
      builder: (ctx) => _RenameDeviceSheet(device: device),
    );
    if (name == null) return;
    try {
      await ref.read(devicesProvider.notifier).rename(device.id, name);
      // Своё устройство переименовано — местное имя обязано поехать следом:
      // заголовок `X-Caramba-Device-Name` следующего запроса иначе вернёт
      // панели прежнее имя, и переименование отменится само собой.
      if (isCurrent) {
        await ref.read(deviceIdentityStoreProvider).rename(name);
        ref.invalidate(deviceIdentityProvider);
      }
      if (context.mounted) showCarambaToast(context, 'Имя устройства изменено');
    } on ApiException catch (e) {
      if (context.mounted) showCarambaToast(context, e.message);
    }
  }

  Future<void> _unbind(BuildContext context, WidgetRef ref) async {
    // Подтверждение только для своего устройства: отвязать чужой телефон это
    // решение, которое видно сразу, а отвязать своё значит оборвать туннель
    // здесь и сейчас — о таком спрашивают.
    if (isCurrent) {
      final ok = await showAdaptiveSheet<bool>(
        context,
        builder: (ctx) => const _ConfirmUnbindSheet(),
      );
      if (ok != true) return;
    }
    try {
      await ref.read(devicesProvider.notifier).remove(device.id);
      if (context.mounted) showCarambaToast(context, 'Устройство отвязано');
    } on ApiException catch (e) {
      if (context.mounted) showCarambaToast(context, e.message);
    }
  }
}

/// Лист переименования: поле с текущим именем и кнопка сохранения. Возвращает
/// новое имя либо `null`, если человек закрыл лист.
class _RenameDeviceSheet extends StatefulWidget {
  final Device device;
  const _RenameDeviceSheet({required this.device});

  @override
  State<_RenameDeviceSheet> createState() => _RenameDeviceSheetState();
}

class _RenameDeviceSheetState extends State<_RenameDeviceSheet> {
  late final TextEditingController _controller = TextEditingController(
    text: widget.device.name,
  );

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    final name = _controller.text.trim();
    if (name.isEmpty) return;
    Navigator.of(context).pop(name);
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
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
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              'Имя устройства',
              style: AppType.titleLg.copyWith(color: c.textHi),
            ),
            const SizedBox(height: AppSpace.s1),
            Text(
              'Так это устройство будет называться в списке. Видите его только вы.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s4),
            TextField(
              controller: _controller,
              autofocus: true,
              maxLength: kDeviceNameMaxLength,
              textInputAction: TextInputAction.done,
              onSubmitted: (_) => _submit(),
              style: AppType.bodyMd.copyWith(color: c.textHi),
              decoration: InputDecoration(
                counterText: '',
                hintText: 'Например, Телефон Артёма',
                hintStyle: AppType.bodyMd.copyWith(color: c.textLow),
                filled: true,
                fillColor: c.surface1,
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: AppSpace.s4,
                  vertical: AppSpace.s3 + 2,
                ),
                enabledBorder: OutlineInputBorder(
                  borderRadius: AppRadius.r12,
                  borderSide: BorderSide(color: c.borderSubtle),
                ),
                focusedBorder: OutlineInputBorder(
                  borderRadius: AppRadius.r12,
                  borderSide: BorderSide(color: c.borderStrong),
                ),
              ),
            ),
            const SizedBox(height: AppSpace.s3),
            FilledButton(onPressed: _submit, child: const Text('Сохранить')),
          ],
        ),
      ),
    );
  }
}

/// Подтверждение отвязки СВОЕГО устройства.
class _ConfirmUnbindSheet extends StatelessWidget {
  const _ConfirmUnbindSheet();

  @override
  Widget build(BuildContext context) {
    final c = context.c;
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
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              'Отвязать это устройство',
              style: AppType.titleLg.copyWith(color: c.textHi),
            ),
            const SizedBox(height: AppSpace.s1),
            Text(
              'Подключение на нём прервётся. Устройство займёт слот заново при '
              'следующем подключении.',
              style: AppType.bodyMd.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s4),
            QuietButton(
              label: 'Отвязать',
              onPressed: () => Navigator.of(context).pop(true),
            ),
            GhostButton(
              label: 'Отмена',
              minHeight: 42,
              onPressed: () => Navigator.of(context).pop(false),
            ),
          ],
        ),
      ),
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

/// Карточка одной подписки в списке профиля. Публичный класс (а не приватный
/// `_SubCard`) намеренно: это позволяет тестам монтировать карточку саму по
/// себе, без всего auth-стека `ProfileScreen`, и проверять на РЕНДЕРЕ, что
/// сырой статус панели («throttled», «expired», ...) никогда не долетает до
/// текста на экране.
class SubscriptionCard extends ConsumerWidget {
  final SubPlan sub;
  const SubscriptionCard({required this.sub, super.key});

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
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 130),
                child: Tag(_statusLabel(sub), ok: sub.isActive),
              ),
            ],
          ),
          const SizedBox(height: AppSpace.s3),
          // Панель считает бесплатную норму СУТКАМИ (`plans.daily_traffic_mb`,
          // `quota_period == "day"`), а не неделями — `weekly_free_refill_gb`
          // ниже лишь домножает суточную цифру на 7 для другой витрины. Читать
          // отсюда нужно `access.usedBytes`/`access.limitBytes`: это ровно те
          // байты, которые enforcement считает за сегодня.
          if (sub.kind == SubKind.free && sub.access.limitBytes > 0) ...[
            Text(
              '${formatBytesRu(sub.access.usedBytes)} из '
              '${formatBytesRu(sub.access.limitBytes)} в день',
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
            const SizedBox(height: AppSpace.s3),
            QuotaMeter(
              fraction: _dailyFraction(sub.access),
              low: _dailyFraction(sub.access) > 0.8,
            ),
          ] else
            Text(
              [sub.meta, sub.expiresLabel].where((s) => s != null).join(' · '),
              style: AppType.bodySm.copyWith(color: c.textMed),
            ),
          // Причина отказа и путь к оплате — тот же виджет, что на экранах
          // серверов/дома: второй копии этого текста в приложении быть не
          // должно (см. комментарий в access_card.dart).
          if (!sub.isActive) ...[
            const SizedBox(height: AppSpace.s3),
            AccessCard(access: sub.access),
          ],
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

  void _openFamily(BuildContext context, WidgetRef ref, SubPlan sub) {
    showAdaptiveSheet<void>(context, builder: (ctx) => _FamilySheet(sub: sub));
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
      final link = invite.inviteLinkFor(
        ref.read(activeBrandingProvider).botUrl,
      );
      unawaited(Clipboard.setData(ClipboardData(text: link)));
      ref.invalidate(familyProvider(sub.id));
      if (context.mounted) {
        Navigator.of(context).pop();
        showCarambaToast(context, 'Ссылка-приглашение скопирована');
        if (link.startsWith('http')) await openExternal(context, link);
      }
    } on ApiException catch (e) {
      if (context.mounted) showCarambaToast(context, e.message);
    }
  }
}

/// Человеческая метка статуса подписки для бейджа в карточке.
///
/// НИКОГДА не показывает сырое значение `sub.status` панели («throttled»,
/// «expired», «pending», «banned» — внутренние слова, которых пользователь не
/// должен видеть ни разу). Активная подписка — фиксированное «Активна»; для
/// любого заблокированного состояния текст берётся из [AccessState.shortReason]
/// — готового человеческого предложения, которое уже знает разницу между
/// «сгорела дневная норма» (сама вернётся) и «подписка кончилась» (нужна
/// оплата). Второй словарь строк здесь не заводится: непризнанный статус
/// панели [AccessState.fromLegacy] уже относит к `AccessKind.unknown`, и
/// `shortReason` для него тоже человеческий, а не сырой.
String _statusLabel(SubPlan sub) =>
    sub.isActive ? 'Активна' : sub.access.shortReason;

/// Доля израсходованной дневной нормы 0..1, для полосы прогресса.
double _dailyFraction(AccessState access) {
  final limit = access.limitBytes;
  if (limit <= 0) return 0;
  return (access.usedBytes / limit).clamp(0.0, 1.0);
}
