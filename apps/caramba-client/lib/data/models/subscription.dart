/// Подписка пользователя из `GET /api/v2/app/subscription`.
///
/// Соответствует JSON-ответу `app::get_subscription` в
/// `apps/caramba-panel/src/api/v2/app.rs`. Ключевое поле для Go-ядра (mihomo) —
/// [clashUrl]: ядро тянет именно этот mihomo/clash-конфиг (amnezia-wg уже внутри).
class Subscription {
  /// `subscriptions.id`.
  final int id;

  /// UUID подписки — основа для всех config-URL (caramba-sub отдаёт по нему).
  final String subscriptionUuid;

  /// Имя плана.
  final String? planName;

  /// Статус (`active`, `expired`, ...).
  final String status;

  /// Использованный трафик в байтах.
  final int usedTrafficBytes;

  /// Использованный трафик в ГБ (строкой `"42.10"` от панели).
  final String usedTrafficGb;

  /// Лимит трафика в ГБ (`null`/0 для безлимита — зависит от плана).
  final double? trafficLimitGb;

  /// Дата истечения (RFC3339).
  final DateTime? expiresAt;

  /// Дней до истечения (уже посчитано панелью, >= 0).
  final int daysLeft;

  /// URL mihomo/clash-конфига — **это тянет Go-ядро**.
  final String clashUrl;

  /// Алиас config_url (тот же clash-путь).
  final String configUrl;

  /// URL sing-box конфига (на случай иных клиентов).
  final String singboxUrl;

  /// URL v2ray-подписки.
  final String v2rayUrl;

  /// Базовый URL подписки (без `?client=`).
  final String subscriptionUrl;

  const Subscription({
    required this.id,
    required this.subscriptionUuid,
    this.planName,
    required this.status,
    this.usedTrafficBytes = 0,
    this.usedTrafficGb = '0.00',
    this.trafficLimitGb,
    this.expiresAt,
    this.daysLeft = 0,
    required this.clashUrl,
    required this.configUrl,
    this.singboxUrl = '',
    this.v2rayUrl = '',
    this.subscriptionUrl = '',
  });

  bool get isActive => status == 'active';

  /// Использовано ГБ как число (для прогресс-бара).
  double get usedGb => double.tryParse(usedTrafficGb) ?? 0;

  /// Доля использованного трафика в диапазоне 0..1 (для квота-бара).
  /// Возвращает `null`, если лимит безлимитный/неизвестен.
  double? get usageFraction {
    final limit = trafficLimitGb;
    if (limit == null || limit <= 0) return null;
    return (usedGb / limit).clamp(0.0, 1.0);
  }

  factory Subscription.fromJson(Map<String, dynamic> json) => Subscription(
        id: (json['id'] as num).toInt(),
        subscriptionUuid: json['subscription_uuid'] as String,
        planName: json['plan_name'] as String?,
        status: (json['status'] as String?) ?? 'unknown',
        usedTrafficBytes: (json['used_traffic_bytes'] as num?)?.toInt() ?? 0,
        usedTrafficGb: json['used_traffic_gb']?.toString() ?? '0.00',
        trafficLimitGb: (json['traffic_limit_gb'] as num?)?.toDouble(),
        expiresAt: _parseDate(json['expires_at']),
        daysLeft: (json['days_left'] as num?)?.toInt() ?? 0,
        clashUrl: (json['clash_url'] as String?) ?? '',
        configUrl:
            (json['config_url'] as String?) ?? (json['clash_url'] as String?) ?? '',
        singboxUrl: (json['singbox_url'] as String?) ?? '',
        v2rayUrl: (json['v2ray_url'] as String?) ?? '',
        subscriptionUrl: (json['subscription_url'] as String?) ?? '',
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'subscription_uuid': subscriptionUuid,
        'plan_name': planName,
        'status': status,
        'used_traffic_bytes': usedTrafficBytes,
        'used_traffic_gb': usedTrafficGb,
        'traffic_limit_gb': trafficLimitGb,
        'expires_at': expiresAt?.toIso8601String(),
        'days_left': daysLeft,
        'clash_url': clashUrl,
        'config_url': configUrl,
        'singbox_url': singboxUrl,
        'v2ray_url': v2rayUrl,
        'subscription_url': subscriptionUrl,
      };

  static DateTime? _parseDate(Object? v) {
    if (v is String && v.isNotEmpty) return DateTime.tryParse(v);
    return null;
  }
}
