# Understand Report: caramba-client (branch `connect-app`) + caramba_vpn plugin + caramba-core

Date: 2026-09-02. Produced by an Opus 5 read-only pass before implementation work. Line numbers refer to the tree at commit 0719b95.

Roots:
- App: `apps/caramba-client`
- Plugin: `apps/caramba-client/packages/caramba_vpn`
- Go core: `libs/caramba-core`
- Panel (read-only reference): `apps/caramba-panel/src/api/v2/`

Native artifacts (`exarobot.xcframework`, `caramba.aar`, `libcaramba_core.{so,dll,dylib}`) are absent from the plugin vendor folders; each holds only a README.

---

## 1. App architecture

### 1.1 Entry point
`lib/main.dart:10-38`
- `main()` -> `WidgetsFlutterBinding.ensureInitialized()` -> `runApp(ProviderScope(child: CarambaApp()))`. No async bootstrap.
- `CarambaApp` (ConsumerWidget) watches `themeModeProvider`, `routerProvider`, `activeBrandingProvider`; builds `MaterialApp.router` with `AppTheme.light(brandAccent:)` / `AppTheme.dark(brandAccent:)`.
- Title comes from `branding.displayName(kBrandName)`.

### 1.2 Router
`lib/router/routes.dart` (`AppRoute`): `/` splash, `/login`, `/enroll`, `/autotune`, `/home`, `/servers`, `/profile`, `/settings`, `/protocol`, `/split-tunnel`, `/connections`, `/connections/import`, `/referrals`, `/partner`, `/notifications`, `/tickets`, `/tickets/new`, `AppRoute.ticket(int id)`.

`lib/router/app_router.dart:40-216` (`routerProvider`):
- `initialLocation: AppRoute.splash`; `refreshListenable: _AuthRefresh` (`:220-238`) bridges `authProvider.stage` + `firstRunProvider`.
- Redirect gate (`:48-79`): `AuthStage.unknown` holds on `/` except `/enroll` (`:63`); `unauthenticated`/`authenticating` -> `/login`, again exempting `/enroll` (`:66-68`); `authenticated` + `firstRun` -> `/autotune`; otherwise leaving splash/login/autotune/enroll -> `/home`.
- Full-screen routes above the shell: `/protocol`, `/split-tunnel`, `/connections` (+ `import`), `/referrals`, `/partner`, `/notifications`, `/tickets` (+ `new`, `:id`).
- `StatefulShellRoute.indexedStack` (`:162-205`): home / servers / profile / settings; settings nests `autotune` (`AutotuneScreen(fromSettings: true)`).
- Deep-link intake starts inside the provider body (`:211-213`): `DeepLinkHandler(router)`.

### 1.3 Deep links (`carambaconnect://enroll`)
`lib/router/deep_links.dart:20-58`: `app_links` `uriLinkStream` (warm) + `getInitialLink()` (cold), both non-throwing. `_handle` parses via `EnrollLink.tryParse` and `router.go('/enroll?panel=...&code=...')`.

`lib/data/models/enrollment.dart:30-70`: `EnrollLink.tryParse` requires scheme `carambaconnect`, action from host or first path segment, `panel` + `code` query params. `normalizePanelUrl` reduces to origin, prepends `https://`, rejects non-http(s).

OS-side scheme registration is not yet applied to the platform folders (see INTEGRATION.md step 3b).

### 1.4 Shell
`lib/shell/app_shell.dart`: destinations Home (power), Servers (globe), Profile (user), Settings (sliders). >= 840 px -> `NavigationRail` + content clamped to 960 (`:38-74`). Mobile -> `extendBody: true` + `_BottomNav` with `BackdropFilter(blur 12)`, 92% alpha canvas, hairline top border, 64 px row (`:96-129`).

### 1.5 State layer (Riverpod 2.6)

`lib/state/providers.dart`:
- `tokenStoreProvider`, `apiClientProvider`.
- `vpnConnectionProvider` (`:31-37`): `MethodChannelVpnConnection(configResolver: () => _resolveVpnConfig(ref))` when `_useNativeVpn()`, else `MockVpnConnection()`.
- `_resolveVpnConfig` (`:48-90`): active profile `isRaw` -> `null` (skip configure); `isPanel` with complete panelUrl/subscriptionUuid/accessToken -> those; otherwise tenant-1 fallback (`TokenStore.readAccess()` + `subscriptionProvider.future` + `kApiBaseUrl`).
- `_nativeVpnEnabled = bool.fromEnvironment('USE_NATIVE_VPN')` (`:96-99`); `_useNativeVpn()` also requires non-web and Android/iOS/macOS/Windows/Linux.

`lib/state/vpn_state.dart`: `VpnNotifier extends StateNotifier<VpnStatus>` (`:21-81`). `connect([Server?])` (`:38-66`): no explicit server and active profile `isRaw` -> `connectRaw(raw: profile.rawConfig ?? profile.source, format: 'auto', label: profile.displayName)`; otherwise `server ?? _recommended()`. `toggle()` (`:71-74`). Providers: `vpnProvider`, `trafficProvider` (StreamProvider.autoDispose), `isConnectedProvider`.

`lib/state/connection_profiles_state.dart`: `ConnectionProfilesState` (`profiles`, `activeId`, `loading`); notifier persists after every mutation (`add`, `remove`, `activate`, `addPanelAccount` dedupes by panelUrl, `setBranding`, `rename`). The only notifier with real persistence. `activeConnectionProfileProvider`.

`lib/state/core_config_state.dart`: `CoreConfig` (`:15-75`) with protocol/route/relay/stack/dns/mtu as indices, `fakeIp`, `ipv6`, `killSwitch`, `autoConnect`, `splitMode` + `splitApps`. In-memory only. `installedAppsProvider` -> `SplitApp.demo` (10 hardcoded apps). Nothing in CoreConfig is ever sent to native.

Others: `auth_state.dart` (AuthStage, silent restore, login variants, `/me` preload), `servers_state.dart` (`serversProvider` sorted selectable-first then by ping, `selectedServerProvider`, `recommendedServerProvider`), `subscription_state.dart`, `settings_state.dart` (in-memory `AppSettings`, `themeModeProvider`, `firstRunProvider` defaults true and is in-memory, so onboarding re-runs on every restart), `account_state.dart` (subscriptions, devices, referral, family, relays, partner, traffic history), `branding_state.dart` (follows active panelAccount profile, `GET /branding`, caches into profile), `notifications_state.dart`, `tickets_state.dart`.

### 1.6 Data layer
`lib/data/api_client.dart`: `kApiBaseUrl = String.fromEnvironment('CARAMBA_API_BASE', defaultValue: 'https://exarobot.top')`; Dio base `${base}/api/v2/app`, 15 s / 20 s timeouts, Bearer interceptor with single-flight refresh on 401.

Endpoints (relative to `/api/v2/app`): POST `/register` (+enroll_code), GET `/enroll/{code}`, POST `/login/email`, `/login/telegram`, `/login/code` (+enroll_code), `/logout`, GET `/branding` (never throws), GET `/me`, `/subscription`, `/servers`, `/devices`, PATCH/DELETE `/devices/{id}`, GET `/subscriptions`, `/referrals`, `/family`, POST `/family/invite`, DELETE `/family/{memberId}`, GET `/relays`, `/traffic`, POST `/purchase`, GET `/notifications`, POST `/notifications/{id}/read`, `/notifications/read-all`, GET/POST `/tickets`, GET `/tickets/{id}`, POST `/tickets/{id}/reply`, GET/POST `/partner/codes`, DELETE `/partner/codes/{code}`, POST `/refresh`.

Models under `lib/data/models/` are hand-written (no codegen). `server.dart:59-75`: `id`, `name`, `country_code`, `latency_ms` -> `pingMs`, `load_pct` -> `load`, `status`; `isSelectable = status != 'full'`.

`token_store.dart`: FlutterSecureStorage wrapper. `connection_profiles_store.dart`: keys `caramba.connection_profiles` + `caramba.active_profile_id`; corrupt JSON -> empty list.

---

## 2. VPN abstraction

`lib/vpn/vpn_status.dart`: `VpnStage { disconnected, connecting, connected, reconnecting, error }`; `VpnStatus { stage, server, detail, connectedSince }`; `fromMap` reads `stage`, `detail`, `connectedSinceMs`. `TrafficStats { downBps, upBps, downTotal, upTotal }`.

`lib/vpn/vpn_service.dart:12-48`:
```dart
abstract interface class VpnConnection {
  Stream<VpnStatus> get status;
  Stream<TrafficStats> get traffic;
  VpnStatus get currentStatus;
  Future<void> connect(Server server);
  Future<void> connectRaw({required String raw, required String format, required String label});
  Future<void> disconnect();
  Future<void> dispose();
}
```
`VpnConfig { panelUrl, subscriptionUuid, accessToken }` with `toArgs()` emitting `panelUrl` / `subscriptionUuid` / `accessToken`.

`MethodChannelVpnConnection` (`:106-230`): channels `com.caramba/vpn`, `com.caramba/vpn/status`, `com.caramba/vpn/traffic`. `connect` emits optimistic `connecting`, `_ensureConfigured()`, then `invokeMethod('connect', {serverId: String, serverName, countryCode})`. `connectRaw` clears `_appliedConfig` and invokes `connectRaw({rawConfig, format, label})`. Traffic stream has no Dart-side replay. Dart never calls the `status` method.

`MockVpnConnection` (`:234-347`): 1400 ms connecting -> connected; deterministic fake traffic at 1 Hz.

Plugin facade `packages/caramba_vpn/lib/caramba_vpn.dart`: only `configure({panelUrl, subscriptionId, accessToken})`. Wire-key discrepancy: facade sends `subscriptionId`, app sends `subscriptionUuid`; Android/Linux/Windows accept both, Apple reads only `subscriptionUuid`. `plugin_platform_interface` declared and unused.

Channel contract (canonical): `configure`, `connect`, `connectRaw`, `disconnect`, `status`; status events `{stage, detail?, connectedSinceMs}`; traffic events `{downBps, upBps, downTotal, upTotal}`. Plugin README omits `connectRaw`.

---

## 3. Per-platform plugin implementations

### 3.1 Apple (iOS + macOS): NetworkExtension only, no dart:ffi path
Files: `darwin/Classes/CarambaVpnShared.swift`, `darwin/Classes/CarambaVpnPlugin.swift`, `darwin/Extension/PacketTunnelProvider.swift`, `ios/Classes/CarambaVpnPlugin+iOS.swift`, `macos/Classes/CarambaVpnPlugin+macOS.swift`.
- App Group from Info.plist `CARAMBA_APP_GROUP`; status/traffic cross the process boundary through shared defaults.
- `CarambaVpnPlugin.swift`: `connect` builds `providerConfiguration` and drives `NETunnelProviderManager`; `connectRaw` sets `rawMode="1"`; 1 Hz poll on the main run loop (reads shared state twice per tick at `:298-300`).
- `PacketTunnelProvider.swift` guarded by `#if canImport(Caramba)`; `startCore` creates `CarambaNewClient`, imports or configures, `setTunFd(utun fd)`, `up`. Network settings: `198.18.0.1/16`, `fd00::1/64`, DNS 1.1.1.1/8.8.8.8, MTU 1500.
- Podspecs vendor `Frameworks/exarobot.xcframework` (absent), link `NetworkExtension`, iOS 15 / macOS 11.
- There is no dlopen/dart:ffi path to `libcaramba_core.dylib` on macOS. `macos/` has only the podspec, `Classes/`, `Frameworks/`. INTEGRATION.md's instruction to copy the dylib into `macos/Libraries/` is dead. macOS therefore needs a System Extension, the networkextension entitlement, an App Group and user approval.

### 3.2 Android (Kotlin)
`CarambaVpnContract.kt`, `CarambaVpnPlugin.kt` (FlutterPlugin + ActivityAware; `configure` persists the seam to SharedPreferences; `connect`/`connectRaw` -> `VpnService.prepare` consent -> `startForegroundService`), `CarambaVpnService.kt` (TUN `172.19.0.1/30`, MTU 1500, catch-all routes, `addDisallowedApplication(packageName)`, `runCore` -> `CarambaCore.create/createRaw`, `setTunFd` before `up`, 1 Hz poll, `START_NOT_STICKY`, foreground notification channel `caramba_vpn`), `CarambaCore.kt` (wraps `mobile.Mobile.newClient`), `CarambaVpnBus.kt`.
`android/build.gradle`: `implementation(name: 'caramba', ext: 'aar')` via flatDir `android/libs/` (absent). Kotlin 1.9.22, AGP 8.1.0, compileSdk 34, minSdk 21. Manifest declares the service and permissions.

### 3.3 Linux (C++/GObject)
`dlopen("libcaramba_core.so")` + dlsym of nine symbols; `connect` -> `SetTunFd(-1)` + `CarambaUp`; `connectRaw` -> `CarambaImportSubscription` then `Up("")`; 1 Hz poll on the GLib loop. `linux/include/caramba_core.h` lacks `CarambaImportSubscription` (Windows header has it).

### 3.4 Windows (C++)
`LoadLibraryW(L"libcaramba_core.dll")`, RAII loader, `SetTunFd(-1)`, bundles `libcaramba_core.dll` + `wintun.dll` (both absent), needs admin.

### 3.5 Expected artifacts
| Platform | Artifact | Location | TUN owner |
|---|---|---|---|
| Android | `caramba.aar` | `android/libs/` | app VpnService fd -> SetTunFd |
| iOS | `exarobot.xcframework` | `ios/Frameworks/` | extension utun fd |
| macOS | `exarobot.xcframework` | `macos/Frameworks/` | extension utun fd |
| Linux | `libcaramba_core.so` | `linux/lib/` | mihomo, needs root |
| Windows | `libcaramba_core.dll` + `wintun.dll` | `windows/lib/` | mihomo/wintun, needs admin |

---

## 4. Go core (`libs/caramba-core`)

### 4.1 Build modes
`go.mod` requires `github.com/metacubex/mihomo v1.19.27`. `go mod tidy` was run on 2026-09-02 and `go build -tags mihomo ./...` passes on macOS arm64. Three consumers: `gomobile bind -tags mihomo ./mobile` (AAR / xcframework), `go build -tags mihomo -buildmode=c-shared ./ffi` (desktop lib), CLI. All need `CGO_ENABLED=1`.

### 4.2 Engine
`engine.go`: `State {stopped, starting, connected, error}`, `Status`, `Traffic`, `Engine` interface. `engine_stub.go` (`!mihomo`) flips to connected without moving packets. `engine_mihomo.go` (`mihomo`) uses `hub/executor` + `tunnel`; `tunFd >= 0` forces tun with the fd and disables auto-route.

### 4.3 `mobile.Client`
`NewClient(panelURL, subURL, workDir, tokenPath)`; `Login`, `Register`, `LoginCode`, `LoginTelegram`, `Logout`, `SetSubscriptionID`, `Configure(panelURL, subscriptionID, accessToken)` (panelURL is discarded, `mobile.go:148`), `ImportSubscription(raw, format) -> metadata JSON`, `SetProtocol`, `SetRelay`, `ApplyPreset`, `SetSplitTunnel(bypassDomains, perAppMode, apps)` CSV, `ListPresets`, `SetTunFd`, `Up(serverID) -> UpResult JSON`, `Down`, `Status`, `StatusJSON` (flat contract shape), `TrafficJSON`, `AutoTune` (does not call Up). Go never emits `reconnecting`.

### 4.4 C-ABI (`ffi/ffi.go`)
`CarambaNew`, `CarambaConfigure`, `CarambaImportSubscription`, `CarambaSetTunFd`, `CarambaUp`, `CarambaDown`, `CarambaStatus`, `CarambaTraffic`, `CarambaFree`, `CarambaFreeString`. Every returned `char*` must be released with `CarambaFreeString`. No desktop plugin calls `CarambaFree`.

### 4.5 Profile generation (`profile/profile.go`)
`CarambaSelector = "CARAMBA"` (panel<->client contract, never rename). `DefaultPolicy()`: TUN on, gvisor, `caramba-tun`, MTU 1280, auto-route, dns-hijack any:53; DNS on 127.0.0.1:1053, DoH 1.1.1.1/8.8.8.8, fallback tls://1.1.1.1:853, fake-ip 198.18.0.1/16; kill-switch on. `AssembleMihomoConfig` applies general/tun/dns/rule-providers/rules/protocol onto the panel YAML. Never sets `port`/`socks-port`/`mixed-port`. `applyRules` hardcodes `GEOIP,CN,DIRECT` + `GEOSITE,cn,DIRECT` when no routing preset (wrong default for RU/IR/BY users). `applyProtocol` builds a `Caramba-Proto` url-test group.

### 4.6 Subscription import (`subimport/`)
Formats `auto|clash|singbox|v2ray|uri`. Detection order: single known-scheme URI -> `uri`; `{` -> `singbox`; `proxies:` -> `clash`; base64 body of URIs -> `v2ray`; multiline URIs -> `v2ray`; else `clash`. Schemes: vless, vmess, trojan, ss, ssr, hysteria2/hy2, tuic, wireguard/wg, naive. `Import` returns clash YAML + Metadata; `marshalClash` emits `proxies:` and a `CARAMBA` selector with every node plus DIRECT, no listener ports.

### 4.7 Auth client and subscription fetch
`auth/client.go`: `/api/v2/app/register|login/email|login/code|login/telegram|refresh|logout`, token store on disk, `DoAuthorized` with 401 retry. `subscription/subscription.go:116-167`: `GET {subBaseURL}/sub/{uuid}` with `node_id`/`relay_country`, parses `subscription-userinfo`.

### 4.8 `api.Core.Up` (`api/api.go:528-583`)
Raw path (imported config, no auth) or panel path (auth + `FetchProfile`), then `AssembleMihomoConfig`, write `workDir/config.yaml` (0600), `engine.Start`.

### 4.9 Autotune, routing presets
`ProtocolPriority = [AmneziaWG, VLESS-Reality, Hysteria2, TUIC, Shadowsocks]`, TCP prober by default, mihomo prober under the tag, `Recommend` picks server/protocol/stack/relay. Presets: `ru-smart`, `ru-full`, `telegram-only`, `ir-smart`, `by-smart`, `cn-smart`, `streaming`, `adblock`, `global`.

---

## 5. Screens (`lib/features/*`)

| Screen | File | Shows | State |
|---|---|---|---|
| Splash | `splash/splash_screen.dart` | session probe, animated ring, honors reduce-motion | complete |
| Login | `auth/login_screen.dart` | Telegram bot code + deep link | complete |
| Enroll | `enroll/enroll_screen.dart` | invite-code enrollment stages | complete |
| Autotune | `autotune/autotune_screen.dart` | 3-step progress | simulated with Timers, hardcoded result, never calls Go AutoTune |
| Home | `home/home_screen.dart` | dial + config rows + 4 stats + traffic chart | complete UI; rows write in-memory CoreConfig only |
| Servers | `servers/servers_screen.dart` | public/private pools, ping + load, pull-to-refresh | complete, panel-only |
| Profile | `profile/profile_screen.dart` | plans, devices, referrals, family, partner | complete |
| Settings | `settings/settings_screen.dart` | theme, toggles, protocol/route, logout | complete UI, not persisted |
| Protocol | `protocol/protocol_screen.dart` | picker | complete |
| Split tunnel | `split/split_tunnel_screen.dart` | mode + searchable app list | UI only, demo data, never reaches native |
| Connections | `connections/connections_screen.dart` | multi-profile list | complete; tap only activates, does not connect |
| Import | `connections/connection_import_screen.dart` | name + URL/raw + format | complete except QR/file stubs (toasts); format choice discarded |
| Referrals / Partner / Notifications / Tickets | respective files | complete | |

Connect dial `lib/widgets/connect_dial.dart`: 196/232 px ring, 146 px face (not scaled on desktop), `_RingPainter` draws a rotating arc while busy and a full stage-colored circle otherwise; reduce-motion freezes the arc but the controller keeps repeating.

---

## 6. Theme (`lib/theme/*`)
`colors.dart`: neutral `AppColors` (dark/light), `withBrandAccent` patches the accent quartet, `AppShadows`. `tokens.dart`: `AppTokens` ThemeExtension, `lerp` snaps at 0.5; `context.tokens` / `context.c`. `spacing.dart`: `AppSpace`, `AppRadius`, `AppBorders`, `AppMotion` (micro 120, standard 220, large 320, ringMorph 400), `AppOrb`, `AppBreakpoints`. `typography.dart`: `AppType` getters with tabular figures. `app_theme.dart`: `AppTheme.light/dark({brandAccent})`.
Reduce-motion honored only in `connect_dial.dart` and `splash_screen.dart`.

---

## 7. Broken / missing / stale (numbered for tracking)
1. Native artifacts absent in every vendor folder (see 3.5).
2. Platform folders now materialized by `flutter create .` (2026-09-02); INTEGRATION step 2/3b edits still pending.
3. `subscriptionId` vs `subscriptionUuid` key mismatch; Apple plugin reads only `subscriptionUuid`.
4. Plugin README omits `connectRaw`.
5. Linux header lacks `CarambaImportSubscription`.
6. INTEGRATION.md references a nonexistent `macos/Libraries/` dylib path; no macOS FFI path exists.
7. `mobile.Client.Configure` discards `panelURL`; multi-tenant panel mode is not functional end to end.
8. `vpn_state.dart:51` hardcodes `format: 'auto'`; `ConnectionProfile` has no format field.
9. `CoreConfig` never reaches native (protocol, preset, relay, stack, DNS, MTU, kill-switch, split).
10. `CoreConfig`, `AppSettings`, `firstRunProvider` are in-memory; onboarding re-runs every launch.
11. Autotune screen is a simulation; Go `AutoTune` unreachable from Dart.
12. `installedAppsProvider` is demo data; no platform channel enumerates apps.
13. `CarambaFree` never called by desktop loaders.
14. Dart traffic stream has no replay; Linux/Windows do not replay on `onListen`.
15. Stub engine reports connected while moving no packets; never ship artifacts built without `-tags mihomo`.
16. `applyRules` hardcodes CN geo rules on the default policy path.
17. `subimport.marshalClash` emits no listener ports; imported profiles are TUN-only.
18. `webview_flutter`, `plugin_platform_interface`, freezed/json_serializable/build_runner declared and unused.
19. `docs/API.md` does not cover `/api/v2/app/*`; contract of record is `apps/caramba-panel/src/api/v2/mod.rs:137-200`.
20. Desktop dial uses the mobile face size.
21. Enroll creates the `panelAccount` profile before validation (orphan on invalid); `subscriptionUuid`/`accessToken` on the profile are never filled after login, so `_resolveVpnConfig` always falls back to tenant-1.

---

## 8. Generic-mode readiness
- Import subscription URL / raw text: yes (`ConnectionImportScreen` fetches with a bare Dio and stores `rawConfig`).
- `rawSub` profile kind: fully wired Dart -> channel -> all five natives -> `ImportSubscription` -> `Up("")`.
- Format selection: captured but discarded (item 8).
- QR / file import: stubs.
- Server list with latency: panel-only; `subimport.Metadata.Servers` never surfaced to Dart.
- Latency probing for imported nodes: no.
- Split tunnel, autotune, protocol/preset/relay: UI only; Go side complete but unreachable.
- Multi-profile switching: yes.
- Connect from Connections screen: no (activate only).

## 9. Enroll / panel mode
Flow: deep link or manual entry -> `EnrollLink` -> `EnrollNotifier.startWith` creates a placeholder `panelAccount` profile, validates via a panel-scoped `ApiClient(baseUrl: panelUrl)`, upgrades the name, then `registerWithEnroll` / `loginCodeWithEnroll` -> shared `TokenStore` -> `ref.invalidate(authProvider)`.
All client endpoints match `apps/caramba-panel/src/api/v2/mod.rs:137-200`: `/enroll/{code}`, `/branding`, `/register` (+enroll_code), `/login/code` (+enroll_code, consumed only when a new account is created), the auth set, and every protected endpoint. No drift found.
Risks: orphan profile on invalid code; profile never receives `subscriptionUuid`/`accessToken`; `Configure` discards `panelURL`; so a non-tenant-1 enrollment ends up configuring the core against `kApiBaseUrl`.
