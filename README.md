# Caramba

> **DPI-resistant VPN panel with a Telegram Mini App.**
> Built on [sing-box](https://sing-box.sagernet.org/). Installer-first.
> Self-hostable. One binary, one command, one systemd unit per role.

[![CI](https://github.com/semanticparadox/caramba/actions/workflows/ci.yml/badge.svg)](https://github.com/semanticparadox/caramba/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/semanticparadox/caramba)](https://github.com/semanticparadox/caramba/releases)

---

## What you get

A complete VPN service control plane in a single Rust workspace:

- **Admin panel** — Axum, PostgreSQL, Redis. Nodes, plans, promos, payments,
  analytics, branding. Cookie + CSRF auth, role-based admin groups.
- **Telegram bot** — Teloxide. User flows, admin commands, payment notifications.
- **Telegram Mini App** — React + TypeScript. Subscription, plans, devices,
  servers, store, tickets, referrals.
- **Node agent** — runs on every VPN node. Sings-box config, SNI rotation,
  health heartbeats, self-update.
- **Subscription service** — sing-box / V2Ray / Clash config generation by
  user, protocol, and country.
- **Installer** — `caramba` CLI. `install`, `upgrade`, `doctor`, `backup`,
  `uninstall`. Single entry point for every server.

**sing-box** is the engine on the wire. The panel generates its config and
ships sing-box-shaped subscription URLs.

### Supported protocols

| Family      | Variants                                                      |
| ----------- | ------------------------------------------------------------- |
| VLESS       | Reality, WebSocket, HTTPUpgrade, gRPC                         |
| Hysteria2   | UDP, congestion control                                        |
| TUIC        | v5                                                             |
| Shadowsocks | 2022                                                          |
| NaiveProxy  | HTTP/3                                                         |
| VMess       | TCP / WebSocket                                                |
| Trojan      | TLS / WebSocket                                                |
| AmneziaWG   | WireGuard with obfuscation (gated, see `docs/`)                |

---

## Quick start

The installer is the only thing an operator needs:

```bash
curl -fsSL https://raw.githubusercontent.com/semanticparadox/caramba/main/scripts/install.sh | sudo bash
```

It detects the role from flags, downloads the right binary, writes the systemd
unit, generates `.env`, runs migrations, and starts the service. To upgrade
later: `sudo caramba upgrade`. To roll back: `sudo caramba upgrade --to v0.9.49`.

### Deployment modes

| Mode        | Topology                                              | Use case                              |
| ----------- | ----------------------------------------------------- | ------------------------------------- |
| **Hub**     | Panel + Sub (+ Bot) on one host                       | Quick start, tests, small installs    |
| **Distributed** | Panel on a controller, Sub / Bot / Node on separate hosts | Production, isolation, scaling    |

See [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md) for the full topology and the
[release workflow](.github/workflows/release.yml) for the supply-chain
hardening (SHA-256 manifest verified before the installer is executed as root).

---

## Architecture

```
┌──────────────────┐    ┌──────────────────┐    ┌──────────────────┐
│  caramba-panel   │◄──►│  caramba-sub     │◄──►│  caramba-node    │
│  (Axum + pg/redis)│   │  (subscription)  │    │  (sing-box)      │
└────────┬─────────┘    └──────────────────┘    └──────────────────┘
         │                                              ▲
         ▼                                              │
┌──────────────────┐                          ┌──────────────────┐
│  caramba-bot     │                          │  caramba-node    │
│  (Teloxide)      │                          │  (on each node)  │
└────────┬─────────┘                          └──────────────────┘
         │
         ▼
┌──────────────────┐
│  caramba-app     │   Telegram Mini App (React + TS)
└──────────────────┘
```

Layout:

```
apps/
  caramba-panel/         admin UI, APIs, orchestration
  caramba-node/          node agent (sing-box + heartbeats + self-update)
  caramba-sub/           subscription edge
  caramba-bot/           Telegram bot
  caramba-installer/     `caramba` CLI (install / upgrade / doctor / …)
  caramba-app/           Telegram Mini App (React + TS)
libs/
  caramba-db/            sqlx models, repositories, migrations
  caramba-shared/        shared types, license verification
docs/                    user-facing docs + vendored sing-box 1.13 reference
scripts/                 install.sh
```

---

## Development

```bash
# Rust check + clippy + test (fast — no live DB required)
cargo check --workspace
cargo test --workspace
cd apps/caramba-app && npm run build
```

Local run:

```bash
cargo run -p caramba-panel
cargo run -p caramba-sub
cargo run -p caramba-bot
cargo run -p caramba-node
```

### Build profile

The release profile is heavy by design — LTO, `opt-level = "z"`, `strip`,
single codegen unit. **Release builds are slow.** Use `cargo check` for
dev loops and let CI build the artifacts. `cargo build --release` locally
is wasteful.

### CI

`.github/workflows/ci.yml` runs on every push and PR:

- `cargo fmt --all --check`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `flutter analyze` + `flutter test` for the Mini App (`allow-failure` until
  the WIP client lands its first end-to-end tunnel)

Release artifacts (`x86_64-unknown-linux-musl`, SHA-256 manifest) are built
and published by `.github/workflows/release.yml` on `v*` tags.

---

## Documentation

| File                                    | What's in it                                      |
| --------------------------------------- | ------------------------------------------------- |
| [`docs/API.md`](docs/API.md)            | HTTP API surface (v1 + v2)                        |
| [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md) | Environment variables and runtime config |
| [`docs/DATABASE.md`](docs/DATABASE.md)  | Schema, migrations, key tables                    |
| [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md) | Install, upgrade, backup, multi-host layout   |
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | Build, test, contribution workflow         |
| [`docs/MODULES.md`](docs/MODULES.md)    | Per-crate module tour                             |
| [`docs/protocols.md`](docs/protocols.md) | sing-box protocol mappings                       |
| [`docs/PAYPALYCH-API-SPEC.md`](docs/PAYPALYCH-API-SPEC.md) | Paypalych (pal24.pro) API reference |
| [`docs/POST-INCIDENT-ROADMAP.md`](docs/POST-INCIDENT-ROADMAP.md) | Open work + lessons learned       |
| [`docs/sing-box/`](docs/sing-box)       | Vendored sing-box 1.13 reference (en + zh)        |

`AGENTS.md` (in this repo root) is the operator runbook for AI agents: ship
flow, payment-provider checklist, sing-box safety, etc.

---

## Status

Beta. ~20 real users in production. The active maintainer ships from
`main` via `v*` tags; `sudo caramba upgrade` is the deploy on every host.

## License

No `LICENSE` file yet — repository content is source-available, all rights
reserved by default, until a license is added.
