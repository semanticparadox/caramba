#ifndef RUNNER_FLUTTER_WINDOW_H_
#define RUNNER_FLUTTER_WINDOW_H_

#include <flutter/dart_project.h>
#include <flutter/encodable_value.h>
#include <flutter/flutter_view_controller.h>
#include <flutter/method_channel.h>

#include <memory>

#include "win32_window.h"

// A window that does nothing but host a Flutter view.
class FlutterWindow : public Win32Window {
 public:
  // Creates a new FlutterWindow hosting a Flutter view running |project|.
  explicit FlutterWindow(const flutter::DartProject& project);
  virtual ~FlutterWindow();

 protected:
  // Win32Window:
  bool OnCreate() override;
  void OnDestroy() override;
  LRESULT MessageHandler(HWND window, UINT const message, WPARAM const wparam,
                         LPARAM const lparam) noexcept override;

 private:
  // Прячет окно вместо сворачивания и сообщает об этом Dart-стороне.
  void HideToTray(HWND hwnd);

  // The project to run.
  flutter::DartProject project_;

  // The Flutter instance hosted by this window.
  std::unique_ptr<flutter::FlutterViewController> flutter_controller_;

  // Канал `caramba/window`: Dart говорит, прятать ли окно при сворачивании,
  // раннер отвечает событием `onHiddenToTray`.
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>>
      window_channel_;

  // Зачем флаг живёт здесь, а не читается из настроек: раннер настроек не
  // знает, а сворачивание надо перехватить ДО того, как система свернёт окно
  // (плагин window_manager узнаёт о нём уже по факту, через WM_SIZE).
  bool minimize_to_tray_ = false;
};

#endif  // RUNNER_FLUTTER_WINDOW_H_
