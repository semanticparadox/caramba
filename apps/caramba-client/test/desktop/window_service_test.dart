// Окно как процесс: красная кнопка, память геометрии, выход с опусканием.
//
// Главное здесь — третье. На macOS ядро крутится ВНУТРИ нашего процесса
// (dart:ffi), поэтому выход раньше подтверждённой остановки оставляет систему
// с настроенным прокси и без того, кто на нём слушает: интернет пропадает
// целиком, а виноватого процесса в списке уже нет. Тест фиксирует порядок:
// disconnect, кадр остановки, гашение трея, только потом выход.
//
// Плагин `window_manager` в тестах не поднимается: всё идёт через WindowPort,
// сюда подставлен фейк.

import 'dart:async';
import 'dart:ui' show AppExitResponse;

import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'package:caramba_client/data/models/server.dart';
import 'package:caramba_client/desktop/desktop_prefs.dart';
import 'package:caramba_client/desktop/ports/window_port.dart';
import 'package:caramba_client/desktop/window_service.dart';
import 'package:caramba_client/state/providers.dart';
import 'package:caramba_client/state/vpn_state.dart';
import 'package:caramba_client/vpn/vpn_service.dart';
import 'package:caramba_client/vpn/vpn_status.dart';

import '../support/fake_csm_device.dart';

/// Окно-заглушка: записывает вызовы вместо разговора с плагином.
class FakeWindowPort implements WindowPort {
  final List<String> calls = <String>[];

  bool visible = true;
  bool maximized = false;
  bool preventClose = false;
  String title = '';
  Rect bounds = const Rect.fromLTWH(120, 80, 1120, 720);

  WindowPortListener? listener;

  @override
  Future<void> show() async {
    calls.add('show');
    visible = true;
  }

  @override
  Future<void> hide() async {
    calls.add('hide');
    visible = false;
  }

  @override
  Future<void> focus() async => calls.add('focus');

  @override
  Future<bool> isVisible() async => visible;

  @override
  Future<Rect> getBounds() async {
    calls.add('getBounds');
    return bounds;
  }

  @override
  Future<void> setBounds(Rect value) async {
    calls.add('setBounds');
    bounds = value;
  }

  @override
  Future<bool> isMaximized() async => maximized;

  @override
  Future<void> maximize() async => maximized = true;

  @override
  Future<void> unmaximize() async => maximized = false;

  @override
  Future<void> setPreventClose(bool value) async {
    calls.add('setPreventClose($value)');
    preventClose = value;
  }

  @override
  Future<void> setTitle(String value) async => title = value;

  @override
  void addListener(WindowPortListener value) => listener = value;

  @override
  void removeListener(WindowPortListener value) {
    if (identical(listener, value)) listener = null;
  }

  /// Как система: пользователь нажал красную кнопку.
  void tapClose() => listener?.onClose?.call();

  /// Как система: жест изменения размера завершён.
  void endResize(Rect value) {
    bounds = value;
    listener?.onResized?.call();
  }
}

/// Ядро-заглушка: отдаёт ровно те кадры, которые ему велели отдать.
class FakeVpnConnection with FakeCsmDevice implements VpnConnection {
  final StreamController<VpnStatus> _ctrl =
      StreamController<VpnStatus>.broadcast();

  VpnStatus _current = const VpnStatus.disconnected();

  /// Отвечает ли `disconnect` кадром об остановке, как отвечает живой мост.
  /// `false` — ядро зависло в разборке: команда ушла, кадра нет. Ровно тот
  /// случай, ради которого потолок ожидания и существует.
  bool haltsOnDisconnect = true;

  int disconnects = 0;

  @override
  VpnStatus get currentStatus => _current;

  @override
  Stream<VpnStatus> get status => _ctrl.stream;

  @override
  Stream<TrafficStats> get traffic => const Stream<TrafficStats>.empty();

  @override
  Future<void> connect(Server server) async {}

  @override
  Future<void> connectRaw({
    required String raw,
    required String format,
    required String label,
    String? serverId,
  }) async {}

  @override
  Future<ImportResult> importSubscription({
    required String raw,
    required String format,
  }) async => ImportResult.empty;

  @override
  Future<List<ProbeResult>> probe({
    Duration timeout = const Duration(seconds: 5),
  }) async => const <ProbeResult>[];

  @override
  Future<void> setPolicy(CorePolicy policy) async {}

  @override
  Future<void> setTunnelMode(TunnelMode mode, {int mixedPort = 7890}) async {}

  @override
  Future<void> disconnect() async {
    disconnects++;
    if (haltsOnDisconnect) emit(const VpnStatus.disconnected());
  }

  @override
  Future<VpnStatus> refreshStatus() async => _current;

  @override
  Future<void> dispose() async => _ctrl.close();

  void emit(VpnStatus status) {
    _current = status;
    _ctrl.add(status);
  }
}

/// Прокрутить очередь, чтобы кадры статуса и `unawaited`-вызовы доехали.
Future<void> pump() => Future<void>.delayed(Duration.zero);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() => SharedPreferences.setMockInitialValues(<String, Object>{}));

  late FakeWindowPort port;
  late FakeVpnConnection core;
  late ProviderContainer container;
  late DesktopPrefs prefs;
  late List<String> order;
  late WindowService service;

  /// Собирает сервис на фейковом окне и фейковом ядре.
  ///
  /// Настройки держим локальной переменной, а не провайдером: проверяется
  /// решение сервиса, а не гидратация снимка (у неё свой тест). Стадия зато
  /// идёт через настоящий [vpnProvider] — ждать кадр остановки иначе негде.
  WindowService build({
    DesktopPrefs initial = const DesktopPrefs(),
    Duration stopLimit = const Duration(milliseconds: 200),
    Duration boundsDebounce = const Duration(milliseconds: 10),
  }) {
    prefs = initial;
    service = WindowService(
      port: port,
      readPrefs: () => prefs,
      writeBounds: (bounds, {bool maximized = false}) {
        order.add('bounds');
        prefs = prefs.copyWith(
          windowBounds: bounds,
          clearWindowBounds: bounds == null,
          maximized: maximized,
        );
      },
      readStage: () => container.read(vpnProvider).stage,
      disconnect: () {
        order.add('disconnect');
        return container.read(vpnProvider.notifier).disconnect();
      },
      listenStage: (onStage) {
        final sub = container.listen<VpnStage>(
          vpnProvider.select((s) => s.stage),
          (_, next) => onStage(next),
        );
        return sub.close;
      },
      exitProcess: () async => order.add('exit'),
      beforeExit: () async => order.add('tray'),
      stopLimit: stopLimit,
      boundsDebounce: boundsDebounce,
    );
    addTearDown(service.detach);
    return service;
  }

  /// Поднимает туннель до стадии [stage] так, как это делает ядро: кадром.
  Future<void> raiseTunnel(VpnStage stage) async {
    container.read(vpnProvider);
    core.emit(VpnStatus(stage: stage));
    await pump();
    expect(container.read(vpnProvider).stage, stage);
  }

  setUp(() {
    port = FakeWindowPort();
    core = FakeVpnConnection();
    order = <String>[];
    container = ProviderContainer(
      overrides: <Override>[vpnConnectionProvider.overrideWithValue(core)],
    );
    addTearDown(container.dispose);
  });

  test('attach просит окно отдавать закрытие нам', () async {
    build().attach();
    await pump();

    expect(port.preventClose, isTrue);
    expect(port.listener, isNotNull);
  });

  test('красная кнопка прячет окно и не трогает процесс', () async {
    build(initial: const DesktopPrefs()).attach();

    port.tapClose();
    await pump();

    expect(port.calls, contains('hide'));
    expect(order, isEmpty, reason: 'ни выхода, ни опускания туннеля');
  });

  test(
    'с выключенным closeToTray красная кнопка завершает приложение',
    () async {
      build(initial: const DesktopPrefs(closeToTray: false)).attach();

      port.tapClose();
      await pump();
      await pump();

      expect(port.calls, isNot(contains('hide')));
      expect(order, <String>['tray', 'exit']);
    },
  );

  test('выход при поднятом туннеле ждёт кадр остановки', () async {
    core.haltsOnDisconnect = false;
    await raiseTunnel(VpnStage.connected);
    final service = build();

    final quit = service.quitApplication();
    await pump();

    // Команда ушла, подтверждения нет: выходить ещё нельзя.
    expect(core.disconnects, 1);
    expect(order, <String>['disconnect']);

    core.emit(const VpnStatus.disconnected());
    await quit;

    expect(order, <String>['disconnect', 'tray', 'exit']);
  });

  test('кадр ошибки тоже считается подтверждением остановки', () async {
    core.haltsOnDisconnect = false;
    await raiseTunnel(VpnStage.connecting);
    final service = build();

    final quit = service.quitApplication();
    await pump();
    core.emit(const VpnStatus(stage: VpnStage.error));
    await quit;

    expect(order, <String>['disconnect', 'tray', 'exit']);
  });

  test('зависшее в разборке ядро не запирает приложение', () async {
    core.haltsOnDisconnect = false;
    await raiseTunnel(VpnStage.connected);
    final service = build(stopLimit: const Duration(milliseconds: 20));

    await service.quitApplication();

    expect(order, <String>['disconnect', 'tray', 'exit']);
  });

  test('без туннеля выход не зовёт disconnect вовсе', () async {
    container.read(vpnProvider);
    final service = build();

    await service.quitApplication();

    expect(core.disconnects, 0);
    expect(order, <String>['tray', 'exit']);
  });

  test('второй запрос выхода не запускает второе опускание', () async {
    await raiseTunnel(VpnStage.connected);
    final service = build();

    await service.quitApplication();
    await service.quitApplication();

    expect(core.disconnects, 1);
    expect(order, <String>['disconnect', 'tray', 'exit']);
  });

  test('запрос выхода от системы опускает туннель и разрешает выход', () async {
    await raiseTunnel(VpnStage.connected);
    build().attach();

    final response = await WidgetsBinding.instance.handleRequestAppExit();

    expect(response, AppExitResponse.exit);
    expect(core.disconnects, 1);
    // Процесс завершает система, сами не выходим — но трей гасим.
    expect(order, <String>['disconnect', 'tray']);
  });

  test('завершённый жест запоминает геометрию', () async {
    final service = build();
    service.attach();

    port.endResize(const Rect.fromLTWH(40, 60, 1000, 700));
    await Future<void>.delayed(const Duration(milliseconds: 40));

    expect(prefs.windowBounds, const Rect.fromLTWH(40, 60, 1000, 700));
    expect(prefs.maximized, isFalse);
  });

  test('серия жестов пишет настройки один раз', () async {
    final service = build();
    service.attach();

    port.endResize(const Rect.fromLTWH(0, 0, 1000, 700));
    port.endResize(const Rect.fromLTWH(10, 10, 1010, 710));
    port.endResize(const Rect.fromLTWH(20, 20, 1020, 720));
    await Future<void>.delayed(const Duration(milliseconds: 40));

    expect(order, <String>['bounds']);
    expect(prefs.windowBounds, const Rect.fromLTWH(20, 20, 1020, 720));
  });

  test('развёрнутое окно не затирает размер, к которому вернётся', () async {
    const saved = Rect.fromLTWH(120, 80, 1120, 720);
    final service = build(initial: const DesktopPrefs(windowBounds: saved));
    port.maximized = true;

    await service.saveBounds();

    expect(prefs.windowBounds, saved);
    expect(prefs.maximized, isTrue);
    expect(port.calls, isNot(contains('getBounds')));
  });

  test('show поднимает окно и отдаёт ему фокус', () async {
    final service = build();
    port.visible = false;

    await service.show();

    expect(port.calls, <String>['show', 'focus']);
  });

  test('toggle прячет видимое и показывает скрытое', () async {
    final service = build();

    await service.toggle();
    expect(port.visible, isFalse);

    await service.toggle();
    expect(port.visible, isTrue);
  });

  test('провайдер собирает сервис на подставленном порту', () async {
    final wired = ProviderContainer(
      overrides: <Override>[
        vpnConnectionProvider.overrideWithValue(core),
        windowPortProvider.overrideWithValue(port),
      ],
    );
    addTearDown(wired.dispose);

    final service = wired.read(windowServiceProvider);
    service.attach();
    await pump();

    // Дефолт настроек — прятать в трей, поэтому окно только скрывается.
    port.tapClose();
    await pump();

    expect(port.preventClose, isTrue);
    expect(port.calls, contains('hide'));
  });
}
