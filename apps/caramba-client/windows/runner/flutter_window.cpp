#include "flutter_window.h"

#include <flutter/standard_method_codec.h>

#include <optional>

#include "flutter/generated_plugin_registrant.h"

namespace {

// Имя канала совпадает с `kCarambaWindowChannel` в
// lib/desktop/ports/window_port.dart.
constexpr char kWindowChannelName[] = "caramba/window";

}  // namespace

FlutterWindow::FlutterWindow(const flutter::DartProject& project)
    : project_(project) {}

FlutterWindow::~FlutterWindow() {}

bool FlutterWindow::OnCreate() {
  if (!Win32Window::OnCreate()) {
    return false;
  }

  RECT frame = GetClientArea();

  // The size here must match the window dimensions to avoid unnecessary surface
  // creation / destruction in the startup path.
  flutter_controller_ = std::make_unique<flutter::FlutterViewController>(
      frame.right - frame.left, frame.bottom - frame.top, project_);
  // Ensure that basic setup of the controller was successful.
  if (!flutter_controller_->engine() || !flutter_controller_->view()) {
    return false;
  }
  RegisterPlugins(flutter_controller_->engine());
  SetChildContent(flutter_controller_->view()->GetNativeWindow());

  // Канал окна. Dart ставит флаг «сворачивать в трей» при подписке на окно и
  // при каждой смене настройки; всё остальное раннер решает сам по флагу.
  window_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(), kWindowChannelName,
          &flutter::StandardMethodCodec::GetInstance());
  window_channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
             std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
                 result) {
        if (call.method_name() == "setMinimizeToTray") {
          const auto* flag = call.arguments() == nullptr
                                 ? nullptr
                                 : std::get_if<bool>(call.arguments());
          minimize_to_tray_ = flag != nullptr && *flag;
          result->Success();
          return;
        }
        result->NotImplemented();
      });

  flutter_controller_->engine()->SetNextFrameCallback([&]() {
    this->Show();
  });

  // Flutter can complete the first frame before the "show window" callback is
  // registered. The following call ensures a frame is pending to ensure the
  // window is shown. It is a no-op if the first frame hasn't completed yet.
  flutter_controller_->ForceRedraw();

  return true;
}

void FlutterWindow::OnDestroy() {
  // Канал снимается раньше движка: обработчик держит `this`, а сообщение,
  // пришедшее в уже разрушаемое окно, читать некому.
  if (window_channel_) {
    window_channel_->SetMethodCallHandler(nullptr);
    window_channel_ = nullptr;
  }
  if (flutter_controller_) {
    flutter_controller_ = nullptr;
  }

  Win32Window::OnDestroy();
}

void FlutterWindow::HideToTray(HWND hwnd) {
  // SW_HIDE убирает окно и с экрана, и из панели задач; значок в трее к этому
  // моменту уже стоит (его ставит Dart при старте сервисов).
  ::ShowWindow(hwnd, SW_HIDE);
  if (window_channel_) {
    window_channel_->InvokeMethod("onHiddenToTray", nullptr);
  }
}

LRESULT
FlutterWindow::MessageHandler(HWND hwnd, UINT const message,
                              WPARAM const wparam,
                              LPARAM const lparam) noexcept {
  // Сворачивание в трей перехватывается ДО плагинов: window_manager узнаёт о
  // сворачивании только по WM_SIZE/SIZE_MINIMIZED, то есть когда окно уже
  // свёрнуто, и Dart-путь «развернуть и спрятать» на Windows оставлял
  // миниатюру в панели задач. Здесь окно ещё не свёрнуто.
  if (minimize_to_tray_) {
    // Кнопка «свернуть», Win+Down, пункт системного меню: SC_MINIMIZE в
    // младших битах wparam служебные, поэтому маска.
    if (message == WM_SYSCOMMAND && (wparam & 0xFFF0) == SC_MINIMIZE) {
      HideToTray(hwnd);
      return 0;
    }
    // Страховка: сворачивание без SC_MINIMIZE (Win+D, «свернуть все окна»
    // из панели задач). Окно уже свёрнуто, прячем его как есть; показ из
    // трея идёт через show + focus, а focus плагина разворачивает свёрнутое.
    // Сообщение не отдаём дальше, иначе плагин пришлёт Dart событие
    // сворачивания и тот попытается развернуть спрятанное окно.
    if (message == WM_SIZE && wparam == SIZE_MINIMIZED) {
      HideToTray(hwnd);
      return 0;
    }
  }

  // Give Flutter, including plugins, an opportunity to handle window messages.
  if (flutter_controller_) {
    std::optional<LRESULT> result =
        flutter_controller_->HandleTopLevelWindowProc(hwnd, message, wparam,
                                                      lparam);
    if (result) {
      return *result;
    }
  }

  switch (message) {
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}
