# Caramba Database Schema

The application uses PostgreSQL with `sqlx` migrations.

## Core Tables

### `users`
Stores end-user accounts (Telegram bound).
*   `id`: Primary Key (BigInt)
*   `telegram_id`: Unique constraint (BigInt)
*   `username`: (Text)
*   `balance`: (Integer) Current credit balance.

### `nodes`
Represents VPN server instances.
*   `id`: Primary Key (BigInt)
*   `name`: (Text) Friendly name.
*   `ip`: (Text) Public IP address.
*   `access_token`: (Text) Auth token for Agent communication.
*   `is_active`: (Boolean) Administration status.

### `subscriptions`
Links users to plans/access.
*   `id`: Primary Key (BigInt)
*   `user_id`: Foreign Key (`users.id`)
*   `status`: Enum ('active', 'expired', 'banned')

### `frontend_servers`
Manages edge nodes for client load balancing.
*   `id`: Primary Key (BigInt)
*   `domain`: (Text) Frontend domain.
*   `region`: (Text) Geographic region code.

### `plans`
Subscription tiers.
*   `id`: Primary Key (BigInt)
*   `name`: (Text) Plan title.
*   `price`: (Integer) Cost per period.

## Migrations

Database structure is managed via `migrations/`. To apply changes:

```bash
sqlx migrate run
```

To revert the last migration:

```bash
sqlx migrate revert
```
