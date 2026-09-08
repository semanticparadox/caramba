/// Значок в строке меню (в трее): второй интерфейс приложения.
///
/// ЗАЧЕМ он вообще. На десктопе окно закрывается, а туннель остаётся жить:
/// красная кнопка прячет окно, процесс продолжается. С этого момента значок
/// становится единственным, что от приложения видно, и через него человек
/// обязан уметь всё то же, что и в окне: понять состояние, подключиться или
/// отключиться, сменить узел, выйти с опусканием туннеля.
///
/// Здесь живут ТОЛЬКО подписки и маршрутизация кликов. Что показать, решает
/// чистая модель ([buildTrayMenu], [trayIconPath]), а как это показать, знает
/// порт ([TrayPort]). Поэтому ни один вызов плагина в тест не попадает, а
/// каждое правило меню закрыто тестом модели.
library;

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/data/models/exit_location.dart';
import 'package:caramba_client/desktop/desktop_strings.dart';
import 'package:caramba_client/desktop/ports/tray_port.dart';
import 'package:caramba_client/desktop/tray_icons.dart';
import 'package:caramba_client/desktop/tray_menu_model.dart';
import 'package:caramba_client/desktop/window_service.dart';
import 'package:caramba_client/domain/autopilot/auto_pick.dart'
    show namingOfProxy;
import 'package:caramba_client/domain/autopilot/autopilot_state.dart'
    show fleetFactsProvider;
import 'package:caramba_client/domain/offering/offering.dart' show ExitOffer;
import 'package:caramba_client/domain/offering/offering_providers.dart';
import 'package:caramba_client/features/servers/exit_node_list.dart'
    show sortedExits;
import 'package:caramba_client/features/servers/fleet_alignment.dart';
import 'package:caramba_client/router/app_router.dart' show routerProvider;
import 'package:caramba_client/router/routes.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/exit_inventory_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Пауза перед пересборкой меню.
///
/// Источников у меню шесть, и один жест (подключение) шевелит их все подряд:
/// стадия, инвентарь, предложение, прокси. Без паузы каждое такое движение
/// уходило бы в системное меню отдельным вызовом канала.
const Duration kTrayRebuildDebounce = Duration(milliseconds: 200);

/// Снимает подписку, выданную `listenChanges`.
typedef TrayChangesCanceller = void Function();

/// Значок и его меню: ставит, обновляет, разводит клики по действиям.
class TrayService {
  final TrayPort port;

  final TargetPlatform _platform;
  final TrayMenuInput Function() _readInput;
  final TrayChangesCanceller Function(VoidCallback onChange) _listenChanges;

  final Future<void> Function() _toggleConnection;
  final Future<void> Function(String nodeKey) _selectExit;
  final Future<void> Function() _selectAuto;
  final Future<void> Function(String profileId) _activateProfile;
  final Future<void> Function() _showWindow;
  final Future<void> Function() _toggleWindow;
  final Future<void> Function(String route) _openRoute;
  final Future<void> Function(String text) _copyText;
  final Future<void> Function() _quit;

  final Duration rebuildDebounce;

  TrayPortListener? _listener;
  TrayChangesCanceller? _cancel;
  Timer? _timer;

  /// Что уже стоит на значке. Плагин каждый раз тащит картинку через канал (на
  /// macOS ещё и в base64), а стадия меняется чаще, чем состояние значка:
  /// `connecting` и `reconnecting` рисуются одним файлом.
  String? _iconPath;
  String? _tooltip;

  TrayService({
    required this.port,
    required TargetPlatform platform,
    required TrayMenuInput Function() readInput,
    required TrayChangesCanceller Function(VoidCallback onChange) listenChanges,
    required Future<void> Function() toggleConnection,
    required Future<void> Function(String nodeKey) selectExit,
    required Future<void> Function() selectAuto,
    required Future<void> Function(String profileId) activateProfile,
    required Future<void> Function() showWindow,
    required Future<void> Function() toggleWindow,
    required Future<void> Function(String route) openRoute,
    required Future<void> Function(String text) copyText,
    required Future<void> Function() quitApplication,
    this.rebuildDebounce = kTrayRebuildDebounce,
  }) : _platform = platform,
       _readInput = readInput,
       _listenChanges = listenChanges,
       _toggleConnection = toggleConnection,
       _selectExit = selectExit,
       _selectAuto = selectAuto,
       _activateProfile = activateProfile,
       _showWindow = showWindow,
       _toggleWindow = toggleWindow,
       _openRoute = openRoute,
       _copyText = copyText,
       _quit = quitApplication;

  /// Ставит значок и меню и подписывается на изменения.
  ///
  /// Зовётся ДО показа окна: при `startInTray` окна не будет вовсе, и значок
  /// остаётся единственным следом запуска. Пустая строка меню в этот момент
  /// читается как «приложение не запустилось».
  Future<void> start() async {
    if (_listener != null) return;
    final listener = TrayPortListener(
      onLeftClick: _handleLeftClick,
      onRightClick: _handleRightClick,
      onItemClick: handleItem,
    );
    _listener = listener;
    port.addListener(listener);
    await refresh();
    _cancel = _listenChanges(_scheduleRefresh);
  }

  /// Пересобирает значок, подсказку и меню прямо сейчас.
  Future<void> refresh() async {
    final input = _readInput();

    final path = trayIconPath(
      input.stage,
      accessBlocked: input.accessBlocked,
      noConnections: input.noConnections,
      platform: _platform,
    );
    if (path != _iconPath) {
      _iconPath = path;
      await port.setIcon(path, isTemplate: trayIconIsTemplate(_platform));
    }

    // Подсказка ставится ПОСЛЕ иконки: плагин прикрепляет её к уже созданному
    // значку, и наоборот она не доезжает.
    final tooltip = DesktopStrings.trayTooltip(
      DesktopStrings.stageLabel(
        input.stage,
        accessBlocked: input.accessBlocked,
        noConnections: input.noConnections,
      ),
    );
    if (tooltip != _tooltip) {
      _tooltip = tooltip;
      await port.setToolTip(tooltip);
    }

    await port.setContextMenu(buildTrayMenu(input));
  }

  /// Снимает подписки и убирает значок.
  ///
  /// Порядок важен: значок, снятый после смерти процесса, снять уже некому, и
  /// он висит в строке меню до перезахода в систему.
  Future<void> dispose() async {
    _timer?.cancel();
    _timer = null;
    _cancel?.call();
    _cancel = null;
    final listener = _listener;
    if (listener != null) {
      port.removeListener(listener);
      _listener = null;
    }
    await port.destroy();
  }

  void _scheduleRefresh() {
    _timer?.cancel();
    _timer = Timer(rebuildDebounce, () => unawaited(refresh()));
  }

  /// Левый клик. macOS: меню (там у значка одно действие, и это меню).
  /// Windows: показать или спрятать окно, как ждут от значка в трее.
  /// Linux: событие не приходит, меню показывает сама панель.
  void _handleLeftClick() {
    switch (_platform) {
      case TargetPlatform.windows:
        unawaited(_toggleWindow());
      case TargetPlatform.macOS:
        unawaited(port.popUpContextMenu());
      default:
        break;
    }
  }

  void _handleRightClick() {
    if (_platform == TargetPlatform.linux) return;
    unawaited(port.popUpContextMenu());
  }

  /// Разводит клик по пункту. Публичный, потому что это и есть поведение
  /// значка: тест жмёт пункт по ключу, как это делает система.
  @visibleForTesting
  void handleItem(String key) {
    final nodeKey = TrayKeys.nodeKeyOf(key);
    if (nodeKey != null) {
      unawaited(_selectExit(nodeKey));
      return;
    }
    final profileId = TrayKeys.profileIdOf(key);
    if (profileId != null) {
      unawaited(_activateProfile(profileId));
      return;
    }
    switch (key) {
      case TrayKeys.toggle:
        unawaited(_toggleConnection());
      case TrayKeys.serverAuto:
        unawaited(_selectAuto());
      case TrayKeys.proxy:
        unawaited(_copyProxy());
      case TrayKeys.openWindow:
        unawaited(_showWindow());
      case TrayKeys.settings:
        unawaited(_openRoute(AppRoute.settings));
      case TrayKeys.allServers:
        unawaited(_openRoute(AppRoute.servers));
      case TrayKeys.addConnection:
        unawaited(_openRoute(AppRoute.connectionImport));
      case TrayKeys.quit:
        unawaited(_quit());
      default:
        // Заголовок состояния и родители подменю кликов не несут.
        break;
    }
  }

  /// Адрес берём заново, а не из последнего меню: между показом меню и
  /// нажатием туннель мог перезайти на другой порт.
  Future<void> _copyProxy() async {
    final endpoint = _readInput().proxyEndpoint;
    if (endpoint == null) return;
    await _copyText(endpoint);
  }
}

/// Снимок состояния для меню.
///
/// Отдельным провайдером, а не чтением внутри сервиса, по двум причинам.
/// Первая: Riverpod сам пересчитает его при движении ЛЮБОГО из шести
/// источников, и сервису достаётся одна подписка вместо шести разъезжающихся.
/// Вторая: снимок читается тестом напрямую, поэтому «какое имя узла попадёт в
/// заголовок» проверяется без значка и без плагина.
final trayMenuInputProvider = Provider<TrayMenuInput>((ref) {
  final status = ref.watch(vpnProvider);
  final profiles = ref.watch(connectionProfilesProvider);
  final inventory = ref.watch(exitInventoryProvider);

  // Доступ: живой отказ старше панельного снимка, и порядок этот держит сам
  // [subscriptionAccessProvider]. Второй раз его здесь не пересобираем.
  final access = ref.watch(subscriptionAccessProvider);
  final blocked = access != null && access.isBlocked;

  return TrayMenuInput(
    stage: status.stage,
    detail: status.detail,
    // Туннель поднят, а доступа нет: только в этом сочетании слово «подключено»
    // становится ложью, и заголовок обязан назвать причину.
    accessBlocked: status.isConnected && blocked,
    blockedReason: access?.shortReason,
    activeNodeName: _activeNodeName(ref, status),
    proxyEndpoint: ref.watch(proxyEndpointProvider),
    // Профили ещё читаются: пустой список в этот момент означает «не знаем», а
    // не «подключений нет», и мигать этим в строке меню незачем.
    noConnections: !profiles.loading && profiles.profiles.isEmpty,
    exits: _trayExits(ref, inventory, blocked: blocked),
    autoSelected: inventory.selectedNodeKey == null,
    profiles: <TrayProfileItem>[
      for (final p in profiles.profiles)
        TrayProfileItem(
          id: p.id,
          name: p.displayName,
          active: p.id == profiles.active?.id,
        ),
    ],
  );
});

/// Имя узла для заголовка меню.
///
/// Спрашиваем то, что НА ПРОВОДЕ: узел, на который встал селектор ядра. Пин
/// профиля тут не годится, потому что автоподбор мог уйти в другую страну, и
/// заголовок назвал бы чужой узел действующим.
String? _activeNodeName(Ref ref, VpnStatus status) {
  final proxy = status.activeProxy;
  if (proxy != null && proxy.isNotEmpty) {
    return namingOfProxy(proxy, ref.watch(fleetFactsProvider)).title;
  }
  return status.server?.name;
}

/// Машины для подменю «Сервер».
///
/// Строка это МАШИНА, а не прокси: в теле подписки узла как сущности нет, и
/// восемь инбаундов одной машины читались бы как восемь серверов. Машины берём
/// из предложения, ключ выбора из инвентаря, порядок и разведение тёзок из
/// того же [sortedExits], что держит экран «Серверы»: разъехавшись, меню и
/// экран называли бы один флот по-разному.
List<TrayExitItem> _trayExits(
  Ref ref,
  ExitInventory inventory, {
  required bool blocked,
}) {
  final selectedKey = inventory.selectedNodeKey;
  final offering = ref.watch(offeringProvider);
  // Половины описывают разные источники: показать машины одного со списком
  // выбора другого значит отправить нажатие не туда.
  final offers = fleetSourcesAgree(inventory.source, offering.source)
      ? offering.exits
      : const <ExitOffer>[];

  // Предложения нет: строкой остаётся узел инвентаря, беднее, но правдиво.
  if (offers.isEmpty) {
    return <TrayExitItem>[
      for (final n in inventory.nodes)
        TrayExitItem(
          key: n.key,
          code: n.countryCode,
          title: n.name,
          latencyMs: n.latency.ms,
          available: n.isAvailable && !blocked,
          selected: n.key == selectedKey,
        ),
    ];
  }

  final sorted = sortedExits(offers, inventory.nodes);
  final titles = disambiguateTitles(
    sorted.map(machineTitleOf).toList(growable: false),
  );
  final items = <TrayExitItem>[];
  for (var i = 0; i < sorted.length; i++) {
    final exit = sorted[i];
    final node = nodeForExit(exit, inventory.nodes);
    // Машины нет в списке выбора: закреплять её нечем, и пункт был бы кнопкой
    // без действия.
    if (node == null) continue;
    items.add(
      TrayExitItem(
        key: node.key,
        code: exit.countryCode,
        title: titles[i],
        latencyMs: node.latency.ms ?? exit.pingMs,
        // Отказ подписки накрывает весь флот разом: причину в системном меню
        // показать негде, поэтому недоступные машины из него уходят.
        available: exit.isAvailable && node.isAvailable && !blocked,
        selected: exitHoldsKey(exit, selectedKey),
      ),
    );
  }
  return items;
}

/// Живой порт значка. Отдельным провайдером, чтобы тест подменял его фейком,
/// не трогая проводку [trayServiceProvider].
final trayPortProvider = Provider<TrayPort>((ref) => TrayManagerPort());

/// Значок приложения. Создаётся один на процесс; ставит его `start()`
/// (его зовёт десктопный хост сервисов ДО показа окна), убирает `dispose()`.
final trayServiceProvider = Provider<TrayService>((ref) {
  final service = TrayService(
    port: ref.watch(trayPortProvider),
    platform: defaultTargetPlatform,
    readInput: () => ref.read(trayMenuInputProvider),
    // Одна подписка на весь снимок: Riverpod пересчитает его сам, когда
    // шевельнётся любой из источников.
    listenChanges: (onChange) {
      final sub = ref.container.listen<TrayMenuInput>(
        trayMenuInputProvider,
        (_, __) => onChange(),
      );
      return sub.close;
    },
    toggleConnection: () => ref.read(vpnProvider.notifier).toggle(),
    selectExit: (key) async {
      final node = _nodeByKey(ref, key);
      if (node == null) return;
      await ref.read(exitSelectionControllerProvider).selectNode(node);
    },
    selectAuto: () async {
      await ref.read(exitSelectionControllerProvider).selectCountry(null);
    },
    activateProfile: (id) =>
        ref.read(connectionProfilesProvider.notifier).activate(id),
    showWindow: () => ref.read(windowServiceProvider).show(),
    toggleWindow: () => ref.read(windowServiceProvider).toggle(),
    // Сначала окно, потом маршрут: переход в спрятанном окне человек бы не
    // увидел, и пункт меню выглядел бы ничего не делающим.
    openRoute: (route) async {
      await ref.read(windowServiceProvider).show();
      ref.read(routerProvider).go(route);
    },
    copyText: (text) => Clipboard.setData(ClipboardData(text: text)),
    quitApplication: () => ref.read(windowServiceProvider).quitApplication(),
  );

  // Значок гасим ПЕРЕД выходом. Крючок ставится здесь, а не в хосте сборки:
  // это часть контракта самого трея, и забыть его при сборке нельзя.
  ref.read(windowServiceProvider).beforeExit = service.dispose;

  ref.onDispose(() => unawaited(service.dispose()));
  return service;
});

/// Узел инвентаря по ключу; `null` если узел успел уйти из выдачи между
/// показом меню и нажатием.
ExitNode? _nodeByKey(Ref ref, String key) {
  for (final n in ref.read(exitInventoryProvider).nodes) {
    if (n.key == key) return n;
  }
  return null;
}
