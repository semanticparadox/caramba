import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/api_client.dart';
import 'package:caramba_client/data/models/ticket.dart';
import 'package:caramba_client/features/profile/panel_required.dart';
import 'package:caramba_client/features/support/tickets_screen.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/auth_state.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/tickets_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Детали тикета: лента сообщений в чат-стиле (пользователь справа neutral-strong,
/// поддержка слева на surface) + композер ответа. mono-таймстемпы.
///
/// Переписка живая: пока экран открыт и тикет не завершён, лента опрашивается
/// раз в [ticketPollIntervalProvider] и обновляется свайпом вниз. Новый ответ
/// поддержки, пришедший между опросами, отмечается плашкой «Новый ответ
/// поддержки» и прокруткой к концу ленты. Push-уведомлений в приложении нет,
/// и без этого ответ был виден только после переоткрытия экрана.
class TicketDetailScreen extends ConsumerStatefulWidget {
  final int ticketId;
  const TicketDetailScreen({required this.ticketId, super.key});

  @override
  ConsumerState<TicketDetailScreen> createState() => _TicketDetailScreenState();
}

class _TicketDetailScreenState extends ConsumerState<TicketDetailScreen> {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  bool _sending = false;
  Timer? _poll;

  /// Сколько сообщений поддержки было в последней увиденной ленте; null,
  /// пока первая загрузка не пришла (первую ленту за «новый ответ» не считаем).
  int? _seenSupportCount;

  /// Плашка «Новый ответ поддержки» показана и ещё не погашена таймером.
  bool _newReplyFlash = false;
  Timer? _flashTimer;

  @override
  void initState() {
    super.initState();
    _startPolling();
  }

  @override
  void dispose() {
    _poll?.cancel();
    _flashTimer?.cancel();
    _input.dispose();
    _scroll.dispose();
    super.dispose();
  }

  void _startPolling() {
    _poll?.cancel();
    final period = ref.read(ticketPollIntervalProvider);
    _poll = Timer.periodic(period, (_) => _tick());
  }

  /// Один шаг опроса. Завершённый тикет не опрашиваем: писать в него нельзя,
  /// и новых ответов не будет. Пока идёт отправка своего сообщения, тоже
  /// пропускаем — `_send` сам перезапросит ленту.
  void _tick() {
    if (!mounted || _sending) return;
    final status = ref
        .read(ticketDetailProvider(widget.ticketId))
        .valueOrNull
        ?.status;
    if (status != null && !status.isOpen) return;
    ref.invalidate(ticketDetailProvider(widget.ticketId));
  }

  Future<void> _refresh() async {
    ref.invalidate(ticketDetailProvider(widget.ticketId));
    try {
      await ref.read(ticketDetailProvider(widget.ticketId).future);
    } catch (_) {
      // Ошибку покажет сам экран, индикатору достаточно завершить жест.
    }
  }

  /// Сверяет свежую ленту с последней увиденной: если поддержка ответила,
  /// зажигает плашку и прокручивает к концу. Вызывается из `ref.listen`.
  void _onDetail(TicketDetail detail) {
    final count = detail.supportMessageCount;
    final seen = _seenSupportCount;
    _seenSupportCount = count;
    if (seen == null || count <= seen) return;
    _flashTimer?.cancel();
    setState(() => _newReplyFlash = true);
    _flashTimer = Timer(const Duration(seconds: 4), () {
      if (mounted) setState(() => _newReplyFlash = false);
    });
    _scrollToEnd();
  }

  void _scrollToEnd() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_scroll.hasClients) return;
      _scroll.jumpTo(_scroll.position.maxScrollExtent);
    });
  }

  Future<void> _send() async {
    final text = _input.text.trim();
    if (text.isEmpty || _sending) return;
    setState(() => _sending = true);
    try {
      await ref.read(apiClientProvider).replyTicket(widget.ticketId, text);
      _input.clear();
      ref.invalidate(ticketDetailProvider(widget.ticketId));
      ref.invalidate(ticketsProvider);
      _scrollToEnd();
    } on ApiException catch (e) {
      if (mounted) showCarambaToast(context, e.message);
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    // Раздел живёт у аккаунта панели: в generic-режиме показываем, что нужно
    // сделать, чтобы он заработал, вместо 401 за каждым провайдером.
    if (ref.watch(authProvider).stage != AuthStage.authenticated) {
      return const PanelRequiredScreen(title: 'Запрос в поддержку');
    }
    ref.listen<AsyncValue<TicketDetail>>(
      ticketDetailProvider(widget.ticketId),
      (_, next) {
        final d = next.valueOrNull;
        if (d != null && !next.isLoading) _onDetail(d);
      },
    );
    final async = ref.watch(ticketDetailProvider(widget.ticketId));
    final detail = async.valueOrNull;
    final canReply = detail?.status.isOpen ?? false;

    return PopScope(
      // Уход с экрана: список тикетов обязан узнать, что этот прочитан
      // (панель сняла `has_unread` при загрузке переписки), иначе бейдж
      // «новое» на карточке переживёт само прочтение.
      onPopInvokedWithResult: (didPop, _) {
        if (didPop) ref.invalidate(ticketsProvider);
      },
      child: Scaffold(
        backgroundColor: c.bgCanvas,
        body: SafeArea(
          child: Column(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(
                  AppSpace.s5,
                  AppSpace.s4,
                  AppSpace.s5,
                  AppSpace.s2,
                ),
                child: Row(
                  children: [
                    IconBtn(
                      Lucide.arrowLeft,
                      size: 40,
                      onTap: () => _back(context),
                    ),
                    const SizedBox(width: AppSpace.s3),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            detail?.subject ?? 'Запрос #${widget.ticketId}',
                            overflow: TextOverflow.ellipsis,
                            style: AppType.titleMd.copyWith(color: c.textHi),
                          ),
                          Text(
                            '#${widget.ticketId}',
                            style: AppType.monoSm.copyWith(color: c.textLow),
                          ),
                        ],
                      ),
                    ),
                    if (detail != null) ...[
                      const SizedBox(width: AppSpace.s2),
                      StatusPill(status: detail.status),
                    ],
                  ],
                ),
              ),
              Divider(height: 1, thickness: 1, color: c.borderSubtle),
              if (_newReplyFlash) const _NewReplyFlash(),
              Expanded(
                child: async.when(
                  // Опрос не должен ронять уже показанную переписку: пока есть
                  // данные, сбой очередного запроса просто пропускается.
                  skipError: true,
                  data: (d) => RefreshIndicator(
                    color: c.accent,
                    backgroundColor: c.surface2,
                    onRefresh: _refresh,
                    child: d.messages.isEmpty
                        ? ListView(
                            physics: const AlwaysScrollableScrollPhysics(),
                            children: const [
                              InlineEmpty(message: 'Сообщений пока нет'),
                            ],
                          )
                        : ListView.builder(
                            controller: _scroll,
                            physics: const AlwaysScrollableScrollPhysics(),
                            padding: const EdgeInsets.fromLTRB(
                              AppSpace.s5,
                              AppSpace.s4,
                              AppSpace.s5,
                              AppSpace.s4,
                            ),
                            itemCount: d.messages.length,
                            itemBuilder: (_, i) =>
                                _Bubble(message: d.messages[i]),
                          ),
                  ),
                  loading: () => const InlineLoading(),
                  error: (_, __) => InlineError(
                    message: 'Не удалось загрузить переписку',
                    onRetry: () =>
                        ref.invalidate(ticketDetailProvider(widget.ticketId)),
                  ),
                ),
              ),
              if (canReply)
                _Composer(controller: _input, sending: _sending, onSend: _send)
              else if (detail != null)
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.symmetric(
                    horizontal: AppSpace.s5,
                    vertical: AppSpace.s4,
                  ),
                  decoration: BoxDecoration(
                    color: c.surface1,
                    border: Border(top: BorderSide(color: c.borderSubtle)),
                  ),
                  child: Text(
                    'Запрос ${detail.status.label.toLowerCase()}. Создайте новый, если нужна помощь.',
                    textAlign: TextAlign.center,
                    style: AppType.bodySm.copyWith(color: c.textMed),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  void _back(BuildContext context) {
    // Список тикетов перечитывается и здесь: `PopScope` ловит только
    // системный «назад», а стрелка в шапке может уйти через `go`.
    ref.invalidate(ticketsProvider);
    if (context.canPop()) {
      context.pop();
    } else {
      context.go(AppRoute.tickets);
    }
  }
}

/// Плашка «Новый ответ поддержки»: нейтральная поверхность, без цвета
/// статуса. Живёт четыре секунды после того, как опрос принёс новый ответ.
class _NewReplyFlash extends StatelessWidget {
  const _NewReplyFlash();

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(
        horizontal: AppSpace.s5,
        vertical: AppSpace.s2,
      ),
      color: c.surface2,
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          LucideIcon(Lucide.messageSquare, color: c.textMed, size: 14),
          const SizedBox(width: AppSpace.s2),
          Text(
            'Новый ответ поддержки',
            style: AppType.bodySm.copyWith(color: c.textMed),
          ),
        ],
      ),
    );
  }
}

class _Bubble extends StatelessWidget {
  final TicketMessage message;
  const _Bubble({required this.message});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final mine = message.fromUser;
    final bg = mine ? c.surface3 : c.surface1;
    final align = mine ? CrossAxisAlignment.end : CrossAxisAlignment.start;
    return Padding(
      padding: const EdgeInsets.only(bottom: AppSpace.s3),
      child: Column(
        crossAxisAlignment: align,
        children: [
          ConstrainedBox(
            constraints: BoxConstraints(
              maxWidth: MediaQuery.sizeOf(context).width * 0.78,
            ),
            child: Container(
              padding: const EdgeInsets.symmetric(
                horizontal: AppSpace.s4,
                vertical: AppSpace.s3,
              ),
              decoration: BoxDecoration(
                color: bg,
                borderRadius: BorderRadius.only(
                  topLeft: const Radius.circular(AppRadius.button),
                  topRight: const Radius.circular(AppRadius.button),
                  bottomLeft: Radius.circular(mine ? AppRadius.button : 4),
                  bottomRight: Radius.circular(mine ? 4 : AppRadius.button),
                ),
                border: Border.all(color: c.borderSubtle),
              ),
              child: Text(
                message.body,
                style: AppType.bodyMd.copyWith(color: c.textHi),
              ),
            ),
          ),
          const SizedBox(height: 3),
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (!mine) ...[
                Text(
                  'Поддержка',
                  style: AppType.monoSm.copyWith(color: c.textLow),
                ),
                const SizedBox(width: AppSpace.s2),
              ],
              Text(
                message.timeLabel,
                style: AppType.monoSm.copyWith(color: c.textLow),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _Composer extends StatelessWidget {
  final TextEditingController controller;
  final bool sending;
  final VoidCallback onSend;
  const _Composer({
    required this.controller,
    required this.sending,
    required this.onSend,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    return Container(
      padding: EdgeInsets.fromLTRB(
        AppSpace.s4,
        AppSpace.s3,
        AppSpace.s4,
        AppSpace.s3 + MediaQuery.viewInsetsOf(context).bottom,
      ),
      decoration: BoxDecoration(
        color: c.surface1,
        border: Border(top: BorderSide(color: c.borderSubtle)),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          Expanded(
            child: TextField(
              controller: controller,
              minLines: 1,
              maxLines: 5,
              textInputAction: TextInputAction.newline,
              style: AppType.bodyMd.copyWith(color: c.textHi),
              decoration: InputDecoration(
                hintText: 'Сообщение',
                hintStyle: AppType.bodyMd.copyWith(color: c.textLow),
                filled: true,
                fillColor: c.surfaceInset,
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: AppSpace.s4,
                  vertical: AppSpace.s3,
                ),
                border: OutlineInputBorder(
                  borderRadius: AppRadius.r12,
                  borderSide: BorderSide(color: c.borderSubtle),
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
          ),
          const SizedBox(width: AppSpace.s2),
          SizedBox(
            width: 48,
            height: 48,
            child: Material(
              color: c.accent,
              borderRadius: AppRadius.r12,
              child: InkWell(
                borderRadius: AppRadius.r12,
                onTap: sending ? null : onSend,
                child: Center(
                  child: sending
                      ? SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            color: c.textOnAccent,
                          ),
                        )
                      : LucideIcon(
                          Lucide.send,
                          color: c.textOnAccent,
                          size: 18,
                        ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
