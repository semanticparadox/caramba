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

## Reference

- `current_state_2026-02-18.md`
  - Snapshot of implemented features, gaps, and priorities.
