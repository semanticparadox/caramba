import Cocoa
import FlutterMacOS
import window_manager

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    AppDelegate.registerDesktopChannels(controller: flutterViewController, window: self)
    RegisterGeneratedPlugins(registry: flutterViewController)

    super.awakeFromNib()
  }

  // Зачем: окно не должно мигать на экране до того, как Dart решит, показывать
  // ли его вообще. Восстановление размера/позиции и режим «запускаться только
  // значком в строке меню» живут в desktop_bootstrap.dart и срабатывают уже
  // после старта движка; без этой врезки AppKit успевает показать окно
  // дефолтного размера в дефолтном месте. hiddenWindowAtLaunch() (расширение
  // NSWindow из window_manager) один раз прячет окно при первом упорядочивании,
  // дальше показ идёт только через windowManager.show() из waitUntilReadyToShow.
  // Требование README window_manager 0.5.x.
  override public func order(_ place: NSWindow.OrderingMode, relativeTo otherWin: Int) {
    super.order(place, relativeTo: otherWin)
    hiddenWindowAtLaunch()
  }
}
