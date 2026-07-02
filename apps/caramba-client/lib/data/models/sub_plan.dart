import 'package:caramba_client/widgets/lucide.dart';

enum SubKind { free, unlimited, pool }

/// Подписка пользователя в профиле (одна из нескольких). Соответствует элементу
/// `AppSubscription` из `apps/caramba-panel/src/api/v2/app_account.rs`
/// (`GET /api/v2/app/subscriptions`):
/// ```json
/// { "id":7, "subscription_uuid":"...", "plan_name":"Pro", "status":"active",
///   "kind":"paid", "used_traffic_bytes":123, "used_traffic_gb":"0.42",
///   "traffic_quota_gb":50, "weekly_free_refill_gb":7.0,
///   "expires_at":"RFC3339", "days_left":18,
///   "device_used":2, "device_limit":5,
///   "pool_name":"Private", "relay_country":"NL" }
/// ```
/// `kind`: free | paid | private (private = семейная, выданная родителем).
class SubPlan {
  final int id;
  final String subscriptionUuid;
  final String name;
  final SubKind kind;
  final String status;
  final String icon; // Lucide glyph

  /// Использовано трафика в ГБ (число), и недельная квота для free
  /// (`weekly_free_refill_gb`), иначе 0 = безлимит/нет квоты.
  final double usedGb;
  final double quotaGb;

  /// Лимит трафика плана в ГБ (`traffic_quota_gb`), null = безлимит.
  final int? trafficQuotaGb;

  /// Дней до истечения и дата истечения (для платных/семейных).
  final int daysLeft;
  final DateTime? expiresAt;

  /// Имя пула узлов (node group) для подписки, если назначен.
  final String? poolName;

  /// relay-страна, привязанная к подписке (если задана).
  final String? relayCountry;

  /// Устройства: занято за 15 мин / лимит плана.
  final int devUsed;
  final int devLimit;

  const SubPlan({
    required this.id,
    this.subscriptionUuid = '',
    required this.name,
    required this.kind,
    this.status = 'active',
    required this.icon,
    this.usedGb = 0,
    this.quotaGb = 0,
    this.trafficQuotaGb,
    this.daysLeft = 0,
    this.expiresAt,
    this.poolName,
    this.relayCountry,
    required this.devUsed,
    required this.devLimit,
  });

  bool get isActive => status == 'active';
  bool get shareable => kind != SubKind.free;

  int get freeSlots => (devLimit - devUsed).clamp(0, devLimit);

  /// Метка пула для UI (панель отдаёт имя группы или null).
  String get poolLabel {
    final p = poolName?.trim();
    if (p != null && p.isNotEmpty) return p;
    return kind == SubKind.pool ? 'Частный пул' : 'Общие серверы';
  }

  /// Краткая мета платной/семейной подписки.
  String get meta {
    if (trafficQuotaGb == null) return 'Безлимит трафика';
    return 'Лимит $trafficQuotaGb ГБ';
  }

  /// Подпись о сроке («осталось N дн.»), null если безлимитный срок.
  String? get expiresLabel {
    if (expiresAt == null) return null;
    if (daysLeft <= 0) return 'истекает';
    return 'осталось $daysLeft дн.';
  }

  /// Подпись о пополнении free-квоты.
  String? get refillNote =>
      kind == SubKind.free && quotaGb > 0 ? 'пополняется еженедельно' : null;

  /// Доля квоты 0..1 (только для free).
  double get quotaFraction =>
      quotaGb <= 0 ? 0 : (usedGb / quotaGb).clamp(0.0, 1.0);
  bool get quotaLow => quotaFraction > 0.8;

  factory SubPlan.fromJson(Map<String, dynamic> json) {
    final kindStr = (json['kind'] as String?) ?? 'paid';
    final kind = switch (kindStr) {
      'free' => SubKind.free,
      'private' || 'pool' => SubKind.pool,
      _ => SubKind.unlimited,
    };
    final weeklyFree = (json['weekly_free_refill_gb'] as num?)?.toDouble() ?? 0;
    return SubPlan(
      id: (json['id'] as num?)?.toInt() ?? 0,
      subscriptionUuid: (json['subscription_uuid'] as String?) ?? '',
      name:
          (json['plan_name'] as String?) ?? (json['name'] as String?) ?? 'План',
      kind: kind,
      status: (json['status'] as String?) ?? 'active',
      icon: switch (kind) {
        SubKind.free => Lucide.gift,
        SubKind.unlimited => Lucide.infinity,
        SubKind.pool => Lucide.key,
      },
      usedGb: double.tryParse(json['used_traffic_gb']?.toString() ?? '') ?? 0,
      // free: квота = недельное пополнение; иначе лимит плана (для бара).
      quotaGb: kind == SubKind.free
          ? weeklyFree
          : ((json['traffic_quota_gb'] as num?)?.toDouble() ?? 0),
      trafficQuotaGb: (json['traffic_quota_gb'] as num?)?.toInt(),
      daysLeft: (json['days_left'] as num?)?.toInt() ?? 0,
      expiresAt: _parseDate(json['expires_at']),
      poolName: json['pool_name'] as String?,
      relayCountry: json['relay_country'] as String?,
      devUsed: (json['device_used'] as num?)?.toInt() ?? 0,
      devLimit: (json['device_limit'] as num?)?.toInt() ?? 1,
    );
  }

  static DateTime? _parseDate(Object? v) {
    if (v is String && v.isNotEmpty) return DateTime.tryParse(v);
    return null;
  }

  /// Публичный парсер RFC3339-дат для моделей в других файлах
  /// (см. `data/models/partner.dart`).
  static DateTime? parseDate(Object? v) => _parseDate(v);
}

/// Устройство-лиза (`AppDevice` из `app_account.rs`, `GET /api/v2/app/devices`):
/// ```json
/// { "id":1, "subscription_id":7, "name":"iPhone", "last_ip":"203.0.113.*",
///   "user_agent":"...", "first_seen_at":"RFC3339", "last_seen_at":"RFC3339",
///   "online":true }
/// ```
class Device {
  final int id;
  final int subscriptionId;
  final String name;
  final String icon; // Lucide glyph
  final String lastIp;
  final String? userAgent;
  final DateTime? lastSeenAt;
  final bool online;

  const Device({
    required this.id,
    this.subscriptionId = 0,
    required this.name,
    required this.icon,
    this.lastIp = '',
    this.userAgent,
    this.lastSeenAt,
    this.online = false,
  });

  /// Человекочитаемая метка последней активности (плоский текст, без em-dash).
  String get lastSeenLabel {
    if (online) return 'Активно сейчас';
    final t = lastSeenAt;
    if (t == null) return 'Не в сети';
    final d = DateTime.now().difference(t);
    if (d.inMinutes < 60) return '${d.inMinutes} мин назад';
    if (d.inHours < 24) return '${d.inHours} ч назад';
    if (d.inDays < 30) return '${d.inDays} дн назад';
    return 'Давно';
  }

  factory Device.fromJson(Map<String, dynamic> json) {
    final ua = (json['user_agent'] as String?)?.toLowerCase() ?? '';
    final name =
        (json['name'] as String?) ??
        (json['display_name'] as String?) ??
        'Устройство';
    final isPhone =
        ua.contains('iphone') ||
        ua.contains('android') ||
        ua.contains('mobile') ||
        name.toLowerCase().contains('iphone') ||
        name.toLowerCase().contains('android');
    return Device(
      id: (json['id'] as num?)?.toInt() ?? 0,
      subscriptionId: (json['subscription_id'] as num?)?.toInt() ?? 0,
      name: name,
      icon: isPhone ? Lucide.phone : Lucide.laptop,
      lastIp: (json['last_ip'] as String?) ?? '',
      userAgent: json['user_agent'] as String?,
      lastSeenAt: SubPlan._parseDate(json['last_seen_at']),
      online: (json['online'] as bool?) ?? false,
    );
  }
}

/// Статус приглашённого пользователя в реферальном списке.
///   * registered: зарегистрировался по ссылке, но ещё не платил;
///   * purchased: совершил первую оплату (за неё начислен бонус рефереру).
enum ReferralStatus { registered, purchased }

/// Один приглашённый пользователь (`referrals[]` из `/app/referrals`):
/// ```json
/// { "username_masked":"al***v", "joined_at":"RFC3339",
///   "status":"registered"|"purchased", "earned":120 }
/// ```
/// `earned`: начисленный рефереру бонус за этого пользователя, в минорных
/// единицах (копейки/центы), как в биллинге.
class ReferralEntry {
  final String usernameMasked;
  final DateTime? joinedAt;
  final ReferralStatus status;
  final int earnedCents;

  const ReferralEntry({
    required this.usernameMasked,
    this.joinedAt,
    this.status = ReferralStatus.registered,
    this.earnedCents = 0,
  });

  bool get purchased => status == ReferralStatus.purchased;

  /// Отображаемое имя: маскированный username или плейсхолдер.
  String get displayName {
    final u = usernameMasked.trim();
    return u.isEmpty ? 'Пользователь' : u;
  }

  /// Начислено рефереру за этого пользователя, мажорные единицы строкой.
  String get earnedLabel => ReferralInfo.formatMinor(earnedCents);

  factory ReferralEntry.fromJson(Map<String, dynamic> json) => ReferralEntry(
    usernameMasked:
        (json['username_masked'] as String?) ??
        (json['username'] as String?) ??
        '',
    joinedAt: SubPlan._parseDate(json['joined_at']),
    status: switch ((json['status'] as String?) ?? 'registered') {
      'purchased' => ReferralStatus.purchased,
      _ => ReferralStatus.registered,
    },
    earnedCents: (json['earned'] as num?)?.toInt() ?? 0,
  );
}

/// Реферальная сводка (`AppReferrals` из `app_account.rs`,
/// `GET /api/v2/app/referrals`). Авторитетная денежная модель: реферер
/// получает внутренний баланс (не трафик), приглашённый получает скидку на
/// ПЕРВУЮ оплату. Поля-имена из контракта; legacy-ключи терпим для совместимости:
/// ```json
/// { "referral_code":"EXARO7K2",
///   "referral_link":"https://exarobot.top/r/EXARO7K2",
///   "invited_count":4,
///   "balance":1240, "balance_earned":3600,
///   "reward_percent":20, "referee_discount_percent":15,
///   "referrals":[ {"username_masked":"al***v","joined_at":"...",
///                  "status":"purchased","earned":600} ] }
/// ```
/// `balance` / `balance_earned` / `earned`: минорные единицы (копейки/центы).
class ReferralInfo {
  final String code;
  final int invited;

  /// Текущий баланс пользователя в минорных единицах (`users.balance`).
  final int balanceCents;

  /// Всего начислено с рефералов за всё время, минорные единицы.
  final int balanceEarnedCents;

  /// % оплаты приглашённого, который начисляется рефереру.
  final int rewardPercent;

  /// % скидки приглашённому на его ПЕРВУЮ платную покупку.
  final int refereeDiscountPercent;

  final String shareLink;
  final String? botLink;

  /// Список приглашённых (маскированный username, статус, начислено).
  final List<ReferralEntry> referrals;

  const ReferralInfo({
    required this.code,
    required this.invited,
    this.balanceCents = 0,
    this.balanceEarnedCents = 0,
    this.rewardPercent = 20,
    this.refereeDiscountPercent = 15,
    this.shareLink = '',
    this.botLink,
    this.referrals = const [],
  });

  String get inviteLink =>
      shareLink.isNotEmpty ? shareLink : 'https://t.me/exarobot?start=$code';

  /// Текущий баланс в денежных единицах (минорные / 100).
  double get balance => balanceCents / 100;

  /// Всего начислено в денежных единицах.
  double get balanceEarned => balanceEarnedCents / 100;

  /// Текущий баланс строкой без копеек (мажорные единицы): «12», «0».
  String get balanceLabel => formatMinor(balanceCents);

  /// Всего начислено строкой без копеек: «36», «0».
  String get balanceEarnedLabel => formatMinor(balanceEarnedCents);

  /// Минорные единицы (копейки/центы) -> мажорная строка без хвостовых нулей.
  static String formatMinor(int cents) {
    if (cents <= 0) return '0';
    final major = cents / 100;
    return major == major.roundToDouble()
        ? major.toStringAsFixed(0)
        : major.toStringAsFixed(2);
  }

  factory ReferralInfo.fromJson(Map<String, dynamic> json) {
    final list = (json['referrals'] as List?) ?? const [];
    return ReferralInfo(
      code:
          (json['referral_code'] as String?) ?? (json['code'] as String?) ?? '',
      invited:
          (json['invited_count'] as num?)?.toInt() ??
          (json['invited'] as num?)?.toInt() ??
          list.length,
      // Минорные единицы из контракта; терпим legacy bonus_cents.
      balanceCents:
          (json['balance'] as num?)?.toInt() ??
          (json['balance_cents'] as num?)?.toInt() ??
          0,
      balanceEarnedCents: (json['balance_earned'] as num?)?.toInt() ?? 0,
      rewardPercent: (json['reward_percent'] as num?)?.toInt() ?? 20,
      refereeDiscountPercent:
          (json['referee_discount_percent'] as num?)?.toInt() ?? 15,
      shareLink:
          (json['referral_link'] as String?) ??
          (json['share_link'] as String?) ??
          '',
      botLink: json['bot_link'] as String?,
      referrals: list
          .whereType<Map>()
          .map((e) => ReferralEntry.fromJson(e.cast<String, dynamic>()))
          .toList(growable: false),
    );
  }
}

/// Участник семьи (`FamilyMember` из `app_account.rs`).
class FamilyMember {
  final int userId;
  final String? username;
  final String? fullName;
  final bool hasActiveSub;
  final DateTime? joinedAt;

  const FamilyMember({
    required this.userId,
    this.username,
    this.fullName,
    this.hasActiveSub = false,
    this.joinedAt,
  });

  /// Отображаемое имя: full_name -> @username -> id.
  String get displayName {
    final fn = fullName?.trim();
    if (fn != null && fn.isNotEmpty) return fn;
    final un = username?.trim();
    if (un != null && un.isNotEmpty) return '@$un';
    return 'Пользователь $userId';
  }

  factory FamilyMember.fromJson(Map<String, dynamic> json) => FamilyMember(
    userId: (json['user_id'] as num?)?.toInt() ?? 0,
    username: json['username'] as String?,
    fullName: json['full_name'] as String?,
    hasActiveSub: (json['has_active_sub'] as bool?) ?? false,
    joinedAt: SubPlan._parseDate(json['joined_at']),
  );
}

/// Семья пользователя (`FamilyResponse`).
class Family {
  final bool isParent;
  final List<FamilyMember> members;

  const Family({this.isParent = false, this.members = const []});

  factory Family.fromJson(Map<String, dynamic> json) => Family(
    isParent: (json['is_parent'] as bool?) ?? false,
    members: ((json['members'] as List?) ?? const [])
        .whereType<Map>()
        .map((e) => FamilyMember.fromJson(e.cast<String, dynamic>()))
        .toList(growable: false),
  );
}

/// Инвайт в семью (`FamilyInviteResponse`).
class FamilyInvite {
  final String code;
  final DateTime? expiresAt;
  final int maxUses;
  final int usedCount;

  const FamilyInvite({
    required this.code,
    this.expiresAt,
    this.maxUses = 1,
    this.usedCount = 0,
  });

  /// Ссылка-приглашение в бота (deeplink). Код подставляется в start-параметр.
  String get inviteLink => 'https://t.me/exarobot?start=family-$code';

  factory FamilyInvite.fromJson(Map<String, dynamic> json) => FamilyInvite(
    code: (json['code'] as String?) ?? '',
    expiresAt: SubPlan._parseDate(json['expires_at']),
    maxUses: (json['max_uses'] as num?)?.toInt() ?? 1,
    usedCount: (json['used_count'] as num?)?.toInt() ?? 0,
  );
}
