2# Configuration Reference

The application is configured via environment variables, typically loaded from a `.env` file in the root directory.

## Core Settings

| Variable | Description | Default |
| :--- | :--- | :--- |
| `SERVER_DOMAIN` | The public domain for the panel (e.g., `panel.example.com`). | - |
| `API_DOMAIN` | The domain for API access (often same as `SERVER_DOMAIN`). | - |
| `ADMIN_PATH` | The URL path prefix for the admin panel. | `/admin` |
| `DATABASE_URL` | PostgreSQL connection string. | `postgres://...` |
| `REDIS_URL` | Redis connection string for sessions/jobs. | `redis://127.0.0.1:6379` |
| `SESSION_SECRET` | Random string for signing session cookies. | - |
| `PANEL_PORT` | Port to listen on. | `3000` |

## Bot Configuration

| Variable | Description |
| :--- | :--- |
| `BOT_TOKEN` | Telegram Bot API Token from @BotFather. |

## Payment Gateways (Optional)

Configure these if accepting payments.

*   `STRIPE_SECRET_KEY`
*   `STRIPE_WEBHOOK_SECRET`
*   `CRYPTOMUS_MERCHANT_ID`
*   `CRYPTOMUS_PAYMENT_API_KEY`
*   `NOWPAYMENTS_KEY`
*   `CRYSTALPAY_LOGIN`
*   `CRYSTALPAY_SECRET`

## Trial Settings

| Variable | Description | Default |
| :--- | :--- | :--- |
| `FREE_TRIAL_DAYS` | Standard trial duration in days. | `3` |
| `CHANNEL_TRIAL_DAYS` | Extended trial for channel subscribers. | `7` |
| `REQUIRED_CHANNEL_ID` | Telegram Channel ID (e.g., `-100...`) for verification. | - |
