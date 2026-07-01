/// Платформенные бренд-константы и матрица лицензионных тиров.
///
/// Бренд по умолчанию — нейтральный («Caramba Connect»). Это операторская
/// настройка уровня сборки/тенанта, а не жёстко зашитое имя: переопределяется
/// через `--dart-define=CARAMBA_BRAND_NAME=...`. Значения exarobot живут только
/// как дефолты тенанта №1 (kTenant1*), не как платформенный дефолт.
library;

/// Имя бренда, показываемое свежему пользователю по умолчанию.
/// Нейтральное платформенное имя; per-tenant переопределяется оператором.
const String kBrandName = String.fromEnvironment(
  'CARAMBA_BRAND_NAME',
  defaultValue: 'Caramba Connect',
);

/// URL логотипа бренда. Пусто по умолчанию — UI использует текстовый wordmark
/// (анти-слоп: без картинки-логотипа, без градиента, без свечения).
const String kBrandLogoUrl = String.fromEnvironment(
  'CARAMBA_BRAND_LOGO_URL',
  defaultValue: '',
);

/// URL поддержки бренда. Пусто по умолчанию (per-tenant настройка оператора).
const String kBrandSupportUrl = String.fromEnvironment(
  'CARAMBA_BRAND_SUPPORT_URL',
  defaultValue: '',
);

/// Deep-link бота бренда. Пусто по умолчанию (per-tenant настройка оператора).
const String kBrandBotUrl = String.fromEnvironment(
  'CARAMBA_BRAND_BOT_URL',
  defaultValue: '',
);

// ---------------------------------------------------------------------------
// Дефолты тенанта №1 (exarobot). НЕ платформенный дефолт — именованные
// константы тенанта №1, на которые опираются dart-define оверрайды деплоя.
// ---------------------------------------------------------------------------

/// URL панели тенанта №1. Совпадает с дефолтом `kApiBaseUrl` в api_client.dart.
const String kTenant1PanelUrl = 'https://exarobot.top';

/// Username бота тенанта №1 без `@`. Совпадает с дефолтом `CARAMBA_BOT_USERNAME`.
const String kTenant1BotUsername = 'exarobot';

/// Deep-link бота тенанта №1 в Telegram.
const String kTenant1BotUrl = 'https://t.me/$kTenant1BotUsername';

// ---------------------------------------------------------------------------
// Матрица лицензионных тиров. Статические дефолты, зеркалят контракт панели
// (тир приходит от панели). Все числа держим в одном файле для удобства правки.
// ---------------------------------------------------------------------------

/// Лицензионный тир инстанса панели.
enum LicenseTier { free, pro }

/// Конфигурация одного лицензионного тира.
class LicenseTierConfig {
  /// Максимум нод.
  final int maxNodes;

  /// Максимум пользователей.
  final int maxUsers;

  /// Доступен ли биллинг конечных пользователей.
  final bool endUserBilling;

  /// Доступен ли кастомный бренд оператора.
  final bool branding;

  /// Показывается ли реклама апстрима.
  final bool upstreamAds;

  /// Требуется ли ручное подтверждение пользователей.
  final bool manualApproval;

  const LicenseTierConfig({
    required this.maxNodes,
    required this.maxUsers,
    required this.endUserBilling,
    required this.branding,
    required this.upstreamAds,
    required this.manualApproval,
  });
}

/// Тир Free: малый лимит, без биллинга/брендинга, с рекламой и ручным аппрувом.
const LicenseTierConfig kLicenseTierFree = LicenseTierConfig(
  maxNodes: 2,
  maxUsers: 100,
  endUserBilling: false,
  branding: false,
  upstreamAds: true,
  manualApproval: true,
);

/// Тир Pro: высокий лимит нод (настраиваемый), биллинг и брендинг, без рекламы.
const LicenseTierConfig kLicenseTierPro = LicenseTierConfig(
  maxNodes: 1000,
  maxUsers: 0,
  endUserBilling: true,
  branding: true,
  upstreamAds: false,
  manualApproval: false,
);

/// Конфигурация по заданному тиру.
LicenseTierConfig licenseTierConfig(LicenseTier tier) {
  switch (tier) {
    case LicenseTier.free:
      return kLicenseTierFree;
    case LicenseTier.pro:
      return kLicenseTierPro;
  }
}
