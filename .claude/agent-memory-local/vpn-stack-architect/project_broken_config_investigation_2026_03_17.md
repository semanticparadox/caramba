---
name: Broken singbox config investigation — 2026-03-17
description: Full trace of "01 - Reality (Direct)" broken config report — root cause found, tests written
type: project
---

The broken singbox config with "01 - Reality (Direct)" numbered tags is NOT produced by any current code path.

**Why:** Searched all Rust source files — zero matches for "01 - ", "Reality (Direct)", or numbered tag format.

**generate_singbox_config() is correct:** Produces proper dns/inbounds/route/outbounds with `slug·tag·d` format.

**The full call chain (verified):**
1. `subscription.rs:subscription_handler()` → checks Redis cache key `sub_config_v3:{uuid}:{client}:{node_id}:{variant}`
2. Cache miss → fetches nodes via `store_service.get_user_nodes()` → `get_node_infos_with_relays()`
3. `fetch_inbounds_for_nodes()` fetches from DB with `WHERE enable = TRUE AND node_id = ANY($1)`
4. `subscription_service.generate_singbox()` → `generate_singbox_config()` in subscription_generator.rs
5. Caches result for 60 seconds, serves response

**Most likely cause of broken config shown by user:** Stale Redis cache from an older server version. The `sub_config_v3` key was introduced to namespace away from older broken keys, but if the same UUID was cached with the v3 prefix during a deployment where the generator was buggy/different, that 60s cache could have been served.

**Secondary issue found:** `RealitySettings.dest: String` is not Optional and has no `#[serde(default)]`. If an inbound's `stream_settings` JSON has a `realitySettings` block without `dest` (client-facing inbounds typically don't have dest), serde deserialization fails silently and `parse_stream_settings()` falls back to node-level `reality_public_key`/`short_id`. This is the correct fallback behavior — node-level keys are authoritative — but it means inbound-level publicKey in stream_settings is ignored unless `dest` is also present.

**Rotation types — security_service.rs:** Only 2 rotation functions exist:
- `rotate_node_sni()` — rotates only the SNI (updates `nodes.reality_sni`, logs to sni_rotation_log)
- `get_best_sni_for_node()` / `get_next_sni()` — SNI selection helpers

Port rotation is handled by `generator_service.rotate_inbound()` which does both port AND SNI together (via template). There is NO standalone "rotate ports only" function. The RotationService background worker calls `rotate_inbound()` which changes port + regenerates stream_settings (including SNI).

**So rotation types in practice:**
1. SNI-only rotation: `security_service.rotate_node_sni()` — only changes node.reality_sni
2. Port+SNI+template rotation: `generator_service.rotate_inbound()` — full inbound rotation
3. Both-together: no separate function, rotate_inbound() does port+SNI simultaneously

**detect_client_type() UA matching:**
- hiddify / sing-box → "singbox"
- clash / stash → "clash"
- v2ray / xray / fair / shadowrocket → "v2ray"
- mozilla / chrome / safari → "html"
- anything else → "singbox" (default)
- Hiddify is correctly detected.

**Tests written:** `/apps/caramba-panel/src/singbox/repro_bug.rs` — 13 tests covering all failure modes described in the bug report. All pass. Added to `singbox/mod.rs` as `#[cfg(test)] mod repro_bug;`

**How to apply:** When investigating "wrong config served" complaints, first flush Redis key `sub_config_v3:{uuid}:singbox:0:default` then re-fetch. If still broken, check inbounds table for the user's node.
