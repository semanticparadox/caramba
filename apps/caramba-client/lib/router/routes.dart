/// Canonical route paths for the exarobot app. Keep in sync with [routerProvider].
abstract final class AppRoute {
  /// Session-probe gate shown while [AuthStage.unknown] (no protected calls).
  static const splash = '/';

  /// Вход по коду из Telegram-бота.
  static const login = '/login';

  /// Энроллмент по инвайт-коду (deeplink `carambaconnect://enroll`, ручной ввод
  /// или QR). Pre-auth: доступен из unauthenticated. Query: `panel`, `code`.
  static const enroll = '/enroll';

  /// Подключение панели по ссылке `caramba://connect` (диплинк, вставка или
  /// QR). Pre-auth, как и [enroll]: ссылку открывают до всякого аккаунта.
  /// Query: `link` — сырая ссылка целиком.
  static const connect = '/connect';

  /// Автоподбор настроек при первом входе.
  static const autotune = '/autotune';

  // Нижняя навигация: Главная / Серверы / Профиль / Настройки.
  static const home = '/home';
  static const servers = '/servers';
  static const profile = '/profile';
  static const settings = '/settings';

  // Детальные экраны выбора (вне табов, открываются поверх).
  static const protocol = '/protocol';
  static const splitTunnel = '/split-tunnel';

  /// Страна ВХОДА в цепочку (relay). Полноэкранный маршрут, а не нижний лист:
  /// в generic-режиме все его строки видны выключенными с причиной, и листу
  /// такой объём объяснения не по размеру.
  static const relay = '/relay';

  // Экраны проверки CSM/1 (INV-17..INV-21), полноэкранные поверх шелла.

  /// Личность оператора: имя, отпечаток корня, дата и происхождение пина.
  static const csmOperator = '/csm/operator';

  /// Состояние проверки документов в работе.
  static const csmDocuments = '/csm/documents';

  /// Транспортная лестница: все семь ступеней и история попыток.
  static const csmTransport = '/csm/transport';

  /// Что приложение отправляет оператору, с кнопкой копирования.
  static const csmDisclosure = '/csm/disclosure';

  /// Профили подключения (мульти-профиль, полноэкранный поверх шелла).
  static const connections = '/connections';

  /// Импорт подписки в профиль подключения (полноэкранный поверх шелла).
  static const connectionImport = '/connections/import';

  /// Реферальная программа (полноэкранная, поверх шелла).
  static const referrals = '/referrals';

  /// Партнёрский дашборд (полноэкранный, поверх шелла; только для партнёров).
  static const partner = '/partner';

  // Уведомления и поддержка (полноэкранные, поверх шелла).
  static const notifications = '/notifications';
  static const tickets = '/tickets';
  static const newTicket = '/tickets/new';

  /// Путь к деталям тикета по id.
  static String ticket(int id) => '/tickets/$id';
}
