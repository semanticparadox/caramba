// caramba_core_ffi.h — runtime loader for libcaramba_core.so (Linux).
//
// The Go engine is a cgo c-shared object. We dlopen it at runtime (bundled next
// to the host binary under lib/) and resolve the caramba_core.h C ABI. Strings
// returned by the library are released via CarambaFreeString.

#ifndef FLUTTER_PLUGIN_CARAMBA_CORE_FFI_LINUX_H_
#define FLUTTER_PLUGIN_CARAMBA_CORE_FFI_LINUX_H_

#include <dlfcn.h>
#include <glib.h>

#include "include/caramba_core.h"

// Function-pointer aliases for the resolved exports.
typedef CarambaHandle (*caramba_new_fn)(const char*, const char*, const char*,
                                        const char*);
// Configure/SetTunFd/Down return a CarambaFreeString-owned JSON error string
// ({ "error": ... }) on failure, or NULL on success.
typedef char* (*caramba_configure_fn)(CarambaHandle, const char*, const char*,
                                      const char*);
// ABI v4: вся сессия целиком — access, refresh и срок жизни access
// (unix-секунды; 0 = «не знаю»). РЕЗОЛВИТСЯ ОПЦИОНАЛЬНО, как символы ABI v2:
// библиотека на диске может быть старее приложения. NULL здесь означает, что
// ядру можно отдать только 15-минутный access и нечем его продлить, поэтому
// откат на caramba_configure_fn допустим ТОЛЬКО когда refresh пуст.
typedef char* (*caramba_configure_session_fn)(CarambaHandle, const char*,
                                              const char*, const char*,
                                              const char*, long long);
// CarambaImportSubscription парсит сырую подписку [raw] формата [format] в
// mihomo-конфиг и сохраняет её как импортированный источник. Возвращает
// CarambaFreeString-owned JSON-строку: метаданные при успехе или { "error": ... }
// при неудаче (parse the "error" field, а не NULL).
typedef char* (*caramba_import_subscription_fn)(CarambaHandle, const char*,
                                                const char*);
typedef char* (*caramba_set_tun_fd_fn)(CarambaHandle, int);
// ABI v2. SetTunnelMode/SetPolicy отдают NULL при успехе, Probe — всегда JSON.
// Все три РЕЗОЛВЯТСЯ ОПЦИОНАЛЬНО: библиотека, собранная до ABI v2, их не несёт,
// и жёсткая проверка заблокировала бы весь desktop-путь. NULL-указатель здесь
// означает «ядро старое» — вызывающий отвечает понятной ошибкой.
typedef char* (*caramba_set_tunnel_mode_fn)(CarambaHandle, const char*, int);
typedef char* (*caramba_set_policy_fn)(CarambaHandle, const char*);
typedef char* (*caramba_probe_fn)(CarambaHandle, int);
// ABI v3, CSM/1. Каждый символ принимает и отдаёт одну строку JSON. Тоже
// ОПЦИОНАЛЬНЫЕ: библиотека, собранная до ABI v3, их не несёт, и клиент
// деградирует до «CSM недоступен в этой сборке», а не падает.
typedef char* (*caramba_json_call_fn)(CarambaHandle, const char*);
// ABI v3 reads: handle in, JSON out. CsmState и CsmLadder аргумента не берут,
// потому что ничего не применяют и ничего не запрашивают по сети.
typedef char* (*caramba_handle_call_fn)(CarambaHandle);
// ABI v3: хэндл и целое. CarambaCsmRefresh берёт таймаут в секундах, потому
// что цикл выборки лезет по лестнице и обязан быть ограничен.
typedef char* (*caramba_handle_int_call_fn)(CarambaHandle, int);
typedef char* (*caramba_up_fn)(CarambaHandle, const char*);
typedef char* (*caramba_down_fn)(CarambaHandle);
typedef char* (*caramba_status_fn)(CarambaHandle);
typedef char* (*caramba_traffic_fn)(CarambaHandle);
typedef void (*caramba_free_string_fn)(char*);

// CarambaCoreFfi holds the dlopen handle and resolved symbols.
typedef struct {
  void* module;
  caramba_new_fn New;
  caramba_configure_fn Configure;
  caramba_configure_session_fn ConfigureSession;  // optional (ABI v4)
  caramba_import_subscription_fn ImportSubscription;
  caramba_set_tun_fd_fn SetTunFd;
  caramba_set_tunnel_mode_fn SetTunnelMode;  // optional (ABI v2)
  caramba_set_policy_fn SetPolicy;           // optional (ABI v2)
  caramba_probe_fn Probe;                    // optional (ABI v2)
  caramba_json_call_fn DeviceKeygen;         // optional (ABI v3)
  caramba_json_call_fn DeviceSign;           // optional (ABI v3)
  caramba_json_call_fn DeviceAgree;          // optional (ABI v3)
  caramba_json_call_fn CsmRequestSettings;   // optional (ABI v3)
  caramba_handle_call_fn CsmState;           // optional (ABI v3)
  caramba_handle_call_fn CsmLadder;          // optional (ABI v3)
  caramba_json_call_fn CsmEnroll;            // optional (ABI v3)
  caramba_handle_int_call_fn CsmRefresh;     // optional (ABI v3)
  caramba_json_call_fn CsmSetLadder;         // optional (ABI v3)
  caramba_json_call_fn CsmAnswerCatalogChange;  // optional (ABI v3)
  caramba_json_call_fn CsmSelectProfile;     // optional (ABI v3)
  caramba_up_fn Up;
  caramba_down_fn Down;
  caramba_status_fn Status;
  caramba_traffic_fn Traffic;
  caramba_free_string_fn FreeString;
} CarambaCoreFfi;

// caramba_core_ffi_load resolves libcaramba_core.so. The loader relies on the
// runtime search path (the bundle ships the .so under lib/ next to the host
// binary, which Flutter adds to the rpath). Returns TRUE once every symbol is
// bound.
static inline gboolean caramba_core_ffi_load(CarambaCoreFfi* ffi) {
  if (ffi->module != NULL) {
    return ffi->New != NULL;
  }
  ffi->module = dlopen("libcaramba_core.so", RTLD_NOW | RTLD_LOCAL);
  if (ffi->module == NULL) {
    return FALSE;
  }
  ffi->New = (caramba_new_fn)dlsym(ffi->module, "CarambaNew");
  ffi->Configure = (caramba_configure_fn)dlsym(ffi->module, "CarambaConfigure");
  // ABI v4, optional: absent in a core built before the session seam.
  ffi->ConfigureSession = (caramba_configure_session_fn)dlsym(
      ffi->module, "CarambaConfigureSession");
  ffi->ImportSubscription = (caramba_import_subscription_fn)dlsym(
      ffi->module, "CarambaImportSubscription");
  ffi->SetTunFd = (caramba_set_tun_fd_fn)dlsym(ffi->module, "CarambaSetTunFd");
  // ABI v2, optional: absent in a core built before the policy/probe wave.
  ffi->SetTunnelMode = (caramba_set_tunnel_mode_fn)dlsym(
      ffi->module, "CarambaSetTunnelMode");
  ffi->SetPolicy =
      (caramba_set_policy_fn)dlsym(ffi->module, "CarambaSetPolicy");
  ffi->Probe = (caramba_probe_fn)dlsym(ffi->module, "CarambaProbe");
  // ABI v3, optional: absent in a core built before CSM/1.
  ffi->DeviceKeygen =
      (caramba_json_call_fn)dlsym(ffi->module, "CarambaDeviceKeygen");
  ffi->DeviceSign =
      (caramba_json_call_fn)dlsym(ffi->module, "CarambaDeviceSign");
  ffi->DeviceAgree =
      (caramba_json_call_fn)dlsym(ffi->module, "CarambaDeviceAgree");
  ffi->CsmRequestSettings =
      (caramba_json_call_fn)dlsym(ffi->module, "CarambaCsmRequestSettings");
  ffi->CsmState =
      (caramba_handle_call_fn)dlsym(ffi->module, "CarambaCsmState");
  ffi->CsmLadder =
      (caramba_handle_call_fn)dlsym(ffi->module, "CarambaCsmLadder");
  ffi->CsmEnroll =
      (caramba_json_call_fn)dlsym(ffi->module, "CarambaCsmEnroll");
  ffi->CsmRefresh =
      (caramba_handle_int_call_fn)dlsym(ffi->module, "CarambaCsmRefresh");
  ffi->CsmSetLadder =
      (caramba_json_call_fn)dlsym(ffi->module, "CarambaCsmSetLadder");
  ffi->CsmAnswerCatalogChange = (caramba_json_call_fn)dlsym(
      ffi->module, "CarambaCsmAnswerCatalogChange");
  ffi->CsmSelectProfile =
      (caramba_json_call_fn)dlsym(ffi->module, "CarambaCsmSelectProfile");
  ffi->Up = (caramba_up_fn)dlsym(ffi->module, "CarambaUp");
  ffi->Down = (caramba_down_fn)dlsym(ffi->module, "CarambaDown");
  ffi->Status = (caramba_status_fn)dlsym(ffi->module, "CarambaStatus");
  ffi->Traffic = (caramba_traffic_fn)dlsym(ffi->module, "CarambaTraffic");
  ffi->FreeString =
      (caramba_free_string_fn)dlsym(ffi->module, "CarambaFreeString");
  return ffi->New != NULL && ffi->Configure != NULL &&
         ffi->ImportSubscription != NULL && ffi->SetTunFd != NULL &&
         ffi->Up != NULL && ffi->Down != NULL && ffi->Status != NULL &&
         ffi->Traffic != NULL && ffi->FreeString != NULL;
}

#endif  // FLUTTER_PLUGIN_CARAMBA_CORE_FFI_LINUX_H_
