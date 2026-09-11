import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/state/tickets_state.dart';
import 'package:caramba_client/theme/tokens.dart';
import 'package:caramba_client/theme/typography.dart';

/// Бейдж непрочитанных ответов поддержки для пункта «Запросы в поддержку» в
/// профиле (мобильном и десктопном). Показывает число тикетов, в которых
/// есть ответ, которого человек ещё не открывал; при нуле не рисует ничего,
/// чтобы строка выглядела как обычная.
///
/// Цвет один (danger), как у колокольчика уведомлений: он несёт единственный
/// смысл «есть непрочитанное», а не статус соединения.
class TicketsUnreadBadge extends ConsumerWidget {
  const TicketsUnreadBadge({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final count = ref.watch(unreadTicketsCountProvider);
    if (count <= 0) return const SizedBox.shrink();
    final c = context.c;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6),
      constraints: const BoxConstraints(minWidth: 20),
      height: 20,
      decoration: BoxDecoration(
        color: c.danger,
        borderRadius: BorderRadius.circular(10),
      ),
      alignment: Alignment.center,
      child: Text(
        count > 99 ? '99+' : '$count',
        style: AppType.monoSm.copyWith(
          color: Colors.white,
          fontSize: 10,
          height: 1,
        ),
      ),
    );
  }
}
