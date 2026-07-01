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
typedef char* (*caramba_set_tun_fd_fn)(CarambaHandle, int);
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
  caramba_set_tun_fd_fn SetTunFd;
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
  ffi->SetTunFd = (caramba_set_tun_fd_fn)dlsym(ffi->module, "CarambaSetTunFd");
  ffi->Up = (caramba_up_fn)dlsym(ffi->module, "CarambaUp");
  ffi->Down = (caramba_down_fn)dlsym(ffi->module, "CarambaDown");
  ffi->Status = (caramba_status_fn)dlsym(ffi->module, "CarambaStatus");
  ffi->Traffic = (caramba_traffic_fn)dlsym(ffi->module, "CarambaTraffic");
  ffi->FreeString =
      (caramba_free_string_fn)dlsym(ffi->module, "CarambaFreeString");
  return ffi->New != NULL && ffi->Configure != NULL && ffi->SetTunFd != NULL &&
         ffi->Up != NULL && ffi->Down != NULL && ffi->Status != NULL &&
         ffi->Traffic != NULL && ffi->FreeString != NULL;
}

#endif  // FLUTTER_PLUGIN_CARAMBA_CORE_FFI_LINUX_H_
