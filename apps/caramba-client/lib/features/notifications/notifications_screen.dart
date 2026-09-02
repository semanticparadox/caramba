import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/notification.dart';
import 'package:caramba_client/features/profile/panel_required.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/notifications_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Колокол с бейджем непрочитанных для шапки Home/Profile. Открывает
/// [NotificationsScreen]. Цвет иконки нейтральный; бейдж — единственный цвет
/// (danger), несёт смысл «есть непрочитанное».
class NotificationBell extends ConsumerWidget {
  const NotificationBell({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final unread = ref.watch(unreadCountProvider);
    return SizedBox(
      width: 44,
      height: 44,
      child: Material(
        color: c.surface1,
        borderRadius: AppRadius.r12,
        child: InkWell(
          borderRadius: AppRadius.r12,
          onTap: () => context.go(AppRoute.notifications),
          child: Container(
            decoration: BoxDecoration(
              borderRadius: AppRadius.r12,
              border: Border.all(color: c.borderSubtle),
            ),
            alignment: Alignment.center,
            child: Stack(
              clipBehavior: Clip.none,
              children: [
                LucideIcon(Lucide.bell, color: c.textMed, size: 20),
                if (unread > 0)
                  Positioned(
                    right: -6,
                    top: -6,
                    child: Container(
                      padding: const EdgeInsets.symmetric(horizontal: 4),
                      constraints: const BoxConstraints(minWidth: 16),
                      height: 16,
                      decoration: BoxDecoration(
                        color: c.danger,
                        borderRadius: BorderRadius.circular(8),
                        border: Border.all(color: c.surface1, width: 1.5),
                      ),
                      alignment: Alignment.center,
                      child: Text(
                        unread > 99 ? '99+' : '$unread',
                        style: AppType.monoSm.copyWith(
                          color: Colors.white,
                          fontSize: 9,
                          height: 1,
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Экран уведомлений: лента, пометка прочитанным по тапу, действие «Прочитать
/// все», пустое состояние. Непрочитанные выделяются плотностью поверхности и
/// точкой-маркером (без семантического цвета — панель его не присылает).
class NotificationsScreen extends ConsumerWidget {
  const NotificationsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    // Раздел живёт у аккаунта панели: в generic-режиме показываем, что нужно
    // сделать, чтобы он заработал, вместо 401 за каждым провайдером.
    if (ref.watch(authProvider).stage != AuthStage.authenticated) {
      return const PanelRequiredScreen(title: 'Уведомления');
    }
    final async = ref.watch(notificationsProvider);
    final hasUnread = (async.valueOrNull?.items ?? const <AppNotification>[])
        .any((n) => !n.read);

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: RefreshIndicator(
          color: c.accent,
          backgroundColor: c.surface2,
          onRefresh: () async {
            ref.invalidate(notificationsProvider);
            await ref
                .read(notificationsProvider.future)
                .catchError(
                  (_) => const NotificationsPage(items: <AppNotification>[]),
                );
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
                'Уведомления',
                trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
              ),
              if (hasUnread) ...[
                Align(
                  alignment: Alignment.centerLeft,
                  child: TextButton.icon(
                    onPressed: () =>
                        ref.read(notificationsProvider.notifier).markAllRead(),
                    icon: LucideIcon(
                      Lucide.checkCheck,
                      color: c.textMed,
                      size: 16,
                    ),
                    label: Text(
                      'Прочитать все',
                      style: AppType.bodySm.copyWith(color: c.textMed),
                    ),
                    style: TextButton.styleFrom(
                      padding: const EdgeInsets.symmetric(
                        horizontal: AppSpace.s2,
                        vertical: AppSpace.s1,
                      ),
                      minimumSize: Size.zero,
                      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                    ),
                  ),
                ),
                const SizedBox(height: AppSpace.s2),
              ],
              async.when(
                data: (page) => page.items.isEmpty
                    ? const _EmptyInbox()
                    : Column(
                        children: [
                          for (final n in page.items)
                            _NotifCard(
                              notif: n,
                              onTap: n.read
                                  ? null
                                  : () => ref
                                        .read(notificationsProvider.notifier)
                                        .markRead(n.id),
                            ),
                        ],
                      ),
                loading: () => const InlineLoading(),
                error: (_, __) => InlineError(
                  message: 'Не удалось загрузить уведомления',
                  onRetry: () => ref.invalidate(notificationsProvider),
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
      context.go(AppRoute.home);
    }
  }
}

class _NotifCard extends StatelessWidget {
  final AppNotification notif;
  final VoidCallback? onTap;
  const _NotifCard({required this.notif, this.onTap});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    // Левый акцент: нейтральный. Непрочитанное = более заметная грань.
    final accent = notif.read ? c.borderSubtle : c.borderStrong;
    return Container(
      margin: const EdgeInsets.only(bottom: AppSpace.s2),
      decoration: BoxDecoration(
        color: notif.read ? c.surface1 : c.surface2,
        borderRadius: AppRadius.r14,
        border: Border.all(color: c.borderSubtle),
      ),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onTap,
        child: IntrinsicHeight(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Container(width: 3, color: accent),
              Expanded(
                child: Padding(
                  padding: const EdgeInsets.all(AppSpace.s4),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              notif.title,
                              style: AppType.bodyMd.copyWith(
                                color: c.textHi,
                                fontWeight: notif.read
                                    ? FontWeight.w500
                                    : FontWeight.w600,
                              ),
                            ),
                          ),
                          if (!notif.read) ...[
                            const SizedBox(width: AppSpace.s2),
                            Container(
                              width: 8,
                              height: 8,
                              margin: const EdgeInsets.only(top: 4),
                              decoration: BoxDecoration(
                                color: c.textHi,
                                shape: BoxShape.circle,
                              ),
                            ),
                          ],
                        ],
                      ),
                      if (notif.body.isNotEmpty) ...[
                        const SizedBox(height: 4),
                        Text(
                          notif.body,
                          style: AppType.bodySm.copyWith(color: c.textMed),
                        ),
                      ],
                      const SizedBox(height: AppSpace.s2),
                      Text(
                        notif.whenLabel,
                        style: AppType.monoSm.copyWith(color: c.textLow),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _EmptyInbox extends StatelessWidget {
  const _EmptyInbox();

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.only(top: AppSpace.s16),
      child: Column(
        children: [
          LucideIcon(Lucide.inbox, color: c.textLow, size: 32),
          const SizedBox(height: AppSpace.s3),
          Text(
            'Уведомлений пока нет',
            style: AppType.bodyMd.copyWith(color: c.textMed),
          ),
        ],
      ),
    );
  }
}
