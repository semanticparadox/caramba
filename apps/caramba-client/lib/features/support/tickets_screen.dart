import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/ticket.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/tickets_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Поддержка: список тикетов со статус-пилюлей и превью последнего сообщения.
/// Тап -> [TicketDetailScreen]. Кнопка «Новый запрос» -> [NewTicketScreen].
class TicketsScreen extends ConsumerWidget {
  const TicketsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.c;
    final async = ref.watch(ticketsProvider);

    return Scaffold(
      backgroundColor: c.bgCanvas,
      body: SafeArea(
        bottom: false,
        child: RefreshIndicator(
          color: c.accent,
          backgroundColor: c.surface2,
          onRefresh: () async {
            ref.invalidate(ticketsProvider);
            await ref
                .read(ticketsProvider.future)
                .catchError((_) => <TicketSummary>[]);
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
                'Поддержка',
                trailing: IconBtn(Lucide.x, onTap: () => _close(context)),
              ),
              Text(
                'Опишите проблему. Ответ придёт сюда и в Telegram.',
                style: AppType.bodyMd.copyWith(color: c.textMed),
              ),
              const SizedBox(height: AppSpace.s4),
              FilledButton.icon(
                onPressed: () => context.go(AppRoute.newTicket),
                icon: LucideIcon(Lucide.plus, color: c.textOnAccent, size: 18),
                label: const Text('Новый запрос'),
              ),
              const SizedBox(height: AppSpace.s5),
              async.when(
                data: (tickets) => tickets.isEmpty
                    ? const _EmptyTickets()
                    : Column(
                        children: [
                          for (final t in tickets) _TicketCard(ticket: t),
                        ],
                      ),
                loading: () => const InlineLoading(),
                error: (_, __) => InlineError(
                  message: 'Не удалось загрузить запросы',
                  onRetry: () => ref.invalidate(ticketsProvider),
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

class _TicketCard extends StatelessWidget {
  final TicketSummary ticket;
  const _TicketCard({required this.ticket});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      margin: const EdgeInsets.only(bottom: AppSpace.s2),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r14,
        border: Border.all(color: c.borderSubtle),
      ),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: () => context.go('${AppRoute.tickets}/${ticket.id}'),
        child: Padding(
          padding: const EdgeInsets.all(AppSpace.s4),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      ticket.subject,
                      overflow: TextOverflow.ellipsis,
                      style: AppType.bodyMd.copyWith(color: c.textHi),
                    ),
                  ),
                  const SizedBox(width: AppSpace.s2),
                  StatusPill(status: ticket.status),
                ],
              ),
              if (ticket.lastMessagePreview != null &&
                  ticket.lastMessagePreview!.isNotEmpty) ...[
                const SizedBox(height: 4),
                Text(
                  ticket.lastMessagePreview!,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: AppType.bodySm.copyWith(color: c.textMed),
                ),
              ],
              const SizedBox(height: AppSpace.s2),
              Row(
                children: [
                  Text('#${ticket.id}',
                      style: AppType.monoSm.copyWith(color: c.textLow)),
                  const SizedBox(width: AppSpace.s2),
                  Text(ticket.whenLabel,
                      style: AppType.monoSm.copyWith(color: c.textLow)),
                  const Spacer(),
                  if (ticket.hasUnread)
                    Container(
                      padding:
                          const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
                      decoration: BoxDecoration(
                        color: c.danger,
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: Text(
                        'новое',
                        style: AppType.monoSm
                            .copyWith(color: Colors.white, fontSize: 10),
                      ),
                    ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Статус-пилюля тикета. Цвет несёт только статус соединения (зелёный/жёлтый/
/// красный), поэтому статусы тикета рисуем монохромно: нейтральный контур для
/// любого состояния, а решён/закрыт отличаем самим текстом метки.
class StatusPill extends StatelessWidget {
  final TicketStatus status;
  const StatusPill({required this.status, super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: c.borderStrong),
      ),
      child: Text(
        status.label,
        style: AppType.bodySm.copyWith(color: c.textMed),
      ),
    );
  }
}

class _EmptyTickets extends StatelessWidget {
  const _EmptyTickets();

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Padding(
      padding: const EdgeInsets.only(top: AppSpace.s12),
      child: Column(
        children: [
          LucideIcon(Lucide.lifeBuoy, color: c.textLow, size: 32),
          const SizedBox(height: AppSpace.s3),
          Text('Запросов пока нет',
              style: AppType.bodyMd.copyWith(color: c.textMed)),
        ],
      ),
    );
  }
}
