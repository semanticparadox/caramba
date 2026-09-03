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

// Window-class name and custom message for the hidden platform-thread task
// runner. WM_APP-based so it never collides with system messages.
constexpr wchar_t kTaskWindowClass[] = L"CarambaVpnTaskWindow";
constexpr UINT kRunTasksMessage = WM_APP + 0x51;

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

// Reads an int argument from a MethodCall argument map; fallback if absent.
int ArgInt(const flutter::EncodableMap* args, const char* key, int fallback) {
  if (args == nullptr) {
    return fallback;
  }
  auto it = args->find(flutter::EncodableValue(key));
  if (it == args->end()) {
    return fallback;
  }
  if (const auto* i = std::get_if<int32_t>(&it->second)) {
    return *i;
  }
  if (const auto* i = std::get_if<int64_t>(&it->second)) {
    return static_cast<int>(*i);
  }
  return fallback;
}

}  // namespace

void CarambaVpnPlugin::RegisterWithRegistrar(
    flutter::PluginRegistrarWindows* registrar) {
  auto plugin = std::make_unique<CarambaVpnPlugin>();

  // RegisterWithRegistrar runs on the platform (main) thread; create the hidden
  // task-runner window here so it is owned by the platform thread's message
  // loop and can marshal poll-worker sink sends back onto this thread.
  plugin->InitPlatformThreadRunner();

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
            // onListen is dispatched on the platform thread, so touching the
            // sink directly here is safe. Snapshot last_stage_ under its own
            // mutex (RACE #2) rather than reading it unlocked.
            std::string stage;
            {
              std::lock_guard<std::mutex> stage_lock(plugin_ptr->stage_mutex_);
              stage = plugin_ptr->last_stage_;
            }
            std::lock_guard<std::mutex> lock(plugin_ptr->sink_mutex_);
            plugin_ptr->status_sink_ = std::move(events);
            // Re-emit the last known stage so a fresh subscriber renders now.
            plugin_ptr->status_sink_->Success(flutter::EncodableValue(
                flutter::EncodableMap{
                    {flutter::EncodableValue("stage"),
                     flutter::EncodableValue(stage)},
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
  // Stop the poll worker first: it can no longer post tasks after this returns,
  // so the message window is safe to destroy. The destructor runs on the
  // platform thread (the embedder tears plugins down there), same thread that
  // created task_window_, so DestroyWindow is valid here.
  StopPolling();
  if (task_window_ != nullptr) {
    DestroyWindow(task_window_);
    task_window_ = nullptr;
  }
  UnregisterClassW(kTaskWindowClass, GetModuleHandle(nullptr));
  if (handle_ != 0 && core_.ok() && core_.Down != nullptr) {
    core_.DropString(core_.Down(handle_));
  }
}

void CarambaVpnPlugin::InitPlatformThreadRunner() {
  WNDCLASSW wc = {};
  wc.lpfnWndProc = CarambaVpnPlugin::TaskWindowProc;
  wc.hInstance = GetModuleHandle(nullptr);
  wc.lpszClassName = kTaskWindowClass;
  // RegisterClassW fails harmlessly (ERROR_CLASS_ALREADY_EXISTS) if another
  // instance already registered it; the subsequent CreateWindow still works.
  RegisterClassW(&wc);
  task_window_ = CreateWindowExW(0, kTaskWindowClass, L"", 0, 0, 0, 0, 0,
                                 HWND_MESSAGE, nullptr, GetModuleHandle(nullptr),
                                 nullptr);
  if (task_window_ != nullptr) {
    // Stash `this` so the WndProc can reach the plugin instance.
    SetWindowLongPtr(task_window_, GWLP_USERDATA,
                     reinterpret_cast<LONG_PTR>(this));
  }
}

LRESULT CALLBACK CarambaVpnPlugin::TaskWindowProc(HWND hwnd, UINT msg,
                                                  WPARAM wparam,
                                                  LPARAM lparam) {
  if (msg == kRunTasksMessage) {
    auto* self = reinterpret_cast<CarambaVpnPlugin*>(
        GetWindowLongPtr(hwnd, GWLP_USERDATA));
    if (self != nullptr) {
      self->DrainPlatformTasks();
    }
    return 0;
  }
  return DefWindowProc(hwnd, msg, wparam, lparam);
}

void CarambaVpnPlugin::PostToPlatformThread(std::function<void()> task) {
  HWND window = task_window_;
  if (window == nullptr) {
    return;  // Runner not initialized (or already torn down): drop the send.
  }
  {
    std::lock_guard<std::mutex> lock(tasks_mutex_);
    pending_tasks_.push_back(std::move(task));
  }
  // Coalesced wake: PostMessage is thread-safe and enqueues onto the platform
  // thread's message loop, which invokes TaskWindowProc -> DrainPlatformTasks.
  PostMessage(window, kRunTasksMessage, 0, 0);
}

void CarambaVpnPlugin::DrainPlatformTasks() {
  // Runs on the platform thread. Move the queue out under the lock, then run
  // the closures unlocked so a task may itself post further work.
  std::deque<std::function<void()>> tasks;
  {
    std::lock_guard<std::mutex> lock(tasks_mutex_);
    tasks.swap(pending_tasks_);
  }
  for (auto& task : tasks) {
    task();
  }
}

void CarambaVpnPlugin::SendStatusOnPlatformThread(flutter::EncodableMap map) {
  std::lock_guard<std::mutex> lock(sink_mutex_);
  if (status_sink_) {
    status_sink_->Success(flutter::EncodableValue(std::move(map)));
  }
}

void CarambaVpnPlugin::SendTrafficOnPlatformThread(flutter::EncodableMap map) {
  std::lock_guard<std::mutex> lock(sink_mutex_);
  if (traffic_sink_) {
    traffic_sink_->Success(flutter::EncodableValue(std::move(map)));
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
  if (method == "connectRaw") {
    const std::string raw_config = ArgString(args, "rawConfig");
    const std::string format = ArgString(args, "format");
    // "label" is display-only (profile name) and is not forwarded to the core.
    // "serverId" (ABI v2) pins the CARAMBA selector to one node of the imported
    // config; empty keeps the automatic choice.
    ConnectRaw(raw_config, format, ArgString(args, "serverId"));
    result->Success();
    return;
  }
  // --- generic mode (ABI v2) -------------------------------------------------
  if (method == "importSubscription") {
    // Parse a subscription and return its metadata WITHOUT raising the tunnel.
    // Uses the same core handle a later connectRaw will raise, so the import
    // stays the active config.
    if (!EnsureCore()) {
      result->Error("core_missing", "exarobot core library not found");
      return;
    }
    const std::string json = core_.TakeString(core_.ImportSubscription(
        handle_, ArgString(args, "rawConfig").c_str(),
        ArgString(args, "format").c_str()));
    result->Success(flutter::EncodableValue(json));
    return;
  }
  if (method == "probe") {
    if (!EnsureCore()) {
      result->Error("core_missing", "exarobot core library not found");
      return;
    }
    if (core_.Probe == nullptr) {
      result->Error("core_missing",
                    "CarambaProbe is missing (core predates ABI v2)");
      return;
    }
    const std::string json = core_.TakeString(
        core_.Probe(handle_, ArgInt(args, "timeoutMs", 5000)));
    result->Success(flutter::EncodableValue(json));
    return;
  }
  // --- CSM/1 device keys and the settings write (ABI v3) ---------------------
  //
  // Windows has no hardware key store the core can reach, so the core holds the
  // device keys in software and reports tier 3 HONESTLY rather than claiming
  // hardware. The symbols exist anyway: the surface is the same on all five
  // bridges, and the app does not branch by platform just to learn its own
  // device thumbprint.
  //
  // The settings write goes THROUGH THE CORE, never through a socket opened
  // here: a control plane with its own sockets bypasses the transport ladder,
  // and the app degenerates to rung R0 while the core is still climbing for a
  // configuration it can no longer change (02-SPEC.md 8.9).
  if (method == "deviceKeygen" || method == "deviceSign" ||
      method == "deviceAgree" || method == "csmRequestSettings" ||
      method == "csmEnroll" || method == "csmSetLadder" ||
      method == "csmAnswerCatalogChange") {
    if (!EnsureCore()) {
      result->Error("core_missing", "exarobot core library not found");
      return;
    }
    CarambaCoreFfi::JsonCallFn fn = nullptr;
    const char* symbol = nullptr;
    std::string payload;
    if (method == "deviceKeygen") {
      fn = core_.DeviceKeygen;
      symbol = "CarambaDeviceKeygen";
      payload = R"({"purpose":"sign","require_hardware":true})";
    } else if (method == "deviceSign") {
      fn = core_.DeviceSign;
      symbol = "CarambaDeviceSign";
      // Escaped, never concatenated raw: the value arrives on the channel
      // unvalidated, and one double quote would otherwise close the literal
      // and let the caller write sibling fields into DeviceSignRequest.
      payload = R"({"message_b64":)" +
                json::EscapeString(ArgString(args, "messageB64")) + "}";
    } else if (method == "deviceAgree") {
      fn = core_.DeviceAgree;
      symbol = "CarambaDeviceAgree";
      payload = R"({"rkv":)" + std::to_string(ArgInt(args, "rkv", 0)) +
                R"(,"peer_pub_b64":)" +
                json::EscapeString(ArgString(args, "peerPubB64")) + "}";
    } else if (method == "csmEnroll") {
      fn = core_.CsmEnroll;
      symbol = "CarambaCsmEnroll";
      payload = ArgString(args, "json");
      if (payload.empty()) {
        payload = "{}";
      }
    } else if (method == "csmSetLadder") {
      fn = core_.CsmSetLadder;
      symbol = "CarambaCsmSetLadder";
      payload = ArgString(args, "json");
      if (payload.empty()) {
        payload = "{}";
      }
    } else if (method == "csmAnswerCatalogChange") {
      fn = core_.CsmAnswerCatalogChange;
      symbol = "CarambaCsmAnswerCatalogChange";
      payload = ArgString(args, "json");
      if (payload.empty()) {
        payload = "{}";
      }
    } else {
      fn = core_.CsmRequestSettings;
      symbol = "CarambaCsmRequestSettings";
      payload = ArgString(args, "json");
      if (payload.empty()) {
        payload = "{}";
      }
    }
    if (fn == nullptr) {
      result->Error("core_missing", std::string(symbol) +
                                        " is missing (core predates ABI v3)");
      return;
    }
    const std::string json = core_.TakeString(fn(handle_, payload.c_str()));
    result->Success(flutter::EncodableValue(json));
    return;
  }
  // Reads of what the core already verified: no socket, nothing applied. The
  // state snapshot carries the trusted catalog's resource projection, without
  // which the client cannot notice the posture narrowing that arrives in the
  // catalog rather than in a setting (02-SPEC.md 7.7.1); the ladder call lifts
  // the LOCAL attempt history, which INV-17 requires on screen and 02-SPEC.md
  // 7.10 forbids reporting to the operator.
  if (method == "csmRefresh") {
    if (!EnsureCore()) {
      result->Error("core_missing", "exarobot core library not found");
      return;
    }
    if (core_.CsmRefresh == nullptr) {
      result->Error("core_missing",
                    "CarambaCsmRefresh is missing (core predates ABI v3)");
      return;
    }
    const std::string json = core_.TakeString(core_.CsmRefresh(
        handle_, static_cast<int>(ArgInt(args, "timeoutSec", 30))));
    result->Success(flutter::EncodableValue(json));
    return;
  }
  if (method == "csmSelectProfile") {
    // 02-SPEC.md 1.2: every profile state store MUST be keyed by pid. One
    // store per app would put the second operator's pinned root, device
    // registration and monotonic marks on top of the first operator's.
    if (!EnsureCore()) {
      result->Error("core_missing", "exarobot core library not found");
      return;
    }
    if (core_.CsmSelectProfile == nullptr) {
      result->Error("core_missing",
                    "CarambaCsmSelectProfile is missing (core predates ABI v3)");
      return;
    }
    const std::string json = core_.TakeString(
        core_.CsmSelectProfile(handle_, ArgString(args, "profileKey").c_str()));
    result->Success(flutter::EncodableValue(json));
    return;
  }
  if (method == "csmState" || method == "csmLadder") {
    if (!EnsureCore()) {
      result->Error("core_missing", "exarobot core library not found");
      return;
    }
    const bool ladder = method == "csmLadder";
    CarambaCoreFfi::HandleCallFn fn = ladder ? core_.CsmLadder : core_.CsmState;
    if (fn == nullptr) {
      result->Error("core_missing",
                    std::string(ladder ? "CarambaCsmLadder" : "CarambaCsmState") +
                        " is missing (core predates ABI v3)");
      return;
    }
    result->Success(flutter::EncodableValue(core_.TakeString(fn(handle_))));
    return;
  }
  if (method == "setPolicy") {
    policy_json_ = ArgString(args, "json");
    if (handle_ != 0 && core_.SetPolicy == nullptr) {
      result->Error("core_missing",
                    "CarambaSetPolicy is missing (core predates ABI v2)");
      return;
    }
    ApplyPolicy();
    result->Success();
    return;
  }
  if (method == "setTunnelMode") {
    tunnel_mode_ = ArgString(args, "mode");
    mixed_port_ = ArgInt(args, "port", 7890);
    if (handle_ != 0 && core_.SetTunnelMode == nullptr) {
      result->Error("core_missing",
                    "CarambaSetTunnelMode is missing (core predates ABI v2)");
      return;
    }
    ApplyTunnelMode();
    result->Success();
    return;
  }
  if (method == "disconnect") {
    Disconnect();
    result->Success();
    return;
  }
  if (method == "status") {
    std::string stage;
    {
      std::lock_guard<std::mutex> lock(stage_mutex_);
      stage = last_stage_;
    }
    flutter::EncodableMap map{
        {flutter::EncodableValue("stage"), flutter::EncodableValue(stage)},
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
  // ABI v2: policy + capture mode go in right after the handle exists, before
  // any Up. A missing symbol (pre-ABI-v2 core) is skipped silently here; the
  // explicit setPolicy/setTunnelMode calls surface the error to Dart.
  ApplyPolicy();
  ApplyTunnelMode();
  return true;
}

void CarambaVpnPlugin::ApplyPolicy() {
  if (handle_ == 0 || core_.SetPolicy == nullptr || policy_json_.empty()) {
    return;
  }
  core_.DropString(core_.SetPolicy(handle_, policy_json_.c_str()));
}

void CarambaVpnPlugin::ApplyTunnelMode() {
  if (handle_ == 0 || core_.SetTunnelMode == nullptr || tunnel_mode_.empty()) {
    return;
  }
  core_.DropString(
      core_.SetTunnelMode(handle_, tunnel_mode_.c_str(), mixed_port_));
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

void CarambaVpnPlugin::ConnectRaw(const std::string& raw_config,
                                  const std::string& format,
                                  const std::string& server_id) {
  EmitStage("connecting", "");

  if (!EnsureCore()) {
    return;
  }

  // rawSub path: import the raw subscription into the core instead of relying on
  // the panel session. CarambaImportSubscription returns a non-NULL,
  // FFI-owned JSON string; a present "error" field means failure.
  std::string import_json = core_.TakeString(
      core_.ImportSubscription(handle_, raw_config.c_str(), format.c_str()));
  std::string import_error;
  if (import_json.empty() ||
      json::GetString(import_json, "error", &import_error)) {
    EmitStage("error",
              import_error.empty() ? "failed to import subscription"
                                   : import_error);
    return;
  }

  // Desktop: mihomo owns the TUN (wintun). Pass -1, never establish an fd here.
  core_.DropString(core_.SetTunFd(handle_, -1));

  // serverID (ABI v2) pins the CARAMBA selector to one node of the imported
  // config; empty keeps the automatic choice. CarambaUp always returns non-NULL
  // JSON.
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
  // them off the platform thread. flutter::EventSink is NOT thread-safe, so the
  // actual Success() sends are marshaled onto the platform thread via
  // PostToPlatformThread (RACE #1). last_stage_ is written here under
  // stage_mutex_ (RACE #2).
  while (polling_.load()) {
    if (handle_ != 0 && core_.ok()) {
      // Status: forward the FFI's channel-contract JSON verbatim.
      std::string status_json = core_.TakeString(core_.Status(handle_));
      std::string stage = "connecting";
      if (!status_json.empty()) {
        json::GetString(status_json, "stage", &stage);
        {
          std::lock_guard<std::mutex> stage_lock(stage_mutex_);
          last_stage_ = stage;
        }

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
        PostToPlatformThread(
            [this, map = std::move(map)]() mutable {
              SendStatusOnPlatformThread(std::move(map));
            });
      }

      // Traffic: forward the FFI's channel-contract JSON verbatim. Only emit
      // while the tunnel is up — no point pushing traffic ticks when not
      // connected (the stage machine drives the UI otherwise).
      if (stage == "connected") {
        std::string traffic_json = core_.TakeString(core_.Traffic(handle_));
        if (!traffic_json.empty()) {
          flutter::EncodableMap map{
              {flutter::EncodableValue("downBps"),
               flutter::EncodableValue(
                   json::GetInt(traffic_json, "downBps", 0))},
              {flutter::EncodableValue("upBps"),
               flutter::EncodableValue(json::GetInt(traffic_json, "upBps", 0))},
              {flutter::EncodableValue("downTotal"),
               flutter::EncodableValue(
                   json::GetInt(traffic_json, "downTotal", 0))},
              {flutter::EncodableValue("upTotal"),
               flutter::EncodableValue(
                   json::GetInt(traffic_json, "upTotal", 0))},
          };
          PostToPlatformThread(
              [this, map = std::move(map)]() mutable {
                SendTrafficOnPlatformThread(std::move(map));
              });
        }
      }
    }
    std::this_thread::sleep_for(std::chrono::seconds(1));
  }
}

void CarambaVpnPlugin::EmitStage(const std::string& stage,
                                 const std::string& detail) {
  {
    std::lock_guard<std::mutex> stage_lock(stage_mutex_);
    last_stage_ = stage;
  }
  flutter::EncodableMap map{
      {flutter::EncodableValue("stage"), flutter::EncodableValue(stage)},
      {flutter::EncodableValue("detail"),
       detail.empty() ? flutter::EncodableValue()
                      : flutter::EncodableValue(detail)},
      {flutter::EncodableValue("connectedSinceMs"),
       flutter::EncodableValue(static_cast<int64_t>(0))},
  };
  // EmitStage is invoked both from the platform thread (method-call handlers:
  // Connect/ConnectRaw/Disconnect) and, indirectly, only from those — never
  // from the poll worker. Marshal anyway so the sink is always touched on the
  // platform thread regardless of caller, which keeps the contract uniform.
  PostToPlatformThread(
      [this, map = std::move(map)]() mutable {
        SendStatusOnPlatformThread(std::move(map));
      });
}

}  // namespace caramba_vpn
