import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/ticket.dart';
import 'package:caramba_client/state/providers.dart';

/// Список тикетов поддержки (`GET /app/tickets`).
/// `ref.invalidate(ticketsProvider)` перезапрашивает после создания/ответа,
/// при возврате с экрана тикета и по pull-to-refresh.
final ticketsProvider = FutureProvider.autoDispose<List<TicketSummary>>((
  ref,
) async {
  final api = ref.watch(apiClientProvider);
  return api.getTickets();
});

/// Детали одного тикета с лентой сообщений (`GET /app/tickets/{id}`).
/// Family-ключ — id тикета. Сам запрос на панели отмечает переписку
/// прочитанной, поэтому после него список отдаёт `has_unread = false`.
final ticketDetailProvider = FutureProvider.autoDispose
    .family<TicketDetail, int>((ref, id) async {
      final api = ref.watch(apiClientProvider);
      return api.getTicket(id);
    });

/// Период опроса переписки на открытом экране тикета. Push-уведомлений в
/// приложении нет, а ответ поддержки должен появляться без переоткрытия
/// экрана. 15 секунд: заметно быстрее, чем человек успеет заскучать, и
/// дешевле, чем держать соединение. Провайдер, а не константа: тест
/// подставляет короткий период, не дожидаясь настоящих секунд.
final ticketPollIntervalProvider = Provider<Duration>(
  (_) => const Duration(seconds: 15),
);

/// Число тикетов с непрочитанным ответом поддержки — бейдж на пункте
/// «Запросы в поддержку» в профиле. Считается по списку, который панель и так
/// отдаёт с флагом `has_unread`; 0, пока список не загружен или недоступен
/// (бейдж скрыт, а не врёт).
final unreadTicketsCountProvider = Provider.autoDispose<int>((ref) {
  final tickets = ref.watch(ticketsProvider).valueOrNull;
  if (tickets == null) return 0;
  return tickets.where((t) => t.hasUnread).length;
});
