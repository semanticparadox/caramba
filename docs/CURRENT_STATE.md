# Caramba Current State

This document reflects the current state of the codebase as of **February 2026**.

## Workspace Modules

`apps/caramba-panel`
- **Role:** Control Plane & Orchestrator.
- **Tech:** Rust, Axum, Askama (HTMX), SQLx (PostgreSQL), Redis.
- **Responsibilities:**
    - Admin UI (`/admin`) for managing nodes, users, plans, and settings.
    - Node API (`/api/v2/node`) for agent communication.
    - Bot API (`/api/v2/bot`) for external bot worker.
    - Internal API (`/api/internal`) for trusted workers.
    - Config generation (Sing-box) and distribution.
    - User/Subscription management and billing.

`apps/caramba-node`
- **Role:** Node Agent.
- **Tech:** Rust.
- **Responsibilities:**
    - Runs on VPN servers alongside Sing-box.
    - Pulls config from Panel.
    - Reports heartbeat, traffic stats, and system metrics.
    - Performs neighborhood SNI scanning.
    - Handles self-updates.
    - Implements Kill Switch and Decoy traffic logic.

`apps/caramba-bot`
- **Role:** Telegram Bot Worker.
- **Tech:** Rust, Teloxide.
- **Responsibilities:**
    - Handles Telegram user interactions (start, plans, profile, support).
    - Communicates with Panel via `/api/v2/bot`.
    - Can run locally (embedded) or as a standalone worker.
    - Supports self-updates.

`apps/caramba-sub`
- **Role:** Subscription/Frontend Worker.
- **Tech:** Rust, Axum.
- **Responsibilities:**
    - Serves subscription links (SIP002, Clash, Sing-box).
    - Proxies API requests if needed.
    - Acts as an edge node for the panel.
    - Supports self-updates.

`apps/caramba-installer`
- **Role:** CLI Tool.
- **Tech:** Rust.
- **Responsibilities:**
    - Installation, upgrade, backup, restore, and diagnostics.

`libs/caramba-db`
- **Role:** Shared Data Layer.
- **Tech:** SQLx.
- **Responsibilities:**
    - Database models and repositories.
    - Migrations.

`libs/caramba-shared`
- **Role:** Shared Types.
- **Responsibilities:**
    - API request/response structs.
    - Common configuration types.

## Key Features

### Networking & Censorship Resistance
- **Protocols:** VLESS (Reality, TCP/GRPC), Hysteria2, Trojan, TUIC, NaiveProxy, ShadowSocks, AmneziaWG.
- **SNI Management:** Automated Reality SNI rotation, neighborhood scanning, manual pinning/blocking.
- **Geo-Routing:** Intelligent node selection based on client location.
- **Relay Support:** Node relaying for high-restriction environments.

### Operations
- **One-Command Install:** `curl | bash` installer for all roles.
- **Distributed Topology:** Supports separate hosts for Panel, Nodes, Bot, and Subscription workers.
- **Self-Updating:** Agents and workers can update themselves from GitHub releases via Panel orchestration.
- **Observability:** Real-time node status, traffic monitoring, and logs collection.

### User Management & Billing
- **Telegram Integration:** Bot-driven user flow.
- **Plans:** Traffic-limited, time-limited, and trial plans.
- **Payments:** Crypto (Cryptomus, NowPayments), Telegram Stars, Lava, AAIO.
- **Referrals:** Multi-level referral system.
- **Gift Codes:** Pre-paid subscription codes.

## Known Issues (Audit Findings)

1.  **Sync Timeout:** Node synchronization can time out due to inefficient pub/sub or database locking.
2.  **UI Redundancies:** Some settings in the admin panel are duplicated or misplaced.
3.  **Documentation Drift:** Legacy docs referenced files or features that have changed (now corrected).
