/// Русские строки десктопной оболочки: сайдбар, трей, строка меню, раздел
/// «Приложение» в настройках.
///
/// Собраны в одном месте потому, что одни и те же слова обязаны совпадать в
/// трёх разных местах сразу: заголовок пункта трея, подпись статуса в сайдбаре
/// и всплывающая подсказка значка. Разъехавшиеся формулировки на десктопе
/// заметны мгновенно, ведь все три видны одновременно.
///
/// Длинные тире здесь запрещены (ANTI-SLOP.md): разделитель это точка,
/// запятая, двоеточие или средняя точка.
library;

import 'package:caramba_client/vpn/vpn_status.dart';

abstract final class DesktopStrings {
  /// Имя приложения в заголовке окна, строке меню и подсказке трея.
  /// Операторский бренд, если он настроен, подставляется в рантайме.
  static const String appName = 'Caramba Connect';

  // --- Статус подключения (сайдбар, подсказка значка) ---

  static const String stageDisconnected = 'Отключено';
  static const String stageConnecting = 'Подключаюсь';
  static const String stageConnected = 'Подключено';
  static const String stageReconnecting = 'Переподключаюсь';
  static const String stageError = 'Ошибка';

  /// Профилей подключения нет вовсе: подключаться некуда, и кнопка предлагает
  /// не «Подключить», а завести подключение.
  static const String stageNoConnections = 'Нет подключений';

  /// Короткая подпись стадии для сайдбара и подсказки значка.
  static String stageLabel(
    VpnStage stage, {
    bool accessBlocked = false,
    bool noConnections = false,
  }) {
    if (noConnections) return stageNoConnections;
    return switch (stage) {
      VpnStage.disconnected => stageDisconnected,
      VpnStage.connecting => stageConnecting,
      VpnStage.reconnecting => stageReconnecting,
      VpnStage.connected => stageConnected,
      VpnStage.error => stageError,
    };
  }

  // --- Кнопки сайдбара ---

  static const String actionConnect = 'Подключить';
  static const String actionDisconnect = 'Отключить';
  static const String actionCancel = 'Отмена';
  static const String actionAddConnection = 'Добавить подключение';

  // --- Навигация сайдбара ---

  static const String navConnection = 'Подключение';
  static const String navServers = 'Серверы';
  static const String navProfile = 'Профиль';
  static const String navSettings = 'Настройки';

  // --- Низ сайдбара ---

  static const String copyProxyHint = 'Скопировать адрес прокси';
  static const String proxyCopied = 'Адрес прокси скопирован';

  /// Подпись версии: `v1.0.0 (105)`.
  static String versionLabel(String version) => 'v$version';

  // --- Трей / строка меню ---

  /// Подсказка значка: имя приложения и текущее состояние.
  static String trayTooltip(String state, {String brand = appName}) =>
      '$brand · $state';

  /// Заголовок меню трея: первая, неактивная строка.
  ///
  /// Она отвечает на единственный вопрос, ради которого человек и открывает
  /// это меню: работает сейчас туннель или нет, и через какой узел. Поэтому
  /// причина отказа доезжает сюда целиком, а не подменяется словом «Ошибка».
  static String trayHeadline(
    VpnStage stage, {
    String? node,
    String? errorText,
    bool accessBlocked = false,
    String? blockedReason,
    bool noConnections = false,
  }) {
    if (noConnections) return trayNoConnections;
    switch (stage) {
      case VpnStage.disconnected:
        return stageDisconnected;
      case VpnStage.connecting:
        return 'Подключаюсь…';
      case VpnStage.reconnecting:
        return 'Переподключаюсь…';
      case VpnStage.connected:
        if (accessBlocked) {
          final reason = _trim(blockedReason);
          return reason == null
              ? 'Подключено · доступ закрыт'
              : 'Подключено · доступ закрыт: $reason';
        }
        final where = _trim(node);
        return where == null ? stageConnected : 'Подключено · $where';
      case VpnStage.error:
        final text = _trim(errorText);
        return text == null ? stageError : 'Ошибка: $text';
    }
  }

  /// Меню трея, когда ни одного профиля подключения нет.
  static const String trayNoConnections = 'Подключений нет';

  /// Пункт с адресом прокси: клик кладёт адрес в буфер обмена.
  static String trayProxyItem(String endpoint) =>
      'Прокси $endpoint · копировать';

  static const String trayConnect = actionConnect;
  static const String trayDisconnect = actionDisconnect;
  static const String trayCancelConnect = 'Отмена подключения';
  static const String trayReconnect = 'Подключиться снова';
  static const String trayAddConnection = 'Добавить подключение…';

  static const String trayServersSubmenu = 'Сервер';
  static const String trayServerAuto = 'Авто';
  static const String trayAllServers = 'Все серверы…';

  static const String trayProfilesSubmenu = 'Подключение';

  static String trayOpenWindow({String brand = appName}) => 'Открыть $brand';

  static const String traySettings = 'Настройки…';
  static const String trayQuit = 'Выйти';

  /// Когда туннель поднят, выход его опускает, и пункт обязан это назвать.
  static const String trayQuitAndDisconnect = 'Выйти и отключить VPN';

  /// Задержка узла в подменю выбора сервера: `42 мс`.
  static String trayLatency(int ms) => '$ms мс';

  // --- Строка меню macOS ---

  static const String menuConnection = 'Подключение';
  static const String menuServers = 'Серверы';
  static const String menuSettings = 'Настройки…';

  /// Меню разделов: те же три ветки, что в сайдбаре, и их сочетания ⌘1/2/3.
  ///
  /// ЗАЧЕМ ОТДЕЛЬНОЕ МЕНЮ. На macOS сочетание, объявленное только в
  /// [Shortcuts], до приложения не доходит: ⌘-события система сначала отдаёт
  /// строке меню, и не найдя там пункта, до вида Flutter их уже не доносит.
  /// Проверка на живой сборке это подтвердила: ⌘, и ⌘⇧S (пункты меню)
  /// работали, ⌘1/2/3 (только [Shortcuts]) — нет. Поэтому раскладка разделов
  /// живёт здесь.
  static const String menuView = 'Вид';

  /// Замер задержки. Живой пункт только на экране серверов.
  static const String menuProbe = 'Замерить задержку';

  static const String menuWindow = 'Окно';
  static const String menuCloseWindow = 'Закрыть окно';

  // --- Настройки, раздел «Приложение» ---

  static const String settingsAppSection = 'Приложение';

  static const String launchAtLoginTitle = 'Запускать при входе в систему';

  /// `SMAppService` появился в macOS 13; на 12 переключатель выключен, и
  /// описание объясняет причину, вместо того чтобы молча не срабатывать.
  ///
  /// Показывается ТОЛЬКО когда система действительно отказала (см.
  /// `autostartSupportedProvider`): подпись, висящая на любой macOS, врёт
  /// каждому, у кого версия новее.
  static const String launchAtLoginUnsupportedMac = 'Нужна macOS 13 или новее';

  /// То же самое на Windows и Linux: система автозапуск не приняла.
  static const String launchAtLoginUnsupported =
      'Система не поддерживает автозапуск';

  /// Отказ системы на попытку включить автозапуск: переключатель возвращается
  /// в прежнее положение, а человек узнаёт, что именно не вышло.
  static const String launchAtLoginFailed = 'Не удалось изменить автозапуск';

  /// macOS зарегистрировала заявку, но ждёт разрешения человека.
  static const String launchAtLoginNeedsApproval =
      'Разрешите Caramba Connect в Системных настройках → Основные → Объекты входа';

  /// Настройка одна на два жеста: красную кнопку (⌘W, Alt+F4) и сворачивание
  /// (жёлтая кнопка, ⌘M). Подпись обязана называть оба, иначе человек ждёт
  /// от сворачивания прежнего поведения и теряет окно.
  static const String onWindowCloseTitle = 'При закрытии и сворачивании окна';

  /// На macOS значок живёт в строке меню, на Windows и Linux в трее.
  static String onWindowCloseHide({required bool isMac}) =>
      isMac ? 'Сворачивать в строку меню' : 'Сворачивать в трей';

  static const String onWindowCloseQuit = 'Завершать приложение';

  static const String onLaunchTitle = 'При запуске';
  static const String onLaunchShowWindow = 'Показывать окно';

  static String onLaunchTrayOnly({required bool isMac}) =>
      isMac ? 'Только значок в строке меню' : 'Только значок в трее';

  static const String signOut = 'Выйти из аккаунта';

  /// Пустая строка это отсутствие значения, а не значение: подставлять её в
  /// заголовок значит показать висящий разделитель.
  static String? _trim(String? value) {
    final text = value?.trim();
    if (text == null || text.isEmpty) return null;
    return text.length <= _maxHeadlineDetail
        ? text
        : '${text.substring(0, _maxHeadlineDetail - 1).trimRight()}…';
  }

  /// Меню строки состояния узкое: длинная причина отказа в нём обрезается
  /// системой в непредсказуемом месте, поэтому режем сами.
  static const int _maxHeadlineDetail = 48;
}
