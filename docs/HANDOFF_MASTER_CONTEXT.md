# CARAMBA Handoff Master Context

Status: local handoff document for continuing work in other IDE agents (Antigravity, Claude Code, Cursor).

## 1. Snapshot

- Repository: `semanticparadox/caramba`
- Local path: `/Users/smtcprdx/Documents/caramba`
- Branch: `main`
- Current head: `3b5fed0`
- Latest release tag: `v0.3.38`
- Release workflow: `.github/workflows/release.yml` (trigger on `v*` tags)
- Latest release run URL: [Release Build #42](https://github.com/semanticparadox/caramba/actions/runs/22269560036)

Important: this file is intentionally for local context transfer and should not be pushed unless explicitly requested.

## 2. Product Definition

CARAMBA is an installer-first control plane for censorship-resistant VPN operations.

High-level goals:

1. Install and operate panel/node/sub/bot via one installer CLI.
2. Support both hub mode and distributed mode.
3. Manage nodes, inbounds, SNI discovery, subscriptions, users, promo/referral flows.
4. Deliver user-facing flows via Telegram Bot + Mini App + subscription links.

## 3. Workspace Layout

Rust workspace members:

- `/Users/smtcprdx/Documents/caramba/apps/caramba-panel`
- `/Users/smtcprdx/Documents/caramba/apps/caramba-node`
- `/Users/smtcprdx/Documents/caramba/apps/caramba-sub`
- `/Users/smtcprdx/Documents/caramba/apps/caramba-bot`
- `/Users/smtcprdx/Documents/caramba/apps/caramba-installer`
- `/Users/smtcprdx/Documents/caramba/libs/caramba-db`
- `/Users/smtcprdx/Documents/caramba/libs/caramba-shared`

Additional frontend app (not Rust member):

- `/Users/smtcprdx/Documents/caramba/apps/caramba-app` (React + Vite Mini App)

## 4. Runtime Topology

### Hub mode

Single host typically running:

- `caramba-panel.service`
- `caramba-sub.service`
- optional `caramba-bot.service`
- optional local `caramba-node.service`

### Distributed mode

Control plane host:

- Panel + DB + Redis

Remote hosts:

- Node agents
- Sub/frontend workers
- Bot worker

Design intent in current codebase:

- topology controls exist in settings UI and rollout logic;
- installer role model is canonical deployment entrypoint;
- `install.sh` is expected bootstrap entrypoint for all roles.

## 5. Installer Model (Canonical Ops Path)

Primary script:

- `/Users/smtcprdx/Documents/caramba/scripts/install.sh`

Installer binary source:

- `/Users/smtcprdx/Documents/caramba/apps/caramba-installer/src/main.rs`
- `/Users/smtcprdx/Documents/caramba/apps/caramba-installer/src/install.rs`
- `/Users/smtcprdx/Documents/caramba/apps/caramba-installer/src/setup.rs`

Supported installer roles:

- `hub`
- `panel`
- `node` (`agent` alias)
- `sub` (`frontend` alias)
- `bot`

Core installer flows currently present:

1. Fresh install
2. Upgrade preserving existing config defaults
3. TUI-like interactive management for existing installs
4. Diagnostics
5. Uninstall with optional DB purge

Operational expectation carried through recent work:

- upgrades should not re-ask domain/admin/db details if installation already exists.

## 6. Panel Routing and API Surface

Main router entry:

- `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/main.rs`

Important path groups:

1. Admin UI under `ADMIN_PATH` (default `/admin`)
2. Client API:
   - `/api/client/*`
   - `/caramba-api/client/*` (compat path)
3. Node/Bot/Internal APIs under `/caramba-api/*`
4. Subscription endpoint:
   - `/sub/{uuid}`
5. Bootstrap installer endpoint:
   - `/install.sh`

Current strategic direction from recent refactors:

- avoid exposing sensitive control endpoints at overly obvious root patterns;
- keep compatibility aliases during migrations.

## 7. Mini App and Bot State

Mini App source:

- `/Users/smtcprdx/Documents/caramba/apps/caramba-app/src`

Notable implemented areas:

- auth context using Telegram init data
- plans/subscription screens
- promo/referral sections
- PIN lock support (4-digit local app lock)
- support/settings page with PIN lifecycle

Bot and panel bot integration:

- embedded and worker modes have been iterated repeatedly through tags `v0.3.11+`
- notification workflows now include richer payload options (text formatting/media/CTA buttons) in admin UI

## 8. Node and Subscription Pipeline

Node agent:

- `/Users/smtcprdx/Documents/caramba/apps/caramba-node`

Panel orchestration and sing-box generation:

- `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/services/orchestration_service.rs`
- `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/singbox/generator.rs`
- `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/singbox/subscription_generator.rs`
- `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/services/subscription_service.rs`

- Reality SNI mismatch in generated subscription output was fixed.
- Referrer resolution route was added to fix null references in `v0.3.38`.

Why it mattered:

- mismatch between actual node inbound Reality SNI and subscription output caused `REALITY: processed invalid connection` handshake failures.

## 9. What Has Been Delivered Across Recent Iterations

Based on release/commit history (`v0.3.0` -> `v0.3.29`), major delivered tracks include:

1. Distributed installer roles and internal APIs.
2. Installer hardening (`npm` handling, ETXTBSY atomic binary replacement, uninstall/upgrade fixes).
3. Node creation and pending->active lifecycle restorations.
4. DB compatibility and migration unification work.
5. Node telemetry visibility, schema drift handling, active metrics and UI fixes.
6. Mini App auth stabilization and 502-path fixes.
7. Promo/gift/referral flows integration and reliability improvements.
8. Device and traffic accounting hardening.
9. `/caramba-api` compatibility routing.
10. Notification system expansion (formatting, media, CTA).
11. UI/UX revisions in settings/topology/rollout sections.
12. Reality SNI generation fix in subscriptions (`v0.3.29`).
13. Global UI/UX polish (macOS-style menubar, corrected module layouts, purged JS errors) (`v0.3.37`).
14. Complete fix for bot `/start` 500 DB constraints crashes on missing referrer/user APIs (`v0.3.38`).

## 10. Known Active Pain Points (from operator reports)

These items were repeatedly reported during iterative testing and should remain high-priority validation targets:

1. Settings save intermittently returning `422` on `/admin/settings/save`.
2. Configuration UX complexity in system/settings page (too many controls, low clarity).
3. Auto-token ergonomics for role install commands and token lifecycle visibility.
4. Need clearer secure-path strategy (admin path, API pathing, exposure minimization).
5. Update center clarity and real rollout confidence across remote workers.
6. Mini App consistency checks after each release (buy/sub/referral/promo regressions were frequent).

## 11. Recommended Validation Matrix (Before and After Any Change)

### Build and formatting

```bash
cargo fmt --all
cargo check --workspace
```

### Mini App build

```bash
cd /Users/smtcprdx/Documents/caramba/apps/caramba-app
npm ci
npm run build
```

### Installer smoke

- Verify latest release resolution in `scripts/install.sh`.
- Verify role install command generation in panel settings and node pages.

### Runtime smoke (hub)

1. Login panel via custom admin path.
2. Save settings in all tabs (`General/Payments/Frontend/Trials/System`).
3. Create node -> obtain install command with token.
4. Node joins and heartbeat appears.
5. Create inbound/template, sync config.
6. Validate `/sub/{uuid}` and `?client=singbox` output.
7. Validate bot `/start`, Mini App auth, plans, subscriptions, promo/referral.

## 12. High-Value Next Work (Suggested Order)

1. Close `422 /admin/settings/save` root cause with deterministic tests.
2. Redesign settings/system page into task-oriented sections with less operator overload.
3. Token lifecycle UX:
   - automatic issuance where safe,
   - explicit rotate/revoke flows,
   - clear copyable install commands per role.
4. Strengthen SNI scanner quality filters and explainability.
5. Implement stronger device identity and traffic limit accounting model (not only request-seen heuristics).
6. Finish Mini App parity goals (full purchase/activation/referral/promo without fallback gaps).

## 13. Handoff Checklist for New IDE Agent

When starting in another IDE, load these first:

1. `/Users/smtcprdx/Documents/caramba/README.md`
2. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_MASTER_CONTEXT.md`
3. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_RELEASE_TIMELINE.md`
4. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_OPEN_WORKSTREAMS.md`
5. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_SKILLS_AND_AGENT_OPERATIONS.md`

Then immediately run:

```bash
cd /Users/smtcprdx/Documents/caramba
cargo check --workspace
```

If changing frontend flows:

```bash
cd /Users/smtcprdx/Documents/caramba/apps/caramba-app
npm run build
```

