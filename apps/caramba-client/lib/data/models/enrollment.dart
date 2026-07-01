/// Модели энроллмента (P2, contract A/B).
///
/// Энроллмент — это вход в инстанс панели по инвайт-коду. Клиент открывает
/// deeplink `carambaconnect://enroll?panel=<https>&code=<invite>` (или вводит
/// код + URL панели вручную / сканирует QR с тем же URI), валидирует код на
/// панели и заводит [ConnectionProfile] типа panelAccount, после чего ведёт
/// пользователя в регистрацию/вход. Аккаунт обязателен всегда.

/// Разобранная enroll-ссылка: URL панели + инвайт-код.
///
/// Источники, дающие [EnrollLink]:
///   * deeplink `carambaconnect://enroll?panel=...&code=...`;
///   * ручной ввод (поле «код» + поле «URL панели»);
///   * QR, несущий тот же URI.
class EnrollLink {
  /// HTTPS-URL инстанса панели (origin без хвоста `/api/...`).
  final String panelUrl;

  /// Инвайт-код энроллмента. Сопоставляется панелью с inviter/org.
  final String code;

  const EnrollLink({required this.panelUrl, required this.code});

  /// Парсит `carambaconnect://enroll?panel=<https>&code=<invite>`.
  ///
  /// Возвращает `null`, если схема/хост не те, обязательные параметры пусты,
  /// или `panel` не является http(s)-URL. Хост у custom-scheme URI может попасть
  /// либо в [Uri.host] (`carambaconnect://enroll`), либо в первый сегмент пути
  /// (`carambaconnect:///enroll`) — учитываем оба.
  static EnrollLink? tryParse(String raw) {
    final uri = Uri.tryParse(raw.trim());
    if (uri == null) return null;
    if (uri.scheme.toLowerCase() != 'carambaconnect') return null;

    final action = uri.host.isNotEmpty
        ? uri.host
        : (uri.pathSegments.isNotEmpty ? uri.pathSegments.first : '');
    if (action.toLowerCase() != 'enroll') return null;

    final panel = (uri.queryParameters['panel'] ?? '').trim();
    final code = (uri.queryParameters['code'] ?? '').trim();
    return fromParts(panelUrl: panel, code: code);
  }

  /// Собирает [EnrollLink] из отдельных полей (ручной ввод / QR-парсер).
  /// Нормализует URL панели и проверяет, что это http(s). Возвращает `null`
  /// при пустом коде или невалидном URL.
  static EnrollLink? fromParts({
    required String panelUrl,
    required String code,
  }) {
    final trimmedCode = code.trim();
    if (trimmedCode.isEmpty) return null;
    final normalized = normalizePanelUrl(panelUrl);
    if (normalized == null) return null;
    return EnrollLink(panelUrl: normalized, code: trimmedCode);
  }

  /// Нормализует URL панели до origin (`scheme://host[:port]`), отбрасывая
  /// путь/хвост. Допускает ввод без схемы (добавляем `https://`). Отвергает
  /// не-http(s) и пустой хост. Возвращает `null` при невалидном вводе.
  static String? normalizePanelUrl(String raw) {
    var v = raw.trim();
    if (v.isEmpty) return null;
    if (!v.contains('://')) v = 'https://$v';
    final uri = Uri.tryParse(v);
    if (uri == null) return null;
    final scheme = uri.scheme.toLowerCase();
    if (scheme != 'https' && scheme != 'http') return null;
    if (uri.host.isEmpty) return null;
    final port = uri.hasPort ? ':${uri.port}' : '';
    return '$scheme://${uri.host}$port';
  }
}

/// Ответ публичной валидации `GET /api/v2/app/enroll/{code}`.
///
/// Контракт панели (источник истины):
/// `{ valid: bool, reason?: string, panel_name?: string,
///    onboarding_traffic_mb: int }`.
/// Это ЧИСТО валидация — код не расходуется (used_count не растёт). PII не
/// отдаётся: только имя панели, число онбординг-трафика и флаг валидности.
class EnrollValidation {
  /// Код существует, не истёк и остались использования.
  final bool valid;

  /// Машинная причина невалидности (`expired` / `exhausted` / `unknown`).
  /// Заполняется только при `valid == false`. Клиент маппит на свой текст.
  final String? reason;

  /// Имя панели для ранней подписи профиля (брендинг приходит позже, P3).
  final String? panelName;

  /// Разовый онбординг-трафик (МБ), который получит новый неоплаченный аккаунт.
  /// `0` — онбординг выключен на этой панели.
  final int onboardingTrafficMb;

  const EnrollValidation({
    required this.valid,
    this.reason,
    this.panelName,
    this.onboardingTrafficMb = 0,
  });

  bool get hasOnboardingTraffic => onboardingTrafficMb > 0;

  factory EnrollValidation.fromJson(Map<String, dynamic> json) =>
      EnrollValidation(
        valid: json['valid'] == true,
        reason: (json['reason'] as String?)?.trim().isEmpty == true
            ? null
            : json['reason'] as String?,
        panelName: (json['panel_name'] as String?)?.trim().isEmpty == true
            ? null
            : json['panel_name'] as String?,
        onboardingTrafficMb:
            (json['onboarding_traffic_mb'] as num?)?.toInt() ?? 0,
      );
}
