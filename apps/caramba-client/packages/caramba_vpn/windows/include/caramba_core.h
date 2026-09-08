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

// CarambaConfigureSession (ABI v4) injects the WHOLE session: access token,
// refresh token, and when the access token expires (unix seconds; <= 0 means
// "unknown" and the core reads the JWT's own exp claim). refreshToken may be
// NULL/"" and then this behaves exactly like CarambaConfigure — which is to say
// the session dies in ~15 minutes with nothing to renew it. Returns NULL on
// success, or a CarambaFreeString-owned JSON error string on failure.
CARAMBA_API char* CarambaConfigureSession(CarambaHandle h,
                                          const char* panelUrl,
                                          const char* subscriptionUuid,
                                          const char* accessToken,
                                          const char* refreshToken,
                                          long long accessExpiryUnix);

// CarambaSetTunFd passes the platform TUN fd to the engine BEFORE CarambaUp.
// On desktop pass -1: mihomo creates the TUN itself (wintun/tun), which needs
// elevated privileges. Returns NULL on success, or a CarambaFreeString-owned
// JSON error string on failure.
CARAMBA_API char* CarambaSetTunFd(CarambaHandle h, int fd);

// CarambaImportSubscription parses a raw subscription (raw, in the given format:
// auto|clash|singbox|v2ray|uri) into a mihomo config and stores it as the
// imported source, so a subsequent CarambaUp(h, "") raises that config without
// touching the panel. Always returns a non-NULL, CarambaFreeString-owned JSON
// string; on failure it carries an "error" field (parse it, do NOT test NULL).
CARAMBA_API char* CarambaImportSubscription(CarambaHandle h,
                                            const char* raw,
                                            const char* format);

// CarambaSetTunnelMode switches how traffic is captured.
//   mode = "tun" (or "" / NULL): system TUN inbound, needs privileges
//          (root / CAP_NET_ADMIN, administrator on Windows);
//   mode = "proxy": local mixed inbound (SOCKS5 + HTTP) on 127.0.0.1:port with
//          NO privileges; the app or the OS proxy settings steer traffic into it.
// port <= 0 keeps the default (7890) and only matters in proxy mode. Applied at
// the next CarambaUp. Returns NULL on success, or a CarambaFreeString-owned
// JSON error string.
CARAMBA_API char* CarambaSetTunnelMode(CarambaHandle h, const char* mode,
                                       int port);

// CarambaSetPolicy applies the app-side CoreConfig before CarambaUp (ABI v2).
// json carries the optional fields protocol / preset / relay / stack / mtu /
// ipv6 / fakeIp / killSwitch / dns / split; unknown fields are ignored and
// absent fields keep their current value. Returns NULL on success, or a
// CarambaFreeString-owned JSON error string.
CARAMBA_API char* CarambaSetPolicy(CarambaHandle h, const char* json);

// CarambaProbe measures the latency of every proxy node of the currently loaded
// config (imported or panel) WITHOUT raising the tunnel (ABI v2). Blocking for
// up to timeoutMs. Always returns a non-NULL, CarambaFreeString-owned JSON
// string: { "servers": [ { "id", "name", "type", "server", "port", "country",
// "latencyMs" } ] } with latencyMs = -1 on timeout, or { "error": ... }.
CARAMBA_API char* CarambaProbe(CarambaHandle h, int timeoutMs);

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

// CarambaFree stops the tunnel and releases the core behind the handle. Safe to
// call with an unknown handle.
CARAMBA_API void CarambaFree(CarambaHandle h);

// CarambaFreeString releases a string returned by CarambaUp / CarambaStatus /
// CarambaTraffic. Safe to call with NULL.
CARAMBA_API void CarambaFreeString(char* s);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // CARAMBA_CORE_H_
