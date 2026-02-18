# CARAMBA

Rust workspace for censorship-resistant VPN orchestration.

Caramba manages node provisioning, Sing-box config generation, subscriptions, billing/bot workflows, and disposable frontend delivery.

## Components

- `apps/caramba-panel`: control plane (admin UI, APIs, orchestration, telemetry, billing, SNI pool, relay logic)
- `apps/caramba-node`: node agent (heartbeat, config pull, update flow, telemetry, SNI neighbor scan)
- `apps/caramba-sub`: edge frontend/proxy for `/sub`, `/app`, `/api`, with heartbeat to panel
- `apps/caramba-bot`: Telegram bot binary
- `apps/caramba-installer`: install/bootstrap utility
- `apps/caramba-app`: user mini app frontend assets
- `libs/caramba-db`: shared DB models/repositories/migrations
- `libs/caramba-shared`: shared API/config contracts

## Current Highlights

- Relay mode supports auth rollout modes: `legacy`, `v1`, `dual` (default).
- Guardrail: switch to `v1` is blocked if legacy relay traffic was observed within last 24h.
- Adaptive node capacity (`max_users`) based on speed and host load.
- Consolidation support: `/app` + `/sub` can live on one domain (panel or sub edge).

Detailed state: `current_state_2026-02-18.md`.

## Build & Test

```bash
cargo check --workspace
cargo test --workspace
```

Run panel locally:

```bash
cargo run -p caramba-panel -- serve
```

Run node/bot/sub binaries:

```bash
cargo run -p caramba-node
cargo run -p caramba-bot
cargo run -p caramba-sub
```

## Docs

- `docs/DEPLOYMENT.md`
- `docs/CONFIGURATION.md`
- `docs/DEVELOPMENT.md`
- `docs/MODULES.md`
- `docs/DATABASE.md`
- `docs/API.md`

## CI/CD

GitHub Actions workflow: `.github/workflows/release.yml`

- Triggered on tags `v*`
- Can be launched manually via `workflow_dispatch`
- Performs workspace check + release build (musl target)

## License

Proprietary / Closed Source
