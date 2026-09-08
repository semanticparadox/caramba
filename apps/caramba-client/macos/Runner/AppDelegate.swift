import Cocoa
import FlutterMacOS
import ServiceManagement

@main
class AppDelegate: FlutterAppDelegate {
  private static weak var desktopWindow: NSWindow?

  // Зачем false: красная кнопка окна на десктопе не убивает приложение — окно
  // прячется (windowManager.setPreventClose + hide), а туннель продолжает
  // работать, потому что ядро крутится внутри этого же процесса. С дефолтным
  // true AppKit завершал бы процесс сразу после закрытия последнего окна и
  // ронял бы VPN вместе с ним.
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    return false
  }

  // Зачем: раз процесс живёт без окон, клик по иконке в Dock (или запуск второй
  // копии из Launchpad) должен вернуть уже существующее окно, а не создать
  // второе — иначе в Dock появляется дубль, а туннелем управляют два UI.
  //
  // Возвращаем true, а не false, как было в первой редакции. false означает
  // «системное поведение не нужно», а именно оно возвращает приложение из
  // состояния «спрятано целиком» (после ⌘H или Hide в меню) и разворачивает
  // окно, свёрнутое в Dock. Подавив его и ограничившись makeKeyAndOrderFront,
  // мы получили окно, которое после «красной кнопки» не поднимал ни клик по
  // тайлу в Dock, ни пункт трея. Свою часть работы делаем сами
  // ([presentMainWindow]), системную не отбираем.
  override func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
    presentMainWindow()
    return true
  }

  // MARK: - Возврат окна на экран

  /// Поднимает главное окно из ЛЮБОГО состояния, в котором оно могло исчезнуть.
  ///
  /// ЗАЧЕМ ЛЕСТНИЦА, А НЕ ОДИН `makeKeyAndOrderFront`. С экрана окно уходит
  /// тремя разными способами, и каждый снимается своим вызовом:
  ///   * `windowManager.hide()` убирает окно (`orderOut`) — лечится
  ///     `setIsVisible(true)` + `makeKeyAndOrderFront`;
  ///   * ⌘H / «Скрыть» прячет ПРИЛОЖЕНИЕ целиком — окна спрятанного приложения
  ///     не поднимаются ничем, пока не вызван `NSApp.unhide`;
  ///   * ⌘M сворачивает окно в Dock — нужен `deminiaturize`.
  /// Пропущенная ступень выглядит одинаково: человек нажимает «Открыть
  /// Caramba Connect», и не происходит ничего.
  func presentMainWindow() {
    guard let window = AppDelegate.desktopWindow ?? mainFlutterWindow else { return }
    AppDelegate.present(window)
  }

  private static func present(_ window: NSWindow) {
    NSApp.unhide(nil)
    if window.isMiniaturized {
      window.deminiaturize(nil)
    }
    window.setIsVisible(true)
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
  }

  override func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
    return true
  }

  // MARK: - Автозапуск при входе в систему (канал launch_at_startup)

  // Зачем свой нативный код вместо штатной macOS-части пакета launch_at_startup:
  // она тянет SPM-пакет LaunchAtLogin, а он кладёт в бандл отдельный
  // helper-бандл и требует run script phase в Xcode-проекте. Для нас это лишний
  // подписываемый бинарь, ручная правка project.pbxproj и риск для App Sandbox.
  // SMAppService.mainApp (macOS 13+) регистрирует сам основной бандл, работает
  // в песочнице и не добавляет артефактов сборки. Dart-сторона пакета на macOS
  // и так ходит только через MethodChannel('launch_at_startup') с двумя
  // методами — их и реализуем здесь.
  // Installed with generated plugins, before Dart startup requests can arrive.
  // The explicit window/controller avoids looking up a hidden main window.
  static func registerDesktopChannels(controller: FlutterViewController, window: NSWindow) {
    desktopWindow = window
    let channel = FlutterMethodChannel(
      name: "launch_at_startup",
      binaryMessenger: controller.engine.binaryMessenger
    )
    channel.setMethodCallHandler { call, result in
      AppDelegate.handleLaunchAtStartupCall(call, result: result)
    }

    // Зачем свой канал вместо windowManager.show(): плагин умеет только
    // упорядочить окно, а вернуть его надо и из «приложение спрятано целиком»,
    // и из «свёрнуто в Dock». Полный подъём знает только AppKit-сторона, и
    // Dart зовёт её этим каналом — из пункта трея, диплинка и повторного
    // запуска.
    let windowChannel = FlutterMethodChannel(
      name: "caramba/desktop_window",
      binaryMessenger: controller.engine.binaryMessenger
    )
    windowChannel.setMethodCallHandler { [weak window] call, result in
      switch call.method {
      case "present":
        guard let window = window else {
          result(FlutterError(code: "window_unavailable", message: "Main window is unavailable", details: nil))
          return
        }
        AppDelegate.present(window)
        result(nil)
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }

  private static func handleLaunchAtStartupCall(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    switch call.method {
    case "launchAtStartupIsEnabled":
      if #available(macOS 13.0, *) {
        let status = SMAppService.mainApp.status
        if status == .requiresApproval {
          result(FlutterError(code: "requires_approval", message: AppDelegate.describe(status), details: nil))
        } else {
          result(status == .enabled)
        }
      } else {
        result(FlutterError(
          code: "unsupported",
          message: "Launch at login requires macOS 13 or newer",
          details: nil
        ))
      }

    case "launchAtStartupSetEnabled":
      guard #available(macOS 13.0, *) else {
        result(FlutterError(
          code: "unsupported",
          message: "Launch at login requires macOS 13 or newer",
          details: nil
        ))
        return
      }
      let arguments = call.arguments as? [String: Any]
      let enabled = arguments?["setEnabledValue"] as? Bool ?? false
      do {
        let currentStatus = SMAppService.mainApp.status
        if enabled {
          if currentStatus != .enabled && currentStatus != .requiresApproval {
            try SMAppService.mainApp.register()
          }
        } else if currentStatus != .notRegistered && currentStatus != .notFound {
          try SMAppService.mainApp.unregister()
        }
        // Зачем перечитывать статус: register() умеет вернуть управление без
        // ошибки, а система при этом оставляет заявку висеть (.requiresApproval
        // — человек должен разрешить её в «Объектах входа»). Без сверки
        // переключатель в настройках оставался бы включённым, обещая
        // автозапуск, которого не будет.
        if enabled {
          let status = SMAppService.mainApp.status
          if status != .enabled {
            result(FlutterError(
              code: status == .requiresApproval ? "requires_approval" : "failed",
              message: AppDelegate.describe(status),
              details: nil
            ))
            return
          }
        }
        result(nil)
      } catch {
        // Типичная причина отказа — приложение запущено не из /Applications
        // (или бандл не подписан), поднимаем это в Dart как ошибку, чтобы
        // переключатель вернулся в прежнее положение, а не соврал.
        result(FlutterError(
          code: "failed",
          message: error.localizedDescription,
          details: nil
        ))
      }

    default:
      result(FlutterMethodNotImplemented)
    }
  }

  /// Человекочитаемое состояние регистрации: попадает в сообщение об отказе.
  @available(macOS 13.0, *)
  private static func describe(_ status: SMAppService.Status) -> String {
    switch status {
    case .enabled: return "enabled"
    case .requiresApproval: return "requires approval in Login Items"
    case .notFound: return "not found"
    case .notRegistered: return "not registered"
    @unknown default: return "unknown status"
    }
  }
}
