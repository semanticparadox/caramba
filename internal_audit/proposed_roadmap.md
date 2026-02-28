# Proposed Roadmap: Phase 2 (Refactoring & Stabilization)

## Objective
Address the critical "Timeout" errors in node synchronization, refactor the Admin UI for clarity, and stabilize the distributed architecture.

---

## 1. Fix Synchronization "Timeout" (High Priority)

**Goal:** Eliminate race conditions and DB contention causing sync failures.

*   **Step 1.1: Optimize Heartbeat (`api/v2/node.rs` & `telemetry_service.rs`)**
    *   **Action:** Decouple "Online Status" updates from "Heavy Telemetry" processing.
    *   **Detail:** `heartbeat` handler should do a lightweight `UPDATE nodes SET last_seen=NOW()` and respond 200 OK immediately. Spawn a background task for traffic usage, CPU/RAM stats, and SNI discovery.
    *   **Benefit:** Agent gets a fast response, preventing client-side timeouts.

*   **Step 1.2: Refactor Polling Endpoint (`poll_updates`)**
    *   **Action:** Implement "State Check before Wait".
    *   **Detail:** The Node Agent sends `config_hash`. The server should compare this against the latest config hash for that node *before* entering the PubSub wait loop. If different -> return `update: true` immediately.
    *   **Benefit:** Fixes the race condition where a node restarts polling *after* the "Update" event was published but *before* it processed the new config.

*   **Step 1.3: Stabilize PubSub**
    *   **Action:** Improve `PubSubService` to handle Redis reconnections more gracefully (remove 5s sleep or buffer events).

---

## 2. Refactor Admin UI/UX (Medium Priority)

**Goal:** Organize settings into logical domains and reduce cognitive load.

*   **Step 2.1: Restructure Settings Page**
    *   **Action:** Split `settings.html` into distinct forms or a unified form with clear sections:
        *   `General`: Branding, Terms.
        *   `Bot`: Token, Username, Interface Mode (consolidated).
        *   `Security`: Decoy, Kill Switch (moved from System).
        *   `System`: Topology, Internal Token.
    *   **Action:** Remove the confusing "Trials" separate form; integrate it or move to a "Plans & Billing" area.

*   **Step 2.2: Create "Update Center"**
    *   **Action:** Move "Agent Rollout" and "Worker Rollout" out of the Settings > System tab.
    *   **Detail:** Create a new page/modal specifically for Fleet Management. Show a table of all nodes/workers and their version status.

*   **Step 2.3: Simplify Topology Config**
    *   **Action:** Replace manual "Local Sub/Bot" toggles with a high-level "Mode" selector (Hub vs Distributed) that applies the correct presets automatically.

---

## 3. Bot Worker Separation (Low Priority / Cleanup)

**Goal:** Clarify the boundary between the Panel's internal bot logic and the external Bot Worker.

*   **Step 3.1: Code Cleanup**
    *   **Action:** Clearly mark legacy embedded bot code in `caramba-panel` vs new worker code in `apps/caramba-bot`.
    *   **Detail:** Ensure `apps/caramba-bot` is the single source of truth for the Telegram bot logic going forward.

---

## Execution Plan

1.  **Week 1:** Fix Timeouts (Step 1.1, 1.2). Validate with load test.
2.  **Week 2:** UI Refactoring (Step 2.1, 2.2).
3.  **Week 3:** Documentation Finalization & Bot Cleanup.
