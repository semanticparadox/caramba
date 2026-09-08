/// Порт окна: единственная дверь из десктопного слоя в `window_manager`.
///
/// ЗАЧЕМ порт, а не прямые вызовы плагина. `windowManager` — синглтон поверх
/// метод-канала, и любой его вызов в тесте падает `MissingPluginException`
/// (или, хуже, тихо виснет на `await`). Логика окна при этом ровно та, которую
/// проверять и надо: что делает красная кнопка, когда сохранять геометрию,
/// в каком порядке опускать туннель перед выходом. Порт отделяет решение от
/// канала: [WindowService] знает только этот интерфейс, тест подставляет
/// фейк, а живой [WindowManagerPort] остаётся тонким переходником без единой
/// ветки.
library;

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:window_manager/window_manager.dart';

/// Канал возврата окна на экран, реализованный в `macos/Runner/AppDelegate.swift`.
///
/// ЗАЧЕМ ОН ЕСТЬ, ЕСЛИ У ПЛАГИНА УЖЕ ЕСТЬ `show()`. Плагин умеет ровно одно:
/// упорядочить окно (`setIsVisible` + `makeKeyAndOrderFront`). Этого хватает,
/// пока окно просто убрано с экрана, и не хватает, когда спрятано ПРИЛОЖЕНИЕ
/// целиком (⌘H, «Скрыть») или окно свёрнуто в Dock: окна спрятанного
/// приложения не поднимает ничто, кроме `NSApp.unhide`. Отсюда своя дверь в
/// AppKit; на Windows и Linux канала нет, и его отсутствие — не ошибка.
const MethodChannel kDesktopWindowChannel = MethodChannel(
  'caramba/desktop_window',
);

/// Набор колбэков на события окна.
///
/// Один объект, а не четыре отдельные подписки: снимать их надо всем скопом
/// (`detach`), и разъехавшийся набор означал бы окно, которое ещё пишет
/// геометрию в уже отпущенный сервис.
@immutable
class WindowPortListener {
  /// Пользователь закрыл окно (красная кнопка, ⌘W, Alt+F4). Приходит только
  /// когда включён `setPreventClose(true)`, иначе окно закроется само.
  final VoidCallback? onClose;

  /// Изменение размера ЗАВЕРШЕНО (не каждый кадр перетаскивания).
  final VoidCallback? onResized;

  /// Перемещение ЗАВЕРШЕНО.
  final VoidCallback? onMoved;

  final VoidCallback? onFocus;

  const WindowPortListener({
    this.onClose,
    this.onResized,
    this.onMoved,
    this.onFocus,
  });
}

/// То, что десктопному слою нужно от окна, и ничего сверх этого.
abstract class WindowPort {
  Future<void> show();

  Future<void> hide();

  Future<void> focus();

  Future<bool> isVisible();

  /// Геометрия в логических пикселях экрана (не окна).
  Future<Rect> getBounds();

  Future<void> setBounds(Rect bounds);

  Future<bool> isMaximized();

  Future<void> maximize();

  Future<void> unmaximize();

  /// `true` — закрытие окна приходит колбэком [WindowPortListener.onClose]
  /// вместо того, чтобы уничтожить окно. На этом держится «прятать в трей».
  Future<void> setPreventClose(bool value);

  Future<void> setTitle(String title);

  void addListener(WindowPortListener listener);

  void removeListener(WindowPortListener listener);
}

/// Живая реализация поверх `window_manager` 0.5.2.
///
/// Здесь нет ни одного условия: любая ветка (платформа, состояние, настройка)
/// живёт выше и потому проверяется тестом.
class WindowManagerPort implements WindowPort {
  /// Плагин принимает свой `WindowListener`, а наружу мы отдаём собственный
  /// тип, поэтому переходники держим по ключу-подписчику: иначе
  /// `removeListener` не нашёл бы, что именно снимать.
  final Map<WindowPortListener, _ManagerListenerAdapter> _adapters =
      <WindowPortListener, _ManagerListenerAdapter>{};

  @override
  Future<void> show() async {
    await presentNatively();
    await windowManager.show();
  }

  /// Просит AppKit поднять окно из любого состояния. Отказ канала не имеет
  /// права сорвать показ: следом всё равно идёт `windowManager.show()`.
  Future<void> presentNatively() async {
    try {
      await kDesktopWindowChannel.invokeMethod<void>('present');
    } on MissingPluginException {
      // Не macOS: там окно поднимает сам плагин.
    } on PlatformException {
      // Нативная сторона отказала — показ ниже остаётся единственным шансом,
      // и он лучше исключения, из-за которого окно не вернётся вовсе.
    }
  }

  @override
  Future<void> hide() => windowManager.hide();

  @override
  Future<void> focus() => windowManager.focus();

  @override
  Future<bool> isVisible() => windowManager.isVisible();

  @override
  Future<Rect> getBounds() => windowManager.getBounds();

  @override
  Future<void> setBounds(Rect bounds) => windowManager.setBounds(bounds);

  @override
  Future<bool> isMaximized() => windowManager.isMaximized();

  @override
  Future<void> maximize() => windowManager.maximize();

  @override
  Future<void> unmaximize() => windowManager.unmaximize();

  @override
  Future<void> setPreventClose(bool value) =>
      windowManager.setPreventClose(value);

  @override
  Future<void> setTitle(String title) => windowManager.setTitle(title);

  @override
  void addListener(WindowPortListener listener) {
    if (_adapters.containsKey(listener)) return;
    final adapter = _ManagerListenerAdapter(listener);
    _adapters[listener] = adapter;
    windowManager.addListener(adapter);
  }

  @override
  void removeListener(WindowPortListener listener) {
    final adapter = _adapters.remove(listener);
    if (adapter == null) return;
    windowManager.removeListener(adapter);
  }
}

/// Переходник плагинного `WindowListener` в наши колбэки.
///
/// Берём `onWindowResized`/`onWindowMoved` (события ЗАВЕРШЕНИЯ), а не
/// `onWindowResize`/`onWindowMove`: последние приходят на каждый кадр
/// перетаскивания, и запись настроек шла бы сотнями за один жест.
class _ManagerListenerAdapter with WindowListener {
  final WindowPortListener _callbacks;

  _ManagerListenerAdapter(this._callbacks);

  @override
  void onWindowClose() => _callbacks.onClose?.call();

  @override
  void onWindowResized() => _callbacks.onResized?.call();

  @override
  void onWindowMoved() => _callbacks.onMoved?.call();

  @override
  void onWindowFocus() => _callbacks.onFocus?.call();
}
