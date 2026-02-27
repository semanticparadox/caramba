# Open Workstreams and Known Gaps (Operator-Driven)

This file tracks active engineering gaps observed during iterative deployment tests.

## 1. P0 / Immediate Reliability

### WS-01: Settings save intermittently returns 422

- Symptom:
  - Browser console shows repeated `POST /admin/settings/save 422 (Unprocessable Content)`.
  - User reported inability to persist critical settings (for example bot token) from panel UI.
- Likely areas:
  - `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/handlers/admin/settings.rs`
  - request DTO parsing + validation + default handling.
- Why P0:
  - breaks operational control from UI.
- Done criteria:
  1. Save works for every tab (`General`, `Payments`, `Frontend`, `Trials`, `System`) without 422.
  2. Invalid fields return actionable inline error, not opaque failure loop.
  3. Add regression test(s) for field combinations previously causing 422.

### WS-02: Subscription/client consistency hardening

- Status:
  - major fix delivered in `v0.3.29` for Reality SNI source priority.
- Remaining work:
  - add tests for generated outputs across:
    - direct `vless://` links
    - `/sub/{uuid}?client=singbox` JSON
    - mixed inbound types and node SNI updates.
- Files:
  - `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/services/subscription_service.rs`
  - `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/singbox/subscription_generator.rs`

## 2. P1 / Product-Operation Usability

### WS-03: System settings UX complexity

- Symptom:
  - operator feedback: system page is overloaded and hard to understand for first-time admins.
- Areas:
  - topology section, rollout section, token usage explanation, update controls.
- Files:
  - `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/handlers/admin/settings.rs`
  - relevant Askama templates/components used by settings page.
- Target:
  1. task-oriented grouped sections
  2. progressive disclosure (basic vs advanced)
  3. explicit token lifecycle and copyable commands with context

### WS-04: Token lifecycle and role-install ergonomics

- Symptom:
  - confusion about where tokens come from, rotation policy, and which token is needed for each role.
- Need:
  1. auto-issued tokens where possible
  2. visible inventory of active tokens by role/scope
  3. rotate/revoke UI with audit events
  4. command generator linked to selected token

### WS-05: Update center trust model

- Symptom:
  - operator concern that updates appear "not real" for remote services.
- Need:
  1. clearer state model per worker/node (`queued`, `polled`, `downloaded`, `applied`, `failed`)
  2. per-host evidence for applied version
  3. less manual field editing for rollout metadata

## 3. P1 / Node and Traffic Control

### WS-06: Device limit model still heuristic-heavy

- Existing direction:
  - lease/device accounting was introduced and improved over multiple tags.
- Remaining gap:
  - stronger identity model across protocol/client variations and NAT churn.
- Need:
  1. explicit lease expiration semantics
  2. anti-noise filtering (exclude service-side and non-user artifacts)
  3. deterministic reconciliation job

### WS-07: Traffic quota accounting end-to-end confidence

- Existing direction:
  - quota and accounting hardening is in place but needs full production confidence testing.
- Need:
  1. reproducible quota test matrix
  2. cross-check panel totals vs node-reported usage
  3. clear user-facing quota state transitions

## 4. P1 / Mini App Completion and UX

### WS-08: Mini App parity and resilience

- History:
  - repeated regressions fixed for auth/502/loading loops.
- Remaining priority:
  1. stabilize all main flows under real telegram sessions:
     - buy plan
     - activate/gift subscription
     - retrieve links
     - promo redeem/manage
     - referral stats and bind flow
  2. ensure all screens have reliable back navigation and consistent error surfaces.

### WS-09: Deep-linking into client apps

- Goal:
  - one-tap connect/import from Mini App into common clients (where platform supports URI/deep link).
- Need:
  1. per-platform capability matrix
  2. fallback copy/open UX
  3. secure handling of links in Telegram webview context

## 5. P2 / SNI Discovery Quality

### WS-10: Scanner quality filters and explainability

- Existing direction:
  - filters/blacklisting were strengthened.
- Requested improvements:
  1. drop garbage domains with no valid cert path or obvious deny patterns
  2. robust denylist patterns for hosting panel/service artifacts
  3. show why candidate was filtered or accepted

## 6. Suggested Execution Order

1. WS-01 (`422` settings save) - blocks operator control.
2. WS-02 tests for subscription generation integrity.
3. WS-03 + WS-04 (system UX and token lifecycle).
4. WS-08 (Mini App final parity hardening).
5. WS-06 + WS-07 (device/traffic limits confidence).
6. WS-10 (SNI quality and explainability).
7. WS-05 (update center trust model) in parallel with 3/4 when possible.

## 7. Quick Repro Checklist (for every release candidate)

1. Save settings in all tabs (no 422).
2. Bot token persists and bot starts.
3. Node joins from generated command and becomes active.
4. Inbound/template sync generates valid node config.
5. `/sub/{uuid}` and `?client=singbox` both connect successfully.
6. Mini App auth succeeds and key flows work end-to-end.
7. Device and traffic counters show sane values under controlled traffic.

