/// Поведение окна как процесса: красная кнопка, память геометрии, выход.
///
/// ЗАЧЕМ отдельный сервис. Десктопное окно — не экран, а вход в приложение,
/// которое живёт и без него. Три решения, которые никакому виджету не
/// принадлежат: закрытие окна прячет его, а не убивает туннель; размер и
/// позиция переживают перезапуск; выход опускает туннель ДО того, как процесс
/// исчезнет. Последнее особенно: на macOS ядро крутится внутри нашего же
/// процесса (dart:ffi), и `exit()` без опускания оставляет систему с
/// настроенным прокси и без того, кто на нём слушает, — интернет пропадает
/// целиком, а виноватого приложения в списке процессов уже нет.
///
/// Плагин сюда не заглядывает: всё через [WindowPort], все чтения провайдеров
/// через замыкания. Поэтому каждое из трёх решений проверяется тестом.
library;

import 'dart:async';
import 'dart:ui' show AppExitResponse, AppExitType;

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:caramba_client/desktop/desktop_prefs.dart';
import 'package:caramba_client/desktop/ports/window_port.dart';
import 'package:caramba_client/state/connection_profiles_state.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

/// Пауза перед записью геометрии. Событие завершения приходит по одному на
/// жест, но жестов за секунду бывает несколько (подтянул край, поправил, ещё
/// раз), а каждая запись — поход в `SharedPreferences`.
const Duration kWindowBoundsDebounce = Duration(milliseconds: 300);

/// Стадии, при которых выход обязан сначала опустить туннель.
bool _tunnelIsUp(VpnStage stage) =>
    stage == VpnStage.connected ||
    stage == VpnStage.connecting ||
    stage == VpnStage.reconnecting;

/// Снимает подписку, выданную `listenStage`.
typedef StageSubscriptionCanceller = void Function();

class WindowService {
  final WindowPort port;

  final DesktopPrefs Function() _readPrefs;
  final void Function(Rect? bounds, {bool maximized}) _writeBounds;
  final VpnStage Function() _readStage;
  final Future<void> Function() _disconnect;
  final StageSubscriptionCanceller Function(void Function(VpnStage stage))
  _listenStage;
  final Future<void> Function() _exitProcess;

  /// Потолок ожидания подтверждённой остановки. По умолчанию тот же
  /// [kStopConfirmationLimit], которым меряет ожидание сам туннель: два разных
  /// потолка на одну и ту же разборку разошлись бы при первой же правке.
  final Duration stopLimit;

  final Duration boundsDebounce;

  /// Что сделать перед самым выходом (D4 гасит здесь значок в трее: живой
  /// значок исчезнувшего процесса остаётся в строке меню до перезахода).
  Future<void> Function()? beforeExit;

  /// Дождаться начатых записей на диск (профиль подписки в связке ключей,
  /// снимок окна в prefs).
  ///
  /// ЗАЧЕМ ОТДЕЛЬНО ОТ [beforeExit]. Тот занят гашением значка, а слот один;
  /// но главное — это разные обязательства. Значок гасим, чтобы не оставить
  /// мусор в строке меню, а здесь ждём, чтобы не потерять данные: мутации
  /// профиля пишут в secure storage своим оборотом, и ⌘Q сразу после импорта
  /// подписки убивал бы процесс раньше записи. Именно так профиль и не пережил
  /// перезапуск в ручной проверке (D-10).
  Future<void> Function()? flushWrites;

  WindowPortListener? _listener;
  AppLifecycleListener? _lifecycle;
  Timer? _boundsTimer;

  /// Выход уже идёт. Второй ⌘Q поверх первого запустил бы второй `disconnect()`
  /// и второе ожидание, то есть отложил бы сам выход.
  bool _quitting = false;

  WindowService({
    required this.port,
    required DesktopPrefs Function() readPrefs,
    required void Function(Rect? bounds, {bool maximized}) writeBounds,
    required VpnStage Function() readStage,
    required Future<void> Function() disconnect,
    required StageSubscriptionCanceller Function(void Function(VpnStage stage))
    listenStage,
    Future<void> Function()? exitProcess,
    this.beforeExit,
    this.flushWrites,
    this.stopLimit = kStopConfirmationLimit,
    this.boundsDebounce = kWindowBoundsDebounce,
  }) : _readPrefs = readPrefs,
       _writeBounds = writeBounds,
       _readStage = readStage,
       _disconnect = disconnect,
       _listenStage = listenStage,
       _exitProcess = exitProcess ?? _systemExit;

  /// Подписывается на окно и на запрос выхода от системы.
  ///
  /// `setPreventClose(true)` повторяет то, что уже сделал `initDesktop()`:
  /// вызов идемпотентен, а сервис, который сам не гарантирует себе доставку
  /// `onClose`, ловил бы красную кнопку через раз в зависимости от порядка
  /// инициализации.
  void attach() {
    if (_listener != null) return;
    final listener = WindowPortListener(
      onClose: _handleClose,
      onResized: _scheduleBoundsSave,
      onMoved: _scheduleBoundsSave,
    );
    _listener = listener;
    port.addListener(listener);
    unawaited(port.setPreventClose(true));
    // ⌘Q и «Выйти» из меню приложения приходят сюда, а не в `onClose`: окна
    // может не быть вовсе (жизнь только в трее), а туннель опустить всё равно
    // надо.
    _lifecycle = AppLifecycleListener(onExitRequested: _handleExitRequested);
  }

  void detach() {
    final listener = _listener;
    if (listener != null) {
      port.removeListener(listener);
      _listener = null;
    }
    _boundsTimer?.cancel();
    _boundsTimer = null;
    _lifecycle?.dispose();
    _lifecycle = null;
  }

  /// Показать окно и отдать ему фокус (пункт трея, повторный запуск, диплинк).
  Future<void> show() async {
    await port.show();
    await port.focus();
  }

  /// Левый клик по значку в трее на Windows: видимое прячем, скрытое
  /// показываем.
  Future<void> toggle() async {
    if (await port.isVisible()) {
      await port.hide();
      return;
    }
    await show();
  }

  /// Красная кнопка: по умолчанию окно прячется, туннель продолжает работать.
  void _handleClose() {
    if (_readPrefs().closeToTray) {
      unawaited(port.hide());
      return;
    }
    unawaited(quitApplication());
  }

  void _scheduleBoundsSave() {
    _boundsTimer?.cancel();
    _boundsTimer = Timer(boundsDebounce, () => unawaited(saveBounds()));
  }

  /// Запоминает геометрию окна.
  ///
  /// У РАЗВЁРНУТОГО окна геометрия — это геометрия экрана, и записав её, мы
  /// потеряли бы размер, к которому окно возвращается по «восстановить».
  /// Поэтому в этом случае обновляется только флаг, а прямоугольник остаётся
  /// прежним.
  Future<void> saveBounds() async {
    final maximized = await port.isMaximized();
    if (maximized) {
      _writeBounds(_readPrefs().windowBounds, maximized: true);
      return;
    }
    _writeBounds(await port.getBounds(), maximized: false);
  }

  /// Выход: сначала опустить туннель, потом погасить трей, потом умереть.
  ///
  /// [exitSelf] `false` — процесс завершает система (мы отвечаем ей
  /// [AppExitResponse.exit] после того, как всё опустили).
  Future<void> quitApplication({bool exitSelf = true}) async {
    if (_quitting) return;
    _quitting = true;
    _boundsTimer?.cancel();
    _boundsTimer = null;

    if (_tunnelIsUp(_readStage())) await _stopTunnel();

    await beforeExit?.call();
    // Барьер стоит ДО `exitSelf`: при ⌘Q процесс убивает система, как только
    // мы ответим ей `AppExitResponse.exit`, и «не наш» выход теряет записи
    // ровно так же, как наш собственный.
    await flushWrites?.call();
    if (!exitSelf) return;
    await _exitProcess();
  }

  /// Опускает туннель и ждёт ПОДТВЕРЖДЕНИЯ кадром состояния.
  ///
  /// Ждём именно кадра, а не возврата из `disconnect()`: команда уходит в ядро
  /// и возвращает управление сразу, а сама разборка идёт отдельным оборотом.
  /// Выйдя по возврату команды, мы убили бы процесс посреди разборки — тот же
  /// случай, что и выход без опускания вовсе.
  ///
  /// Потолок обязателен: зависшее в разборке ядро не имеет права запереть
  /// приложение навсегда. По таймауту выходим как есть — не выйти хуже.
  Future<void> _stopTunnel() async {
    final halted = Completer<void>();
    final cancel = _listenStage((stage) {
      if (stage != VpnStage.disconnected && stage != VpnStage.error) return;
      if (!halted.isCompleted) halted.complete();
    });
    try {
      await _disconnect();
      await halted.future.timeout(stopLimit, onTimeout: () {});
    } catch (_) {
      // Отказ разборки — не повод остаться в памяти навсегда.
    } finally {
      cancel();
    }
  }

  Future<AppExitResponse> _handleExitRequested() async {
    await quitApplication(exitSelf: false);
    return AppExitResponse.exit;
  }

  static Future<void> _systemExit() async {
    await ServicesBinding.instance.exitApplication(AppExitType.required);
  }
}

/// Живой порт окна. Отдельным провайдером, чтобы тест подменял его фейком, не
/// трогая проводку [windowServiceProvider].
final windowPortProvider = Provider<WindowPort>((ref) => WindowManagerPort());

/// Сервис окна. Создаётся один на процесс; слушателей вешает `attach()`
/// (его зовёт десктопный хост сервисов), снимает — dispose провайдера.
final windowServiceProvider = Provider<WindowService>((ref) {
  final service = WindowService(
    port: ref.watch(windowPortProvider),
    readPrefs: () => ref.read(desktopPrefsProvider),
    writeBounds: (bounds, {bool maximized = false}) => ref
        .read(desktopPrefsProvider.notifier)
        .setWindowBounds(bounds, maximized: maximized),
    readStage: () => ref.read(vpnProvider).stage,
    disconnect: () => ref.read(vpnProvider.notifier).disconnect(),
    // Через контейнер, а не `ref.listen`: подписка нужна ПОСЛЕ сборки
    // провайдера, в момент выхода, и живёт ровно до подтверждения остановки.
    listenStage: (onStage) {
      final sub = ref.container.listen<VpnStage>(
        vpnProvider.select((s) => s.stage),
        (_, next) => onStage(next),
      );
      return sub.close;
    },
    // Читаем нотифаеры ЛЕНИВО, внутри замыкания: провайдер окна собирается на
    // старте, а профили подключения к этому моменту поднимать незачем.
    flushWrites: () async {
      await ref.read(connectionProfilesProvider.notifier).flush();
      await ref.read(desktopPrefsProvider.notifier).flush();
    },
  );
  ref.onDispose(service.detach);
  return service;
});
