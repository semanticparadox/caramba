/// Порт трея: единственная дверь из десктопного слоя в `tray_manager`.
///
/// ЗАЧЕМ порт, а не прямые вызовы плагина. `trayManager` это синглтон поверх
/// метод-канала: в тесте любой его вызов уходит в `MissingPluginException`, а
/// проверять надо ровно то, что вокруг него, то есть какая иконка встала на
/// какой стадии и что случилось после клика по пункту. Порт отделяет решение
/// от канала: [TrayService] знает только этот интерфейс, тест подставляет
/// [FakeTrayPort], а живой [TrayManagerPort] остаётся переходником без единой
/// ветки поведения.
///
/// Меню сюда приходит ОПИСАНИЕМ ([TrayMenuSpec]), а не плагинным `Menu`:
/// правила сборки пунктов живут в чистой модели и потому проверяются без
/// плагина вовсе.
library;

import 'package:flutter/foundation.dart';
import 'package:tray_manager/tray_manager.dart';

import 'package:caramba_client/desktop/tray_menu_model.dart';

/// Набор колбэков на события значка.
///
/// Один объект, а не три подписки: снимать их надо всем скопом, а разъехавшийся
/// набор означал бы живой значок, который дёргает уже отпущенный сервис.
@immutable
class TrayPortListener {
  /// Левый клик по значку. На macOS открывает меню, на Windows показывает или
  /// прячет окно, на Linux не приходит вовсе.
  final VoidCallback? onLeftClick;

  final VoidCallback? onRightClick;

  /// Нажат пункт меню с этим ключом ([TrayKeys]).
  final void Function(String key)? onItemClick;

  const TrayPortListener({
    this.onLeftClick,
    this.onRightClick,
    this.onItemClick,
  });
}

/// То, что десктопному слою нужно от значка в строке меню, и ничего сверх.
abstract class TrayPort {
  /// [isTemplate] это macOS: чёрно-прозрачная картинка, которую система сама
  /// перекрашивает под тему и под выделение строки меню.
  Future<void> setIcon(String path, {bool isTemplate = false});

  Future<void> setToolTip(String tooltip);

  Future<void> setContextMenu(TrayMenuSpec spec);

  /// Открыть меню принудительно. На macOS и Windows меню по клику показываем
  /// мы; на Linux его показывает сама панель, и звать это не нужно.
  Future<void> popUpContextMenu();

  /// Убрать значок. Без этого значок мёртвого процесса висит в строке меню до
  /// перезахода в систему.
  Future<void> destroy();

  void addListener(TrayPortListener listener);

  void removeListener(TrayPortListener listener);
}

/// Живая реализация поверх `tray_manager` 0.5.3.
///
/// Здесь нет ни одной ветки по платформе или состоянию: всё это живёт выше и
/// потому проверяется тестом.
class TrayManagerPort implements TrayPort {
  /// Плагин принимает свой `TrayListener`, а наружу мы отдаём собственный тип,
  /// поэтому переходники держим по ключу-подписчику: иначе `removeListener` не
  /// нашёл бы, что именно снимать.
  final Map<TrayPortListener, _ManagerListenerAdapter> _adapters =
      <TrayPortListener, _ManagerListenerAdapter>{};

  @override
  Future<void> setIcon(String path, {bool isTemplate = false}) =>
      trayManager.setIcon(path, isTemplate: isTemplate);

  @override
  Future<void> setToolTip(String tooltip) => trayManager.setToolTip(tooltip);

  @override
  Future<void> setContextMenu(TrayMenuSpec spec) =>
      trayManager.setContextMenu(_menuOf(spec.entries));

  @override
  Future<void> popUpContextMenu() => trayManager.popUpContextMenu();

  @override
  Future<void> destroy() => trayManager.destroy();

  @override
  void addListener(TrayPortListener listener) {
    if (_adapters.containsKey(listener)) return;
    final adapter = _ManagerListenerAdapter(listener);
    _adapters[listener] = adapter;
    trayManager.addListener(adapter);
  }

  @override
  void removeListener(TrayPortListener listener) {
    final adapter = _adapters.remove(listener);
    if (adapter == null) return;
    trayManager.removeListener(adapter);
  }

  /// Перекладывает описание в плагинное меню.
  ///
  /// Тип пункта выбирается по содержимому описания, а не отдельным полем:
  /// «есть дети» это подменю, «есть отметка» это чекбокс, и держать рядом с
  /// ними ещё и перечисление типов значило бы завести второй источник одного
  /// и того же знания.
  Menu _menuOf(List<TrayEntry> entries) =>
      Menu(items: <MenuItem>[for (final e in entries) _itemOf(e)]);

  MenuItem _itemOf(TrayEntry entry) {
    if (entry.separator) return MenuItem.separator();
    if (entry.children.isNotEmpty) {
      return MenuItem.submenu(
        key: entry.key,
        label: entry.label,
        disabled: !entry.enabled,
        submenu: _menuOf(entry.children),
      );
    }
    final checked = entry.checked;
    if (checked != null) {
      return MenuItem.checkbox(
        key: entry.key,
        label: entry.label,
        checked: checked,
        disabled: !entry.enabled,
      );
    }
    return MenuItem(
      key: entry.key,
      label: entry.label,
      disabled: !entry.enabled,
    );
  }
}

/// Переходник плагинного `TrayListener` в наши колбэки.
///
/// Берём события НАЖАТИЯ (`MouseDown`), а не отпускания: меню строки состояния
/// открывается по нажатию, и ожидание отпускания добавляло бы значку заметную
/// задержку.
class _ManagerListenerAdapter with TrayListener {
  final TrayPortListener _callbacks;

  _ManagerListenerAdapter(this._callbacks);

  @override
  void onTrayIconMouseDown() => _callbacks.onLeftClick?.call();

  @override
  void onTrayIconRightMouseDown() => _callbacks.onRightClick?.call();

  @override
  void onTrayMenuItemClick(MenuItem menuItem) {
    final key = menuItem.key;
    // Пункты без ключа (заголовок состояния, разделители) кликов не несут.
    if (key == null || key.isEmpty) return;
    _callbacks.onItemClick?.call(key);
  }
}

/// Значок-заглушка для тестов: записывает вызовы вместо разговора с плагином.
///
/// Живёт в `lib`, а не в тестовом файле, намеренно: этот же фейк нужен и тесту
/// сборки десктопного хоста, и копия фейка в двух файлах разошлась бы с портом
/// при первой же правке контракта.
class FakeTrayPort implements TrayPort {
  /// Порядок вызовов, как их видел плагин.
  final List<String> calls = <String>[];

  String? iconPath;
  bool iconIsTemplate = false;
  String? tooltip;
  TrayMenuSpec? menu;
  bool destroyed = false;
  int popUps = 0;

  TrayPortListener? listener;

  @override
  Future<void> setIcon(String path, {bool isTemplate = false}) async {
    calls.add('setIcon($path)');
    iconPath = path;
    iconIsTemplate = isTemplate;
  }

  @override
  Future<void> setToolTip(String value) async {
    calls.add('setToolTip');
    tooltip = value;
  }

  @override
  Future<void> setContextMenu(TrayMenuSpec spec) async {
    calls.add('setContextMenu');
    menu = spec;
  }

  @override
  Future<void> popUpContextMenu() async {
    calls.add('popUpContextMenu');
    popUps++;
  }

  @override
  Future<void> destroy() async {
    calls.add('destroy');
    destroyed = true;
  }

  @override
  void addListener(TrayPortListener value) => listener = value;

  @override
  void removeListener(TrayPortListener value) {
    if (identical(listener, value)) listener = null;
  }

  /// Как система: клик по значку.
  void tapLeft() => listener?.onLeftClick?.call();

  void tapRight() => listener?.onRightClick?.call();

  /// Как система: выбран пункт меню с этим ключом.
  void tapItem(String key) => listener?.onItemClick?.call(key);
}
