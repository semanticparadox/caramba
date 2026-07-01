import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/notification.dart';
import 'package:caramba_client/state/providers.dart';

/// Уведомления пользователя (`GET /app/notifications`) + операции пометки
/// прочитанным. Держим [NotificationsPage] (лента + серверный `unread_count`),
/// чтобы бейдж брал авторитетный счётчик панели, а не локальную оценку.
/// Экраны отрисовывают loading/empty/error через `AsyncValue.when`.
class NotificationsNotifier
    extends AutoDisposeAsyncNotifier<NotificationsPage> {
  @override
  Future<NotificationsPage> build() async {
    final api = ref.watch(apiClientProvider);
    return api.getNotifications();
  }

  /// Пометить одно прочитанным (оптимистично, с откатом при ошибке).
  Future<void> markRead(int id) async {
    final api = ref.read(apiClientProvider);
    final current = state.valueOrNull;
    if (current != null) {
      final items = [
        for (final n in current.items)
          n.id == id && !n.read
              ? n.copyWith(read: true, readAt: DateTime.now())
              : n,
      ];
      state = AsyncData(NotificationsPage(
        items: items,
        unreadCount: _decrement(current.unreadCount),
      ));
    }
    try {
      await api.markNotificationRead(id);
    } catch (_) {
      ref.invalidateSelf();
      await future;
    }
  }

  /// Пометить все прочитанными (оптимистично).
  Future<void> markAllRead() async {
    final api = ref.read(apiClientProvider);
    final current = state.valueOrNull;
    if (current != null) {
      final now = DateTime.now();
      final items = [
        for (final n in current.items)
          n.read ? n : n.copyWith(read: true, readAt: now),
      ];
      state = AsyncData(NotificationsPage(items: items, unreadCount: 0));
    }
    try {
      await api.markAllNotificationsRead();
    } catch (_) {
      ref.invalidateSelf();
      await future;
    }
  }

  int? _decrement(int? count) {
    if (count == null) return null;
    return count > 0 ? count - 1 : 0;
  }
}

final notificationsProvider = AutoDisposeAsyncNotifierProvider<
    NotificationsNotifier, NotificationsPage>(NotificationsNotifier.new);

/// Кол-во непрочитанных для бейджа в шапке. Берём авторитетный серверный
/// `unread_count`; если панель его не прислала — считаем по ленте локально.
/// 0, пока данные не загружены или при ошибке (бейдж скрыт).
final unreadCountProvider = Provider.autoDispose<int>((ref) {
  final page = ref.watch(notificationsProvider).valueOrNull;
  if (page == null) return 0;
  final server = page.unreadCount;
  if (server != null) return server;
  return page.items.where((n) => !n.read).length;
});
