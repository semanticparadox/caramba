# caramba-core ABI v2 (agreed 2026-09-02, shared by the Go and plugin agents)

All functions follow the existing conventions in `libs/caramba-core/ffi/caramba_core.h`:
handles are `long`, strings are UTF-8 `char*` owned by the caller after return and released with
`CarambaFreeString`, setters return NULL on success or `{"error":"..."}`.
`mobile.Client` exposes the same surface for gomobile (Android/iOS).

## Existing (unchanged)
CarambaNew, CarambaConfigure, CarambaImportSubscription, CarambaSetTunFd, CarambaUp, CarambaDown,
CarambaStatus, CarambaTraffic, CarambaFree, CarambaFreeString, CarambaSetTunnelMode(h, mode, port).

## New
- `char* CarambaSetPolicy(long h, char* json)` / `Client.SetPolicyJSON(json string) error`
  Applies the app-side CoreConfig before Up. JSON (all fields optional, unknown ignored):
  ```json
  {"protocol":"auto|AmneziaWG|VLESS-Reality|Hysteria2|TUIC|Shadowsocks",
   "preset":"ru-smart|ru-full|telegram-only|ir-smart|by-smart|cn-smart|streaming|adblock|global|",
   "relay":"TR|KZ|FI|",
   "stack":"gvisor|system|mixed",
   "mtu":1280, "ipv6":false, "fakeIp":true, "killSwitch":true,
   "dns":{"nameservers":["https://1.1.1.1/dns-query"],"fallback":["tls://1.1.1.1:853"]},
   "split":{"mode":"off|bypass|allow","apps":["com.example.app"],"bypassDomains":["example.com"]}}
  ```
- `char* CarambaProbe(long h, int timeoutMs)` / `Client.ProbeJSON(timeoutMs int) (string, error)`
  Measures latency of every proxy node in the currently loaded config (imported or panel) without
  raising the tunnel. Returns
  `{"servers":[{"id":"<proxy name>","name":"...","type":"vless","server":"host","port":443,"country":"NL","latencyMs":42}]}`
  with `latencyMs: -1` on timeout. Uses a TCP connect (default build) or the mihomo URL test
  under `-tags mihomo`. Should run probes concurrently (bounded, e.g. 8 at a time).
- `CarambaImportSubscription` metadata JSON is the contract for the imported server list; the
  `servers` array must carry `id` (proxy name, used later as the Up serverID), `name`, `type`,
  `server`, `port`, `country` (ISO-2 if derivable from the name/emoji, else "").
- `CarambaUp(h, serverID)`: for imported configs a non-empty serverID pins the CARAMBA selector to
  that proxy name; empty keeps automatic.
- StatusJSON adds `activeProxy` (current selector target name) when connected.

## Behavioural fixes (Go side, same wave)
1. Kill-switch must not turn the final rule into MATCH,REJECT. Semantics: final MATCH,CARAMBA always;
   under kill-switch remove DIRECT from the CARAMBA selector fallback so traffic fails closed
   instead of leaking, and add `MATCH,REJECT` only for an allow-list split (traffic outside the
   allow-list). Keep tests consistent.
2. `api.Core` must call `constant.SetHomeDir(workDir)` (mihomo tag) and create the dir so geo
   databases download into workDir instead of failing on a clean machine.
3. Default rules must not hardcode CN geo rules. Without a preset use only
   `GEOIP,private,DIRECT,no-resolve` + bypass rules + MATCH,CARAMBA.
4. `mobile.Client.Configure` and `api.Core` honour a new panelURL (rebuild the auth/sub clients
   when it differs from the one given to NewClient).
