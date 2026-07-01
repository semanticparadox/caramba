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

#include <flutter/event_channel.h>
#include <flutter/event_sink.h>
#include <flutter/method_channel.h>
#include <flutter/plugin_registrar_windows.h>

#include <atomic>
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

  CarambaCoreFfi core_;
  CarambaHandle handle_ = 0;

  std::unique_ptr<flutter::EventSink<flutter::EncodableValue>> status_sink_;
  std::unique_ptr<flutter::EventSink<flutter::EncodableValue>> traffic_sink_;
  std::mutex sink_mutex_;

  std::thread poll_thread_;
  std::atomic<bool> polling_{false};
  std::string last_stage_ = "disconnected";

  // Auth/config seam captured from configure(); applied in EnsureCore.
  std::string panel_url_;
  std::string subscription_id_;
  std::string access_token_;
  bool configured_ = false;
};

}  // namespace caramba_vpn

#endif  // FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_H_
