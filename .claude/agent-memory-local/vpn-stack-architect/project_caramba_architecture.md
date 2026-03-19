---
name: caramba_architecture
description: Key architecture decisions, file paths, relay/subscription flow for Caramba VPN panel
type: project
---

# Caramba VPN Panel Architecture

**Why:** Reference for future conversations so we don't re-explore the same files.
**How to apply:** Use when working on subscription flow, relay configuration, or singbox config generation.

## Stack
- Rust/Axum backend at `apps/caramba-panel/src/`
- React/TypeScript miniapp at `apps/caramba-app/src/`
- PostgreSQL via sqlx + caramba-db library at `libs/caramba-db/`

## Key Subscription Flow Files
- `src/subscription.rs` — HTTP handler for `/sub/{uuid}`, node filtering by geo (Russian IP detection), calls `get_node_infos_with_relays` then `generate_singbox_config`
- `src/services/subscription_service.rs` — `get_node_infos_with_relays()`, `generate_singbox()`, `get_subscription_links()`, `get_user_keys()`
- `src/singbox/subscription_generator.rs` — `generate_singbox_config()`, `build_singbox_outbound()`, `ensure_relay_outbound()`, `parse_stream_settings()`
- `src/singbox/connection_variants.rs` — 9 fixed variants (3 direct + 6 relay), `apply_connection_variant()`
- `src/services/store_service.rs` — `get_user_nodes()` (plan-based), `get_active_nodes()` (fallback)
- `libs/caramba-db/src/repositories/node_repo.rs` — `get_nodes_for_plan()`, `get_active_nodes()`

## Node Architecture
- `Node.is_relay = true` → pure infrastructure relay, NOT a user destination (transit-only)
- `Node.relay_id = Some(id)` → exit node that routes through relay node as entry point
- Exit nodes with relay_id are VALID user destinations — they support both direct and relay paths

## Relay Outbound Chain in sing-box
- Each exit node generates: direct outbounds (tag: `{slug}·{inbound_tag}·d`) + relay-chained outbounds (tag: `{slug}·{inbound_tag}·r`)
- Relay outbound tag: `relay·{relay_node_slug}` — shared across all exits using same relay
- `detour` field on relayed outbound points to relay outbound tag
- Config has: `auto-all` (urltest all), `auto-relay` (urltest relay-chained only), `auto-direct` (urltest direct only)

## Security Default Bug (Fixed 2026-03-17)
- `parse_stream_settings()` was defaulting security to `"reality"` when not specified
- This broke ws/grpc/httpupgrade inbounds by injecting reality TLS config incorrectly
- Fixed: auto-detect from presence of realitySettings > tlsSettings > "none"

## API Client Endpoints (miniapp)
- `GET /api/client/user/subscriptions` — full subscription list with singbox_variants, subscription_url, vless_links
- `GET /api/client/plans` — available plans
- `POST /api/client/payment/invoice` — create payment
- `POST /api/client/auth/telegram` — initData auth, returns JWT
