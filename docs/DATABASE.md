# Caramba Database Schema

The application uses PostgreSQL with `sqlx` migrations.

## Core Tables

### `users`
Stores end-user accounts (Telegram bound).
*   `id`: Primary Key (BigInt)
*   `tg_id`: Unique Telegram ID (BigInt)
*   `username`: (Text) Telegram username
*   `full_name`: (Text) Telegram full name
*   `balance`: (BigInt) Current credit balance in smallest currency unit (e.g. cents).
*   `referral_code`: (Text) Unique referral code for the user.
*   `referrer_id`: (BigInt) ID of the user who referred this user.
*   `is_banned`: (Boolean)
*   `language_code`: (Text)
*   `warning_count`: (Integer)
*   `trial_used`: (Boolean)
*   `created_at`: (Timestamp)

### `nodes`
Represents VPN server instances.
*   `id`: Primary Key (BigInt)
*   `name`: (Text) Friendly name.
*   `ip`: (Text) Public IP address.
*   `status`: (Text) 'active', 'disabled', 'new', etc.
*   `vpn_port`: (BigInt) Port used for VPN services.
*   `join_token`: (Text) Token for node agent enrollment.
*   `is_enabled`: (Boolean) Administrative enable/disable toggle.
*   `country_code`: (Text) ISO country code.
*   `country`: (Text) Country name.
*   `city`: (Text) City name.
*   `flag`: (Text) Emoji flag.
*   `reality_sni`: (Text) SNI domain used for Reality.
*   `reality_pub`, `reality_priv`, `short_id`: (Text) Reality security keys.
*   `max_ram`, `cpu_cores`, `cpu_model`: (BigInt/Int/Text) Hardware specs.
*   `speed_limit_mbps`, `max_users`: (Int) Limits.
*   `total_ingress`, `total_egress`: (BigInt) Traffic stats.
*   `version`, `target_version`: (Text) Agent version tracking.
*   `last_seen`: (Timestamp) Last heartbeat.
*   `is_relay`, `relay_id`: (Boolean/BigInt) Relay configuration.

### `subscriptions`
Links users to plans and nodes.
*   `id`: Primary Key (BigInt)
*   `user_id`: Foreign Key (`users.id`)
*   `plan_id`: Foreign Key (`plans.id`)
*   `node_id`: Foreign Key (`nodes.id`)
*   `status`: (Text) 'active', 'expired', 'pending', 'banned'
*   `expires_at`: (Timestamp)
*   `vless_uuid`: (Text) User's UUID for VLESS/Trojan.
*   `subscription_uuid`: (Text) UUID for subscription link access.
*   `used_traffic`: (BigInt) Bytes consumed.
*   `auto_renew`: (Boolean)
*   `is_trial`: (Boolean)

### `plans`
Subscription tiers.
*   `id`: Primary Key (BigInt)
*   `name`: (Text) Plan title.
*   `description`: (Text)
*   `traffic_limit_gb`: (Integer)
*   `device_limit`: (Integer)
*   `is_active`: (Boolean)
*   `is_trial`: (Boolean)

### `plan_durations`
Pricing options for plans.
*   `id`: Primary Key (BigInt)
*   `plan_id`: Foreign Key (`plans.id`)
*   `duration_days`: (Integer)
*   `price`: (BigInt) Price in smallest currency unit.

### `inbounds`
Network inbounds (ports/protocols) configured on nodes.
*   `id`: Primary Key (BigInt)
*   `node_id`: Foreign Key (`nodes.id`)
*   `tag`: (Text) Unique tag for the inbound.
*   `protocol`: (Text) 'vless', 'hysteria2', 'trojan', 'shadowsocks', 'vmess', 'naive', 'tuic'.
*   `listen_port`: (BigInt)
*   `settings`: (Text) JSON configuration for the protocol.
*   `stream_settings`: (Text) JSON configuration for transport/stream.
*   `enable`: (Boolean)

### `frontend_servers`
Manages edge nodes for client load balancing (Legacy/Optional).
*   `id`: Primary Key (BigInt)
*   `domain`: (Text) Frontend domain.
*   `auth_token_hash`: (Text)
*   `is_active`: (Boolean)

### `orders`
*   `id`: Primary Key (BigInt)
*   `user_id`: Foreign Key (`users.id`)
*   `total_amount`: (BigInt)
*   `status`: (Text) 'pending', 'paid', 'cancelled'

### `payments`
*   `id`: Primary Key (BigInt)
*   `user_id`: Foreign Key (`users.id`)
*   `amount`: (BigInt)
*   `method`: (Text) 'cryptomus', 'nowpayments', 'telegram_stars', etc.
*   `status`: (Text)

### `promo_codes`
*   `id`: Primary Key (BigInt)
*   `code`: (Text)
*   `type`: (Text) 'balance', 'discount', 'traffic'
*   `balance_amount`, `discount_percent`, `traffic_gb`: (BigInt/Int)
*   `max_uses`: (Integer)

### `sni_pool`
Discovered SNI domains for Reality.
*   `id`: Primary Key (BigInt)
*   `domain`: (Text)
*   `health_score`: (Integer)
*   `is_premium`: (Boolean)

### `worker_update_reports` & `worker_runtime_status`
Tracks self-update status of distributed workers (bot, sub, node).

## Migrations

Database structure is managed via `migrations/`. To apply changes:

```bash
sqlx migrate run
```

To revert the last migration:

```bash
sqlx migrate revert
```
