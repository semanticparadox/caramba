---
name: sni_rotation_fix
description: SNI rotation infinite loop fix and admin UI rotation controls — what was changed and why
type: project
---

## SNI Rotation Infinite Loop Fix (2026-03-17)

**Root cause:** caramba-node agent validates SNI every ~100s. On TLS validation failure it immediately calls `/api/v2/node/rotate-sni`. The panel's `get_next_sni()` picks from `sni_pool` but does NOT verify that the new SNI's certificate matches the server's actual cert. If the pool is exhausted or all SNIs share the same broken certificate, every rotation fails → triggers another rotation → infinite loop (100+ rotations/day).

**Why:** The `sni_pool` stores SNIs like `timecard365.de` whose TLS cert is only valid for `*.your-server.de`, not the domain itself.

**How to apply:** When debugging future SNI issues, check both the pool health_score AND whether the cert covers the exact domain name.

---

## Changes Made

### 1. caramba-node circuit-breaker (`apps/caramba-node/src/main.rs`)

Added to `AgentState`:
- `last_sni_rotation: Option<std::time::Instant>`
- `sni_rotation_count_this_hour: u32`
- `sni_rotation_hour: u64`

Constants:
- `SNI_ROTATION_COOLDOWN_SECS = 1800` (30 min between rotations)
- `SNI_MAX_ROTATIONS_PER_HOUR = 3`

Logic: before calling `rotate_sni`, checks cooldown + rate limit. On 409 (pool exhausted) also sets the cooldown timestamp to prevent hammering.

### 2. DB migration (`libs/caramba-db/migrations/20260317000000_nodes_sni_rotation_controls.sql`)

Adds to `nodes` table:
- `last_sni_rotation TIMESTAMPTZ DEFAULT NULL`
- `sni_renew_interval_hours INTEGER DEFAULT NULL`

Meaning of `sni_renew_interval_hours`:
- NULL = use global `auto_sni_rotation_interval_hours` setting
- 0 = never rotate automatically
- 24/168/720 = daily/weekly/monthly

### 3. Node model (`libs/caramba-db/src/models/node.rs`)

Added fields + helper methods: `sni_interval_is_global()`, `sni_interval_is_never()`, `sni_interval_is_daily()`, `sni_interval_is_weekly()`, `sni_interval_is_monthly()` — used in Askama templates since Option comparison doesn't work directly in Jinja2 syntax.

### 4. `infrastructure_service.rs` — Node constructor updated with new fields

### 5. `node_repo.rs` — `row_to_node()` updated to read new fields

### 6. security_service.rs `rotate_node_sni()` — now also sets `last_sni_rotation = NOW()`

### 7. API handler `api/v2/node.rs` `rotate_sni()` — also sets `last_sni_rotation = NOW()`

### 8. monitoring.rs `check_and_rotate_snis()` — updated SQL query to respect per-node intervals; now also updates `last_sni_rotation` after rotation

### 9. Admin handlers (`handlers/admin/nodes.rs`)

Two new handlers:
- `admin_rotate_node_sni` → `POST /nodes/{id}/rotate-sni` — manual admin trigger, bypasses agent cooldown
- `update_sni_interval` → `POST /nodes/{id}/sni-interval` — updates per-node rotation interval

### 10. Routes (`main.rs`) — two new routes registered

### 11. Template (`templates/partials/nodes_rows.html`)

Per node row, added:
- Violet shield button → hx-post rotate-sni with confirm dialog
- Select dropdown → hx-trigger="change" posts to sni-interval; options: Глобально / Никогда / Каждый день / Каждую неделю / Каждый месяц
