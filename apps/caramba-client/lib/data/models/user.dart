/// Профиль пользователя из `GET /api/v2/app/me`.
///
/// Соответствует JSON-ответу `app::get_me` в
/// `apps/caramba-panel/src/api/v2/app.rs`.
class User {
  /// `users.id`.
  final int id;

  /// Telegram-id (если аккаунт привязан к Telegram), иначе `null`.
  final int? tgId;

  /// Email (если регистрация по email), иначе `null`.
  final String? email;

  /// Telegram username, иначе `null`.
  final String? username;

  /// Отображаемое имя.
  final String? fullName;

  /// Баланс в денежных единицах (рубли/доллары — `balance_cents / 100`).
  final double balance;

  /// Баланс в копейках/центах (исходное целочисленное значение).
  final int balanceCents;

  /// Реферальный код пользователя.
  final String? referralCode;

  /// Подтверждён ли email.
  final bool emailVerified;

  /// Провайдер аутентификации (`telegram` / `email` / ...), если задан.
  final String? authProvider;

  /// Кол-во активных подписок.
  final int activeSubscriptions;

  /// Имя текущего (наиболее «свежего») активного плана.
  final String? planName;

  const User({
    required this.id,
    this.tgId,
    this.email,
    this.username,
    this.fullName,
    this.balance = 0,
    this.balanceCents = 0,
    this.referralCode,
    this.emailVerified = false,
    this.authProvider,
    this.activeSubscriptions = 0,
    this.planName,
  });

  /// Имя для приветствия на Home («Hi, Alex»). Падаем с full_name → username →
  /// email-локалpart → "there".
  String get displayName {
    final fn = fullName?.trim();
    if (fn != null && fn.isNotEmpty) return fn;
    final un = username?.trim();
    if (un != null && un.isNotEmpty) return un;
    final em = email;
    if (em != null && em.contains('@')) return em.split('@').first;
    return 'there';
  }

  bool get hasActivePlan => activeSubscriptions > 0;

  factory User.fromJson(Map<String, dynamic> json) => User(
        id: (json['id'] as num).toInt(),
        tgId: (json['tg_id'] as num?)?.toInt(),
        email: json['email'] as String?,
        username: json['username'] as String?,
        fullName: json['full_name'] as String?,
        balance: (json['balance'] as num?)?.toDouble() ?? 0,
        balanceCents: (json['balance_cents'] as num?)?.toInt() ?? 0,
        referralCode: json['referral_code'] as String?,
        emailVerified: (json['email_verified'] as bool?) ?? false,
        authProvider: json['auth_provider'] as String?,
        activeSubscriptions:
            (json['active_subscriptions'] as num?)?.toInt() ?? 0,
        planName: json['plan_name'] as String?,
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'tg_id': tgId,
        'email': email,
        'username': username,
        'full_name': fullName,
        'balance': balance,
        'balance_cents': balanceCents,
        'referral_code': referralCode,
        'email_verified': emailVerified,
        'auth_provider': authProvider,
        'active_subscriptions': activeSubscriptions,
        'plan_name': planName,
      };
}
