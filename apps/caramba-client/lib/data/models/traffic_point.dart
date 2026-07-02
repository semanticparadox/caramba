/// Точка ряда трафика для графика (fl_chart) на Home/Профиле.
///
/// Контракт `GET /api/v2/app/traffic` (`app_billing.rs`), элемент `points`:
/// ```json
/// { "date":"YYYY-MM-DD", "up_bytes":123, "down_bytes":456, "total_bytes":579 }
/// ```
class TrafficPoint {
  final DateTime ts;
  final int upBytes;
  final int downBytes;

  const TrafficPoint({required this.ts, this.upBytes = 0, this.downBytes = 0});

  int get totalBytes => upBytes + downBytes;

  /// Суммарно в МБ (для оси графика).
  double get totalMb => totalBytes / (1024 * 1024);

  factory TrafficPoint.fromJson(Map<String, dynamic> json) => TrafficPoint(
    // Панель отдаёт `date` (YYYY-MM-DD); `ts` (RFC3339) — фолбэк на случай
    // иного источника.
    ts:
        DateTime.tryParse((json['date'] ?? json['ts'])?.toString() ?? '') ??
        DateTime.fromMillisecondsSinceEpoch(0),
    upBytes: (json['up_bytes'] as num?)?.toInt() ?? 0,
    downBytes: (json['down_bytes'] as num?)?.toInt() ?? 0,
  );
}
