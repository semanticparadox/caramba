/// Уведомление пользователя из `GET /api/v2/app/notifications`.
///
/// Контракт панели (`app_support.rs::AppNotification`):
/// ```json
/// { "id":42, "title":"...", "body":"...", "kind":"billing",
///   "created_at":"RFC3339", "read":false,
///   "ticket_id":7, "payload":{"ticket_id":7,"url":"/support/7"} }
/// ```
/// Поле статуса прочитанности приходит явным `read:bool`. Категория — в `kind`
/// (исторически могла называться `category`, поэтому читаем оба). `ticket_id`
/// и `payload` есть только у уведомлений, привязанных к сущности; старая
/// панель их не шлёт, тогда тап по уведомлению лишь помечает его прочитанным.
class AppNotification {
  final int id;
  final String category;
  final String title;
  final String body;
  final bool read;
  final DateTime? createdAt;
  final DateTime? readAt;

  /// Тикет, к которому ведёт уведомление (`support_ticket`). Null для всех
  /// остальных категорий и для ответов старой панели без payload.
  final int? ticketId;

  /// Сырой payload панели: остальные ссылки (`url`, `payment_id`) читаются
  /// отсюда, когда появятся переходы к другим сущностям.
  final Map<String, dynamic>? payload;

  const AppNotification({
    required this.id,
    this.category = '',
    required this.title,
    this.body = '',
    this.read = false,
    this.createdAt,
    this.readAt,
    this.ticketId,
    this.payload,
  });

  /// Категория уведомлений о тикетах поддержки (`tickets_service.rs`).
  static const String supportTicketKind = 'support_ticket';

  /// Тап ведёт к тикету: панель прислала положительный id. Категорию не
  /// требуем: id без категории надёжнее категории без id.
  bool get opensTicket => (ticketId ?? 0) > 0;

  /// Человекочитаемое «когда» (плоский текст, без em-dash).
  String get whenLabel {
    final t = createdAt;
    if (t == null) return '';
    final d = DateTime.now().difference(t);
    if (d.inMinutes < 1) return 'только что';
    if (d.inMinutes < 60) return '${d.inMinutes} мин назад';
    if (d.inHours < 24) return '${d.inHours} ч назад';
    if (d.inDays < 7) return '${d.inDays} дн назад';
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(t.day)}.${two(t.month)}.${t.year}';
  }

  factory AppNotification.fromJson(Map<String, dynamic> json) {
    final readAt = _parseDate(json['read_at']);
    final status = (json['status'] as String?)?.toLowerCase();
    // Панель отдаёт `payload`; мини-аппный контракт называет его `payload_json`.
    final rawPayload = json['payload'] ?? json['payload_json'];
    final payload = (rawPayload is Map)
        ? rawPayload.cast<String, dynamic>()
        : null;
    return AppNotification(
      id: (json['id'] as num?)?.toInt() ?? 0,
      category:
          (json['kind'] as String?) ?? (json['category'] as String?) ?? '',
      title: (json['title'] as String?) ?? '',
      body: (json['body'] as String?) ?? '',
      read: (json['read'] as bool?) ?? (status == 'read' || readAt != null),
      createdAt: _parseDate(json['created_at']),
      readAt: readAt,
      ticketId: _parseTicketId(json['ticket_id']) ?? _ticketIdFrom(payload),
      payload: payload,
    );
  }

  AppNotification copyWith({bool? read, DateTime? readAt}) => AppNotification(
    id: id,
    category: category,
    title: title,
    body: body,
    read: read ?? this.read,
    createdAt: createdAt,
    readAt: readAt ?? this.readAt,
    ticketId: ticketId,
    payload: payload,
  );

  /// id тикета из payload: числовое/строковое `ticket_id`, иначе из `url`
  /// вида `/support/{id}` (так панель писала до появления явного поля).
  static int? _ticketIdFrom(Map<String, dynamic>? payload) {
    if (payload == null) return null;
    final direct = _parseTicketId(payload['ticket_id']);
    if (direct != null) return direct;
    final url = payload['url'];
    if (url is String) {
      final m = RegExp(r'^/support/(\d+)').firstMatch(url);
      if (m != null) return int.tryParse(m.group(1)!);
    }
    return null;
  }

  static int? _parseTicketId(Object? v) {
    if (v is num) return v.toInt() > 0 ? v.toInt() : null;
    if (v is String) {
      final n = int.tryParse(v.trim());
      return (n != null && n > 0) ? n : null;
    }
    return null;
  }

  static DateTime? _parseDate(Object? v) {
    if (v is String && v.isNotEmpty) return DateTime.tryParse(v)?.toLocal();
    return null;
  }
}

/// Результат `GET /app/notifications`: лента + авторитетный счётчик непрочитанных
/// от панели (`unread_count`). Клиент предпочитает серверный счётчик локальному.
class NotificationsPage {
  final List<AppNotification> items;
  final int? unreadCount;

  const NotificationsPage({required this.items, this.unreadCount});
}
