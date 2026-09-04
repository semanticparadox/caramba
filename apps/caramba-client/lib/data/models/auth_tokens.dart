import 'dart:convert';

/// Когда истекает access-токен, по claim `exp` внутри него. `null` — срок не
/// читается (не JWT, нет claim, мусор).
///
/// Подпись НЕ проверяется: ключа панели у клиента нет, а нужен не факт
/// подлинности, а срок — им ядро решает, идти с этим токеном в сеть или сначала
/// обновиться. Подделанный срок ничего не открывает: запрос просто получит 401.
///
/// Нужно, потому что `TokenStore` хранит саму пару и не хранит её срок, а срок
/// обязан доехать до Go-ядра вместе с токеном: только по нему ядро отличает
/// «сессия жива» от «пора обновиться» и не выдаёт протухший токен за
/// авторизацию.
DateTime? accessTokenExpiry(String accessToken) {
  final parts = accessToken.trim().split('.');
  if (parts.length != 3) return null;
  try {
    // JWT кодируется base64url БЕЗ выравнивания '=' (RFC 7515 §2).
    final payload = utf8.decode(base64Url.decode(base64Url.normalize(parts[1])));
    final claims = jsonDecode(payload);
    if (claims is! Map) return null;
    final exp = claims['exp'];
    if (exp is! num || exp <= 0) return null;
    return DateTime.fromMillisecondsSinceEpoch(exp.toInt() * 1000);
  } on FormatException {
    return null;
  }
}

/// Пара JWT, выпускаемая панелью (`/api/v2/app/*`).
///
/// Соответствует Rust-структуре `TokenPair` в
/// `apps/caramba-panel/src/api/v2/app_auth.rs`:
/// ```json
/// {
///   "access_token": "...",
///   "refresh_token": "...",
///   "token_type": "Bearer",
///   "expires_in": 900,
///   "user_id": 42
/// }
/// ```
class AuthTokens {
  /// Короткоживущий access-токен (HS256, ~15 мин). Идёт в `Authorization: Bearer`.
  final String accessToken;

  /// Непрозрачный refresh-токен (~30 дней). Хранится в secure storage,
  /// используется для ротации через `/api/v2/app/refresh`.
  final String refreshToken;

  /// Тип токена; панель всегда отдаёт `Bearer`.
  final String tokenType;

  /// Время жизни access-токена в секундах (для планирования refresh).
  final int expiresIn;

  /// ID пользователя (`users.id`).
  final int userId;

  const AuthTokens({
    required this.accessToken,
    required this.refreshToken,
    required this.tokenType,
    required this.expiresIn,
    required this.userId,
  });

  factory AuthTokens.fromJson(Map<String, dynamic> json) => AuthTokens(
    accessToken: json['access_token'] as String,
    refreshToken: json['refresh_token'] as String,
    tokenType: (json['token_type'] as String?) ?? 'Bearer',
    expiresIn: (json['expires_in'] as num?)?.toInt() ?? 900,
    userId: (json['user_id'] as num?)?.toInt() ?? 0,
  );

  Map<String, dynamic> toJson() => {
    'access_token': accessToken,
    'refresh_token': refreshToken,
    'token_type': tokenType,
    'expires_in': expiresIn,
    'user_id': userId,
  };

  AuthTokens copyWith({
    String? accessToken,
    String? refreshToken,
    String? tokenType,
    int? expiresIn,
    int? userId,
  }) => AuthTokens(
    accessToken: accessToken ?? this.accessToken,
    refreshToken: refreshToken ?? this.refreshToken,
    tokenType: tokenType ?? this.tokenType,
    expiresIn: expiresIn ?? this.expiresIn,
    userId: userId ?? this.userId,
  );
}
