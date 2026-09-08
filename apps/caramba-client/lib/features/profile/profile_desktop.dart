/// Профиль в десктопной раскладке: две колонки вместо одной ленты.
///
/// ПОЧЕМУ отдельный файл, а не ветка внутри `ProfileScreen`. Мобильный профиль
/// это один вертикальный список: аккаунт, баланс, подписки, устройства,
/// рефералы, поддержка. На экране 1120 та же лента растягивается в колонку
/// текста шириной с ладонь по центру пустого поля, а карточки подписок и
/// таблица устройств оказываются на разных экранах прокрутки, хотя обе
/// отвечают на один вопрос («что у меня оплачено и кто этим пользуется»).
/// Здесь слева стоит всё про деньги (аккаунт, баланс, подписки), справа всё
/// про использование (устройства, рефералы, партнёрство, поддержка), и обе
/// колонки видны одновременно.
///
/// ЧТО ОБЩЕЕ с мобильным экраном: ровно те же провайдеры и та же семантика
/// действий. Данные тянутся из тех же `currentUserProvider`,
/// `subscriptionsProvider`, `devicesProvider`, `referralProvider`,
/// `isPartnerProvider`; карточка подписки это тот же публичный
/// [SubscriptionCard] из `profile_screen.dart` (второй копии карточки с её
/// разбором статусов панели в приложении быть не должно).
///
/// ЧТО РАЗЛИЧАЕТСЯ: `RefreshIndicator` заменён явной кнопкой обновления (жеста
/// «потянуть вниз» на мыши нет), заголовок раздела рисует тулбар десктопного
/// шелла, поэтому `ScreenHead` здесь не строится.
///
/// Приватные секции устройств и рефералов из `profile_screen.dart` не
/// импортируются: они приватные для той библиотеки. Их содержимое повторено
/// ниже; логика удаления устройства, тосты и тексты условий совпадают
/// дословно, различается только раскладка.
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart' hide Family;

import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/sub_plan.dart';
import 'package:caramba_client/desktop/desktop_tokens.dart';
import 'package:caramba_client/features/notifications/notifications_screen.dart';
import 'package:caramba_client/features/profile/profile_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/account_state.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Левая колонка: карточка подписки с квота-баром и метой читается без
/// переносов, а `SubscriptionCard` не растягивается на всю ширину окна.
const double _leftPaneWidth = 400;

/// Пустое состояние без сессии: колонка текста, а не центр экрана. На 1120
/// центрированный столбец в 520 висит в пустоте; прижатый влево он стоит там
/// же, где начнётся содержимое, когда аккаунт появится.
const double _emptyPaneWidth = 520;

class ProfileDesktopScreen extends ConsumerWidget {
  const ProfileDesktopScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    // Тот же гейт, что на мобильном: без аккаунта панели тарифы, устройства и
    // рефералы не существуют, а протянутые провайдеры ушли бы в 401.
    if (ref.watch(authProvider).stage != AuthStage.authenticated) {
      return Scaffold(
        backgroundColor: c.bgBase,
        body: const _PanelRequiredPane(),
      );
    }

    final user = ref.watch(currentUserProvider);
    final subsAsync = ref.watch(subscriptionsProvider);
    final devicesAsync = ref.watch(devicesProvider);
    final referralAsync = ref.watch(referralProvider);
    // Гейт партнёрской роли: вход в дашборд рендерим только когда панель
    // подтвердила is_partner.
    final isPartner = ref.watch(isPartnerProvider);

    final handle = (user?.username != null && user!.username!.isNotEmpty)
        ? '@${user.username}'
        : (user?.displayName ?? 'Аккаунт');

    return Scaffold(
      backgroundColor: c.bgBase,
      body: Padding(
        padding: const EdgeInsets.all(DesktopTokens.contentPad),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              width: _leftPaneWidth,
              child: ListView(
                padding: EdgeInsets.zero,
                children: [
                  _AccountLine(handle: handle, email: user?.email),

                  // ---- Баланс кошелька (money-модель: пополняется рефералами).
                  const SectionTitle('Баланс'),
                  RowsGroup(
                    children: [
                      CRow(
                        icon: Lucide.wallet,
                        label: 'На балансе',
                        // Та же логика, что у мобильного `_balanceLabel`:
                        // минорные единицы -> мажорная строка.
                        value: ReferralInfo.formatMinor(
                          user?.balanceCents ?? 0,
                        ),
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
                  GhostButton(
                    label: 'Купить или продлить',
                    icon: Lucide.creditCard,
                    onPressed: () => context.push(AppRoute.plans),
                  ),
                ],
              ),
            ),
            const SizedBox(width: DesktopTokens.columnGap),
            Expanded(
              child: ListView(
                padding: EdgeInsets.zero,
                children: [
                  Row(
                    children: [
                      const Spacer(),
                      const NotificationBell(),
                      const SizedBox(width: AppSpace.s2),
                      // Замена pull-to-refresh: мышью экран не тянут вниз, а
                      // перезапрашивать панель после покупки или отзыва
                      // устройства нужно так же часто, как на телефоне.
                      IconBtn(
                        Lucide.refresh,
                        onTap: () {
                          ref.invalidate(subscriptionsProvider);
                          ref.invalidate(devicesProvider);
                          ref.invalidate(referralProvider);
                          ref.invalidate(partnerProvider);
                        },
                      ),
                    ],
                  ),

                  // ---- Устройства
                  devicesAsync.when(
                    data: (devices) => _DesktopDevicesSection(devices: devices),
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
                    data: (r) => _DesktopReferralSection(referral: r),
                    loading: () => const InlineLoading(),
                    error: (_, __) => InlineError(
                      message: 'Не удалось загрузить рефералов',
                      onRetry: () => ref.invalidate(referralProvider),
                    ),
                  ),

                  // ---- Партнёрам (только при подтверждённой роли)
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
          ],
        ),
      ),
    );
  }
}

/// Шапка аккаунта: аватар, handle, почта. Ширина колонки фиксированная,
/// поэтому длинный handle обрезается многоточием, а не ломает раскладку.
class _AccountLine extends StatelessWidget {
  final String handle;
  final String? email;
  const _AccountLine({required this.handle, this.email});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Row(
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
              if (email != null && email!.isNotEmpty) ...[
                const SizedBox(height: 2),
                Text(
                  email!,
                  overflow: TextOverflow.ellipsis,
                  style: AppType.bodySm.copyWith(color: c.textMed),
                ),
              ],
            ],
          ),
        ),
      ],
    );
  }
}

/// Пустое состояние без аккаунта панели. Текст дословно тот же, что в
/// `PanelRequiredScreen`: ответ на вопрос «почему тут пусто» не должен
/// расходиться между платформами. Отличия только в раскладке: колонка прижата
/// влево, кнопка по содержимому (min 200x44), а не во всю ширину окна.
class _PanelRequiredPane extends StatelessWidget {
  const _PanelRequiredPane();

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.all(DesktopTokens.contentPad),
      child: Align(
        alignment: Alignment.topLeft,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: _emptyPaneWidth),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              LucideIcon(Lucide.appWindow, color: c.textLow, size: 32),
              const SizedBox(height: AppSpace.s3),
              Text(
                'Аккаунта панели пока нет',
                style: AppType.titleMd.copyWith(color: c.textHi),
              ),
              const SizedBox(height: AppSpace.s2),
              Text(
                'Тарифы, устройства, рефералы и поддержка появляются вместе с '
                'аккаунтом панели. Его подключает ссылка caramba:// из бота '
                'оператора.',
                style: AppType.bodyMd.copyWith(color: c.textMed),
              ),
              const SizedBox(height: AppSpace.s5),
              FilledButton(
                style: FilledButton.styleFrom(minimumSize: const Size(200, 44)),
                onPressed: () => context.go(AppRoute.login),
                child: const Text('Подключить панель'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Устройства аккаунта. Копия мобильной секции (её класс приватен в
/// `profile_screen.dart`): те же строки, то же удаление с теми же тостами.
class _DesktopDevicesSection extends ConsumerWidget {
  final List<Device> devices;
  const _DesktopDevicesSection({required this.devices});

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

/// Реферальная сводка. Копия мобильной секции по той же причине, что и
/// устройства: её класс приватен в `profile_screen.dart`.
class _DesktopReferralSection extends StatelessWidget {
  final ReferralInfo referral;
  const _DesktopReferralSection({required this.referral});

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
