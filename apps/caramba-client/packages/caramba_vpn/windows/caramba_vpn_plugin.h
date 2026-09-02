// caramba_vpn_plugin.h — Windows C++ plugin for the exarobot VPN tunnel.
//
// Registers the federation channels (com.caramba/vpn MethodChannel +
// com.caramba/vpn/status and com.caramba/vpn/traffic EventChannels), loads
// libcaramba_core.dll (the cgo c-shared Go engine running mihomo), and drives
// stage transitions + ~1 Hz status/traffic ticks by polling the FFI.
//
// On Windows mihomo creates the TUN itself via the bundled wintun.dll, so the
// plugin passes tunFd = -1 and never owns a descriptor. The host process must
// run elevated (see INTEGRATION.md).

#ifndef FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_H_
#define FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_H_

#include <windows.h>

#include <flutter/encodable_value.h>
#include <flutter/event_channel.h>
#include <flutter/event_sink.h>
#include <flutter/method_channel.h>
#include <flutter/plugin_registrar_windows.h>

#include <atomic>
#include <deque>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <thread>

#include "caramba_core_ffi.h"

namespace caramba_vpn {

class CarambaVpnPlugin : public flutter::Plugin {
 public:
  static void RegisterWithRegistrar(
      flutter::PluginRegistrarWindows* registrar);

  CarambaVpnPlugin();
  ~CarambaVpnPlugin() override;

  // Creates the hidden message-only window used to marshal work back onto the
  // platform thread. Must be called on the platform thread (from
  // RegisterWithRegistrar).
  void InitPlatformThreadRunner();

  // Non-copyable.
  CarambaVpnPlugin(const CarambaVpnPlugin&) = delete;
  CarambaVpnPlugin& operator=(const CarambaVpnPlugin&) = delete;

 private:
  // MethodChannel com.caramba/vpn: connect / disconnect / status.
  void HandleMethodCall(
      const flutter::MethodCall<flutter::EncodableValue>& call,
      std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result);

  // configure(panelUrl, subscriptionUuid|subscriptionId, accessToken): store
  // the auth/config seam and push it into the core (CarambaConfigure). Called
  // before the first connect; idempotent.
  void Configure(const std::string& panel_url,
                 const std::string& subscription_id,
                 const std::string& access_token);
  // connect(serverId, serverName, countryCode): ensure the core handle, set
  // tunFd = -1, bring the tunnel up, and start the poll loop.
  void Connect(const std::string& server_id);
  // connectRaw(rawConfig, format, label, serverId): ensure the core handle,
  // import the raw subscription, set tunFd = -1, bring the tunnel up (serverId
  // is the ABI v2 pin of the CARAMBA selector to one node of the imported
  // config; empty means automatic), and start the poll loop.
  void ConnectRaw(const std::string& raw_config, const std::string& format,
                  const std::string& server_id);
  // ABI v2 policy + capture mode. Stored on the plugin and pushed into the core
  // in EnsureCore (and immediately when the core already exists).
  void ApplyPolicy();
  void ApplyTunnelMode();
  // disconnect(): bring the tunnel down and stop the poll loop.
  void Disconnect();

  // ~1 Hz loop: pushes status (stage transitions) and traffic snapshots to the
  // EventChannel sinks while running.
  void PollLoop();
  void StopPolling();

  // Emits a synthetic status map onto the status sink (used for connecting /
  // error states the core has not surfaced yet).
  void EmitStage(const std::string& stage, const std::string& detail);

  // Lazily loads libcaramba_core.dll and creates the core handle. Returns true
  // on success; on failure emits an error stage and returns false.
  bool EnsureCore();

  // Platform-thread marshaling. flutter::EventSink and MethodChannel objects
  // are NOT thread-safe and must only be touched on the platform (main) thread.
  // The poll worker and any non-platform-thread caller must funnel sink sends
  // through PostToPlatformThread, which queues the closure and wakes the hidden
  // message window so its WndProc runs the closure on the platform thread.
  void PostToPlatformThread(std::function<void()> task);
  // Drains the pending-task queue; invoked only from the platform thread
  // (message-window WndProc).
  void DrainPlatformTasks();
  // Sends a status/traffic map to the corresponding sink. MUST run on the
  // platform thread (call only via PostToPlatformThread or from a method-call
  // handler, which the embedder already dispatches on the platform thread).
  void SendStatusOnPlatformThread(flutter::EncodableMap map);
  void SendTrafficOnPlatformThread(flutter::EncodableMap map);

  // WndProc for the hidden message-only task-runner window.
  static LRESULT CALLBACK TaskWindowProc(HWND hwnd, UINT msg, WPARAM wparam,
                                         LPARAM lparam);

  CarambaCoreFfi core_;
  CarambaHandle handle_ = 0;

  std::unique_ptr<flutter::EventSink<flutter::EncodableValue>> status_sink_;
  std::unique_ptr<flutter::EventSink<flutter::EncodableValue>> traffic_sink_;
  std::mutex sink_mutex_;

  std::thread poll_thread_;
  std::atomic<bool> polling_{false};

  // last_stage_ is read/written from both the platform thread (status(),
  // onListen, EmitStage) and the poll worker; every access is guarded by
  // stage_mutex_. Do NOT reuse sink_mutex_ (which guards the sinks) for this.
  std::string last_stage_ = "disconnected";
  std::mutex stage_mutex_;

  // Hidden message-only window + task queue used to run closures on the
  // platform thread (see PostToPlatformThread). The window is created on the
  // platform thread in InitPlatformThreadRunner and destroyed in the
  // destructor (which also runs on the platform thread).
  HWND task_window_ = nullptr;
  std::deque<std::function<void()>> pending_tasks_;
  std::mutex tasks_mutex_;

  // Auth/config seam captured from configure(); applied in EnsureCore.
  std::string panel_url_;
  std::string subscription_id_;
  std::string access_token_;
  bool configured_ = false;

  // ABI v2 seam captured from setPolicy() / setTunnelMode() before the core
  // exists. Empty policy_json_ means "not set"; tunnel_mode_ empty means the
  // core default (tun).
  std::string policy_json_;
  std::string tunnel_mode_;
  int mixed_port_ = 7890;
};

}  // namespace caramba_vpn

#endif  // FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_H_
