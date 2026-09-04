// caramba_core_ffi.h — runtime loader for libcaramba_core.dll (Windows).
//
// The Go engine is a cgo c-shared DLL. We load it at runtime with LoadLibrary so
// the plugin links cleanly even when the DLL is resolved next to the host .exe
// (CMake bundles it there alongside wintun.dll). All exported symbols follow the
// caramba_core.h C ABI; returned strings are freed via CarambaFreeString.

#ifndef FLUTTER_PLUGIN_CARAMBA_CORE_FFI_WINDOWS_H_
#define FLUTTER_PLUGIN_CARAMBA_CORE_FFI_WINDOWS_H_

#include <windows.h>

#include <string>

extern "C" {
#include "caramba_core.h"
}

namespace caramba_vpn {

// CarambaCoreFfi resolves the libcaramba_core.dll exports lazily. Load() is
// idempotent; ok() reports whether every required symbol resolved.
class CarambaCoreFfi {
 public:
  using NewFn = CarambaHandle (*)(const char*, const char*, const char*,
                                  const char*);
  // Configure/SetTunFd/Down return a CarambaFreeString-owned JSON error string
  // ({ "error": ... }) on failure, or NULL on success.
  using ConfigureFn = char* (*)(CarambaHandle, const char*, const char*,
                                const char*);
  // ABI v4: the whole session — access + refresh + access expiry (unix seconds,
  // 0 = unknown). Optional for the same reason the ABI v2 symbols are: a DLL on
  // disk may predate the app. A null pointer here means the core can only be
  // given a ~15-minute access token with nothing to renew it with, so the
  // caller falls back to ConfigureFn ONLY when there is no refresh to lose.
  using ConfigureSessionFn = char* (*)(CarambaHandle, const char*, const char*,
                                       const char*, const char*, long long);
  using SetTunFdFn = char* (*)(CarambaHandle, int);
  // ABI v2. Resolved OPTIONALLY: a DLL built before the policy/probe wave does
  // not export them, and a hard check would block the whole desktop path. A null
  // pointer here means "old core"; the caller answers with a clear error.
  using SetTunnelModeFn = char* (*)(CarambaHandle, const char*, int);
  using SetPolicyFn = char* (*)(CarambaHandle, const char*);
  using ProbeFn = char* (*)(CarambaHandle, int);
  // ABI v3, CSM/1: one JSON string in, one JSON string out. Optional for the
  // same reason as the ABI v2 symbols: a core built before CSM/1 does not carry
  // them, and the client degrades to "CSM unavailable on this build".
  using JsonCallFn = char* (*)(CarambaHandle, const char*);
  // ABI v3 reads: handle in, JSON out. CsmState and CsmLadder take no argument
  // because they apply nothing and request nothing over the network.
  using HandleCallFn = char* (*)(CarambaHandle);
  // ABI v3: handle plus one integer. CarambaCsmRefresh takes a timeout in
  // seconds because a fetch cycle climbs the ladder and must be bounded.
  using HandleIntCallFn = char* (*)(CarambaHandle, int);
  // ImportSubscription parses a raw subscription (rawConfig in the given format)
  // into a mihomo config and stores it as the imported source. Returns a
  // CarambaFreeString-owned JSON string; on failure it carries an "error" field.
  using ImportSubscriptionFn = char* (*)(CarambaHandle, const char*,
                                         const char*);
  using UpFn = char* (*)(CarambaHandle, const char*);
  using DownFn = char* (*)(CarambaHandle);
  using StatusFn = char* (*)(CarambaHandle);
  using TrafficFn = char* (*)(CarambaHandle);
  using FreeStringFn = void (*)(char*);

  CarambaCoreFfi() = default;
  ~CarambaCoreFfi() {
    if (module_ != nullptr) {
      FreeLibrary(module_);
      module_ = nullptr;
    }
  }

  // Load resolves libcaramba_core.dll (searched next to the host .exe). Returns
  // true once all symbols are bound.
  bool Load() {
    if (module_ != nullptr) {
      return ok();
    }
    module_ = LoadLibraryW(L"libcaramba_core.dll");
    if (module_ == nullptr) {
      return false;
    }
    New = reinterpret_cast<NewFn>(GetProcAddress(module_, "CarambaNew"));
    Configure = reinterpret_cast<ConfigureFn>(
        GetProcAddress(module_, "CarambaConfigure"));
    // ABI v4, optional: absent in a core built before the session seam.
    ConfigureSession = reinterpret_cast<ConfigureSessionFn>(
        GetProcAddress(module_, "CarambaConfigureSession"));
    SetTunFd =
        reinterpret_cast<SetTunFdFn>(GetProcAddress(module_, "CarambaSetTunFd"));
    // ABI v2, optional: absent in a core built before the policy/probe wave.
    SetTunnelMode = reinterpret_cast<SetTunnelModeFn>(
        GetProcAddress(module_, "CarambaSetTunnelMode"));
    SetPolicy = reinterpret_cast<SetPolicyFn>(
        GetProcAddress(module_, "CarambaSetPolicy"));
    Probe = reinterpret_cast<ProbeFn>(GetProcAddress(module_, "CarambaProbe"));
    DeviceKeygen = reinterpret_cast<JsonCallFn>(
        GetProcAddress(module_, "CarambaDeviceKeygen"));
    DeviceSign = reinterpret_cast<JsonCallFn>(
        GetProcAddress(module_, "CarambaDeviceSign"));
    DeviceAgree = reinterpret_cast<JsonCallFn>(
        GetProcAddress(module_, "CarambaDeviceAgree"));
    CsmRequestSettings = reinterpret_cast<JsonCallFn>(
        GetProcAddress(module_, "CarambaCsmRequestSettings"));
    CsmState = reinterpret_cast<HandleCallFn>(
        GetProcAddress(module_, "CarambaCsmState"));
    CsmLadder = reinterpret_cast<HandleCallFn>(
        GetProcAddress(module_, "CarambaCsmLadder"));
    CsmEnroll = reinterpret_cast<JsonCallFn>(
        GetProcAddress(module_, "CarambaCsmEnroll"));
    CsmRefresh = reinterpret_cast<HandleIntCallFn>(
        GetProcAddress(module_, "CarambaCsmRefresh"));
    CsmSetLadder = reinterpret_cast<JsonCallFn>(
        GetProcAddress(module_, "CarambaCsmSetLadder"));
    CsmAnswerCatalogChange = reinterpret_cast<JsonCallFn>(
        GetProcAddress(module_, "CarambaCsmAnswerCatalogChange"));
    CsmSelectProfile = reinterpret_cast<JsonCallFn>(
        GetProcAddress(module_, "CarambaCsmSelectProfile"));
    ImportSubscription = reinterpret_cast<ImportSubscriptionFn>(
        GetProcAddress(module_, "CarambaImportSubscription"));
    Up = reinterpret_cast<UpFn>(GetProcAddress(module_, "CarambaUp"));
    Down = reinterpret_cast<DownFn>(GetProcAddress(module_, "CarambaDown"));
    Status =
        reinterpret_cast<StatusFn>(GetProcAddress(module_, "CarambaStatus"));
    Traffic =
        reinterpret_cast<TrafficFn>(GetProcAddress(module_, "CarambaTraffic"));
    FreeString = reinterpret_cast<FreeStringFn>(
        GetProcAddress(module_, "CarambaFreeString"));
    return ok();
  }

  bool ok() const {
    return New != nullptr && Configure != nullptr && SetTunFd != nullptr &&
           ImportSubscription != nullptr && Up != nullptr && Down != nullptr &&
           Status != nullptr && Traffic != nullptr && FreeString != nullptr;
  }

  // TakeString copies an FFI-owned string and frees it via CarambaFreeString.
  std::string TakeString(char* s) const {
    if (s == nullptr) {
      return std::string();
    }
    std::string out(s);
    if (FreeString != nullptr) {
      FreeString(s);
    }
    return out;
  }

  // DropString frees an FFI-owned string we do not need to read (e.g. the NULL
  // or error result of Configure/SetTunFd/Down). Safe with NULL.
  void DropString(char* s) const {
    if (s != nullptr && FreeString != nullptr) {
      FreeString(s);
    }
  }

  NewFn New = nullptr;
  ConfigureFn Configure = nullptr;
  ConfigureSessionFn ConfigureSession = nullptr;
  SetTunFdFn SetTunFd = nullptr;
  SetTunnelModeFn SetTunnelMode = nullptr;  // optional (ABI v2)
  SetPolicyFn SetPolicy = nullptr;          // optional (ABI v2)
  ProbeFn Probe = nullptr;                  // optional (ABI v2)
  JsonCallFn DeviceKeygen = nullptr;        // optional (ABI v3)
  JsonCallFn DeviceSign = nullptr;          // optional (ABI v3)
  JsonCallFn DeviceAgree = nullptr;         // optional (ABI v3)
  JsonCallFn CsmRequestSettings = nullptr;  // optional (ABI v3)
  HandleCallFn CsmState = nullptr;          // optional (ABI v3)
  HandleCallFn CsmLadder = nullptr;         // optional (ABI v3)
  JsonCallFn CsmEnroll = nullptr;           // optional (ABI v3)
  HandleIntCallFn CsmRefresh = nullptr;     // optional (ABI v3)
  JsonCallFn CsmSetLadder = nullptr;        // optional (ABI v3)
  JsonCallFn CsmAnswerCatalogChange = nullptr;  // optional (ABI v3)
  JsonCallFn CsmSelectProfile = nullptr;    // optional (ABI v3)
  ImportSubscriptionFn ImportSubscription = nullptr;
  UpFn Up = nullptr;
  DownFn Down = nullptr;
  StatusFn Status = nullptr;
  TrafficFn Traffic = nullptr;
  FreeStringFn FreeString = nullptr;

 private:
  HMODULE module_ = nullptr;
};

}  // namespace caramba_vpn

#endif  // FLUTTER_PLUGIN_CARAMBA_CORE_FFI_WINDOWS_H_
