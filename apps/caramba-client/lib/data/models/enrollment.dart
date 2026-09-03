/// Модели энроллмента (P2, contract A/B).
///
/// Энроллмент — это вход в инстанс панели по инвайт-коду. Клиент открывает
/// deeplink `carambaconnect://enroll?panel=<https>&code=<invite>` (или вводит
/// код + URL панели вручную / сканирует QR с тем же URI), валидирует код на
/// панели и заводит [ConnectionProfile] типа panelAccount, после чего ведёт
/// пользователя в регистрацию/вход. Аккаунт обязателен всегда.
library;

/// Почему ссылка отвергнута.
///
/// Отказ обязан быть ОБЪЯСНИМЫМ. Раньше оба парсера возвращали голый `null`, и
/// вызывающая сторона могла сказать пользователю только «проверьте ссылку»: для
/// отказа по INV-8 это худший из возможных текстов, потому что адрес выглядит
/// рабочим и человек будет вводить его снова. [LinkRefusal.message] даёт
/// готовую фразу, а `tryParse`/`fromParts`/`fromUrl` сохраняют старый контракт
/// с `null` и остаются совместимыми.
enum LinkRefusal {
  /// Схема или действие не наши (не `carambaconnect://enroll|import`).
  notOurLink,

  /// URL пуст, не разбирается или у него нет хоста.
  malformedUrl,

  /// Инвайт-код пуст.
  emptyCode,

  /// INV-8: обычный `http://`, и хост не `.onion`.
  insecureTransport,
}

/// Текст отказа для пользователя.
extension LinkRefusalMessage on LinkRefusal {
  String get message => switch (this) {
    LinkRefusal.notOurLink => 'Ссылка не распознана: это не ссылка Caramba.',
    LinkRefusal.malformedUrl =>
      'Адрес не разбирается: нужен полный URL с хостом.',
    LinkRefusal.emptyCode => 'Не хватает инвайт-кода.',
    LinkRefusal.insecureTransport =>
      'Адрес по http:// отклонён: манифест, конфигурация, правила и geo '
          'забираются только по https, единственное исключение это адрес .onion. '
          'Введите https-адрес.',
  };
}

/// Результат разбора ссылки: либо значение, либо причина отказа.
///
/// Ровно одно из полей непусто.
class LinkParseResult<T extends Object> {
  /// Разобранная ссылка, если разбор удался.
  final T? value;

  /// Причина отказа, если не удался.
  final LinkRefusal? refusal;

  const LinkParseResult.ok(T this.value) : refusal = null;

  const LinkParseResult.refused(LinkRefusal this.refusal) : value = null;

  bool get isOk => value != null;

  /// Готовая фраза для UI, либо `null` при успехе.
  String? get message => refusal?.message;
}

/// Общая для обеих ссылок проверка схемы (INV-8, 02-SPEC.md 8.10).
///
/// Единственное исключение без TLS это `.onion`: там аутентичность и
/// конфиденциальность даёт сам onion-адрес, а не TLS.
bool _isOnionHost(Uri uri) => uri.host.toLowerCase().endsWith('.onion');

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
  static EnrollLink? tryParse(String raw) => parse(raw).value;

  /// Тот же разбор, но с причиной отказа.
  static LinkParseResult<EnrollLink> parse(String raw) {
    final uri = Uri.tryParse(raw.trim());
    if (uri == null) {
      return const LinkParseResult.refused(LinkRefusal.malformedUrl);
    }
    if (uri.scheme.toLowerCase() != 'carambaconnect') {
      return const LinkParseResult.refused(LinkRefusal.notOurLink);
    }
    if (_action(uri) != 'enroll') {
      return const LinkParseResult.refused(LinkRefusal.notOurLink);
    }

    final panel = (uri.queryParameters['panel'] ?? '').trim();
    final code = (uri.queryParameters['code'] ?? '').trim();
    return parseParts(panelUrl: panel, code: code);
  }

  /// Собирает [EnrollLink] из отдельных полей (ручной ввод / QR-парсер).
  /// Нормализует URL панели и проверяет, что это http(s). Возвращает `null`
  /// при пустом коде или невалидном URL.
  static EnrollLink? fromParts({
    required String panelUrl,
    required String code,
  }) => parseParts(panelUrl: panelUrl, code: code).value;

  /// Тот же сбор из полей, но с причиной отказа.
  static LinkParseResult<EnrollLink> parseParts({
    required String panelUrl,
    required String code,
  }) {
    final trimmedCode = code.trim();
    if (trimmedCode.isEmpty) {
      return const LinkParseResult.refused(LinkRefusal.emptyCode);
    }
    final normalized = parsePanelUrl(panelUrl);
    if (!normalized.isOk) {
      return LinkParseResult.refused(normalized.refusal!);
    }
    return LinkParseResult.ok(
      EnrollLink(panelUrl: normalized.value!, code: trimmedCode),
    );
  }

  /// Нормализует URL панели до origin (`scheme://host[:port]`), отбрасывая
  /// путь/хвост. Допускает ввод без схемы (добавляем `https://`). Отвергает
  /// не-http(s) и пустой хост. Возвращает `null` при невалидном вводе.
  static String? normalizePanelUrl(String raw) => parsePanelUrl(raw).value;

  /// Та же нормализация, но с причиной отказа.
  static LinkParseResult<String> parsePanelUrl(String raw) {
    var v = raw.trim();
    if (v.isEmpty) {
      return const LinkParseResult.refused(LinkRefusal.malformedUrl);
    }
    if (!v.contains('://')) v = 'https://$v';
    final uri = Uri.tryParse(v);
    if (uri == null || uri.host.isEmpty) {
      return const LinkParseResult.refused(LinkRefusal.malformedUrl);
    }
    final scheme = uri.scheme.toLowerCase();
    // INV-8: http отвергается для любой выборки манифеста, конфигурации, правил
    // и geo, и единственное исключение без TLS это .onion. Проверка стоит
    // ЗДЕСЬ, в точке ввода, а не только в транспорте: иначе пользователь
    // вводит http-адрес, получает «принято», и узнаёт об отказе непрозрачной
    // ошибкой подключения (02-SPEC.md 8.10).
    if (scheme != 'https' && scheme != 'http') {
      return const LinkParseResult.refused(LinkRefusal.malformedUrl);
    }
    if (scheme == 'http' && !_isOnionHost(uri)) {
      return const LinkParseResult.refused(LinkRefusal.insecureTransport);
    }
    final port = uri.hasPort ? ':${uri.port}' : '';
    return LinkParseResult.ok('$scheme://${uri.host}$port');
  }
}

/// Разобранная import-ссылка: URL подписки для generic-режима.
///
/// Второе действие той же схемы: `carambaconnect://import?url=<encoded>`.
/// Ведёт на экран импорта с подставленной ссылкой; аккаунт панели для этого
/// пути не нужен — подписка произвольная.
///
/// Разбор намеренно живёт отдельно от [EnrollLink]: у ссылок разные
/// обязательные параметры, и `EnrollLink.tryParse` обязан по-прежнему отвергать
/// всё, что не `enroll` (обратная совместимость).
class ImportLink {
  /// URL подписки (http/https). Пустых значений здесь не бывает.
  final String url;

  const ImportLink({required this.url});

  /// Парсит `carambaconnect://import?url=<encoded sub url>`.
  ///
  /// Возвращает `null`, если схема/действие не те либо `url` пуст или не
  /// http(s). Как и у [EnrollLink], действие может стоять и в хосте
  /// (`carambaconnect://import`), и в первом сегменте пути
  /// (`carambaconnect:///import`).
  static ImportLink? tryParse(String raw) => parse(raw).value;

  /// Тот же разбор, но с причиной отказа.
  static LinkParseResult<ImportLink> parse(String raw) {
    final uri = Uri.tryParse(raw.trim());
    if (uri == null) {
      return const LinkParseResult.refused(LinkRefusal.malformedUrl);
    }
    if (uri.scheme.toLowerCase() != 'carambaconnect') {
      return const LinkParseResult.refused(LinkRefusal.notOurLink);
    }
    if (_action(uri) != 'import') {
      return const LinkParseResult.refused(LinkRefusal.notOurLink);
    }
    return parseUrl(uri.queryParameters['url'] ?? '');
  }

  /// Собирает [ImportLink] из голого URL (QR/ручной ввод). Требует https и
  /// непустой хост: сырой конфиг сюда не подходит, для него есть поле ввода.
  static ImportLink? fromUrl(String raw) => parseUrl(raw).value;

  /// Тот же сбор, но с причиной отказа.
  ///
  /// INV-8 действует и здесь, а не только на URL панели: ссылка подписки это
  /// та самая «выборка конфигурации», которую 02-SPEC.md 8.10 называет MUST
  /// stop. Раньше этот метод принимал любой `http://`, и импорт по открытому
  /// каналу проходил молча — активный посредник подменял бы конфиг подписки
  /// целиком. Исключение по-прежнему одно: `.onion`.
  static LinkParseResult<ImportLink> parseUrl(String raw) {
    final trimmed = raw.trim();
    if (trimmed.isEmpty) {
      return const LinkParseResult.refused(LinkRefusal.malformedUrl);
    }
    final uri = Uri.tryParse(trimmed);
    if (uri == null || uri.host.isEmpty) {
      return const LinkParseResult.refused(LinkRefusal.malformedUrl);
    }
    final scheme = uri.scheme.toLowerCase();
    if (scheme != 'https' && scheme != 'http') {
      return const LinkParseResult.refused(LinkRefusal.malformedUrl);
    }
    if (scheme == 'http' && !_isOnionHost(uri)) {
      return const LinkParseResult.refused(LinkRefusal.insecureTransport);
    }
    return LinkParseResult.ok(ImportLink(url: trimmed));
  }
}

/// Действие custom-scheme URI: хост либо первый сегмент пути, в нижнем регистре.
String _action(Uri uri) {
  final raw = uri.host.isNotEmpty
      ? uri.host
      : (uri.pathSegments.isNotEmpty ? uri.pathSegments.first : '');
  return raw.toLowerCase();
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
