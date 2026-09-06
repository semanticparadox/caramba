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

  /// Автоподбор, запущенный ПОВТОРНО из приложения (Home, Настройки, Серверы).
  ///
  /// Живёт вне вкладки настроек, хотя путь на неё и похож: экран открывается с
  /// трёх разных мест, и «Назад» с него обязано возвращать туда, откуда пришли,
  /// а не в настройки просто потому, что так называется путь. Вложенным в
  /// ветку настроек он именно это и делал: человек уходил с Главной, а
  /// возвращался в Настройки — и следующее «Назад» закрывало приложение.
  static const settingsAutotune = '/settings/autotune';

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

  /// Тарифы оператора: срок, цена и способ оплаты. Полноэкранный поверх шелла
  /// (открывается из профиля, из карточки отказа и с экрана подключения по
  /// ссылке), а не лист: карточек бывает несколько, и у каждой свой выбор срока.
  static const plans = '/plans';

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

  /// Экраны, которые открываются ПОВЕРХ приложения, а не вместо него.
  ///
  /// ЗАЧЕМ ЭТОТ СПИСОК ВООБЩЕ ЕСТЬ. `context.go` в go_router не «переходит», а
  /// ЗАМЕНЯЕТ стек целиком: после `go('/protocol')` в корневом навигаторе
  /// остаётся ровно одна страница, шелл из-под неё исчезает, и системная
  /// кнопка «Назад» закрывает приложение — а вместе с приложением умирал и
  /// туннель. Ровно это и снял верификатор на «Типе подключения», «Улучшениях»
  /// и логине. Экраны при этом писались в расчёте на стек: почти каждый из них
  /// закрывается через `if (context.canPop()) context.pop()` и падает в
  /// запасное `go(...)` только потому, что попадать было некуда.
  ///
  /// Поэтому решение принимает роутер, а не двадцать мест вызова: маршрут из
  /// этого списка открывается push'ем (см. `CarambaRouter.go`), и «Назад»
  /// возвращает туда, откуда пришли. Список закрыт «вниз»: подмаршруты
  /// (`/tickets/12`, `/connections/import`) наследуют свойство от родителя.
  ///
  /// Гейтовые экраны (сплеш, `/enroll`, `/connect`, первый `/autotune`) сюда НЕ
  /// входят намеренно: они не лежат поверх приложения, они его заменяют, и
  /// стека под ними нет по смыслу. `/login` входит: из шелла на него уходят по
  /// своей воле («Войти или подключить панель»), и возвращаться оттуда есть
  /// куда.
  static const Set<String> overlays = <String>{
    protocol,
    splitTunnel,
    relay,
    connections,
    csmOperator,
    csmDocuments,
    csmTransport,
    csmDisclosure,
    plans,
    referrals,
    partner,
    notifications,
    tickets,
    settingsAutotune,
    login,
  };

  /// Лежит ли [location] поверх приложения. Query-строка отбрасывается: она
  /// приносит параметры экрана (`?url=`, `?link=`), а не другой маршрут.
  static bool isOverlay(String location) {
    final q = location.indexOf('?');
    final path = q < 0 ? location : location.substring(0, q);
    for (final route in overlays) {
      if (path == route || path.startsWith('$route/')) return true;
    }
    return false;
  }
}
