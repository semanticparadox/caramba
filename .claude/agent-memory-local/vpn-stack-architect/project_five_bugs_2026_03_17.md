---
name: Five-bug fix session 2026-03-17
description: Fixes for: hiddify URL scheme, client=hiddify cache key normalization, SNI rotate_sni deduplication, version comparison display, Subscription.tsx QR URL
type: project
---

## Bugs fixed 2026-03-17

### 1. Problem 4 (fixed first): `?client=hiddify` breaks Hiddify import
**Files:** `apps/caramba-app/src/pages/Home.tsx`, `apps/caramba-app/src/pages/Subscription.tsx`

- `openHiddify()` changed from `withClient(url, 'hiddify')` deep-link to `hiddify://import/{encodeURIComponent(cleanUrl)}` with CLEAN URL (no ?client=).
- `copyImportLink()` now copies the clean subscription URL, not `?client=hiddify`.
- `Subscription.tsx` QR code uses `sub.subscription_url` directly (no `withClient`).
- `Subscription.tsx` "Открыть в Hiddify" button uses `hiddify://import/...` with clean URL.
- Removed unused `withClient` function from `Home.tsx` (caused TS6133).

**Why:** Hiddify detects config type via User-Agent automatically. `?client=hiddify` is rejected by Hiddify's URL parser. Server-side `detect_client_type()` already handles Hiddify via UA sniffing.

### 2. Problem 1: Singbox client config incorrect when `?client=hiddify`
**File:** `apps/caramba-panel/src/subscription.rs`

- Added normalization: `"hiddify" => "singbox"` before building the cache key.
- Without this, the cache key was `sub_config_v3:{uuid}:hiddify:0:default` instead of `...:singbox:...`.
- Old stale entries under `hiddify` key could persist in Redis with broken config from previous generator.

**Why:** Even though `hiddify` falls to the `_` arm (singbox generator), the cache key used the raw `client_type = "hiddify"`, creating a separate cache namespace that could contain stale/malformed entries from an old generator path.

### 3. Problem 2: SNI rotation returns 500 from node endpoint
**File:** `apps/caramba-panel/src/api/v2/node.rs`

- `rotate_sni` (POST `/api/v2/node/rotate-sni`) was duplicating all logic already in `security_service::rotate_node_sni`.
- Refactored to delegate completely to `state.security_service.rotate_node_sni(node_id, reason)`.
- Auth query changed from `SELECT id, reality_sni` to `SELECT id` only (only node_id needed for delegation).
- `"No other SNI"` error now returns 409 CONFLICT (not 500).
- Eliminates duplicate UPDATE, log, blocklist logic.

**Why:** The old endpoint manually called `get_next_sni` → UPDATE → log. Any DB error in this chain yielded 500. The canonical `rotate_node_sni` method has better error handling + pinned SNI priority + blocklist integration.

### 4. Problem 3: Version comparison display shows "update needed" when already on newer version
**Files:** `apps/caramba-panel/src/handlers/api/internal.rs`, `apps/caramba-panel/src/handlers/admin/settings.rs`, `apps/caramba-panel/src/handlers/admin/updates.rs`, `apps/caramba-panel/templates/settings.html`

- `should_offer_worker_update` made `pub` (was private `fn`).
- Added `update_available: bool` field to `WorkerInventoryView`.
- Both `fetch_worker_inventory` (settings.rs) and the updates.rs parallel implementation now compute `update_available` using `should_offer_worker_update`.
- Template shows `→ target` in yellow when update needed, `/ target` in gray when current >= target.

**Why:** `worker_runtime_status.target_version` retains the old value (e.g., "0.9.1") even after the worker reaches "0.9.7". Display showed "0.9.7 → 0.9.1" as if a downgrade was needed. The fix separates display from raw DB value.

### 5. Problem 5 (documented): Duplicate rotation mechanisms
Confirmed paths (no code removed, just noted):
- `api/v2/node.rs::rotate_sni` — now delegates to `security_service::rotate_node_sni` ✓
- `handlers/admin/nodes.rs::admin_rotate_node_sni` — already delegated to same ✓
- `services/monitoring.rs::check_and_rotate_snis` — uses `rotate_node_sni` ✓
- `services/rotation_service.rs` — inbound port rotation (different from SNI rotation)

The canonical path is `security_service::rotate_node_sni` — all paths now use it.

**How to apply:** When touching SNI rotation code, always go through `security_service.rotate_node_sni()` — never inline the logic again.
