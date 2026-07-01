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
  using SetTunFdFn = char* (*)(CarambaHandle, int);
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
    SetTunFd =
        reinterpret_cast<SetTunFdFn>(GetProcAddress(module_, "CarambaSetTunFd"));
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
           Up != nullptr && Down != nullptr && Status != nullptr &&
           Traffic != nullptr && FreeString != nullptr;
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
  SetTunFdFn SetTunFd = nullptr;
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
