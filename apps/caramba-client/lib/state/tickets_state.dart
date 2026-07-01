import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/ticket.dart';
import 'package:caramba_client/state/providers.dart';

/// Список тикетов поддержки (`GET /app/tickets`).
/// `ref.invalidate(ticketsProvider)` перезапрашивает после создания/ответа.
final ticketsProvider =
    FutureProvider.autoDispose<List<TicketSummary>>((ref) async {
  final api = ref.watch(apiClientProvider);
  return api.getTickets();
});

/// Детали одного тикета с лентой сообщений (`GET /app/tickets/{id}`).
/// Family-ключ — id тикета.
final ticketDetailProvider =
    FutureProvider.autoDispose.family<TicketDetail, int>((ref, id) async {
  final api = ref.watch(apiClientProvider);
  return api.getTicket(id);
});
