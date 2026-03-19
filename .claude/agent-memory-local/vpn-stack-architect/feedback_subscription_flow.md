---
name: feedback_subscription_flow
description: Bugs diagnosed and fixed in relay/subscription flow (2026-03-17)
type: feedback
---

# Subscription Flow Bug Patterns

## Bug 1: Non-Russian users losing all nodes when exit nodes have relay_id

**Rule:** Never filter out exit nodes (is_relay=false) just because they have relay_id set.

**Why:** In `subscription.rs` geo-aware sorting logic, non-Russian branch was doing
`filter(|n| n.relay_id.is_none())` which removes all exit nodes that have relay configured.
If the plan only assigns exit nodes with relay_id (common setup), result is empty → 404.

**How to apply:** When modifying subscription.rs node filtering: exit nodes with relay_id
are valid destinations. Only nodes where `n.is_relay == true` should be excluded from user configs.
The correct filter for "pure relay infrastructure" is `!n.is_relay`, not `n.relay_id.is_none()`.

---

## Bug 2: parse_stream_settings defaulting security to "reality"

**Rule:** Default security in `parse_stream_settings()` must be auto-detected, NOT hardcoded to "reality".

**Why:** Inbounds for ws/grpc/httpupgrade have security="tls" in stream_settings.
If that field is absent or slightly different, hardcoded "reality" default causes sing-box
to inject Reality TLS config (public_key, short_id) into non-Reality outbounds → VLESS timeout.

**How to apply:** In `src/singbox/subscription_generator.rs::parse_stream_settings()`:
auto-detect: realitySettings present → "reality", tlsSettings present → "tls", else → "none".

---

## Bug 3: generate_singbox_config returning empty JSON {} on no outbounds

**Rule:** Return `Err(...)` not `Ok(json!({}).to_string())` when all_tags is empty.

**Why:** Silent empty config causes clients to silently fail — no error logged, no 500 to diagnose.

**How to apply:** In `src/singbox/subscription_generator.rs::generate_singbox_config()`:
`all_tags.is_empty()` should return `Err(anyhow::anyhow!(...))` with descriptive message.
