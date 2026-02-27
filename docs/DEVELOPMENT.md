# Development Guide

## Prerequisites

*   **Rust**: Latest Stable (`rustup update`)
*   **PostgreSQL**: Local instance for development (`brew install postgresql` or Docker)
*   **Redis**: Local instance (`brew install redis` or Docker)
*   **Node.js**: For mini app frontend (`apps/caramba-app`, v18+)

## Setup

1.  **Clone & Layout**
    ```bash
    git clone https://github.com/semanticparadox/CARAMBA.git
    cd CARAMBA
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
