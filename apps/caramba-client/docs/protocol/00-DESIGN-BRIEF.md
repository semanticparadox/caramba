# Caramba Connect Protocol: Design Brief

Synthesized 2026-09-02 from eleven structured inputs (five code maps of the Caramba repo, six research briefs). Every claim carries a source tag: `[input-key]` plus a `file:line` pointer where the input supplied one. Nothing here is asserted beyond what the inputs support; unresolved items are marked as open questions or risks rather than decided.

---

## 1. What exists today

### 1.1 Panel app API (`/api/v2/app/*`)

The Flutter client talks to a single flat, single-tenant HTTP surface. `app_routes(state)` is nested at `/v2/app` inside `api_routes`, and `api_routes` is itself nested twice, at `/api` and at `/caramba-api`, so every endpoint has two live paths (`apps/caramba-panel/src/main.rs:1470`, `main.rs:1582-1583`, `api/v2/mod.rs:143-209`) `[panel-app-api]`.

Thirty-two endpoints: eight public, twenty-four behind `require_app_jwt` (`api/v2/app_auth.rs:329-379`) `[panel-app-api]`.

Public: `POST /register` (`app_auth.rs:386-538`), `POST /login/email` (`:541-574`), `POST /login/telegram` (`:576-779`), `POST /login/code` (`:781-851`), `POST /refresh` (`:853-908`), `POST /logout` (`:910-922`), `GET /enroll/{code}` (`api/v2/app_enroll.rs:74-124`), `GET /branding` (`api/v2/app_branding.rs:38-86`) `[panel-app-api]`.

Protected: `/me`, `/subscription`, `/servers`, `/devices` (+ PATCH, DELETE), `/subscriptions`, `/relays`, `/referrals`, `/family` (+ invite, DELETE member), `/traffic`, `/purchase`, `/notifications` (+ read, read-all), `/tickets` (+ get, reply), `/partner/codes` (+ create, delete) (`api/v2/mod.rs:160-207`) `[panel-app-api]`, `[client-enroll]`.

**The API is read-only with respect to VPN service configuration.** There is no endpoint to select a server, pick a relay, set routing, or change protocol. `GET /servers` returns a list with synthesized names `"Node #{id} ({mbps} Mbps)"` and filters out relay nodes (`api/v2/app.rs:184-261`); `GET /relays` returns `{country_code, country_name, node_count}` aggregates and is **not plan-scoped**, unlike `/servers` (`api/v2/app_account.rs:712-767`) `[panel-app-api]`. Server pinning, relay choice, protocol variant and routing exist only as query parameters on the config fetch `[panel-app-api]`, `[sub-service]`.

Devices are the one genuinely mutable dimension: `GET /devices`, `PATCH /devices/{id}` (rename), `DELETE /devices/{id}` (which also deletes the `subscription_ip_tracking` row and spawns `kill_subscription_connections`) (`api/v2/app_account.rs:90-255`) `[panel-app-api]`.

Auth model: HS256 access token, claims `{sub, exp, iat, typ:"access"}`, TTL 900s, secret from env `APP_JWT_SECRET` falling back to `SESSION_SECRET`. No `aud`, no `iss`, no `jti`, no device binding, no panel/tenant claim, no key rotation (`app_auth.rs:32,49-60,71-77,94-125,329-379`) `[panel-app-api]`. Refresh token is opaque 256-bit random hex, stored as sha256-hex only, TTL 30 days, rotated on use, with the User-Agent recorded alongside (`app_auth.rs:33,79-152`; `libs/caramba-db/migrations/20260620000000_app_accounts.sql:32-45`) `[panel-app-api]`.

Rate limiting exists on exactly two endpoints: `login/email` and `login/code` share a fixed-window Redis key `app:loginrl:{ip}` at 10 requests per 60s, and it **fails open** when Redis errors (`app_auth.rs:40-41,253-288`) `[panel-app-api]`. The sibling bot router has a real two-layer stack (`require_bot_token` plus per-endpoint limits plus a global 50 requests per 3 seconds, `api/v2/mod.rs:32-135`) `[panel-app-api]`.

There is **zero occurrence of "tenant"** in the entire panel source. No `panel_id`, `tenant_id` or `org_id` in any endpoint, DTO, JWT claim, or table the app API touches. An `OrganizationService` exists (`services/org_service.rs:1-57`) but is never referenced from any app handler. The de-facto tenant boundary is the process and database: one panel deployment equals one operator, addressed by the base URL the client was handed in the enroll link `[panel-app-api]`.

The closest thing to tenant identity is licensing: env `CARAMBA_LICENSE_KEY`, `CARAMBA_INSTANCE_ID`, `CARAMBA_LICENSE_SERVER_URL`, `CARAMBA_LICENSE_PUBKEY`, with activation responses **ed25519-verified** against the operator's own instance_id and cached in a single-row `license_state` table; re-verification every 12 to 24 hours, 14-day offline grace, then soft degradation to Free limits for new privileged actions only (`license/mod.rs:1-29,37,72-105,183-217`; `license/activation.rs:25-26,174-175`) `[panel-app-api]`, `[existing-docs]`. This means **the ed25519 verification machinery a signed subscription protocol needs already ships in the panel.**

Enrollment codes exist as a table (`libs/caramba-db/migrations/20260623000000_enrollment_codes.sql:12-29`) with a transactional redemption path (`services/store_service.rs:528-639`), but **no code path anywhere in the repo issues one.** A repo-wide grep finds five hits total: the CREATE TABLE and three SELECT/UPDATE statements. No admin endpoint, no bot command, no CLI. Rows can only appear via manual SQL `[panel-app-api]`.

### 1.2 Subscription delivery (`/sub/{uuid}`)

`GET /sub/{uuid}` is registered on the root router with no auth middleware (`main.rs:1585-1588`, `subscription.rs:107-271`). **Possession of the UUID is the entire credential.** No JWT, no signature, no device binding, no nonce `[panel-app-api]`, `[sub-service]`.

`GET /api/v2/app/subscription` (`api/v2/app.rs:111-182`) hands the client five prebuilt URLs (`clash_url`, `config_url`, `singbox_url`, `v2ray_url`, `subscription_url`), formatted as `{base}/sub/{uuid}?client=clash|singbox|v2ray`. Base URL resolution: settings `subscription_domain`, then `panel_url`, then the Host header, then env `PANEL_URL` (`app.rs:21-49`) `[panel-app-api]`.

Query contract: `SubParams { client, node_id: Option<i64>, variant, relay_country }` (`subscription.rs:38-44`) `[sub-service]`.

Handler flow: optional 308 redirect if Host mismatches `subscription_domain` (`subscription.rs:113-137`); IP and country extraction from `cf-connecting-ip` / `x-forwarded-for` / `x-country-code` / `cf-ipcountry` (`:76-87,146-153`); Redis rate limit 30 per 60s keyed `rate:sub:{uuid}`, failing open (`:155-167`); 403 on non-active status, quota exceeded, or device-limit breach (`:169-265`); `track_access`; then generation `[sub-service]`.

**Relay selection is a write-through side effect of a GET:** an explicit `?relay_country=` triggers `UPDATE subscriptions SET relay_country = $1` inside the fetch handler (`subscription.rs:745-751`). Priority chain: URL param, then persisted column, then GeoIP country, then all relays; the literal `"none"` disables relays (`subscription.rs:724-772`) `[panel-app-api]`, `[sub-service]`.

**Node pinning is also a side effect:** if no node is pinned and more than one candidate remains, the first is auto-selected and persisted to `subscriptions.node_id`, so a fresh subscription silently becomes single-server (`subscription.rs:617-634`) `[sub-service]`.

**The Clash generator ignores relays entirely.** `generate_clash_config(_sub, nodes, user_keys, _relay_nodes)` never reads `_relay_nodes` (`singbox/subscription_generator.rs:1138-1143`), while the sing-box path emits a full cartesian product of `{exit inbound} x {relay}` outbounds with `"detour"` (`:458-496,1604-1626`). Since the Go core fetches `?client=clash` exclusively (`libs/caramba-core/subscription/subscription.go:127`), **the relay picker changes almost nothing in what the app actually receives** `[sub-service]`.

`caramba-sub` (`apps/caramba-sub`) is a thin proxy in front of the panel: routes `/health`, `/sub/{uuid}`, `/app*` (mini app static), `any /api/{*path}` proxied to the panel (`src/main.rs:69-105`) `[sub-service]`. It normalizes `?client=`, sniffs User-Agent when absent (sing-box family checked first because Hiddify's UA contains both "ClashMeta" and "v2ray"), forwards client IP and UA, caches successful bodies in Redis for 300s under `sub:config:{uuid}:{client}:{relay}:{node}`, and serves that cache **only on a transport error**, not on 5xx (`handlers/subscription.rs:11-19,22-48,124-177,222-265`) `[sub-service]`. It treats an upstream 3xx as a fatal 502 (`:180-189`), and it **silently drops the `variant` parameter** (`:11-19`) `[sub-service]`, `[bot-and-payments]`.

Only three response headers are forwarded: `profile-title`, `profile-update-interval`, `subscription-userinfo`. `announce`, `support-url` and `profile-web-page-url` do not exist in the codebase (`apps/caramba-sub/src/handlers/subscription.rs:191-243`) `[sub-service]`. `profile-update-interval` is the bare string `"2"` (`subscription.rs:840`), which Hiddify and sing-box read as **hours** and the Go core parses as **minutes** (`libs/caramba-core/subscription/subscription.go:183-187`) `[sub-service]`.

The subscription UUID is frequently **the same value as the tunnel credential**: `subscription_user_uuid()` returns `sub.vless_uuid` falling back to `sub.subscription_uuid`, and that value becomes the VLESS/VMess uuid, the Trojan password, the TUIC uuid, and part of the Hysteria2 password (`services/subscription_service.rs:191-205,456-479,638-656`). No rotation path for `subscription_uuid` was found `[sub-service]`.

There is also a public unauthenticated rule-set mirror `GET /rulesets/{name}` with a whitelist (`ru-blocked`, `ru-blocked-ip`, `ir-blocked`, `by-blocked`) that exists specifically so clients behind GitHub blocking can refresh rules (`handlers/rulesets.rs:1-50`, `main.rs:1592-1595`), and an **unauthenticated** AI node-recommendation endpoint `GET /api/v2/client/recommended` (`main.rs:1472-1475`, `api/v2/client.rs:10-77`) `[panel-app-api]`, `[sub-service]`.

### 1.3 Bot and payments

Two bots exist, only one is live. `apps/caramba-bot` is a standalone Teloxide worker whose `StoreService` calls roughly nineteen paths that are **not registered** in the panel's bot router (`services/store_service.rs:102-408` vs `api/v2/mod.rs:32-128`), plus a Stars pre-checkout bug: the payload carries `users.id` but the check compares against `tg_id` (`callback.rs:651` vs `payment.rs:33-101`) `[bot-and-payments]`.

The live bot is embedded in the panel process (`bot_manager.rs:93,134`, started from `main.rs:779`), has direct DB and settings access, and uniquely implements `/login` `[bot-and-payments]`.

**The 6-digit login code is the only existing Telegram-to-non-Telegram handoff.** `send_login_code` generates `{:06}`, stores Redis `app:logincode:{code}` mapped to tg_id with TTL 300s plus a reverse index invalidating the user's prior code (`bot/handlers/command.rs:1428-1481`); `POST /api/v2/app/login/code` validates 6 ASCII digits, does an atomic Redis GETDEL, resolves tg_id to users.id and issues a JWT pair (`api/v2/app_auth.rs:781-851`) `[bot-and-payments]`, `[panel-app-api]`.

A switch already exists for the target end state: `bot_buttons_mode == "app_only"` collapses the bot keyboard to Support alone (`bot/handlers/command.rs:94-103`) `[bot-and-payments]`.

Payments: `POST /api/v2/app/purchase` is license-gated by `check_billing_enabled` and delegates to `marketplace_service.create_session`, returning `{pay_url, pay_url_kind, session_id, amount, ...}`; Telegram Stars is deliberately excluded from this path because `StarsProvider` needs bot_token and tg_id (`api/v2/app_billing.rs:110-367`) `[panel-app-api]`. For external http(s) checkouts the panel DMs the pay link into the bot chat and returns `delivered_via: "bot"`, and the mini app polls `GET /api/client/payment/session/{id}` every 5s up to 10 minutes (`api/client.rs:2174-2190`; `useBotPayment.ts:5-105`) `[bot-and-payments]`.

The mini app (`apps/caramba-app`, React/Vite, served by `caramba-sub` at `/app`) hard-loads `https://telegram.org/js/telegram-web-app.js` and renders an "Open via Telegram" wall when `initData` is absent (`index.html:15`, `main.tsx:16,40-49`), so **it cannot participate in any censorship-resistant path** `[bot-and-payments]`.

### 1.4 Flutter client (`apps/caramba-client`, branch `connect-app`)

Deep links: `DeepLinkHandler` subscribes to `AppLinks().uriLinkStream` plus `getInitialLink()`, and `targetOf(String)` tries `EnrollLink.tryParse` then `ImportLink.tryParse`, with a memoized `_pending` replayed once when the router gate opens (`lib/router/deep_links.dart:30-116`) `[client-enroll]`.

`carambaconnect://enroll?panel=<url>&code=<code>` and `carambaconnect://import?url=<encoded>` (`lib/data/models/enrollment.dart:31-113`). `normalizePanelUrl` reduces input to bare origin and **accepts plain `http://`** (`:60-71`) `[client-enroll]`.

`ConnectionProfile` is a persisted multi-profile record: `{id, type: rawSub|panelAccount, displayName, source, panelUrl?, subscriptionUuid?, accessToken?, rawConfig?, format, servers[], selectedServerId?, lastProbe, serversUpdatedMs, brandingCache, lastActiveMs}`, stored as one JSON blob in `FlutterSecureStorage`, with every decoder deliberately lenient (`lib/data/models/connection_profile.dart:40-233`; `lib/data/connection_profiles_store.dart:13-74`) `[client-enroll]`.

`enrollApiClientProvider` is a `Provider.autoDispose.family<ApiClient, String>` keyed by panelUrl, proving multi-tenancy works end to end today (`lib/features/enroll/enroll_controller.dart:312-319`) `[client-enroll]`.

The subscription fetch is a bare `Dio` with 15s/20s timeouts, `followRedirects: true`, a single GET, no `Authorization` header, no size cap, no signature check, no mirror, no retry (`lib/data/subscription_fetch.dart:22-49`) `[client-enroll]`.

`ApiClient` sets baseUrl `'{panel}/api/v2/app'`, `validateStatus: s < 500`, a Bearer interceptor with `skipAuth` opt-out, and single-flight 401 refresh with one replay (`lib/data/api_client.dart:21-24,60-100,610-644`) `[client-enroll]`.

`CorePolicy` is a complete settings schema (`protocol, preset, relay, stack, mtu, ipv6, fakeIp, killSwitch, dns{nameservers,fallback}, split{mode,apps,bypassDomains}`) whose `toJson()` omits nulls, with the documented contract "absent field means do not change, unknown field is ignored" (`packages/caramba_vpn/lib/src/core_policy.dart:41-176`) `[client-enroll]`.

**Zero cryptography exists in the client.** A grep for ed25519, signature, pinning, sha256, hmac, jws over `lib/` and `packages/` returns only false positives, and `pubspec.yaml` declares no crypto package. No `HttpClientAdapter` override, no `badCertificateCallback` (`[client-enroll]`).

The bot handle is **already correct in Dart** (`kTenant1BotUsername = 'exa_robot'`, `lib/data/brand.dart:41-47`; `login_screen.dart:34-37`). The wrong `@exarobot` survives only in `ANTI-SLOP.md:12`, `docs/CARAMBA-CONNECT-PLAN.md:21`, `docs/CARAMBA-CONNECT-PHASE-A-DESIGN.md:114`, plus a hardcoded tenant-1 brand string in the Android VPN session label (`packages/caramba_vpn/android/.../CarambaVpnService.kt:242`) `[client-enroll]`, `[bot-and-payments]`.

### 1.5 Go core (`libs/caramba-core`)

Two HTTP surfaces, both on stdlib defaults with **no custom `http.Transport` anywhere in the core** (the only ones in the module are in `cmd/caramba-smoke`) `[core-transport]`.

`auth.PanelClient` builds `&http.Client{Timeout: 30s}` with no Transport (`auth/client.go:75-86`), so TLS is Go's crypto/tls defaults: system root pool, Go's own ClientHello fingerprint, no pinning, no SNI control, no uTLS `[core-transport]`.

`subscription.Client` is identical (`subscription/subscription.go:99-109`). `FetchProfile` builds `GET {subBase}/sub/{uuid}?client=clash[&node_id=][&relay_country=]`, sets User-Agent `"caramba-core/1.0 (mihomo) clash.meta"` and Accept, treats any non-2xx as a hard error, reads the body with `io.ReadAll` and **no size limit**, and never verifies anything (`subscription.go:122-171`) `[core-transport]`.

Both clients take a one-method `HTTPDoer` via `WithHTTPClient` (`auth/client.go:38-40,57-60`; `subscription.go:92-96`). **That pair of options is the single cleanest injection seam for a new transport layer** `[core-transport]`.

Retry: exactly one, only on HTTP 401 in `DoAuthorized` (`auth/client.go:305-350`). Network errors return immediately, no backoff, no alternate host `[core-transport]`.

`api.Config { PanelBaseURL, SubBaseURL, WorkDir, TokenStorePath }` has no mirrors, no pins, no proxy address, no fetch policy (`api/api.go:27-39`). `SetPanelURL` rebuilds all three clients under a new base URL, which is the multi-tenant re-bind point (`api/api.go:143-169`) `[core-transport]`.

`policyPatch` decodes an all-optional JSON patch with "unknown keys silently ignored" and atomic validation (`api/policy_json.go:18-36,57-116`) `[core-transport]`. The gomobile and FFI boundaries pass only primitives (`mobile/mobile.go:145-155`; `ffi/ffi.go:99-113`), so structured config must arrive as a JSON string `[core-transport]`.

Server identity: `Server.ID == Server.Name == the Clash proxy name`. Country is recovered by decoding a leading regional-indicator flag emoji or a two-letter token from that name (`subscription/subscription.go:31-48,288-358`). That same string is the key for `Up(serverID)`, for `autotune.Candidate.ServerID`, and for `MihomoProber`'s raw proxy map lookup `[sub-service]`, `[core-transport]`.

`routing/presets.go:24-58` substitutes the literal `{BASE}` in every rule-provider URL with the panel base URL, so rule-sets are mirrored through the panel rather than GitHub. **This is the existing mirror-substitution precedent** `[core-transport]`. But `CompiledProviders` emits no `proxy:` key (`routing/routing.go:186-192`) even though mihomo accepts one (`rules/provider/parse.go:19,59`), and geo databases download **direct from GitHub with no proxy and no override** (`component/geodata/init.go:71-88`; core never calls `SetGeoIpUrl` etc.) `[core-transport]`.

The default policy hardcodes DoH to 1.1.1.1 and 8.8.8.8 and probes `https://www.gstatic.com/generate_204` (`profile/profile.go:149-175,668-675`) `[core-transport]`, `[existing-docs]`.

mihomo is optional behind `//go:build mihomo`, with tagless twins for every tagged file (`engine/engine_mihomo.go:1`, `api/probe_mihomo.go:1`, `api/homedir_mihomo.go:1`) `[core-transport]`.

Vendored mihomo (v1.19.27, copied to `build/mihomo-src` for patching) already ships, but wired **only to proxy outbounds, not to its own fetcher**: uTLS ClientHello mimicry (`component/tls/utls.go:30-101`), ECH (`adapter/outbound/ech.go:12-41`), SHA-256 certificate pinning and custom root pools (`component/ca/config.go:79-124`, `fingerprint.go:14-60`), a SOCKS5 outbound plus `proxydialer` (`adapter/outbound/socks5.go:30-43`; `component/proxydialer/proxydialer.go:20-51`), and `component/http.HttpRequest(..., WithSpecialProxy)` plus `resource.HTTPVehicle` with ETag caching and size limits (`component/http/http.go:32-117`; `component/resource/vehicle.go:87-183`) `[core-transport]`.

---

## 2. Constraints

### 2.1 App store rules

**Apple 5.4 (verbatim):** VPN apps "must utilize the NEVPNManager API and may only be offered by developers enrolled as an organization"; must declare data collection on screen before use; "may not sell, use, or disclose to third parties any data for any purpose"; must not violate local laws; non-compliant apps are removed "and blocked from installing via alternative distribution" `[store-compliance]`. The Network Extension packet-tunnel entitlement is self-serve since November 2016, no Apple approval needed, and `NETunnelProviderManager` subclasses `NEVPNManager` so a packet tunnel satisfies 5.4 `[store-compliance]`.

**Apple 4.2.6** rejects apps from a commercialized template or generation service unless submitted by the content provider, but explicitly blesses "a single binary to host all client content in an aggregated or 'picker' model." This is the licence for one Caramba Connect binary with runtime operator enrollment, and the prohibition on Webq Pro submitting per-operator rebrands `[store-compliance]`.

**Apple 3.1.1** bans "own mechanisms to unlock content or functionality, such as license keys." A pasted subscription URL is structurally a license key. Hiddify, Streisand, V2Box and v2RayTun ship on non-RU storefronts as exactly this shape, which is behavioral evidence and not a written rule `[store-compliance]`.

**Apple 3.1.3:** outside the US storefront, apps "cannot, within the app, encourage users to use a purchasing method other than in-app purchase," and may not include buttons, external links or calls to action pointing at other purchase mechanisms. Communications outside the app are permitted. The US storefront no longer requires an entitlement to link out (3.1.1(a), post Epic v. Apple, Ninth Circuit affirmed 2025-12-11) `[store-compliance]`.

**Apple 2.3.1:** no hidden, dormant or undocumented features; all new functionality must be described with specificity in review notes. **A geo-conditional or failure-triggered Tor fallback is the textbook violation**, with account termination as the repeat-behavior penalty tier `[store-compliance]`.

**Apple 2.5.2 and Play Device and Network Abuse:** may not download, install or execute code that introduces or changes features. Play adds "An app may not download executable code (such as dex, JAR, .so files) from a source other than Google Play" `[store-compliance]`. The existing Caramba position already rests on this: "A subscription is config DATA, never code," verified by source read of `subimport` (no `os/exec`, `plugin`, `unsafe`, `syscall`) `[existing-docs]`.

**Apple 2.1(a):** an app gated behind an operator subscription is rejected without a demo account or a pre-approved demo mode exhibiting full functionality `[store-compliance]`.

**Apple 5.1.1(i) and 5.1.2:** the privacy policy must be in-app, must identify all data collected and used, and must confirm that any third party receiving user data provides equal protection. In a multi-tenant design the enrolled operator is such a third party `[store-compliance]`.

**Google Play VpnService policy:** the declaration form is mandatory; VPN must be the core purpose; data must be encrypted device-to-endpoint; VpnService use must be documented in the listing; prominent in-app disclosure with affirmative consent on a separate screen is required. "Deceptive and non-declared uses of these APIs may result in a suspension of your app and/or termination of your developer account" `[store-compliance]`. Play permits proxy services "only in apps where that is the primary, user-facing core purpose" `[store-compliance]`.

**Payments in Russia are moot in both directions.** Google paused Play billing in RU on 2022-03-10 and exempted RU from the Payments policy on 2022-08-02; Apple ended the last RU payment method on 2026-04-01 `[store-compliance]`. Bot payment is the only option there, and on Android it is expressly outside the Payments policy.

**Takedown is the dominant store risk, not review.** On 2026-03-28 Apple hid Streisand, V2Box, v2RayTun and Happ from the Russian App Store at Roskomnadzor's demand citing art. 15.1 §7, with no published court ruling. Apple removed 1,213 apps in 2025 on RKN request, acting within days; Google largely resists (roughly six of 212 demanded apps removed, and a 22.8M RUB fine instead) `[store-compliance]`. Removals are storefront-scoped: non-RU accounts retain access, installed copies keep working but stop updating `[store-compliance]`.

Russian law since 2025-09-01 fines advertising or popularizing VPN services (200,000 to 500,000 RUB for legal entities) and penalizes deliberate access to extremist-registry material including via VPN `[store-compliance]`, `[censorship-ru]`. Figures are from secondary legal commentary, not the statute text.

### 2.2 Censorship realities (Russia)

**Connection freezing on foreign datacenter IPs.** Since June 2025, TSPU silently freezes TCP connections when three conditions align: HTTPS/TLS, a foreign datacenter IP, and a data threshold inside one connection. Roughly 25 packets, or 15 to 20 KB depending on provider. No RST is sent. Named providers: Hetzner, DigitalOcean, Cloudflare, OVH, Oracle, AWS. Cloudflare independently confirmed a ~16 KB per-connection limit applied via packet injection, affecting HTTP/1.1, HTTP/2 and HTTP/3 alike, with a ~30% traffic drop from Russia from 2025-06-09 `[censorship-ru]`. **A subscription payload under ~8 KB slips through; a large one does not.**

**Infrastructure reputation scoring.** Blocking now incorporates destination ASN heuristics, not only protocol fingerprints. A plan concentrated on a few well-known cloud ASNs is "unusually vulnerable" even when the protocol layer is obfuscated `[censorship-ru]`.

**Protocol blocking:** 469 VPN services blocked as of end-February 2026; the three most popular VPN protocols blocked since December 2025; SOCKS5, VLESS and L2TP more actively blocked since then `[censorship-ru]`. From ~2025-11-01, home ISPs deployed a policy killing VLESS+Reality+`xtls-rprx-vision` on port 443, correlating with the number of simultaneous active TLS sessions rather than bandwidth. Workarounds observed: move off 443, disable flow and use mux, XHTTP/H2/H3 with mux, or non-TLS transports such as Shadowsocks-2022 `[censorship-ru]`.

**Whitelist mode** was introduced September 2025 (57 approved domestic services), tested in Moscow March 2026, and applied on metro Wi-Fi and during mobile shutdowns. It is "especially hostile to centralized circumvention, because a global relay, a stable fallback domain, or any persistent alternative control plane can simply be excluded" `[censorship-ru]`. The one demonstrated escape is tunneling through domestic infrastructure (`vk-turn-proxy` routes WireGuard or Hysteria through VK Calls TURN servers as STUN ChannelData) `[censorship-ru]`.

**ECH is blocked, not a tool.** Since 2024-11-05 TSPU drops any ClientHello containing both the ECH extension and SNI. RKN formally recommended Russian site owners disable it on 2025-04-02. Enabling ECH makes traffic stand out `[censorship-ru]`, `[secure-manifest]`, `[existing-docs]`.

**Classic domain fronting is dead everywhere:** closed by Cloudflare 2015, Google and AWS 2018, Azure 2024-01-08, Fastly 2024 `[sub-conventions]`.

**DoH and DoT are being blocked.** Reported 2026-08-26 across several regions, targeting 1.1.1.1 and 8.8.8.8, after March 2026 testing on Beeline. Port 853 is blocked; field guidance is DoH over `https://` only, split so domestic traffic resolves via Yandex DoH `[censorship-ru]`.

**Telegram is effectively blocked.** RKN confirmed nationwide throttling 2026-02-10, and by mid-March 2026 Telegram was largely inaccessible on home broadband and mobile with the desktop version failing outright. Telegram is not whitelisted. WhatsApp was fully blocked in February 2026 `[censorship-ru]`. **This directly undermines "the bot is only used to pay."**

**Tor is not a dependable path.** Directory authorities and default bridges blocked since 2021-12-01; obfs4 largely blocked and DPI-detected on some mobile ISPs; most WebTunnel bridges enumerated from June 2025; Snowflake blocked by shared JA3/JA4 DTLS ClientHello fingerprint since 2026-03-30 `[tor-embedding]`, `[censorship-ru]`. BridgeDB shut down 2024-09-18 and moat no longer issues new bridges `[tor-embedding]`. Tor itself moved bridge distribution to a Telegram distributor because censors cannot scrape it in real time `[tor-embedding]`.

**TLS authenticity cannot be assumed.** A FOCI 2026 measurement found 28.6% of RuStore apps bundle the Russian Trusted Root or Sub CA, rising to 81.4% of weighted download share among top apps. Since June 2026 seven major Russian banks serve state-issued certificates `[censorship-ru]`. Combined with inline TSPU DPI, this is a concrete MITM capability. **Application-layer signing is required; TLS is not a sufficient trust anchor.**

**The proven resilient pattern** is the Navalny Smart Voting design: signed JSON configuration, low-TTL endpoint rotation, DNS-over-HTTPS resolution, and SNI faking, with aggressive preemptive rotation across large pools of pre-registered domains, exploiting the fact that RKN needs minutes rather than seconds to suppress each new endpoint `[censorship-ru]`.

### 2.3 Toolchain

**gomobile allows exactly one framework per app.** "You cannot have two `gomobile` frameworks as dependencies. There are some common Go runtime functions exported, which would create a name clash." Any Go pluggable transports must be compiled into the **same Go module** as mihomo, producing one AAR and one XCFramework. Orbot already does this (OrbotIPtProxy merges IPtProxy with go-tun2socks for exactly this reason) `[tor-embedding]`.

**iOS Network Extensions are capped at 50 MiB** since iOS 15, covering the whole extension process including all library code. Xray-core already cannot start on iOS once geo files load. **The subscription fetch must happen in the main app process, not in the NE** `[tor-embedding]`.

Measured Android cost, arm64, compressed in-AAR: C-tor 3.0 MB, IPtProxy 7.9 MB (mostly Go runtime, which mihomo already pays, so the marginal cost of adding all PTs to an existing mihomo build is materially less), arti-mobile 6.4 MB `[tor-embedding]`.

**Managed pluggable transports are dead on mobile:** they "will likely not work on the latest version of Android and never worked on iOS." Only the unmanaged form (`ClientTransportPlugin <transport> socks5 127.0.0.1:<port>`) works `[tor-embedding]`.

**Arti is not ready for mobile now.** `arti-mobile` publishes as `org.torproject:arti-mobile:1.7.0.1` while upstream is at 2.6.0 / arti-client 0.46.0; the Tor mobile guide says it is "quite experimental and not actively maintained"; `tor-ptmgr` supports only managed PTs today (arti#666), requiring the experimental `ExternalProxyPlugin` escape hatch; and arti-client documents that it may call `exit(1)` on an obsolete consensus, which in a Flutter app is a silent kill. A Russian user reports Arti 2.4.0 failing through both Snowflake and obfs4 where C-tor succeeds `[tor-embedding]`.

Go core build discipline: any transport built on mihomo primitives needs a stub twin (`transport_default.go`) alongside `transport_mihomo.go`, or `go build ./...` and `go test ./...` break `[core-transport]`.

FFI boundary: gomobile and dart:ffi pass only primitives, so transport configuration must arrive as a JSON string following the `SetPolicyJSON` pattern `[core-transport]`.

Flutter client: no crypto dependency exists; adding one (`cryptography` or `pointycastle`) is a prerequisite for verification. Dio uses the platform HTTP client with no adapter override, so a SOCKS-capable adapter must be introduced before any proxied fetch `[client-enroll]`.

URL scheme registration is mobile-only: Android manifest pins `host="enroll"` and `host="import"` in two separate intent-filters, iOS/macOS declare `CFBundleURLSchemes`, and **Windows and Linux register nothing** `[client-enroll]`.

---

## 3. Compatibility surface to preserve

Breaking any of these breaks paying users, third-party clients, or the frozen panel-client contract.

**Generic client delivery.** `GET /sub/{uuid}` must keep serving base64 URI lists, Clash/mihomo YAML, sing-box JSON and V2Ray output, selected by `?client=` or by User-Agent sniffing, for Hiddify, v2rayNG, v2RayTun, sing-box, Clash and Shadowrocket users `[sub-service]`, `[sub-conventions]`. Marzban's ordered UA regex table is the canonical reference implementation `[sub-conventions]`.

**Response headers.** `Subscription-Userinfo` (`upload=; download=; total=; expire=`, with `total=0` meaning unlimited), `Profile-Title` (with the `base64:` prefix convention), `Profile-Update-Interval`. These must keep being emitted `[sub-service]`, `[sub-conventions]`. Mirror every metadata field into the `#key: value` in-body form in the first ten lines, because v2rayNG **ignores all response headers** in its update path (verified in `AngConfigManager.updateConfigViaSub`) `[sub-conventions]`.

**The `CARAMBA` selector literal.** `profile.CarambaSelector = "CARAMBA"` is a declared panel-client contract that "must never change" `[existing-docs]`. Clash output is `proxy-groups[0] = {name: "CARAMBA", type: select, proxies: [Auto-All, (Auto-Relay), (Auto-Direct), ...]}` with rules ending `MATCH,CARAMBA` (`singbox/subscription_generator.rs:1457-1528`) `[sub-service]`. Note the asymmetry: sing-box names its selector `proxy` with lowercase auto groups `[sub-service]`.

**Proxy name as machine identity.** `Server.ID == Server.Name == the Clash proxy name`, with country decoded from a leading flag emoji. Any relabeling breaks server pinning, the prober, and autotune simultaneously `[sub-service]`, `[core-transport]`. Preserve it while introducing stable ids alongside.

**The `/api/v2/app/*` endpoint set** exactly as enumerated in §1.1. The Flutter `ApiClient` mirrors it one to one (`lib/data/api_client.dart:115-552`), with no drift `[client-enroll]`.

**Enroll deep links.** `carambaconnect://enroll?panel=&code=` and `carambaconnect://import?url=`, with unknown query params treated as forward-compatible. The scheme is brand-neutral and shared by all tenants; the panel URL in the link selects the instance `[existing-docs]`, `[client-enroll]`.

**Login code semantics:** 6 ASCII digits, Redis GETDEL single-use, 300s TTL, reverse index invalidating the prior code `[bot-and-payments]`.

**ABI v2 and the platform channel.** MethodChannel `com.caramba/vpn` with `configure`, `connect`, `disconnect`, `importSubscription`, `probe`, `setPolicy`, `setTunnelMode`; EventChannels `com.caramba/vpn/status` and `/traffic`; the ten C symbols plus `CarambaSetPolicy` and `CarambaProbe` looked up lazily so an older dylib fails at the call site `[existing-docs]`, `[client-enroll]`.

**Artifact filenames** `exarobot.aar` and `exarobot.xcframework` are wired into podspecs, Gradle and CMake and must not be renamed today, even though they are tenant-1 branding inside a licensable component `[existing-docs]`, `[core-transport]`.

**The `{BASE}` rule-set mirror** and `GET /rulesets/{name}`: if the base URL is empty, all RULE-SET rules and providers are dropped rather than failing `[core-transport]`, `[sub-service]`.

**Third-party import links** the mini app emits: `hiddify://import/`, `happ://import/`, `v2raytun://import/`, `sing-box://import-remote-profile?url=` `[bot-and-payments]`.

**Optional but converging:** the HWID request-header set (`x-hwid` matching `^[a-zA-Z0-9=-]{10,64}$`, `x-device-os`, `x-ver-os`, `x-device-model`, plus v2RayTun's `x-app-version`) with `x-hwid-*` response flags. Already implemented in Remnawave, v2RayTun, Happ and 3x-ui `[sub-conventions]`. It cannot be enforced against v2rayNG, v2rayN, Hiddify or stock sing-box, which do not send it `[sub-conventions]`.

---

## 4. Open design questions the protocol must answer

### 4.1 Identity and signing

- **What is a tenant?** There is no tenant concept anywhere in the panel `[panel-app-api]`. Does the protocol introduce a `panel_id` in JWT claims and DTOs, or does it keep the base URL as the sole tenant address and add only a key identity? The enroll link already carries the panel URL, so a key fingerprint is the minimum addition `[client-enroll]`.
- **Two-tier keys or one?** The TUF model is an offline root key delegating to an online signing key, with root rotation requiring the new root at exactly version N+1 signed by both old and new keys, and clients walking intermediate versions `[secure-manifest]`. The Uptane split (offline image repository authoritative about what exists, online director repository targeting individual devices) maps cleanly onto a shared catalog plus a per-device directive `[secure-manifest]`. Does a single-operator reality justify threshold 1 with the threshold field kept in the format?
- **Where does the operator's root key live?** The panel already ships ed25519 verification for licensing (`license/activation.rs`) `[panel-app-api]`. Is the root key generated at license issue by Webq Pro, or by the operator? Does Webq Pro countersign, and if so does that make Webq Pro a censorship target for every tenant at once?
- **What is the signing input?** RFC 8785 JCS keeps the bytes readable JSON; TUF canonical JSON gives byte parity with existing TUF implementations `[secure-manifest]`. A canonicalization mismatch between the Rust panel and the Dart or Go verifier is the most likely interop bug in the whole design.
- **How is the key pinned at enrollment?** Truncated keyid in the enroll link, verified against the fetched root document, so trust does not rest on TLS CAs, the CDN, or whichever mirror served the bytes `[secure-manifest]`. But the enroll link is unauthenticated plaintext today, so it is a bearer credential until exchanged, exactly the MDM QR threat model `[secure-manifest]`, `[client-enroll]`.
- **What happens to `http://` acceptance?** `normalizePanelUrl` accepts it today (`enrollment.dart:67`), already logged as a Phase E blocker `[client-enroll]`.

### 4.2 Manifest schema

- **One document or two?** A shared, cacheable, long-expiry catalog (safe on any CDN or onion mirror) plus a short-lived per-device directive referencing catalog entries by content hash, so a panel compromise can re-target existing servers but not invent new ones `[secure-manifest]`.
- **What are the envelope fields?** TUF's shape is `_type`, `spec_version`, `version` (monotonic integer), `expires` (ISO 8601), plus a `signatures` array of `{keyid, sig}` `[secure-manifest]`.
- **Three independent freshness mechanisms are needed**, not one: monotonic version rejection, expiry rejection, and a client-supplied nonce echoed inside the signed document. The nonce is Uptane's answer for devices whose clock cannot be trusted, and it is what stops replay of a still-unexpired but superseded directive `[secure-manifest]`.
- **Does the node list get a stable id?** Today country is decoded from a flag emoji in a display name `[sub-service]`. `ImportedServer {id, name, type, server, port, country}` and `ProbeResult {id, name, country, latencyMs}` already exist in the ABI as the client's node schema `[client-enroll]`.
- **How are relays represented?** They have no first-class representation in any subscription format in the ecosystem; chaining is hidden inside `dialer-proxy` and `detour` in core-specific config `[sub-conventions]`. Three orthogonal lists (exits, relays, routes) plus a flattened compatibility rendering is one shape `[sub-conventions]`.
- **What is the size budget?** The 15 to 20 KB freeze threshold is a hard protocol constraint, not an implementation detail `[censorship-ru]`. Does the manifest use CBOR or protobuf rather than JSON plus base64? Does it paginate or delta-encode node lists against a client-held snapshot?
- **Where do metadata and refusal reasons live?** `announce`, `support-url`, `profile-web-page-url` do not exist today, and a revoked subscription returns a bare 403 text body with no machine-readable reason `[sub-service]`. Signed fields survive caching and mirrors in a way headers do not.
- **Does the manifest carry a revocation list?** Nebula's blocklist is manually distributed and therefore routinely stale `[secure-manifest]`. Revocation as signed, versioned, pull-based data inside the manifest inherits the same guarantees as everything else.
- **Free-tier and manual-approval states.** A Free instance requires manual admin approval before config issuance, and an unpaid account may have only `onboarding_traffic_mb` `[existing-docs]`. The manifest needs a status enum (`pending_approval`, `onboarding`, `active`, `expired`, `revoked`) with a client-renderable reason.
- **Does it carry an engine-capability signal?** `engine/engine_stub.go` reports `Connected` with no tunnel, so Flutter shows connected while traffic leaks `[existing-docs]`.

### 4.3 Transports and the fallback ladder

- **What order?** The inputs converge on: (1) direct HTTPS to the primary domain; (2) alternate domains or mirrors from the signed catalog; (3) fetch through the app's own already-running tunnel; (4) DoH-resolved direct IP with SNI and Host separation; (5) Tor onion; (6) out-of-band paste `[core-transport]`, `[censorship-ru]`, `[secure-manifest]`, `[sub-conventions]`.
- **Why Tor is last:** Arti bootstrap is minutes without fast-bootstrap optimization, seconds after, so a Tor-first design makes cold start feel broken `[tor-embedding]`. And Tor's own bootstrap is blocked in RU `[tor-embedding]`.
- **Where does the ladder live in code?** `auth.WithHTTPClient` and `subscription.WithHTTPClient` are the two option calls every request funnels through `[core-transport]`. Does `FetchProfile` (`subscription/subscription.go:122-172`) become the loop, with size-limited reads, ETag caching, and a last-good on-disk profile as the final rung? `resource.HTTPVehicle.Read` is a working reference for the caching and limiting parts `[core-transport]`.
- **Does Caramba ship its own Snowflake broker and relay?** IPtProxy exposes `brokerUrl`, `relayUrl`, `stunServer`, `natProbeUrl` as settable properties, so Caramba can point Snowflake at its own infrastructure terminating at its own API, without using public Tor infrastructure at all `[tor-embedding]`. That is a censorship-resistant path that is not Tor.
- **dnstt for the config-only path?** It speaks PT v1 over DoH and DoT resolvers, embeds KCP/smux, encrypts and authenticates the server by public key, and is already in IPtProxy. For a few-KB payload its low bandwidth does not matter `[tor-embedding]`.
- **Which Tor client?** C-tor (`info.guardianproject:tor-android:0.4.9.11` on Android, `pod 'Tor', '~> 409'` on iOS) wired to IPtProxy via unmanaged PTs, versus Arti. The evidence favors C-tor on mobile now and Arti on the desktop dart:ffi path `[tor-embedding]`.
- **Onion client authorization?** v3 restricted discovery (`descriptor:x25519:<base32>`) makes the operator's onion undecryptable, and therefore invisible, to anyone without the key `[secure-manifest]`.
- **How do bridges reach users?** Ship operator-run obfs4 and WebTunnel bridge lines as signed, rotatable manifest parameters. Tor itself moved to a Telegram distributor for the RU market `[tor-embedding]`. But Telegram is now blocked in RU `[censorship-ru]`, so the bot cannot be the only out-of-band channel.
- **Does the ladder need a whitelist rung?** The only demonstrated escape from whitelist mode is tunneling through domestic infrastructure (VK TURN) `[censorship-ru]`. Does the Go core expose a pluggable outbound transport interface so a TURN or WebRTC carrier can drop in without an app-store release?
- **How is the ladder made store-legal?** Apple 2.3.1 forbids dormant or region-conditional features, so every transport must be a user-visible setting described specifically in review notes `[store-compliance]`.
- **Traffic shape.** No ETag, no If-None-Match, `Cache-Control: no-store`, and a client polling every 2 minutes produces regular identically-sized TLS flows to one host `[sub-service]`. Does the protocol mandate jitter and bucketed padding?

### 4.4 Settings sync model

- **Which direction, and what unit?** Every mechanism in the ecosystem is server-to-client push at fetch time; there is no channel for a client to report its chosen server, relay or routing profile back `[sub-conventions]`. `CoreConfig` stores selections as **indices into client-side option lists**, which is version-fragile as a sync unit; `CorePolicy`'s string vocabulary is the stable one, and `corePolicyFrom` is the single translation point `[client-enroll]`.
- **Where does preference state live server-side?** Today relay is persisted as a side effect of a GET and readable only via `/subscriptions`; server pinning is persisted silently; routing and variant have no representation at all `[panel-app-api]`, `[sub-service]`. A `GET`/`PUT /api/v2/app/preferences` returning `{exit_node_id?, relay_country?, routing_mode?, variant?}` would make the config fetch a pure read.
- **Does the client report state back (Uptane vehicle version manifest)?** A signed statement of the currently-held directive version lets the panel detect a device pinned to an old manifest, giving server-side rollback detection `[secure-manifest]`. What is the privacy cost, given Apple 5.4 bans disclosing any data to third parties and 5.1.2 bans surreptitious profiling `[store-compliance]`?
- **Does the relay picker become real?** Docs say relays are auto-selected by geo and hidden from user-facing UI; the vision says the user picks them in the app; the panel exposes only a read-only country aggregate `[existing-docs]`, `[panel-app-api]`. And the Clash generator ignores relays entirely, so the picker is currently decorative `[sub-service]`. This contradiction must be resolved before the schema is fixed.

### 4.5 Device binding

- **What is a device today?** An IP address plus a User-Agent string seen at config-fetch time (`subscription.rs:208-271`) `[panel-app-api]`. Any alternative delivery path (Tor, mirror, CDN) either burns a device slot or blinds tracking entirely `[sub-service]`. **Device-limit enforcement actively punishes the fallback ladder the protocol is meant to add.**
- **Two models are unreconciled:** IP-tracking rows the bot shows versus named device rows the app manages `[bot-and-payments]`.
- **What replaces it?** A per-device keypair generated on-device in the platform keystore, non-exportable, registered at enrollment, with the thumbprint as identity. Tailscale splits machine key (hardware) from node key (session), and the private key never leaves the device `[secure-manifest]`.
- **How is the bearer URL retired?** DPoP (RFC 9449) binds the token to a key thumbprint via the `jkt` confirmation claim, with the proof carrying `htm`, `htu`, `iat`, `jti`, `ath` and a server nonce, so a stolen token is useless `[secure-manifest]`. RFC 9700 requires public-client refresh tokens to be sender-constrained or rotated with family revocation on reuse `[secure-manifest]`.
- **What issues the enrollment credential?** Enrollment codes are never issued by any code path `[panel-app-api]`. Headscale pre-auth key semantics (single-use flag, ephemeral flag, ~1h default expiry) plus RFC 8628 device-flow polling states (`authorization_pending`, `slow_down`, `access_denied`, `expired_token`) are the pattern `[secure-manifest]`.
- **Does the subscription UUID stop being the tunnel credential?** Today `subscription_user_uuid()` makes them the same value, so URL leakage equals credential leakage `[sub-service]`.

### 4.6 Payments hand-off to the bot

- **Can the bot still be the payment channel in Russia?** Telegram is largely inaccessible on RU home broadband and mobile since mid-March 2026, desktop included, and is not whitelisted `[censorship-ru]`. Whether the Bot API, `t.me` deep links, and Telegram Stars still complete from inside Russia is **unverified and must be field-tested** `[censorship-ru]`.
- **What may the app say about payment?** Outside the US storefront: nothing. No bot handle, no prices, no links, no "buy" call to action (Apple 3.1.3) `[store-compliance]`. The enrollment screen must read as configuration, not commerce.
- **Is the pasted subscription a "license key" under Apple 3.1.1?** Unadjudicated. The defence is that the app unlocks nothing and the operator's VPN is a separate multiplatform service. Peer apps pass review with this shape but that is behavioral evidence only `[store-compliance]`.
- **What is the in-app renewal surface?** Happ's `sub-expire` plus `sub-expire-button-link` and `sub-info-*` directives render an operator-driven renewal prompt without an app update `[sub-conventions]`. What is the store-safe equivalent given 3.1.3?
- **What carries the user across the payment gap?** `onboarding_traffic_mb` is the designed escape hatch: enough traffic for a new unpaid account to connect, reach the payment channel, and pay `[existing-docs]`. Note its semantics already changed once (the negative-`used_traffic` trick was removed) `[panel-app-api]`.
- **Does `bot_buttons_mode = "app_only"` flip before or after feature parity?** Gifts, subscription transfer, notes, referral alias editing, referrer entry, the digital store and cart, plan extension and the profile file are bot-only today `[bot-and-payments]`.

### 4.7 Privacy

- **What may appear in a manifest?** No Telegram id, username, phone, email, or payment reference. Device identity should be a public-key thumbprint (high entropy by construction), never a hash of a Telegram id or phone number, which is brute-forceable in seconds `[secure-manifest]`.
- **Should the per-device directive be sealed?** HPKE (RFC 9180, base mode, DHKEM(X25519, HKDF-SHA256) plus ChaCha20Poly1305) to the device key means a CDN, mirror or onion front learns only a ciphertext and a thumbprint, which is what makes hosting on infrastructure you do not control acceptable `[secure-manifest]`.
- **What does the operator learn?** `caramba-sub` deliberately forwards client IP and User-Agent to the panel for device tracking and geo filtering `[sub-service]`, so every refresh leaks the user's real IP with no padding and no jitter `[client-enroll]`.
- **Can the fetch be authorized without identifying?** Signal's delivery token authorizes rate-limiting without authenticating the sender; Oblivious HTTP splits IP knowledge from plaintext knowledge across a non-colluding relay and gateway `[secure-manifest]`. Both come with caveats: OHTTP does not support stateful auth and targets infrequent requests, and Signal explicitly places timing and IP correlation outside its scope `[secure-manifest]`.
- **What obligations does multi-tenancy create?** Apple 5.1.1(i) requires the privacy policy to confirm third parties provide equal protection, while 5.4 bans disclosing data to third parties at all `[store-compliance]`. The operator receiving user data is such a third party, so the licence agreement must bind operators contractually.
- **Should there be a transparency log?** It detects an operator equivocating between users, but publishes an observable operator-activity record which is itself a censorship target `[secure-manifest]`.

---

## 5. Risks

**R1. Telegram unavailability breaks the payment path in the primary market.** Telegram is effectively blocked in RU since mid-March 2026 `[censorship-ru]`, yet the product vision makes the bot the payment channel and several inputs propose it as the out-of-band bootstrap channel `[tor-embedding]`, `[sub-conventions]`. Bot API reachability from inside Russia is unverified `[censorship-ru]`.

**R2. Manifest size versus the freeze threshold.** A verbose JSON plus base64 payload over ~15 to 20 KB is silently frozen on foreign datacenter IPs `[censorship-ru]`. Adding signatures, relay lists, routing policy and mirror lists all push the wrong direction. The threshold is from mid-2025 and may have tightened `[censorship-ru]`.

**R3. Apple 3.1.1 license-key exposure.** No written rule or public precedent resolves whether a pasted operator subscription counts as an "own mechanism to unlock" `[store-compliance]`. This is the single largest unquantified rejection risk.

**R4. RU App Store removal is the expected steady state on iOS.** The exact peer set (Streisand, V2Box, v2RayTun, Happ) was pulled on 2026-03-28 with no court process `[store-compliance]`. Renaming only delays removal, and appeals have no public record of success `[store-compliance]`.

**R5. Apple 2.3.1 account-termination tier.** A Tor or anti-DPI fallback that activates conditionally on network failure or geography is a hidden-feature violation, penalized at the developer-program level `[store-compliance]`.

**R6. gomobile single-framework collision.** Adding Snowflake or lyrebird as a second binding is impossible; the merge into one Go module with mihomo is unverified (Go version alignment, cgo flags, duplicate transitive deps such as quic-go and pion WebRTC versus mihomo's own stack) `[tor-embedding]`.

**R7. iOS Network Extension 50 MiB ceiling.** Running tor plus PTs plus mihomo in the NE is not viable; Xray already exhausts the budget on geo files alone `[tor-embedding]`. Headroom with mihomo alone is unmeasured.

**R8. Arti is a trap on mobile today.** Pinned at 1.7.0 versus upstream 2.6.0, "not actively maintained" per Tor's own guide, managed-PT-only, `exit(1)` on obsolete consensus, roughly 2x C-tor's arm64 footprint, and a live RU report of failure where C-tor succeeds `[tor-embedding]`.

**R9. Device-limit enforcement is incompatible with the fallback ladder.** IP-based counting punishes every alternative delivery path the protocol adds `[sub-service]`, `[panel-app-api]`.

**R10. Canonicalization drift.** Three implementations (Rust panel signer, Go core verifier, Dart client verifier) must agree byte for byte on the signing input `[secure-manifest]`.

**R11. Enrollment codes cannot be minted.** No admin endpoint, no bot command, no CLI. Operators need database access to onboard a user, which blocks the entire licensed-operator story `[panel-app-api]`.

**R12. Access tokens are unrevocable for 15 minutes**, there is no refresh-token reuse detection, no logout-all endpoint despite the index existing, no device binding, and a single symmetric JWT secret with no `kid` and no rotation `[panel-app-api]`.

**R13. Relay work is a prerequisite, not a parallel track.** The Clash generator never reads `_relay_nodes` `[sub-service]`, so shipping an in-app relay picker on top of the current generator ships a placebo.

**R14. Bootstrap dependencies on blocked hosts.** Geo databases download direct from GitHub `[core-transport]`; the sing-box config pulls all rule-sets from `raw.githubusercontent.com` `[sub-service]`; the default policy hardcodes DoH to 1.1.1.1 and 8.8.8.8 `[core-transport]`; every url-test group probes `gstatic.com` `[sub-service]`. All are blocked or being blocked in RU `[censorship-ru]`.

**R15. Availability regression.** `caramba-sub` today serves a 5-minute stale cache instead of a 502 when the panel is unreachable `[bot-and-payments]`, `[sub-service]`. Any redesign must preserve or improve that, and only signing makes serving stale content safe.

**R16. Per-tenant uniformity is a structural censorship risk.** Once a circumvention method becomes uniform, visible and widely reused, it gets targeted `[censorship-ru]`. Licensing the same protocol to many operators creates exactly that uniformity unless per-tenant diversity (domains, SNI sets, transport defaults, rotation schedules) is designed in.

**R17. Legal exposure.** Advertising or popularizing VPN services carries fines to 500,000 RUB for legal entities in Russia since 2025-09-01 `[store-compliance]`, `[censorship-ru]`, and ACF warns a circumvention app operator may be designated extremist `[existing-docs]`. Figures are secondary-sourced and need counsel.

**R18. Nothing in the Connect track has been compiled, bound, or signed.** The plan is explicitly static-only, `flutter analyze` and `dart format` are `continue-on-error` in CI, and the client runs on `MockVpnConnection` unless `--dart-define=USE_NATIVE_VPN=true` `[existing-docs]`. Meanwhile the panel, bot, sub, node and mini app serve real users and must not regress `[existing-docs]`.

**R19. Documentation drift.** `docs/API.md` documents zero `/api/v2/app/*` endpoints `[panel-app-api]`, `docs/UNDERSTAND-2026-09-02.md` understates the VPN contract by four methods and lists three already-fixed defects `[client-enroll]`, and three docs still carry the wrong bot handle `[client-enroll]`.

---

## 6. What to build first

Ordered by dependency and by risk retired per unit of work. Items 1 through 4 are prerequisites for the protocol to exist at all; 5 through 8 are the protocol; 9 through 12 are what make it survive Russia and the stores.

**1. Enrollment code issuance.** Add an operator-facing issuer (admin endpoint plus bot command) writing to `enrollment_codes`, and extend the row with the fields a protocol needs: a panel or tenant identifier, an optional plan binding, a label, and a revoked flag `[panel-app-api]`. Without this no operator can onboard a user without database access, and the licensed-operator story does not exist. Cheapest item on the list, largest blocker removed.

**2. Fix the relay gap in the Clash generator.** Make `generate_clash_config` consume `relay_nodes` and emit relay chains (mihomo `dialer-proxy`, or a relay proxy plus per-node dialer binding) with an Auto-Relay group, mirroring `generate_singbox_config` at `singbox/subscription_generator.rs:1604-1626` `[sub-service]`. Until this lands, every relay feature above it is decorative.

**3. Panel identity and signing keys.** Introduce a stable `panel_id` plus an ed25519 keypair, reusing the license module's existing verification machinery (`license/activation.rs`) `[panel-app-api]`. Two tiers (offline root, online signing key) with a TUF-shaped root document carrying `_type`, `spec_version`, `version`, `expires`, `keys`, `roles` and a `signatures` array `[secure-manifest]`. Put `panel_id` in JWT claims, and extend the enroll link with a truncated root keyid so the client pins the operator rather than trusting whatever host the link named `[secure-manifest]`, `[client-enroll]`.

**4. Reject `http://` and stop following redirects blindly.** `normalizePanelUrl` (`enrollment.dart:60-71`) and `fetchSubscriptionBody` (`subscription_fetch.dart:22-49`) both need hardening: HTTPS only (with a documented `.onion` exception, since onion addresses are self-authenticating), `followRedirects: false` with per-hop scheme and origin validation, and a body size cap `[client-enroll]`, `[secure-manifest]`. Add a crypto dependency to `pubspec.yaml` in the same change.

**5. Define and ship the signed manifest, v1.** Two documents: a shared catalog (long expiry, mirrorable, signed offline) and a per-device directive (short expiry, signed online, referencing catalog entries by content hash) `[secure-manifest]`. Envelope with monotonic `version`, `expires`, and a client-supplied `nonce` echoed inside the signed payload `[secure-manifest]`. Pick one canonicalization (RFC 8785 JCS or TUF canonical JSON) and freeze it in `spec_version` `[secure-manifest]`. Carry the node list in the existing `ImportedServer` shape so `format` never has to be guessed `[client-enroll]`. Budget the whole payload under ~8 KB, in CBOR or protobuf `[censorship-ru]`.

**6. Preferences as real state.** Add `GET`/`PUT /api/v2/app/preferences` carrying the `CorePolicy` string vocabulary (not `CoreConfig` indices) for `{exit_node_id?, relay_country?, routing_mode?, variant?, protocol?, preset?, split?}`, persisted on the subscription. Then make the config fetch a pure read reflecting stored preferences, removing the `UPDATE ... SET relay_country` write-on-GET at `subscription.rs:745` and the silent node auto-pin at `:617-634` `[panel-app-api]`, `[sub-service]`, `[client-enroll]`. Add the inverse of `corePolicyFrom` in `core_policy_mapping.dart` so a fetched policy repopulates the pickers `[client-enroll]`. This is what makes in-app server, relay and routing changes stick across devices, which is the product goal.

**7. The transport ladder in the Go core.** A new `transport` package implementing `HTTPDoer`, wired at exactly two option calls (`auth.WithHTTPClient`, `subscription.WithHTTPClient`), constructed in **both** `NewCore` and `SetPanelURL` (forgetting the second silently reverts a re-enrolled tenant to the default transport) `[core-transport]`. Rewrite `FetchProfile` as a loop over an ordered ladder with per-attempt timeouts, `io.LimitReader`, ETag caching, and a last-good on-disk profile `[core-transport]`. Ship `transport_mihomo.go` and `transport_default.go` following the `engine_mihomo.go` / `engine_stub.go` discipline `[core-transport]`. Deliver configuration as a JSON string through the `SetPolicyJSON` pattern so the flat gomobile and FFI signatures do not change `[core-transport]`.

**8. Device identity and the end of the bearer URL.** Generate a per-device keypair in the platform keystore at enrollment, register the public key, and bind both the refresh token and the config request to it via a DPoP-style proof `[secure-manifest]`. Keep `/sub/{uuid}` for legacy clients but stop treating the UUID as a standalone credential for Caramba Connect, and decouple it from the VLESS/Trojan/TUIC credential `[sub-service]`. Add refresh-token reuse detection with family revocation, a logout-all endpoint (the index already exists), and `jti` plus a cheap Redis denylist `[panel-app-api]`, `[secure-manifest]`. This is also what makes device counting work at all once the fallback ladder changes the apparent source IP `[sub-service]`.

**9. Mirror pool and bootstrap de-blocking.** Generalize the `{BASE}` mechanism (`routing/presets.go:26-58`) from one base URL into an ordered mirror pool shared by the manifest fetch, rule-set providers, and geo databases `[core-transport]`. Emit `proxy:` on rule-providers (`routing/routing.go:186-192`) and call `geodata.SetGeoIpUrl` / `SetGeoSiteUrl` / `SetMmdbUrl` / `SetASNUrl` at panel mirrors beside the existing `SetHomeDir` call `[core-transport]`. Move bootstrap DNS out of the hardcoded 1.1.1.1 / 8.8.8.8 default and into the signed manifest as per-region DoH `[core-transport]`, `[existing-docs]`, `[censorship-ru]`. Bundle a fallback RU ruleset in the binary and fetch RU rulesets with `download_detour: "direct"` `[existing-docs]`. Give every mirror a different ASN and hosting class `[censorship-ru]`.

**10. The merged Go module spike, then Tor.** Two weeks, three measurements before committing: (a) merge mihomo plus IPtProxy transports into one gomobile module and measure the real per-ABI delta and thinned IPA size; (b) wire C-tor to it via unmanaged PTs on both platforms and time cold bootstrap from a Russian vantage point over obfs4, WebTunnel and Snowflake; (c) measure NE memory headroom on iOS with mihomo alone `[tor-embedding]`. Those numbers decide whether the onion rung ships in v1. Prefer WebTunnel over obfs4, host bridges on non-mainstream providers, ship randomised DTLS fingerprints if Snowflake is included, and consider pointing Snowflake at a Caramba-run broker and relay rather than public Tor infrastructure `[tor-embedding]`, `[censorship-ru]`.

**11. Store readiness, in parallel from day one.** Enroll as an Organization (Apple 5.4 auto-rejects individual accounts) `[store-compliance]`. Build the pre-use data declaration screen and the Play prominent-disclosure consent screen as protocol requirements `[store-compliance]`. Provision a permanent demo subscription against a live panel for Apple 2.1(a) `[store-compliance]`. Complete Play's VpnService declaration form `[store-compliance]`. Make every transport a visible setting described specifically in review notes, never conditional `[store-compliance]`. Write the missing store-payments section covering Apple 3.1.1, 3.1.3 and Play Billing against the pay-in-the-bot model, which `STORE-COMPLIANCE.md` currently omits entirely `[existing-docs]`, `[store-compliance]`. Stand up signed direct-APK distribution with published certificate fingerprints and reproducible builds before it is needed `[store-compliance]`.

**12. Out-of-band bootstrap, telemetry, and the cleanup pass.** A QR or `carambaconnect://` deep link carrying a compact signed bootstrap blob (endpoints, root key, expiry) that works when both the mirror pool and the bot are down `[censorship-ru]`, `[secure-manifest]`; wire the working `showQrScanSheet` into the enroll screen in place of `_scanQrStub` and register the scheme on Windows and Linux `[client-enroll]`. Instrument the client for connection success rate, handshake failure type, transport fallback rate and per-ASN anomaly detection, reported over an already-working tunnel, aggregated and coarse-bucketed, because product telemetry is the only reliable availability oracle when TSPU sits close to end users `[censorship-ru]`. Then the cleanup: extend rate limiting across the whole app router mirroring the bot router's design and decide fail-open versus fail-closed deliberately `[panel-app-api]`; plan-scope `GET /relays` `[panel-app-api]`; put `/api/v2/client/recommended` behind auth or fold it into the signed manifest `[panel-app-api]`; forward `variant` through `caramba-sub` or drop the URL-parameter surface for the app entirely `[sub-service]`; settle the `profile-update-interval` unit by emitting both the legacy header and an explicit `refresh_seconds` in the payload `[sub-service]`; unify client-type detection in one shared function `[sub-service]`; consolidate on one bot binary, either deleting `apps/caramba-bot` or repairing its nineteen missing routes, settings allowlist gaps and Stars pre-checkout id comparison `[bot-and-payments]`; strip tenant-1 branding from the licensable core (`CarambaVpnService.kt:242`, and the `exarobot.aar` / `exarobot.xcframework` filenames once the podspec churn is affordable) `[client-enroll]`, `[core-transport]`; fix the `tg_id` `Option<i64>` launch blocker before app-created accounts ship `[existing-docs]`; and correct `@exarobot` to `@exa_robot` in `ANTI-SLOP.md:12`, `docs/CARAMBA-CONNECT-PLAN.md:21` and `docs/CARAMBA-CONNECT-PHASE-A-DESIGN.md:114`, plus refresh `docs/API.md` and `docs/UNDERSTAND-2026-09-02.md` `[client-enroll]`, `[panel-app-api]`.

---

### Closing note on sequencing

Items 1, 2 and 4 are small, unblock everything downstream, and can land against the live system without touching runtime behavior for the 20 existing users `[existing-docs]`. Items 3, 5 and 6 define the protocol and should be specified together, because the manifest schema and the preferences schema are the same vocabulary viewed from two directions. Item 7 is the largest single engineering block and depends only on 5. Item 10 is a measurement gate, not a commitment: if the merged-module spike or the iOS NE headroom measurement fails, the onion rung moves to v2 and the ladder ships with rungs 1 through 4 only, which still retires most of R2, R14 and R15.