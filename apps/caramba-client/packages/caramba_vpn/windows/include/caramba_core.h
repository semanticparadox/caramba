// caramba_core.h — C ABI of the desktop caramba-core shared library.
//
// The Go engine (libs/caramba-core) is built as a cgo `c-shared` library
// (libcaramba_core.dll on Windows, libcaramba_core.so on Linux, exporting these
// `//export Caramba*` symbols). On desktop mihomo creates the TUN device itself
// (wintun on Windows, tun on Linux), so the host passes tunFd = -1 to
// CarambaSetTunFd and never establishes a descriptor of its own.
//
// All strings are UTF-8, NUL-terminated, caller-owned on the way in. Strings
// returned by the library (CarambaUp / CarambaStatus / CarambaTraffic) are
// heap-allocated by Go and MUST be released with CarambaFreeString exactly once;
// they must not be freed with the C runtime free().
//
// Handles are opaque, non-zero on success, 0 on failure. The library is
// single-instance friendly: one handle backs one api.Core for the process
// lifetime, mirroring the gomobile Client.
//
// CODE IDENTIFIERS stay 'caramba' (the user-facing brand is 'exarobot').

#ifndef CARAMBA_CORE_H_
#define CARAMBA_CORE_H_

#ifdef _WIN32
#ifdef CARAMBA_CORE_STATIC
#define CARAMBA_API
#else
#define CARAMBA_API __declspec(dllimport)
#endif
#else
#define CARAMBA_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

// Opaque client handle (cgo-exported Go pointer, surfaced as an integer token).
typedef long long CarambaHandle;

// CarambaNew creates a client bound to the panel. panelURL is required; subURL,
// workDir and tokenPath may be empty (defaults are used). Returns 0 on failure.
CARAMBA_API CarambaHandle CarambaNew(const char* panelURL,
                                     const char* subURL,
                                     const char* workDir,
                                     const char* tokenPath);

// CarambaConfigure injects the live session (JWT) + subscription UUID + panel
// URL so the core can fetch the mihomo/clash config (which already carries the
// amnezia-wg outbound) without a re-login. Returns NULL on success, or a
// CarambaFreeString-owned JSON error string ({ "error": ... }) on failure.
CARAMBA_API char* CarambaConfigure(CarambaHandle h,
                                   const char* panelUrl,
                                   const char* subscriptionUuid,
                                   const char* accessToken);

// CarambaSetTunFd passes the platform TUN fd to the engine BEFORE CarambaUp.
// On desktop pass -1: mihomo creates the TUN itself (wintun/tun), which needs
// elevated privileges. Returns NULL on success, or a CarambaFreeString-owned
// JSON error string on failure.
CARAMBA_API char* CarambaSetTunFd(CarambaHandle h, int fd);

// CarambaUp raises the tunnel to serverID (may be empty for panel/auto choice).
// Always returns a non-NULL, CarambaFreeString-owned JSON string: api.UpResult
// on success, or { "error": ... } on failure. Parse the "error" field to detect
// failure; do NOT test for NULL.
CARAMBA_API char* CarambaUp(CarambaHandle h, const char* serverID);

// CarambaDown stops the tunnel. Returns NULL on success, or a
// CarambaFreeString-owned JSON error string on failure.
CARAMBA_API char* CarambaDown(CarambaHandle h);

// CarambaStatus returns a CarambaFreeString-owned JSON string shaped for the
// channel contract: { "stage": string, "detail": string|null,
// "connectedSinceMs": int }. stage is one of disconnected | connecting |
// connected | reconnecting | error. Returns NULL on failure.
CARAMBA_API char* CarambaStatus(CarambaHandle h);

// CarambaTraffic returns a CarambaFreeString-owned JSON string shaped for the
// channel contract: { "downBps": int, "upBps": int, "downTotal": int,
// "upTotal": int } (instantaneous bytes/s + session totals from mihomo
// statistics). Returns NULL on failure.
CARAMBA_API char* CarambaTraffic(CarambaHandle h);

// CarambaFreeString releases a string returned by CarambaUp / CarambaStatus /
// CarambaTraffic. Safe to call with NULL.
CARAMBA_API void CarambaFreeString(char* s);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // CARAMBA_CORE_H_
