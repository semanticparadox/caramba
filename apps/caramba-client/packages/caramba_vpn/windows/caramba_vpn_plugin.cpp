// caramba_vpn_plugin.cpp — Windows implementation of the exarobot VPN plugin.
//
// Registers the com.caramba/vpn MethodChannel + status/traffic EventChannels,
// bridges to libcaramba_core.dll (the cgo Go engine running mihomo with a
// wintun TUN), and drives stage + ~1 Hz traffic events from the FFI.

#include "caramba_vpn_plugin.h"

#include <flutter/encodable_value.h>
#include <flutter/event_stream_handler_functions.h>
#include <flutter/standard_method_codec.h>

#include <chrono>
#include <memory>
#include <string>
#include <thread>

#include "caramba_json.h"

namespace caramba_vpn {

namespace {

constexpr char kMethodChannel[] = "com.caramba/vpn";
constexpr char kStatusChannel[] = "com.caramba/vpn/status";
constexpr char kTrafficChannel[] = "com.caramba/vpn/traffic";

// Reads a string argument from a MethodCall argument map; empty if absent.
std::string ArgString(const flutter::EncodableMap* args, const char* key) {
  if (args == nullptr) {
    return std::string();
  }
  auto it = args->find(flutter::EncodableValue(key));
  if (it == args->end()) {
    return std::string();
  }
  if (const auto* s = std::get_if<std::string>(&it->second)) {
    return *s;
  }
  return std::string();
}

}  // namespace

void CarambaVpnPlugin::RegisterWithRegistrar(
    flutter::PluginRegistrarWindows* registrar) {
  auto plugin = std::make_unique<CarambaVpnPlugin>();

  auto method_channel =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          registrar->messenger(), kMethodChannel,
          &flutter::StandardMethodCodec::GetInstance());

  CarambaVpnPlugin* plugin_ptr = plugin.get();
  method_channel->SetMethodCallHandler(
      [plugin_ptr](const auto& call, auto result) {
        plugin_ptr->HandleMethodCall(call, std::move(result));
      });

  // Status EventChannel: stage transitions.
  auto status_channel =
      std::make_unique<flutter::EventChannel<flutter::EncodableValue>>(
          registrar->messenger(), kStatusChannel,
          &flutter::StandardMethodCodec::GetInstance());
  status_channel->SetStreamHandler(
      std::make_unique<flutter::StreamHandlerFunctions<flutter::EncodableValue>>(
          [plugin_ptr](const flutter::EncodableValue*,
                       std::unique_ptr<flutter::EventSink<flutter::EncodableValue>>
                           events)
              -> std::unique_ptr<
                  flutter::StreamHandlerError<flutter::EncodableValue>> {
            std::lock_guard<std::mutex> lock(plugin_ptr->sink_mutex_);
            plugin_ptr->status_sink_ = std::move(events);
            // Re-emit the last known stage so a fresh subscriber renders now.
            plugin_ptr->status_sink_->Success(flutter::EncodableValue(
                flutter::EncodableMap{
                    {flutter::EncodableValue("stage"),
                     flutter::EncodableValue(plugin_ptr->last_stage_)},
                    {flutter::EncodableValue("detail"),
                     flutter::EncodableValue()},
                    {flutter::EncodableValue("connectedSinceMs"),
                     flutter::EncodableValue(static_cast<int64_t>(0))},
                }));
            return nullptr;
          },
          [plugin_ptr](const flutter::EncodableValue*)
              -> std::unique_ptr<
                  flutter::StreamHandlerError<flutter::EncodableValue>> {
            std::lock_guard<std::mutex> lock(plugin_ptr->sink_mutex_);
            plugin_ptr->status_sink_.reset();
            return nullptr;
          }));

  // Traffic EventChannel: ~1 Hz throughput snapshots.
  auto traffic_channel =
      std::make_unique<flutter::EventChannel<flutter::EncodableValue>>(
          registrar->messenger(), kTrafficChannel,
          &flutter::StandardMethodCodec::GetInstance());
  traffic_channel->SetStreamHandler(
      std::make_unique<flutter::StreamHandlerFunctions<flutter::EncodableValue>>(
          [plugin_ptr](const flutter::EncodableValue*,
                       std::unique_ptr<flutter::EventSink<flutter::EncodableValue>>
                           events)
              -> std::unique_ptr<
                  flutter::StreamHandlerError<flutter::EncodableValue>> {
            std::lock_guard<std::mutex> lock(plugin_ptr->sink_mutex_);
            plugin_ptr->traffic_sink_ = std::move(events);
            return nullptr;
          },
          [plugin_ptr](const flutter::EncodableValue*)
              -> std::unique_ptr<
                  flutter::StreamHandlerError<flutter::EncodableValue>> {
            std::lock_guard<std::mutex> lock(plugin_ptr->sink_mutex_);
            plugin_ptr->traffic_sink_.reset();
            return nullptr;
          }));

  registrar->AddPlugin(std::move(plugin));
}

CarambaVpnPlugin::CarambaVpnPlugin() = default;

CarambaVpnPlugin::~CarambaVpnPlugin() {
  StopPolling();
  if (handle_ != 0 && core_.ok() && core_.Down != nullptr) {
    core_.DropString(core_.Down(handle_));
  }
}

void CarambaVpnPlugin::HandleMethodCall(
    const flutter::MethodCall<flutter::EncodableValue>& call,
    std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result) {
  const std::string& method = call.method_name();
  const auto* args = std::get_if<flutter::EncodableMap>(call.arguments());

  if (method == "configure") {
    const std::string panel_url = ArgString(args, "panelUrl");
    // The app's VpnConfig.toArgs() sends subscriptionUuid; the plugin facade
    // sends subscriptionId. Accept either on the same channel.
    std::string subscription_id = ArgString(args, "subscriptionUuid");
    if (subscription_id.empty()) {
      subscription_id = ArgString(args, "subscriptionId");
    }
    const std::string access_token = ArgString(args, "accessToken");
    Configure(panel_url, subscription_id, access_token);
    result->Success();
    return;
  }
  if (method == "connect") {
    const std::string server_id = ArgString(args, "serverId");
    Connect(server_id);
    result->Success();
    return;
  }
  if (method == "disconnect") {
    Disconnect();
    result->Success();
    return;
  }
  if (method == "status") {
    flutter::EncodableMap map{
        {flutter::EncodableValue("stage"),
         flutter::EncodableValue(last_stage_)},
        {flutter::EncodableValue("detail"), flutter::EncodableValue()},
        {flutter::EncodableValue("connectedSinceMs"),
         flutter::EncodableValue(static_cast<int64_t>(0))},
    };
    result->Success(flutter::EncodableValue(map));
    return;
  }
  result->NotImplemented();
}

void CarambaVpnPlugin::Configure(const std::string& panel_url,
                                 const std::string& subscription_id,
                                 const std::string& access_token) {
  panel_url_ = panel_url;
  subscription_id_ = subscription_id;
  access_token_ = access_token;
  configured_ = false;  // re-apply on (re)creation / next EnsureCore.
  // If the core already exists (re-configure after a token refresh), push the
  // new seam immediately.
  if (handle_ != 0 && core_.Configure != nullptr) {
    core_.DropString(core_.Configure(handle_, panel_url_.c_str(),
                                     subscription_id_.c_str(),
                                     access_token_.c_str()));
    configured_ = true;
  }
}

bool CarambaVpnPlugin::EnsureCore() {
  if (handle_ != 0) {
    return true;
  }
  if (!core_.Load()) {
    EmitStage("error", "exarobot core library not found");
    return false;
  }
  // panelURL seeds the core; subURL/workDir/tokenPath default inside the core
  // when empty. The live session (subscription UUID + JWT) is applied via
  // CarambaConfigure below.
  handle_ = core_.New(panel_url_.c_str(), "", "", "");
  if (handle_ == 0) {
    EmitStage("error", "exarobot core init failed");
    return false;
  }
  if (!configured_ && core_.Configure != nullptr &&
      (!panel_url_.empty() || !access_token_.empty())) {
    core_.DropString(core_.Configure(handle_, panel_url_.c_str(),
                                     subscription_id_.c_str(),
                                     access_token_.c_str()));
    configured_ = true;
  }
  return true;
}

void CarambaVpnPlugin::Connect(const std::string& server_id) {
  EmitStage("connecting", "");

  if (!EnsureCore()) {
    return;
  }

  // Desktop: mihomo owns the TUN (wintun). Pass -1, never establish an fd here.
  // SetTunFd returns NULL on success or an FFI-owned error string; drop it.
  core_.DropString(core_.SetTunFd(handle_, -1));

  // CarambaUp always returns a non-NULL JSON string: api.UpResult on success or
  // { "error": ... } on failure. Parse the "error" field rather than testing
  // for NULL.
  std::string up_json = core_.TakeString(core_.Up(handle_, server_id.c_str()));
  std::string up_error;
  if (up_json.empty() || json::GetString(up_json, "error", &up_error)) {
    EmitStage("error", up_error.empty() ? "tunnel failed to start" : up_error);
    return;
  }

  // Stage transitions and traffic are driven by the poll loop reading the FFI.
  if (!polling_.load()) {
    polling_.store(true);
    poll_thread_ = std::thread(&CarambaVpnPlugin::PollLoop, this);
  }
}

void CarambaVpnPlugin::Disconnect() {
  if (handle_ != 0 && core_.Down != nullptr) {
    core_.DropString(core_.Down(handle_));
  }
  StopPolling();
  EmitStage("disconnected", "");
}

void CarambaVpnPlugin::StopPolling() {
  polling_.store(false);
  if (poll_thread_.joinable()) {
    poll_thread_.join();
  }
}

void CarambaVpnPlugin::PollLoop() {
  // Runs on a worker thread: the FFI Status/Traffic calls may block, so we keep
  // them off the platform thread. The EventSink sends are guarded by
  // sink_mutex_; the sink itself dispatches the encoded message through the
  // BinaryMessenger, matching the pattern used by the standard Windows plugins
  // (connectivity_plus, battery_plus) that emit periodic events from a polling
  // thread.
  while (polling_.load()) {
    if (handle_ != 0 && core_.ok()) {
      // Status: forward the FFI's channel-contract JSON verbatim.
      std::string status_json = core_.TakeString(core_.Status(handle_));
      if (!status_json.empty()) {
        std::string stage = "connecting";
        json::GetString(status_json, "stage", &stage);
        last_stage_ = stage;

        std::string detail;
        bool has_detail = json::GetString(status_json, "detail", &detail);
        int64_t since = json::GetInt(status_json, "connectedSinceMs", 0);

        flutter::EncodableMap map{
            {flutter::EncodableValue("stage"), flutter::EncodableValue(stage)},
            {flutter::EncodableValue("detail"),
             has_detail ? flutter::EncodableValue(detail)
                        : flutter::EncodableValue()},
            {flutter::EncodableValue("connectedSinceMs"),
             flutter::EncodableValue(since)},
        };
        std::lock_guard<std::mutex> lock(sink_mutex_);
        if (status_sink_) {
          status_sink_->Success(flutter::EncodableValue(map));
        }
      }

      // Traffic: forward the FFI's channel-contract JSON verbatim.
      std::string traffic_json = core_.TakeString(core_.Traffic(handle_));
      if (!traffic_json.empty()) {
        flutter::EncodableMap map{
            {flutter::EncodableValue("downBps"),
             flutter::EncodableValue(json::GetInt(traffic_json, "downBps", 0))},
            {flutter::EncodableValue("upBps"),
             flutter::EncodableValue(json::GetInt(traffic_json, "upBps", 0))},
            {flutter::EncodableValue("downTotal"),
             flutter::EncodableValue(
                 json::GetInt(traffic_json, "downTotal", 0))},
            {flutter::EncodableValue("upTotal"),
             flutter::EncodableValue(json::GetInt(traffic_json, "upTotal", 0))},
        };
        std::lock_guard<std::mutex> lock(sink_mutex_);
        if (traffic_sink_) {
          traffic_sink_->Success(flutter::EncodableValue(map));
        }
      }
    }
    std::this_thread::sleep_for(std::chrono::seconds(1));
  }
}

void CarambaVpnPlugin::EmitStage(const std::string& stage,
                                 const std::string& detail) {
  last_stage_ = stage;
  flutter::EncodableMap map{
      {flutter::EncodableValue("stage"), flutter::EncodableValue(stage)},
      {flutter::EncodableValue("detail"),
       detail.empty() ? flutter::EncodableValue()
                      : flutter::EncodableValue(detail)},
      {flutter::EncodableValue("connectedSinceMs"),
       flutter::EncodableValue(static_cast<int64_t>(0))},
  };
  std::lock_guard<std::mutex> lock(sink_mutex_);
  if (status_sink_) {
    status_sink_->Success(flutter::EncodableValue(map));
  }
}

}  // namespace caramba_vpn
