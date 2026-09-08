# Caramba Modules (Current Architecture)

This document reflects the current workspace layout and runtime responsibilities.

## Workspace Overview

- `apps/caramba-panel`
  - Control plane.
  - Hosts admin UI, public subscription endpoint, app pages, internal/bot/node APIs.
  - Generates Sing-box configs, orchestrates node sync, manages users/billing/subscriptions.
  - Processes node telemetry and frontend heartbeats.
  - Owns the production Telegram bot as an embedded module (`src/bot`, started from `bot_manager.rs` via `crate::bot::run_bot`) — this is the bot that shows the `caramba://connect` sign-in link and code, not `apps/caramba-bot`.
  - Settings `app_download_url_{android,ios,windows,macos,linux}` (Settings → "Caramba Connect app — download links") hold per-platform app download URLs, and `GET /api/client/app/downloads` / `POST /api/client/app/connect-link` expose them and the connect-link issuer to the mini app.

- `apps/caramba-node`
  - Node agent.
  - Pulls configs from panel, reports heartbeat/telemetry, applies updates.
  - Runs connectivity checks and nearby SNI discovery.

- `apps/caramba-sub`
  - Disposable edge for `/sub`, `/app`, `/api` proxy.
  - Serves mini app assets and reports frontend heartbeat to panel.

- `apps/caramba-bot`
  - Separate Telegram bot binary (store/promo/payments/admin) talking to the panel over HTTP.
  - Not the bot that issues the `caramba://connect` sign-in link — there is no `connect_link` logic in this crate; that lives in `apps/caramba-panel/src/bot`.

- `apps/caramba-installer`
  - Install/bootstrap binary.

- `apps/caramba-app`
  - Frontend assets (mini app) used by panel/sub.

- `apps/caramba-client`
  - The end-user VPN client: Flutter UI plus the `caramba_vpn` federated plugin
    (`packages/caramba_vpn`), which bridges the `com.caramba/vpn` method and
    event channels to the Go core.
  - Five platform folders live in the tree (`android/`, `ios/`, `macos/`,
    `windows/`, `linux/`); the first three are committed, `windows/` and
    `linux/` were added in this round and land with it. Apple platforms share
    one `darwin/` source tree via `sharedDarwinSource`.
  - Native runbook: `apps/caramba-client/INTEGRATION.md`. Per-platform status
    and build commands: `apps/caramba-client/README.md`.

- `libs/caramba-core`
  - The Go engine (mihomo, built with `-tags mihomo,with_gvisor`) and every
    binding the client consumes. Requires `go 1.26.0` and `CGO_ENABLED=1`.
  - `scripts/build-mobile.sh android|ios|macos` — gomobile bindings
    (`exarobot.aar`, `exarobot.xcframework`); vendors them into the plugin itself.
  - `scripts/build-desktop-lib.sh [macos|linux|windows]` — the cgo `c-shared`
    library `libcaramba_core.{dylib,so,dll}` for the `dart:ffi` / C++ desktop
    path. macOS is built universal (`arm64 + x86_64`).
  - `scripts/build-windows-lib.sh`, `scripts/fetch-wintun.sh` — the Windows DLL
    (cross-buildable with mingw-w64) and the pinned, SHA-256-verified WinTun
    runtime.
  - `scripts/build-smoke.sh` — `cmd/caramba-smoke`, a privilege-free proxy-mode
    smoke test of the core without Flutter.
  - `scripts/mk-patched-deps.sh` — mihomo needs a patch, or TUN does not start.
  - All build outputs are gitignored; nothing here is a committed artifact.

- `libs/caramba-db`
  - Shared models, repositories, and migrations.

- `libs/caramba-shared`
  - Shared request/response/config payload contracts.

## `caramba-panel` Internal Structure

### Entry and Wiring

- `src/main.rs`
  - Builds `AppState`.
  - Initializes services and routes.
  - Runs server and background monitoring tasks.

### HTTP Handlers

- `src/handlers/admin/*`
  - Admin web UI handlers (Askama + HTMX).
- `src/handlers/api/*`
  - JSON APIs for bot/client/internal flows.
- `src/api/v2/*`
  - Node-facing APIs (heartbeat, config pull, update info).
- `src/handlers/local_app.rs`
  - Serves local mini app assets when enabled.
- `src/handlers/frontend.rs`
  - Frontend server management and heartbeat ingestion.

### Services

- `src/services/orchestration_service.rs`
  - Builds final node config context, injects users, relay context, validates config.
- `src/services/telemetry_service.rs`
  - Handles node telemetry and adaptive `max_users` recommendation.
- `src/services/infrastructure_service.rs`
  - Node/group/template lifecycle and infra operations.
- `src/services/subscription_service.rs`, `store_service.rs`, `catalog_service.rs`, `billing_service.rs`
  - Subscription/store/billing operations.
- `src/services/security_service.rs`
  - SNI selection and security-related helpers.
- `src/services/monitoring.rs`
  - Liveness/offline checks for nodes/frontends.

### Sing-box Generation

- `src/singbox/generator.rs`
  - Converts DB inbounds and node policies into Sing-box JSON.
  - Relay logic includes auth rollout modes:
    - `legacy` (raw token),
    - `v1` (derived password),
    - `dual` (accept both for migration window).
- `src/singbox/subscription_generator.rs`
  - Client-facing config/link generation.

## Relay Rollout Notes

- Runtime setting: `relay_auth_mode`.
- Guardrail: switching to `v1` is blocked if legacy relay traffic was observed during the last 24 hours.
- Legacy usage is observed from node heartbeat `user_usage` (`relay_*_legacy` tags).

## Client Platform Status (2026-09-07)

Honest per-platform state. "Verified" means observed on a real machine, not
described in a workflow file. The long version, with the reasons, is in
`apps/caramba-client/INTEGRATION.md`.

| Platform | Tunnel path | Build | Signed | Verified on a device |
| --- | --- | --- | --- | --- |
| Android | `VpnService` + AAR binding | production, CI-built release APK | yes | yes — served from the panel |
| macOS | `proxy` via `dart:ffi` on `libcaramba_core.dylib`, mixed inbound `127.0.0.1:7890` | release `.app` + unsigned DMG, universal | no | app launches; no Network Extension |
| iOS | Network Extension — target does not exist | compiles/links against the real core (simulator) | no | no |
| Windows | C++ plugin + `libcaramba_core.dll` + `wintun.dll` | defined in CI, never executed | no | no |
| Linux | C++/GObject plugin + `libcaramba_core.so` | defined in CI, never executed | n/a | no |

Blocked on the owner, not on code: Apple Developer Program membership (macOS
signing/notarization, the iOS/macOS Network Extension target), a Windows
code-signing certificate, a physical Windows PC and a Linux machine, and a
decision on Windows elevation (`requireAdministrator` versus a UAC relaunch).

## Client Build and Distribution

- CI: `.github/workflows/client-android.yml` (signed APK) and
  `.github/workflows/client-desktop.yml` (three independent jobs: macOS DMG +
  iOS compile check, Windows ZIP, Linux tar.gz). Both trigger on `v*` tags,
  appending assets to that tag's release, and on `workflow_dispatch`.
  Toolchains pinned: Flutter `3.47.2`, Go `1.26`.
- All build logic lives in `apps/caramba-client/scripts/ci-android.sh` and
  `ci-desktop.sh`, so a local run and CI take the same path. Each asserts that
  the native core actually reached the artifact — on desktop a missing core does
  not fail the build, it silently produces a mock bundle.
- Release asset names are a contract between CI, `apps/caramba-installer` and
  `apps/caramba-panel`; renaming one breaks the mini app's download button:
  `caramba-connect-arm64.apk`, `caramba-connect-armv7.apk`,
  `caramba-connect-macos-arm64.dmg`, `caramba-connect-windows-x64.zip`,
  `caramba-connect-linux-x64.tar.gz`.
- Delivery: the installer copies whichever of those assets exist in the release
  into `<install_dir>/apps/caramba-panel/downloads/` (a missing asset is not an
  error); the panel serves that directory at `/downloads`, and
  `GET /api/client/app/downloads` falls back to `{panel_url}/downloads/<file>`
  when the `app_download_url_<platform>` setting is empty. The setting, when
  set, wins.

## Reference

- `docs/CURRENT_STATE.md`
  - Snapshot of implemented features, gaps, and priorities.
