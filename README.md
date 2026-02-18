# CARAMBA

Rust workspace for censorship-resistant VPN orchestration.

Caramba manages node provisioning, Sing-box config generation, subscriptions, billing/bot workflows, and disposable frontend delivery.

## Installation Model (Installer-First)

Primary installation flow is based on release binary `caramba-installer`.

- `scripts/install.sh` downloads latest release asset `caramba-installer`
- installs it as `/usr/local/bin/caramba`
- runs `caramba install --hub` by default
- also supports role-based installs: `panel`, `node`, `sub/frontend`, `bot`

### One-liner Install

```bash
curl -sSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh | sudo bash
```

Manual download:

```bash
curl -sSLO https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh
chmod +x install.sh
sudo bash install.sh
```

So `caramba-installer` is the control point for install/upgrade/diagnostics/restore/uninstall workflows, not just a one-time bootstrap helper.

### Interactive Setup Inputs

During `install --hub` installer asks for:

- panel domain (`panel.example.com`)
- subscription domain (`sub.example.com`, in hub mode)
- admin path (`/admin` by default)
- install directory (`/opt/caramba` by default)
- PostgreSQL password for `caramba` DB user

Installer then configures Caddy routes (panel -> `127.0.0.1:3000`, sub -> `127.0.0.1:8080`), writes `.env`, sets up DB, downloads release binaries, and installs systemd services.

Admin login/password are finalized in panel setup flow after deployment.

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

## Installer Commands

```bash
caramba install --hub
caramba install --panel
caramba install --node
caramba install --sub
caramba install --bot
caramba upgrade
caramba diagnose
caramba restore --file /path/to/backup.tar.gz
caramba uninstall
```

Role script examples:

```bash
# Node (token can be join token or enrollment key EXA-ENROLL-*)
curl -fsSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh \
  | sudo bash -s -- --role node --panel "https://panel.example.com" --token "EXA-ENROLL-XXXX"

# Frontend/sub edge
curl -fsSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh \
  | sudo bash -s -- --role frontend --panel "https://panel.example.com" --domain "sub.example.com" --token "frontend_token"
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

Source-available development repository.

Important:
- Until a formal `LICENSE` file is added, code is effectively "all rights reserved".
- If you plan to keep repo public, define an explicit license policy (open-source or restricted source-available).
