import 'dart:convert';

/// Тип подключения профиля.
///
/// - [rawSub] — импортированная подписка/конфиг (вставка URL/QR/файла). Ядро
///   поднимает туннель из локального mihomo-конфига без обращения к панели.
/// - [panelAccount] — аккаунт панели (subscription UUID + JWT). Ядро тянет
///   конфиг через панель, как и раньше.
///
/// Имя в JSON совпадает с `name` (стабильный ключ), чтобы старые записи не
/// ломались при добавлении новых типов.
enum ProfileType {
  rawSub,
  panelAccount;

  static ProfileType fromName(String? raw) {
    switch (raw) {
      case 'panelAccount':
        return ProfileType.panelAccount;
      case 'rawSub':
      default:
        return ProfileType.rawSub;
    }
  }
}

/// Профиль подключения — клиентский MULTI-PROFILE примитив (план §4.4).
///
/// ВНИМАНИЕ (коллизия имён): это НЕ аккаунт-профиль (`lib/features/profile/`,
/// `profile_state.dart`, `models/subscription.dart` — там подписки/устройства/
/// рефералы/семья). [ConnectionProfile] описывает одно подключение: либо
/// импортированную подписку ([ProfileType.rawSub]), либо аккаунт панели
/// ([ProfileType.panelAccount]).
///
/// Ровно один профиль активен в любой момент. Активный профиль ведёт И
/// подключение, И (в следующих фазах) тему. В P1 тему НЕ трогаем — только
/// храним [brandingCache] и [lastActiveMs] на будущее.
class ConnectionProfile {
  /// Стабильный идентификатор (генерируется при создании, не меняется).
  final String id;

  /// Тип подключения: импорт-подписка или аккаунт панели.
  final ProfileType type;

  /// Человекочитаемое имя в списке/свитчере.
  final String displayName;

  /// Источник: URL подписки (rawSub) или origin/панель-URL (panelAccount).
  final String source;

  /// URL панели. Заполнен только для [ProfileType.panelAccount].
  final String? panelUrl;

  /// UUID подписки. Заполнен только для [ProfileType.panelAccount].
  final String? subscriptionUuid;

  /// JWT access-токен. Заполнен только для [ProfileType.panelAccount].
  final String? accessToken;

  /// Импортированный mihomo YAML (rawSub), когда хранится локально. Передаётся
  /// нативной стороне через `importRawProfile` (Build C).
  final String? rawConfig;

  /// Кэш брендинга (произвольный JSON). В P1 только хранится, тему не ведёт.
  final Map<String, dynamic>? brandingCache;

  /// Время последней активации (мс эпохи). 0 — ещё не активировался.
  final int lastActiveMs;

  const ConnectionProfile({
    required this.id,
    required this.type,
    required this.displayName,
    required this.source,
    this.panelUrl,
    this.subscriptionUuid,
    this.accessToken,
    this.rawConfig,
    this.brandingCache,
    this.lastActiveMs = 0,
  });

  bool get isRaw => type == ProfileType.rawSub;
  bool get isPanel => type == ProfileType.panelAccount;

  factory ConnectionProfile.fromJson(Map<String, dynamic> json) =>
      ConnectionProfile(
        id: json['id'] as String,
        type: ProfileType.fromName(json['type'] as String?),
        displayName: (json['display_name'] as String?) ?? '',
        source: (json['source'] as String?) ?? '',
        panelUrl: json['panel_url'] as String?,
        subscriptionUuid: json['subscription_uuid'] as String?,
        accessToken: json['access_token'] as String?,
        rawConfig: json['raw_config'] as String?,
        brandingCache: _decodeBranding(json['branding_cache']),
        lastActiveMs: (json['last_active_ms'] as num?)?.toInt() ?? 0,
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'type': type.name,
        'display_name': displayName,
        'source': source,
        'panel_url': panelUrl,
        'subscription_uuid': subscriptionUuid,
        'access_token': accessToken,
        'raw_config': rawConfig,
        'branding_cache': brandingCache,
        'last_active_ms': lastActiveMs,
      };

  ConnectionProfile copyWith({
    String? id,
    ProfileType? type,
    String? displayName,
    String? source,
    String? panelUrl,
    String? subscriptionUuid,
    String? accessToken,
    String? rawConfig,
    Map<String, dynamic>? brandingCache,
    int? lastActiveMs,
  }) =>
      ConnectionProfile(
        id: id ?? this.id,
        type: type ?? this.type,
        displayName: displayName ?? this.displayName,
        source: source ?? this.source,
        panelUrl: panelUrl ?? this.panelUrl,
        subscriptionUuid: subscriptionUuid ?? this.subscriptionUuid,
        accessToken: accessToken ?? this.accessToken,
        rawConfig: rawConfig ?? this.rawConfig,
        brandingCache: brandingCache ?? this.brandingCache,
        lastActiveMs: lastActiveMs ?? this.lastActiveMs,
      );

  /// Тивкость: branding мог быть как map, так и строкой-JSON (старые записи).
  static Map<String, dynamic>? _decodeBranding(Object? v) {
    if (v == null) return null;
    if (v is Map<String, dynamic>) return v;
    if (v is Map) return v.map((k, val) => MapEntry(k.toString(), val));
    if (v is String && v.isNotEmpty) {
      try {
        final decoded = jsonDecode(v);
        if (decoded is Map<String, dynamic>) return decoded;
      } catch (_) {
        return null;
      }
    }
    return null;
  }
}
