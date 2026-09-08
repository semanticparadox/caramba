import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/data/models/subscription.dart';
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/branding_state.dart';
import 'package:caramba_client/state/core_error.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/theme/spacing.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';
import 'package:caramba_client/widgets/lucide.dart';
import 'package:caramba_client/widgets/ui.dart';

/// Карточка «почему нельзя подключиться» и кнопка, ведущая к оплате.
///
/// Живёт рядом с экраном серверов намеренно: это объяснение к ФЛОТУ — почему
/// список виден, а строки не нажимаются, — и его же показывают Home, замер и
/// импорт. Второй копии этого текста в приложении быть не должно: расхождение
/// формулировок здесь читается как две разные поломки.
///
/// Внутренних слов тут нет ни одного. Ни `throttled`, ни `expired`, ни «403»:
/// человек, у которого кончился дневной трафик, не обязан знать ни одного из
/// них, чтобы понять, что произошло и что делать.
class AccessCard extends ConsumerWidget {
  /// Состояние доступа. `null` — берётся из подписки активной панели.
  final AccessState? access;

  /// Показывать ли объяснение целиком. Компактный вид — одна строка причины,
  /// для мест, где карточка стоит рядом с другим содержимым.
  final bool compact;

  const AccessCard({this.access, this.compact = false, super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = access ?? ref.watch(subscriptionAccessProvider);
    if (state == null || !state.isBlocked) return const SizedBox.shrink();
    final c = context.c;
    final pay = accessPayLink(ref);

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(AppSpace.s4),
      decoration: BoxDecoration(
        color: c.warningSubtle,
        borderRadius: AppRadius.r12,
        border: Border.all(color: c.warning.withValues(alpha: 0.35)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              LucideIcon(_glyphOf(state), color: c.warning, size: 18),
              const SizedBox(width: AppSpace.s3),
              Expanded(
                child: Text(
                  state.title,
                  style: AppType.bodyMd.copyWith(color: c.textHi),
                ),
              ),
            ],
          ),
          if (!compact) ...[
            const SizedBox(height: AppSpace.s2),
            Text(state.body, style: AppType.bodySm.copyWith(color: c.textMed)),
            if (state.upgrade != null &&
                state.upgrade!.planName.isNotEmpty) ...[
              const SizedBox(height: AppSpace.s2),
              Text(
                'Снимает ограничение: тариф ${state.upgrade!.planName}'
                '${state.upgrade!.durationDays == null ? '' : ' на ${state.upgrade!.durationDays} дн.'}',
                style: AppType.bodySm.copyWith(color: c.textMed),
              ),
            ],
            if (state.limitBytes > 0 && state.period == 'day') ...[
              const SizedBox(height: AppSpace.s3),
              Text(
                '${formatBytesRu(state.usedBytes)} из '
                '${formatBytesRu(state.limitBytes)} за сегодня',
                style: AppType.monoSm.copyWith(color: c.textLow),
              ),
            ],
          ],
          const SizedBox(height: AppSpace.s4),
          // Куда ведёт «Оплатить», решает наличие ПАНЕЛИ, а не наличие ссылки.
          // Панель есть — ведём на витрину тарифов: там видно, что именно
          // покупается, сколько это стоит и чем платить, и там же честно
          // говорится, если оплата в приложении у оператора выключена. Панели
          // нет (своя подписка, generic-режим) — витрины не существует, и
          // единственное осмысленное действие это внешняя ссылка, если
          // оператор её опубликовал.
          if (ref.watch(apiClientProvider).hasPanel)
            GhostButton(
              label: state.selfHealing ? 'Оплатить и не ждать' : 'Оплатить',
              icon: Lucide.creditCard,
              onPressed: () => context.push(AppRoute.plans),
            )
          else if (pay != null)
            GhostButton(
              label: state.selfHealing ? 'Оплатить и не ждать' : 'Оплатить',
              icon: Lucide.creditCard,
              onPressed: () => openExternal(context, pay),
            )
          else
            // Выдумать чужого бота вместо пустоты нельзя: приложение не
            // принадлежит ни одному оператору, и адрес оплаты публикует он.
            Text(
              'Оператор не опубликовал адрес для оплаты. Он есть там, где вы '
              'оформляли подписку.',
              style: AppType.bodySm.copyWith(color: c.textLow),
            ),
          if (state.kind == AccessKind.deviceLimit) ...[
            const SizedBox(height: AppSpace.s2),
            GhostButton(
              label: 'Мои устройства',
              icon: Lucide.phone,
              onPressed: () => context.go(AppRoute.profile),
            ),
          ],
        ],
      ),
    );
  }

  String _glyphOf(AccessState s) => switch (s.kind) {
    AccessKind.dailyQuota => Lucide.clock,
    AccessKind.planQuota || AccessKind.expired => Lucide.wallet,
    AccessKind.deviceLimit => Lucide.phone,
    _ => Lucide.alert,
  };
}

/// Куда вести за оплатой: сначала то, что назвала панель в состоянии доступа,
/// затем бот из брендинга, затем поддержка. `null` — оператор не опубликовал
/// ничего, и это надо сказать словами, а не подставлять чужой адрес.
String? accessPayLink(WidgetRef ref) {
  final fromAccess = ref.read(subscriptionAccessProvider)?.pay.link;
  if (fromAccess != null && fromAccess.isNotEmpty) return fromAccess;
  final branding = ref.read(activeBrandingProvider);
  final bot = branding.botUrl.trim();
  if (bot.isNotEmpty) return '$bot?start=plans';
  final support = branding.supportUrl.trim();
  return support.isEmpty ? null : support;
}

/// Отказ на экране: перевод, действие и УЛИКА под «Подробности».
///
/// Кнопка повтора появляется только там, где повтор что-то меняет. У
/// исчерпанного лимита её нет: она вернула бы тот же отказ и научила человека,
/// что кнопки в этом приложении не работают.
class FailureNotice extends ConsumerStatefulWidget {
  /// Человеческий текст (уже переведённый).
  final String message;

  /// Исходный текст ядра/панели. `null` — «Подробности» не показываются.
  final String? technical;

  final VoidCallback? onRetry;

  /// Предлагать ли оплату. Считается вызывающим по [CarambaFailure.payable].
  final bool payable;

  const FailureNotice({
    required this.message,
    this.technical,
    this.onRetry,
    this.payable = false,
    super.key,
  });

  /// Собирает уведомление из уже показанной строки: технический текст
  /// достаётся из таблицы переводов ([technicalDetailFor]).
  factory FailureNotice.fromText(
    String message, {
    VoidCallback? onRetry,
    bool payable = false,
  }) => FailureNotice(
    message: message,
    technical: technicalDetailFor(message),
    onRetry: onRetry,
    payable: payable,
  );

  @override
  ConsumerState<FailureNotice> createState() => _FailureNoticeState();
}

class _FailureNoticeState extends ConsumerState<FailureNotice> {
  bool _open = false;

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final pay = widget.payable ? accessPayLink(ref) : null;
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(AppSpace.s4),
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: AppRadius.r12,
        border: Border.all(color: c.borderSubtle),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              LucideIcon(Lucide.alert, color: c.textMed, size: 18),
              const SizedBox(width: AppSpace.s3),
              Expanded(
                child: Text(
                  widget.message,
                  style: AppType.bodySm.copyWith(color: c.textHi),
                ),
              ),
            ],
          ),
          if (pay != null) ...[
            const SizedBox(height: AppSpace.s3),
            GhostButton(
              label: 'Оплатить',
              icon: Lucide.creditCard,
              onPressed: () => openExternal(context, pay),
            ),
          ],
          if (widget.onRetry != null) ...[
            const SizedBox(height: AppSpace.s2),
            GhostButton(
              label: 'Повторить',
              icon: Lucide.refresh,
              onPressed: widget.onRetry,
            ),
          ],
          if (widget.technical != null &&
              widget.technical!.trim().isNotEmpty) ...[
            const SizedBox(height: AppSpace.s1),
            Align(
              alignment: Alignment.centerLeft,
              child: QuietButton(
                label: _open ? 'Скрыть подробности' : 'Подробности',
                color: c.textMed,
                onPressed: () => setState(() => _open = !_open),
              ),
            ),
            if (_open)
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(AppSpace.s3),
                decoration: BoxDecoration(
                  color: c.surfaceInset,
                  borderRadius: AppRadius.r12,
                  border: Border.all(color: c.borderSubtle),
                ),
                child: SelectableText(
                  widget.technical!,
                  style: AppType.monoSm.copyWith(color: c.textMed),
                ),
              ),
          ],
        ],
      ),
    );
  }
}

/// Текст «почему нельзя» для строки списка выходов.
///
/// [ExitAvailability.message] для [ExitUnavailableReason.panelRejected] говорит
/// «Панель отклонила выбор» — это про НАЖАТИЕ на строку, а тут причина другая:
/// подписка закрыта целиком, и выбор ни при чём. Когда причина названа в
/// detail, показывается она.
String exitUnavailableText(ExitAvailability a) {
  if (a.reason == ExitUnavailableReason.panelRejected) {
    final d = a.detail?.trim();
    if (d != null && d.isNotEmpty) return '$d. Подключиться сейчас нельзя.';
  }
  return a.message;
}
