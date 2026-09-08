/// Чистая модель меню значка: состояние приложения на входе, список пунктов
/// на выходе.
///
/// ЗАЧЕМ отдельный файл без плагина и без Riverpod. Меню строки состояния это
/// второй, полноценный интерфейс приложения: при спрятанном окне человек
/// подключается, меняет узел и выходит только через него. Правила там ровно те
/// же, что в сайдбаре (какой глагол показать, что считать «нет подключений»,
/// когда выход обязан назвать себя опускающим туннель), и собранные внутри
/// сервиса они проверялись бы только глазами на живом Маке.
///
/// Здесь нет ни одного вызова платформы, поэтому каждое правило ниже закрыто
/// тестом.
library;

import 'package:flutter/foundation.dart';

import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Сколько машин показывать в подменю «Сервер».
///
/// Полный флот в меню строки состояния нечитаем: список уезжает за край
/// экрана, и выбор в нём хуже, чем на экране «Серверы», куда ведёт последний
/// пункт подменю. Двенадцать это примерно экран меню на ноутбуке.
const int kTrayServerLimit = 12;

/// Ключи пунктов. Клик приходит от системы одним лишь ключом, поэтому строки
/// здесь и в маршрутизаторе кликов обязаны быть одними и теми же.
abstract final class TrayKeys {
  /// Глагол подключения: подключить, отключить, отменить, попробовать снова.
  static const String toggle = 'toggle';

  /// Профилей нет вовсе: пункт ведёт на импорт подключения.
  static const String addConnection = 'add-connection';

  static const String proxy = 'proxy';

  static const String servers = 'servers';
  static const String serverAuto = 'server.auto';
  static const String allServers = 'servers.all';

  static const String profiles = 'profiles';

  static const String openWindow = 'open-window';
  static const String settings = 'settings';
  static const String quit = 'quit';

  /// Ключ узла: префикс плюс ключ машины в инвентаре.
  static const String serverPrefix = 'server:';

  /// Ключ профиля подключения.
  static const String profilePrefix = 'profile:';

  static String server(String nodeKey) => '$serverPrefix$nodeKey';

  static String profile(String id) => '$profilePrefix$id';

  /// Ключ узла из ключа пункта; `null` если пункт не про узел.
  static String? nodeKeyOf(String key) =>
      key.startsWith(serverPrefix) ? key.substring(serverPrefix.length) : null;

  /// Идентификатор профиля из ключа пункта; `null` если пункт не про профиль.
  static String? profileIdOf(String key) => key.startsWith(profilePrefix)
      ? key.substring(profilePrefix.length)
      : null;
}

/// Машина в подменю «Сервер».
@immutable
class TrayExitItem {
  /// Ключ узла, которым закрепляется выбор (`ExitNode.key`).
  final String key;

  /// ISO-2 страны; пусто, если источник её не назвал.
  final String code;

  /// Имя машины, уже разведённое с тёзками.
  final String title;

  /// Задержка в мс: `null` не мерили, отрицательное таймаут.
  final int? latencyMs;

  final bool available;

  final bool selected;

  const TrayExitItem({
    required this.key,
    required this.code,
    required this.title,
    this.latencyMs,
    this.available = true,
    this.selected = false,
  });
}

/// Профиль подключения в подменю «Подключение».
@immutable
class TrayProfileItem {
  final String id;
  final String name;
  final bool active;

  const TrayProfileItem({
    required this.id,
    required this.name,
    required this.active,
  });
}

/// Всё, что меню знает о приложении на момент сборки.
@immutable
class TrayMenuInput {
  final VpnStage stage;

  /// Причина отказа от ядра (`VpnStatus.detail`) для заголовка «Ошибка: ...».
  final String? detail;

  /// Туннель поднят, а доступа нет. Заголовок обязан сказать это словами:
  /// «подключено» в таком состоянии читается как обещание, которого нет.
  final bool accessBlocked;

  /// Короткая причина отказа доступа (`AccessState.shortReason`).
  final String? blockedReason;

  /// Узел, через который идёт трафик; `null` вне сессии.
  final String? activeNodeName;

  /// Адрес локального прокси; `null` в tun-режиме и вне сессии.
  final String? proxyEndpoint;

  /// Ни одного профиля подключения не заведено.
  final bool noConnections;

  final List<TrayExitItem> exits;

  /// Пин снят: узел выбирает ядро.
  final bool autoSelected;

  final List<TrayProfileItem> profiles;

  const TrayMenuInput({
    this.stage = VpnStage.disconnected,
    this.detail,
    this.accessBlocked = false,
    this.blockedReason,
    this.activeNodeName,
    this.proxyEndpoint,
    this.noConnections = false,
    this.exits = const <TrayExitItem>[],
    this.autoSelected = true,
    this.profiles = const <TrayProfileItem>[],
  });

  /// Туннель поднят или поднимается: выход обязан сначала его опустить.
  bool get tunnelIsUp =>
      stage == VpnStage.connected ||
      stage == VpnStage.connecting ||
      stage == VpnStage.reconnecting;
}

/// Пункт меню в том виде, в каком его понимает порт.
@immutable
class TrayEntry {
  /// Ключ для маршрутизации клика; `null` у заголовка и разделителя.
  final String? key;

  final String label;

  final bool enabled;

  /// Отметка выбора; `null` означает «пункт не про выбор», а не «не выбран»:
  /// это разные вещи, и от них зависит сам тип пункта в системном меню.
  final bool? checked;

  final List<TrayEntry> children;

  final bool separator;

  const TrayEntry({
    required this.label,
    this.key,
    this.enabled = true,
    this.checked,
    this.children = const <TrayEntry>[],
    this.separator = false,
  });

  const TrayEntry.separator()
    : key = null,
      label = '',
      enabled = false,
      checked = null,
      children = const <TrayEntry>[],
      separator = true;
}

/// Готовое меню.
@immutable
class TrayMenuSpec {
  final List<TrayEntry> entries;

  const TrayMenuSpec(this.entries);

  /// Пункт по ключу на любой глубине. Нужен тестам и отладке: искать его
  /// перебором по вложенным спискам в каждом ожидании было бы шумно.
  TrayEntry? find(String key) {
    for (final e in entries) {
      if (e.key == key) return e;
      final nested = TrayMenuSpec(e.children).find(key);
      if (nested != null) return nested;
    }
    return null;
  }
}

/// Строка машины в подменю: код страны, имя, задержка.
///
/// Разделителем стоят два пробела, а не точка: системное меню и так рисует
/// пункт одной строкой, а лишняя пунктуация в нём читается как часть имени.
String trayExitLabel(TrayExitItem exit) {
  final head = exit.code.isEmpty ? exit.title : '${exit.code}  ${exit.title}';
  final ms = exit.latencyMs;
  // Отрицательное это таймаут, а не «минус миллисекунды»: числа в строке в
  // таком случае быть не должно вовсе.
  if (ms == null || ms < 0) return head;
  return '$head  ${DesktopStrings.trayLatency(ms)}';
}

/// Собирает меню значка.
TrayMenuSpec buildTrayMenu(TrayMenuInput input) {
  final entries = <TrayEntry>[
    // Заголовок: ради него меню и открывают. Он неактивен, потому что это
    // ответ, а не действие.
    TrayEntry(
      label: DesktopStrings.trayHeadline(
        input.stage,
        node: input.activeNodeName,
        errorText: input.detail,
        accessBlocked: input.accessBlocked,
        blockedReason: input.blockedReason,
        noConnections: input.noConnections,
      ),
      enabled: false,
    ),
    ..._proxyEntries(input),
    const TrayEntry.separator(),
    _verbEntry(input),
    ..._serverEntries(input),
    ..._profileEntries(input),
    const TrayEntry.separator(),
    TrayEntry(key: TrayKeys.openWindow, label: DesktopStrings.trayOpenWindow()),
    const TrayEntry(key: TrayKeys.settings, label: DesktopStrings.traySettings),
    const TrayEntry.separator(),
    TrayEntry(
      key: TrayKeys.quit,
      // Выход при поднятом туннеле опускает его, и пункт обязан это назвать:
      // молчаливое «Выйти» здесь означало бы для человека, что защита
      // останется, пока он не вернётся.
      label: input.tunnelIsUp
          ? DesktopStrings.trayQuitAndDisconnect
          : DesktopStrings.trayQuit,
    ),
  ];
  return TrayMenuSpec(entries);
}

/// Адрес прокси показываем только когда он есть и когда он работает.
///
/// В tun-режиме и вне сессии этого пункта нет: скопированный адрес, на котором
/// никто не слушает, хуже отсутствующего.
List<TrayEntry> _proxyEntries(TrayMenuInput input) {
  final endpoint = input.proxyEndpoint;
  if (input.stage != VpnStage.connected || endpoint == null) {
    return const <TrayEntry>[];
  }
  return <TrayEntry>[
    TrayEntry(
      key: TrayKeys.proxy,
      label: DesktopStrings.trayProxyItem(endpoint),
    ),
  ];
}

/// Главное действие меню.
///
/// Глагол один на все стадии, потому что и кнопка в сайдбаре одна: два разных
/// слова для одного и того же нажатия читались бы как два разных действия.
TrayEntry _verbEntry(TrayMenuInput input) {
  if (input.noConnections) {
    return const TrayEntry(
      key: TrayKeys.addConnection,
      label: DesktopStrings.trayAddConnection,
    );
  }
  final label = switch (input.stage) {
    VpnStage.connected => DesktopStrings.trayDisconnect,
    // Отмена, а не «Отключить»: туннеля ещё нет, и отключать нечего.
    VpnStage.connecting => DesktopStrings.trayCancelConnect,
    VpnStage.reconnecting => DesktopStrings.trayCancelConnect,
    VpnStage.error => DesktopStrings.trayReconnect,
    VpnStage.disconnected => DesktopStrings.trayConnect,
  };
  return TrayEntry(key: TrayKeys.toggle, label: label);
}

/// Подменю «Сервер»: «Авто», лучшие машины, дверь на полный экран выбора.
///
/// Недоступные машины сюда не попадают вовсе. В списке на экране они остаются
/// с причиной рядом, но в меню строки состояния причину показать негде, и
/// нажимаемый пункт, который ничего не делает, был бы хуже отсутствующего.
List<TrayEntry> _serverEntries(TrayMenuInput input) {
  // Подключаться некуда: выбирать узел тем более не из чего.
  if (input.noConnections) return const <TrayEntry>[];

  final available = <TrayExitItem>[
    for (final e in input.exits)
      if (e.available) e,
  ]..sort(_byLatencyThenTitle);

  final shown = available.take(kTrayServerLimit);

  return <TrayEntry>[
    TrayEntry(
      key: TrayKeys.servers,
      label: DesktopStrings.trayServersSubmenu,
      children: <TrayEntry>[
        TrayEntry(
          key: TrayKeys.serverAuto,
          label: DesktopStrings.trayServerAuto,
          checked: input.autoSelected,
        ),
        for (final e in shown)
          TrayEntry(
            key: TrayKeys.server(e.key),
            label: trayExitLabel(e),
            checked: e.selected,
          ),
        const TrayEntry.separator(),
        const TrayEntry(
          key: TrayKeys.allServers,
          label: DesktopStrings.trayAllServers,
        ),
      ],
    ),
  ];
}

/// Подменю «Подключение» появляется только когда выбирать есть из чего.
///
/// С единственным профилем это пункт, который ничего не меняет: он занимал бы
/// строку меню и обещал выбор, которого нет.
List<TrayEntry> _profileEntries(TrayMenuInput input) {
  if (input.profiles.length < 2) return const <TrayEntry>[];
  return <TrayEntry>[
    TrayEntry(
      key: TrayKeys.profiles,
      label: DesktopStrings.trayProfilesSubmenu,
      children: <TrayEntry>[
        for (final p in input.profiles)
          TrayEntry(
            key: TrayKeys.profile(p.id),
            label: p.name,
            checked: p.active,
          ),
      ],
    ),
  ];
}

/// Порядок машин: по задержке, при равной по имени.
///
/// Правило то же, что держит список на экране: неизвестная и отрицательная
/// (таймаут) задержка уходит вниз. «Не мерили» это не «быстрее всех», и в меню,
/// где показаны только первые двенадцать, ошибка порядка стоит дороже: неверно
/// отсортированная машина не уезжает вниз, а исчезает.
int _byLatencyThenTitle(TrayExitItem a, TrayExitItem b) {
  final c = _rank(a.latencyMs).compareTo(_rank(b.latencyMs));
  return c != 0 ? c : a.title.compareTo(b.title);
}

int _rank(int? ms) => (ms == null || ms < 0) ? 1 << 30 : ms;
