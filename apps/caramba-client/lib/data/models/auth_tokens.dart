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
