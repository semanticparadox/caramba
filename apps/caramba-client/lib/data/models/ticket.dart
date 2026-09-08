/// Тикеты поддержки. Зеркалят `Ticket` / `TicketSummary` / `TicketMessage`
/// из `libs/caramba-db/src/models/tickets.rs`.
///
/// Статусы панели: open | in_progress | awaiting_user | resolved | closed
/// (см. tickets_service.rs::set_status). Роли отправителя: user | admin
/// (admin = поддержка), плюс system/bot для авто-сообщений.
library;

enum TicketStatus { open, inProgress, awaitingUser, resolved, closed }

extension TicketStatusX on TicketStatus {
  /// Короткая русская метка для пилюли статуса.
  String get label => switch (this) {
    TicketStatus.open => 'Открыт',
    TicketStatus.inProgress => 'В работе',
    TicketStatus.awaitingUser => 'Ждёт ответа',
    TicketStatus.resolved => 'Решён',
    TicketStatus.closed => 'Закрыт',
  };

  /// Тикет в работе для пользователя (можно отвечать).
  bool get isOpen =>
      this != TicketStatus.closed && this != TicketStatus.resolved;

  /// Завершённый тикет (статус-пилюля окрашивается в success).
  bool get isDone =>
      this == TicketStatus.resolved || this == TicketStatus.closed;

  static TicketStatus parse(String? raw) => switch (raw?.toLowerCase()) {
    'in_progress' || 'inprogress' => TicketStatus.inProgress,
    'awaiting_user' || 'awaitinguser' => TicketStatus.awaitingUser,
    'resolved' => TicketStatus.resolved,
    'closed' => TicketStatus.closed,
    _ => TicketStatus.open,
  };
}

/// Сводка тикета для списка. Контракт панели (`app_support.rs::AppTicketSummary`):
/// ```json
/// { "id":3, "subject":"...", "status":"open", "updated_at":"RFC3339",
///   "unread": true }
/// ```
/// `unread` — булев флаг «есть непрочитанное» (не счётчик). `category` и
/// `last_message_preview` панель не присылает; читаем их опционально, чтобы
/// пережить расширение DTO, и не рисуем, когда их нет.
class TicketSummary {
  final int id;
  final String category;
  final String subject;
  final TicketStatus status;
  final DateTime? updatedAt;
  final String? lastMessagePreview;
  final int unread;

  const TicketSummary({
    required this.id,
    this.category = '',
    required this.subject,
    this.status = TicketStatus.open,
    this.updatedAt,
    this.lastMessagePreview,
    this.unread = 0,
  });

  String get whenLabel => _relative(updatedAt);

  /// Есть непрочитанные сообщения для пользователя. Панель отдаёт булев флаг,
  /// поэтому это надёжнее точного счётчика.
  bool get hasUnread => unread > 0;

  factory TicketSummary.fromJson(Map<String, dynamic> json) {
    // Панель отдаёт unread как bool. Поддерживаем и числовой unread_for_user.
    final u = json['unread'];
    final unread = (u is bool)
        ? (u ? 1 : 0)
        : (u is num)
        ? u.toInt()
        : (json['unread_for_user'] as num?)?.toInt() ?? 0;
    return TicketSummary(
      id: (json['id'] as num?)?.toInt() ?? 0,
      category: (json['category'] as String?) ?? '',
      subject: (json['subject'] as String?) ?? 'Без темы',
      status: TicketStatusX.parse(json['status'] as String?),
      updatedAt:
          _parseDate(json['updated_at']) ?? _parseDate(json['created_at']),
      lastMessagePreview: json['last_message_preview'] as String?,
      unread: unread,
    );
  }
}

/// Одно сообщение тикета. Контракт панели (`app_support.rs::AppTicketMessage`):
/// ```json
/// { "author":"user"|"support", "body":"...", "created_at":"RFC3339" }
/// ```
/// `id`/`ticket_id` панель не присылает (UI их не использует) — оставляем 0.
class TicketMessage {
  final int id;
  final int ticketId;

  /// Автор: "user" — сам пользователь, иначе поддержка/система.
  final String author;
  final String body;
  final DateTime? createdAt;

  const TicketMessage({
    required this.id,
    this.ticketId = 0,
    this.author = 'user',
    required this.body,
    this.createdAt,
  });

  /// Сообщение пользователя (справа, neutral-strong). Иначе — поддержка/система.
  bool get fromUser => author.toLowerCase() == 'user';

  /// mono-метка времени (HH:MM, или DD.MM HH:MM для прошлых дней).
  String get timeLabel {
    final t = createdAt;
    if (t == null) return '';
    String two(int n) => n.toString().padLeft(2, '0');
    final hm = '${two(t.hour)}:${two(t.minute)}';
    final now = DateTime.now();
    final sameDay =
        t.year == now.year && t.month == now.month && t.day == now.day;
    if (sameDay) return hm;
    return '${two(t.day)}.${two(t.month)} $hm';
  }

  factory TicketMessage.fromJson(Map<String, dynamic> json) => TicketMessage(
    id: (json['id'] as num?)?.toInt() ?? 0,
    ticketId: (json['ticket_id'] as num?)?.toInt() ?? 0,
    // Панель присылает author ("user"|"support"); поддерживаем и старое
    // sender_role на случай иной обёртки.
    author:
        (json['author'] as String?) ??
        (json['sender_role'] as String?) ??
        'user',
    body: (json['body'] as String?) ?? '',
    createdAt: _parseDate(json['created_at']),
  );
}

/// Тикет с лентой сообщений (`GET /api/v2/app/tickets/{id}`). Контракт-допущение:
/// объект тикета + поле `messages: TicketMessage[]`. Если панель отдаёт сообщения
/// отдельным ключом/массивом, [TicketDetail.fromJson] это терпит (см. ниже).
class TicketDetail {
  final int id;
  final String category;
  final String subject;
  final TicketStatus status;
  final DateTime? createdAt;
  final DateTime? updatedAt;
  final List<TicketMessage> messages;

  const TicketDetail({
    required this.id,
    this.category = '',
    required this.subject,
    this.status = TicketStatus.open,
    this.createdAt,
    this.updatedAt,
    this.messages = const [],
  });

  factory TicketDetail.fromJson(Map<String, dynamic> json) {
    // Допускаем как плоский тикет с `messages`, так и вложенный `{ticket, messages}`.
    final ticket = (json['ticket'] is Map)
        ? (json['ticket'] as Map).cast<String, dynamic>()
        : json;
    final rawMessages = json['messages'] ?? ticket['messages'];
    final messages = (rawMessages is List)
        ? rawMessages
              .whereType<Map<dynamic, dynamic>>()
              .map((e) => TicketMessage.fromJson(e.cast<String, dynamic>()))
              .toList(growable: false)
        : const <TicketMessage>[];
    return TicketDetail(
      id: (ticket['id'] as num?)?.toInt() ?? 0,
      category: (ticket['category'] as String?) ?? '',
      subject: (ticket['subject'] as String?) ?? 'Без темы',
      status: TicketStatusX.parse(ticket['status'] as String?),
      createdAt: _parseDate(ticket['created_at']),
      updatedAt: _parseDate(ticket['updated_at']),
      messages: messages,
    );
  }
}

String _relative(DateTime? t) {
  if (t == null) return '';
  final d = DateTime.now().difference(t);
  if (d.inMinutes < 1) return 'только что';
  if (d.inMinutes < 60) return '${d.inMinutes} мин назад';
  if (d.inHours < 24) return '${d.inHours} ч назад';
  if (d.inDays < 7) return '${d.inDays} дн назад';
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(t.day)}.${two(t.month)}.${t.year}';
}

DateTime? _parseDate(Object? v) {
  if (v is String && v.isNotEmpty) return DateTime.tryParse(v)?.toLocal();
  return null;
}
