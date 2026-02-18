# Caramba Current State (2026-02-18)

This document reflects what is implemented in code now, after refactor cleanup.

## Workspace Modules

`apps/caramba-panel`
- Main control plane.
- Hosts admin UI (`/admin...`), public subscription endpoint (`/sub/{uuid}`), mini app (`/app...`), node/agent APIs (`/api/v2/node/...`), bot APIs, client APIs, internal APIs.
- Owns orchestration, node lifecycle, policy management, SNI pool management, billing/store/referrals/family, background monitoring loops.

`apps/caramba-node`
- Node agent.
- Pulls configs from panel, reports heartbeat/telemetry, supports long-poll update signals, self-update, kill-switch, on-demand log collection.
- Runs speed test and neighbor SNI scan (hourly + manual trigger).

`apps/caramba-sub`
- Disposable edge frontend for `/sub`, `/app`, and `/api` proxy to panel.
- Generates subscription payloads via panel internal APIs.
- Now reports frontend heartbeat stats to panel (`/api/admin/frontends/{domain}/heartbeat`).

`apps/caramba-bot`
- Telegram bot binary (currently with many dead-code sections, but compiles).

`apps/caramba-installer`
- Installer/diagnostics/systemd setup.

`libs/caramba-db`
- Shared DB models/repositories and Postgres migration schema.

`libs/caramba-shared`
- Shared API/config payload contracts used by agent/panel.

## Key Feature Status

### 1) Neighbor SNI scanner + best selection + manual selection
Status: `Implemented (with caveats)`
- Node scanner probes local `/24` over TLS and sends discoveries in heartbeat.
- Panel persists discovered SNIs, can auto-assign for generic node SNI.
- Admin can pin/unpin/block SNI and trigger manual scan.
- Caveats:
  - Scanner is IPv4-only.
  - Subnet probing is sequential (can be slow on poor links).

### 2) Detailed node monitoring
Status: `Implemented`
- Heartbeat stores latency/CPU/RAM/speed/connections/hardware/uptime/version.
- Background monitor marks nodes/frontends offline by heartbeat timeout.
- On-demand node log collection pipeline exists.

### 3) Recommended users per node
Status: `Implemented (adaptive)`
- `max_users` now uses measured speed plus CPU/RAM load factor with smoothing.
- High host load automatically lowers recommended capacity.
- Still can be improved with protocol mix/QoS weighting and confidence intervals.

### 4) Speed measurement
Status: `Partially implemented`
- Node runs a startup speed test and reports result.
- No periodic re-test scheduler yet.

### 5) Promo center
Status: `Implemented`
- Promo CRUD in admin panel.
- Promo and gift redemption logic in services.
- Promo usage tracking table is used.

### 6) Inbound rotation
Status: `Implemented`
- Background rotation service rotates inbounds by `renew_interval_mins`.
- Group/template flows and manual rotate controls are present.

### 7) Relay node mode
Status: `Implemented with compatibility rollout mode`
- Relay flags/relations are in DB and models.
- Orchestration and Sing-box generators include relay paths.
- Relay transport now resolves target Shadowsocks inbound (port/method) when present.
- Relay auth supports versioned modes via setting `relay_auth_mode`:
  - `legacy`: raw `join_token`.
  - `v1`: deterministic derived password from `join_token + target_node_id`.
  - `dual` (default): target inbound accepts both (`relay_<id>` hashed + `relay_<id>_legacy` raw) while relay outbound uses hashed.
- `relay_auth_mode` is configurable from admin settings UI.
- Relay guardrail:
  - Panel tracks last observed legacy relay traffic from node heartbeat `user_usage` (`relay_*_legacy` tags).
  - Switching to `v1` is blocked if legacy traffic was seen within the last 24 hours.
- Risk areas:
  - Needs integration tests for multi-node relay chains and failure scenarios.

### 8) `app + sub` on one domain
Status: `Implemented in panel`, `supported in sub`
- Panel already serves both `/app` and `/sub`.
- Sub also serves both paths and can proxy APIs.
- This allows consolidation on one domain when desired.

## Regressions Found/Fixed in This Iteration

- Fixed `caramba-sub` mini app delivery: `/app` was effectively stubbed and always 404, now serves files from `apps/caramba-app/dist` with SPA fallback.
- Added heartbeat metrics loop in `caramba-sub` and delivery to panel frontend heartbeat endpoint.
- Removed refactor artifact comments from `caramba-panel/src/main.rs`.

## High-Risk Gaps To Prioritize Next

1. Relay mode hardening
- Add integration tests for relay config generation and chain validation.

2. Node capacity model
- Extend adaptive model with protocol mix/QoS weighting and confidence intervals.

3. Edge frontend observability
- Add per-route/request metrics and panel-side dashboards/alerts for frontend fleet.

4. SNI scanner quality
- Add concurrency limits + timeout budget + dedup scoring.
- Add IPv6 strategy if needed.

5. Dead/legacy paths
- Continue removing duplicate legacy handlers/routes and no-op compatibility surfaces after migration window closes.
