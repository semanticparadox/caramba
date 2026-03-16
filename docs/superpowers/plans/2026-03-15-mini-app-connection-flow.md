# Mini App Connection Flow Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make subscription import load all connections by default, move node and variant selection into an advanced path, and simplify the Mini App screens around one clear connection flow.

**Architecture:** Keep the public subscription URL as the canonical full-config entrypoint and stop treating manual node choice as the default import state. Simplify the React screens so the primary path is `open app -> copy/import subscription -> connect`, while retaining server optimization and variant controls as explicit advanced actions.

**Tech Stack:** Rust (Axum, SQLx), React, TypeScript, CSS

---

## Chunk 1: Restore Full Subscription Import Behavior

### Task 1: Separate full import from pinned node history

**Files:**
- Modify: `apps/caramba-panel/src/subscription.rs`
- Modify: `apps/caramba-panel/src/api/client.rs`
- Modify: `apps/caramba-panel/src/services/subscription_service.rs`

- [ ] Remove the implicit `sub.node_id` filter from the default `/sub/{uuid}` response path so plain `subscription_url` returns the full connection set again.
- [ ] Preserve explicit one-off filtering when `node_id` is passed in the query string.
- [ ] Keep last-used node metadata for UI display, but stop treating it as the default import payload.
- [ ] Update response metadata naming if needed so the UI can show `last selected node` without implying `subscription is pinned`.
- [ ] Bump or invalidate the plain subscription cache path so old single-node payloads are not served after rollout.

### Task 2: Keep advanced optimization as an explicit action

**Files:**
- Modify: `apps/caramba-panel/src/api/client.rs`
- Modify: `apps/caramba-app/src/pages/ServerSelector.tsx`
- Modify: `apps/caramba-app/src/pages/Subscription.tsx`

- [ ] Ensure manual optimization remains a user-triggered server choice, not a hidden default for future imports.
- [ ] Keep success messaging aligned with the new behavior: server choice affects the advanced path, while the base subscription still contains all routes.
- [ ] Update the post-optimize copy on the subscription screen so it no longer tells the user to choose a variant unless they intentionally open advanced controls.

### Task 3: Verify backend behavior

**Files:**
- Test/inspect: `apps/caramba-panel/src/subscription.rs`

- [ ] Add or update focused tests for `/sub/{uuid}` behavior if tests exist nearby.
- [ ] Run: `cargo test -p caramba-panel subscription -- --nocapture` if targeted tests exist; otherwise run `cargo test -p caramba-panel -- --nocapture` and note the exact subscription-related cases covered.
- [ ] Verify these states explicitly: plain URL returns all nodes, `?node_id=` returns scoped config, stale node metadata does not collapse the default config, and a previously cached plain URL now returns the full node set immediately.

## Chunk 2: Simplify Mini App Primary UX

### Task 4: Turn Home into a cleaner launch pad

**Files:**
- Modify: `apps/caramba-app/src/pages/Home.tsx`
- Modify: `apps/caramba-app/src/pages/Home.css`

- [ ] Reduce competing controls in the hero.
- [ ] Remove low-value technical chrome like `haptic-ready` and redundant action chips.
- [ ] Replace the current traffic zero-state with a clearer empty/usage presentation.
- [ ] Make one primary CTA lead to import/connect and demote the rest.

### Task 5: Simplify My Services around import first

**Files:**
- Modify: `apps/caramba-app/src/pages/Subscription.tsx`
- Modify: `apps/caramba-app/src/pages/Subscription.css`

- [ ] Make the base subscription link the main action.
- [ ] Demote sing-box variants and app-specific links into an advanced section.
- [ ] Reduce visual noise in the card meta rows and action rows.
- [ ] Improve copy so the user understands that the subscription already includes all routes.

### Task 6: Reduce technical overload in the support and connection guide flow

**Files:**
- Modify: `apps/caramba-app/src/pages/Support.tsx`
- Modify: `apps/caramba-app/src/pages/ConnectGuide.tsx`

- [ ] Remove the security/PIN block from the middle of the support FAQ flow or reduce its prominence substantially.
- [ ] Tighten connection guide wording so it reinforces the new default: copy/import the subscription, then the client loads connections automatically.

## Chunk 3: Advanced Controls Cleanup

### Task 7: Reframe server and variant selection as advanced tools

**Files:**
- Modify: `apps/caramba-app/src/pages/Servers.tsx`
- Modify: `apps/caramba-app/src/pages/ServerSelector.tsx`
- Modify: `apps/caramba-app/src/pages/Servers.css`

- [ ] Keep server optimization available, but present it as optional tuning rather than a required connection step.
- [ ] Improve language around variants so users see recommendation and fallback meaning, not raw protocol taxonomy first.
- [ ] Reduce visible complexity in variant lists and CTA wording.

### Task 8: Verify the full user journey

**Files:**
- Verify: `apps/caramba-app/src/App.tsx`
- Verify: `apps/caramba-app/src/pages/Home.tsx`
- Verify: `apps/caramba-app/src/pages/Subscription.tsx`
- Verify: `apps/caramba-app/src/pages/Support.tsx`

- [ ] Confirm primary journey: home -> subscription/import -> client import.
- [ ] Confirm advanced journey: home/services -> optimize node or choose variant.
- [ ] Confirm bottom navigation labels and spacing remain readable after UI changes.
- [ ] Manual QA route 1: open `/app/`, tap the primary connect CTA, copy/import the base subscription link, and confirm the user copy says the subscription already includes routes.
- [ ] Manual QA route 2: open `/app/subscription`, expand an active subscription, confirm advanced options are visually secondary, then open optimization and verify its success state does not redefine the base subscription behavior.
- [ ] Manual QA route 3: open `/app/support`, confirm the security block no longer interrupts the FAQ flow as a primary object.

## Validation

- [ ] Run LSP diagnostics on all changed frontend files and require zero errors.
- [ ] Run LSP diagnostics on all changed backend files and require zero errors.
- [ ] Run: `npm run build` in `apps/caramba-app`.
- [ ] Run: `cargo check -p caramba-panel`.
- [ ] Run: `cargo test -p caramba-panel -- --nocapture`.
- [ ] Manually fetch the plain subscription URL before and after a manual optimization action and confirm the plain URL still works as the “all routes” import path, while an explicit `?node_id=` URL remains scoped.
