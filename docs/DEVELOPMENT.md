# Development Guide

## Prerequisites

*   **Rust**: Latest Stable (`rustup update`)
*   **PostgreSQL**: Local instance for development (`brew install postgresql` or Docker)
*   **Redis**: Local instance (`brew install redis` or Docker)
*   **Node.js**: For mini app frontend (`apps/caramba-app`, v18+)

## Setup

1.  **Clone & Layout**
    ```bash
    git clone https://github.com/semanticparadox/caramba.git
    cd caramba
    ```

2.  **Environment**
    Copy `.env.example` to `.env` and fill in your local credentials.

3.  **Database**
    ```bash
    # Install sqlx-cli
    cargo install sqlx-cli

    # Create DB and run migrations
    sqlx database create
    sqlx migrate run
    ```

## Running the Panel

```bash
cargo run -p caramba-panel -- serve
```

The panel will be available at `http://localhost:3000`.

## Running Other Services

```bash
cargo run -p caramba-node
cargo run -p caramba-sub
cargo run -p caramba-bot
```

## Working on the Mini App

The mini app is located in `apps/caramba-app`.

```bash
cd apps/caramba-app
npm install
npm run dev
```

To embed it into the panel:
```bash
npm run build
# panel/sub expect assets in apps/caramba-app/dist
```

## Testing

```bash
cargo check --workspace
cargo test --workspace
```

Targeted fast checks:

```bash
cargo test -p caramba-panel singbox::tests::
cargo check -p caramba-panel
```

## Repository layout

| Directory | Purpose |
| --- | --- |
| `apps/caramba-client` | Caramba Connect native Flutter client |
| `apps/caramba-app` | React + TypeScript Telegram Mini App |
| `apps/caramba-panel` | Admin UI, APIs and orchestration |
| `apps/caramba-node` | sing-box node agent |
| `apps/caramba-sub` | Subscription service |
| `apps/caramba-bot` | Telegram bot |
| `apps/caramba-installer` | Installation and upgrade CLI |
| `libs` | Shared crates and native VPN core |
| `scripts` | Build and deployment tools |

See [Modules](MODULES.md) for the server architecture and each application's documentation for its build instructions.

## Build profiles and CI

Use `cargo check` during development. The Rust release profile uses LTO, size optimization, stripping and one codegen unit, so release builds are substantially slower.

The Mini App is React/TypeScript (`npm run build`), while Caramba Connect is Flutter. Consult the [workflow definitions](../.github/workflows) for current checks and release jobs. Server and client artifacts have separate release workflows.
