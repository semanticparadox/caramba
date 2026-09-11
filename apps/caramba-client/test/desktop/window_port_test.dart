// Возврат окна на экран: одна дверь плагина плюс своя дверь в AppKit.
//
// ЧТО ИМЕННО ЗДЕСЬ СТОРОЖИТСЯ. После «красной кнопки» окно живёт только в
// строке меню, и единственный путь назад — пункт трея, диплинк или повторный
// запуск. На живой сборке этот путь не работал: `windowManager.show()` умеет
// лишь упорядочить окно, а спрятанное ПРИЛОЖЕНИЕ (⌘H) и свёрнутое в Dock окно
// поднимаются только через AppKit. Поэтому показ обязан сперва позвать наш
// канал `caramba/desktop_window`, и обязан довести показ до конца, даже если
// канала нет (Windows, Linux) или он отказал.
//
// Второе: сворачивание. Плагин шлёт `onWindowMinimize`, порт обязан довести
// его до подписчика, иначе решение «прятать в трей» жёлтой кнопки не увидит,
// а окно уедет миниатюрой в Dock (ровно это и было до правки).

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:caramba_client/desktop/ports/window_port.dart';

/// Канал плагина окна. Литерал, а не константа из пакета: сменившееся имя
/// канала — это сломанный показ окна, и тест должен это заметить.
const MethodChannel _pluginChannel = MethodChannel('window_manager');

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late List<String> calls;

  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  /// Плагин перед показом спрашивает, не свёрнуто ли окно: без ответа
  /// `windowManager.show()` падает ещё до самого показа.
  void mockPlugin() {
    messenger.setMockMethodCallHandler(_pluginChannel, (call) async {
      calls.add('plugin:${call.method}');
      return call.method == 'isMinimized' ? false : null;
    });
    addTearDown(() => messenger.setMockMethodCallHandler(_pluginChannel, null));
  }

  void mockNative() {
    messenger.setMockMethodCallHandler(kDesktopWindowChannel, (call) async {
      calls.add('native:${call.method}');
      return null;
    });
    addTearDown(
      () => messenger.setMockMethodCallHandler(kDesktopWindowChannel, null),
    );
  }

  setUp(() => calls = <String>[]);

  /// Событие окна так, как его шлёт нативная сторона плагина: метод `onEvent`
  /// на том же канале, входящим сообщением. Не мок исходящих вызовов, а именно
  /// доставка внутрь — иначе разбор события в самом плагине остался бы
  /// непроверенным.
  Future<void> emitPluginEvent(String name) {
    const codec = StandardMethodCodec();
    return messenger.handlePlatformMessage(
      _pluginChannel.name,
      codec.encodeMethodCall(
        MethodCall('onEvent', <String, Object?>{'eventName': name}),
      ),
      (_) {},
    );
  }

  test('показ сначала просит AppKit поднять окно целиком', () async {
    mockNative();
    mockPlugin();

    await WindowManagerPort().show();

    expect(calls.first, 'native:present');
    expect(calls, contains('plugin:show'));
  });

  test('без нативного канала окно всё равно показывается', () async {
    // Windows и Linux: канал не зарегистрирован, ответ — MissingPluginException.
    mockPlugin();

    await WindowManagerPort().show();

    expect(calls, contains('plugin:show'));
  });

  test('restore идёт в плагин как есть', () async {
    mockPlugin();

    await WindowManagerPort().restore();

    expect(calls, contains('plugin:restore'));
  });

  test('сворачивание доезжает до подписчика', () async {
    var minimized = 0;
    final port = WindowManagerPort();
    final listener = WindowPortListener(onMinimize: () => minimized++);
    port.addListener(listener);
    addTearDown(() => port.removeListener(listener));

    await emitPluginEvent('minimize');

    expect(minimized, 1);
  });

  test('снятый подписчик сворачивания больше не слышит', () async {
    var minimized = 0;
    final port = WindowManagerPort();
    final listener = WindowPortListener(onMinimize: () => minimized++);
    port.addListener(listener);
    port.removeListener(listener);

    await emitPluginEvent('minimize');

    expect(minimized, 0);
  });

  test('отказ нативной стороны не отменяет показ', () async {
    messenger.setMockMethodCallHandler(kDesktopWindowChannel, (call) async {
      calls.add('native:${call.method}');
      throw PlatformException(code: 'failed');
    });
    addTearDown(
      () => messenger.setMockMethodCallHandler(kDesktopWindowChannel, null),
    );
    mockPlugin();

    await WindowManagerPort().show();

    expect(
      calls,
      containsAllInOrder(<String>['native:present', 'plugin:show']),
    );
  });
}
