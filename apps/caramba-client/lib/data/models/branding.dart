import 'dart:ui' show Color;

/// Брендинг инстанса панели (P3, contract A/E).
///
/// Источник истины — `GET /api/v2/app/branding` (ПУБЛИЧНЫЙ, без JWT; нужен ещё
/// до логина). Контракт панели:
/// `{ enabled, brand_name, logo_url, accent_hex, support_url, bot_url,
///    upstream_ads }`.
///
/// Гейт тира на стороне панели:
///   * Free / бренд не настроен → `enabled=false`, `upstream_ads=true`
///     (клиент рисует дефолтный вид Caramba Connect + powered-by/upsell);
///   * Pro с настроенным брендом → `enabled=true`, поля `brand_*` заполнены,
///     `upstream_ads=false`.
///
/// АНТИ-СЛОП: `accent_hex` — это операторский акцент, НЕ статус-цвет. Применять
/// его можно ТОЛЬКО к нейтральным accent-токенам и лишь после
/// [brandAccentColor] (отклоняет purple/violet/indigo и всё, что читается как
/// статус connected/connecting/error). Цвет статуса бренд НЕ трогает.
class Branding {
  /// Оператор настроил собственный бренд И тир это разрешает.
  final bool enabled;

  /// Имя бренда. Пусто на Free/ненастроенном — клиент берёт `kBrandName`.
  final String brandName;

  /// URL логотипа. Пусто по умолчанию — UI рисует текстовый wordmark.
  final String logoUrl;

  /// Операторский акцент `#RRGGBB`. Пусто — нейтральный дефолт-accent.
  /// СЫРОЕ значение; перед применением гонится через [brandAccentColor].
  final String accentHex;

  /// URL поддержки оператора. Пусто — раздел скрыт.
  final String supportUrl;

  /// Deep-link бота оператора. Пусто — раздел скрыт.
  final String botUrl;

  /// Показывать ли powered-by/upsell-блок (Free/ненастроенный или !enabled).
  final bool upstreamAds;

  const Branding({
    this.enabled = false,
    this.brandName = '',
    this.logoUrl = '',
    this.accentHex = '',
    this.supportUrl = '',
    this.botUrl = '',
    this.upstreamAds = true,
  });

  /// Дефолт Caramba Connect: бренд выключен, upsell включён. Используется как
  /// безопасный фолбэк, пока branding ещё не загружен или запрос упал.
  static const Branding fallback = Branding();

  bool get hasLogo => logoUrl.trim().isNotEmpty;
  bool get hasSupport => supportUrl.trim().isNotEmpty;
  bool get hasBot => botUrl.trim().isNotEmpty;

  /// Имя бренда для показа, или [orDefault] (обычно `kBrandName`), если оператор
  /// бренд не настроил / тир не разрешает.
  String displayName(String orDefault) =>
      (enabled && brandName.trim().isNotEmpty) ? brandName.trim() : orDefault;

  /// Валидный операторский accent-цвет ИЛИ `null`, если бренд выключен/акцент
  /// пуст/отклонён анти-слоп фильтром. `null` → тема остаётся на нейтральном
  /// дефолт-accent (см. [parseBrandAccent]). Никогда не возвращает hue из
  /// запрещённой полосы (purple/violet/indigo) и ничего, что читается как статус.
  Color? get brandAccentColor {
    if (!enabled) return null;
    return parseBrandAccent(accentHex);
  }

  factory Branding.fromJson(Map<String, dynamic> json) => Branding(
    enabled: json['enabled'] == true,
    brandName: (json['brand_name'] as String?)?.trim() ?? '',
    logoUrl: (json['logo_url'] as String?)?.trim() ?? '',
    accentHex: (json['accent_hex'] as String?)?.trim() ?? '',
    supportUrl: (json['support_url'] as String?)?.trim() ?? '',
    botUrl: (json['bot_url'] as String?)?.trim() ?? '',
    // Дефолт upstream_ads = true: если поля нет, считаем что upsell нужен.
    upstreamAds: json['upstream_ads'] == null
        ? true
        : json['upstream_ads'] == true,
  );

  /// Сериализация для кэша в `ConnectionProfile.brandingCache`. Ключи совпадают
  /// с контрактом панели, чтобы [Branding.fromJson] читал и сетевой ответ, и кэш.
  Map<String, dynamic> toJson() => {
    'enabled': enabled,
    'brand_name': brandName,
    'logo_url': logoUrl,
    'accent_hex': accentHex,
    'support_url': supportUrl,
    'bot_url': botUrl,
    'upstream_ads': upstreamAds,
  };

  @override
  bool operator ==(Object other) =>
      other is Branding &&
      other.enabled == enabled &&
      other.brandName == brandName &&
      other.logoUrl == logoUrl &&
      other.accentHex == accentHex &&
      other.supportUrl == supportUrl &&
      other.botUrl == botUrl &&
      other.upstreamAds == upstreamAds;

  @override
  int get hashCode => Object.hash(
    enabled,
    brandName,
    logoUrl,
    accentHex,
    supportUrl,
    botUrl,
    upstreamAds,
  );
}

/// Разбирает и КЛАМПИТ операторский accent-hex под анти-слоп.
///
/// Принимает `#RRGGBB` / `#RGB` / `RRGGBB` (опциональный `#`, регистр любой).
/// Возвращает непрозрачный [Color] ТОЛЬКО если он:
///   * валидный 6/3-значный hex (никаких градиентов/именованных/функций);
///   * НЕ попадает в запрещённую hue-полосу indigo/purple/violet
///     (примерно 240..300° на цветовом круге);
///   * НЕ читается как один из статус-цветов (зелёный connected, янтарный
///     connecting, красный error) — те зарезервированы под язык соединения.
/// Иначе возвращает `null` — вызывающий падает на нейтральный дефолт-accent.
///
/// Это первая из двух линий обороны (вторая — валидация в боте при записи).
Color? parseBrandAccent(String? raw) {
  if (raw == null) return null;
  var hex = raw.trim();
  if (hex.isEmpty) return null;
  if (hex.startsWith('#')) hex = hex.substring(1);

  // Только чистый hex: длина 3 или 6, все символы [0-9a-fA-F]. Любой пробел,
  // запятая, скобка, "gradient", rgb(...) и т.п. => отклонить.
  if (hex.length == 3) {
    hex = hex.split('').map((ch) => '$ch$ch').join();
  }
  if (hex.length != 6) return null;
  final value = int.tryParse(hex, radix: 16);
  if (value == null) return null;

  final r = (value >> 16) & 0xFF;
  final g = (value >> 8) & 0xFF;
  final b = value & 0xFF;

  if (_isPurpleVioletIndigo(r, g, b)) return null;
  if (_readsAsStatus(r, g, b)) return null;

  return Color(0xFF000000 | value);
}

/// Запрещённая hue-полоса «лила»: purple / violet / indigo. Считаем HSV-hue и
/// режем диапазон ~250..300°, но только когда цвет достаточно насыщен и не
/// почти-чёрный/почти-белый (нейтрали с микро-оттенком в этой зоне пропускаем,
/// чтобы near-white/near-black accent не ложно-срабатывал).
bool _isPurpleVioletIndigo(int r, int g, int b) {
  final hsv = _hsv(r, g, b);
  final h = hsv.$1;
  final s = hsv.$2;
  final v = hsv.$3;
  if (s < 0.18) return false; // почти нейтральный — не лила
  if (v < 0.08) return false; // почти чёрный
  // Нижняя граница 240 ловит канонический индиго (Tailwind indigo-500
  // #6366F1 h≈239, #4F46E5 h≈243, slateblue h≈248); чистый/azure синий
  // (~210..230, #2563EB h≈217) проходит. Совпадает с floor бота (HSL 240).
  return h >= 240 && h <= 300;
}

/// Цвет «читается как статус», если его hue лежит в зелёной (connected),
/// янтарной/оранжевой (connecting) или красной (error) полосе при заметной
/// насыщенности. Статус-язык зарезервирован — бренд туда не лезет.
bool _readsAsStatus(int r, int g, int b) {
  final hsv = _hsv(r, g, b);
  final h = hsv.$1;
  final s = hsv.$2;
  final v = hsv.$3;
  if (s < 0.22 || v < 0.10) return false; // слишком бледный/тёмный — нейтраль
  // Красный (error): 0..18° и 342..360°.
  if (h <= 18 || h >= 342) return true;
  // Янтарь/оранж (connecting): 30..50°.
  if (h >= 30 && h <= 50) return true;
  // Зелёный (connected): 95..160°.
  if (h >= 95 && h <= 160) return true;
  return false;
}

/// HSV из 0..255 RGB. Возвращает (hue 0..360, sat 0..1, val 0..1).
(double, double, double) _hsv(int r, int g, int b) {
  final rf = r / 255.0;
  final gf = g / 255.0;
  final bf = b / 255.0;
  final maxC = [rf, gf, bf].reduce((a, x) => a > x ? a : x);
  final minC = [rf, gf, bf].reduce((a, x) => a < x ? a : x);
  final delta = maxC - minC;

  double h;
  if (delta == 0) {
    h = 0;
  } else if (maxC == rf) {
    h = 60 * (((gf - bf) / delta) % 6);
  } else if (maxC == gf) {
    h = 60 * (((bf - rf) / delta) + 2);
  } else {
    h = 60 * (((rf - gf) / delta) + 4);
  }
  if (h < 0) h += 360;

  final s = maxC == 0 ? 0.0 : delta / maxC;
  return (h, s, maxC);
}
