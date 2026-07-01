/// Уведомление пользователя из `GET /api/v2/app/notifications`.
///
/// Контракт панели (`app_support.rs::AppNotification`):
/// ```json
/// { "id":42, "title":"...", "body":"...", "kind":"billing",
///   "created_at":"RFC3339", "read":false }
/// ```
/// Поле статуса прочитанности приходит явным `read:bool`. Категория — в `kind`
/// (исторически могла называться `category`, поэтому читаем оба).
class AppNotification {
  final int id;
  final String category;
  final String title;
  final String body;
  final bool read;
  final DateTime? createdAt;
  final DateTime? readAt;

  const AppNotification({
    required this.id,
    this.category = '',
    required this.title,
    this.body = '',
    this.read = false,
    this.createdAt,
    this.readAt,
  });

  /// Человекочитаемое «когда» (плоский текст, без em-dash).
  String get whenLabel {
    final t = createdAt;
    if (t == null) return '';
    final d = DateTime.now().difference(t);
    if (d.inMinutes < 1) return 'только что';
    if (d.inMinutes < 60) return '${d.inMinutes} мин назад';
    if (d.inHours < 24) return '${d.inHours} ч назад';
    if (d.inDays < 7) return '${d.inDays} дн назад';
    final two = (int n) => n.toString().padLeft(2, '0');
    return '${two(t.day)}.${two(t.month)}.${t.year}';
  }

  factory AppNotification.fromJson(Map<String, dynamic> json) {
    final readAt = _parseDate(json['read_at']);
    final status = (json['status'] as String?)?.toLowerCase();
    return AppNotification(
      id: (json['id'] as num?)?.toInt() ?? 0,
      category: (json['kind'] as String?) ?? (json['category'] as String?) ?? '',
      title: (json['title'] as String?) ?? '',
      body: (json['body'] as String?) ?? '',
      read: (json['read'] as bool?) ?? (status == 'read' || readAt != null),
      createdAt: _parseDate(json['created_at']),
      readAt: readAt,
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
      );

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
