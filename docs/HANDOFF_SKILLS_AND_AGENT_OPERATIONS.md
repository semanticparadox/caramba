# Skills Inventory and Agent Operations Handoff

Purpose: transfer the exact agent operating context so another IDE assistant can continue with minimal drift.

## 1. Installed Skills (from session AGENTS context)

| Skill | Purpose (short) | Path |
|---|---|---|
| `3proxy` | Deploy/configure 3proxy proxy servers | `/Users/smtcprdx/.codex/skills/3proxy/SKILL.md` |
| `a11y-semantics-pass` | Accessibility and semantics coverage pass | `/Users/smtcprdx/.codex/skills/a11y-semantics-pass/SKILL.md` |
| `code-review` | Rust/WASM code review for bugs/security/perf | `/Users/smtcprdx/.codex/skills/code-review/SKILL.md` |
| `create-plan` | Build concise executable work plans | `/Users/smtcprdx/.codex/skills/create-plan/SKILL.md` |
| `cybersecurity-hardening-pro` | Security audit + hardening workflow | `/Users/smtcprdx/.codex/skills/cybersecurity-hardening-pro/SKILL.md` |
| `debugging` | Systematic Rust debugging workflow | `/Users/smtcprdx/.codex/skills/debugging/SKILL.md` |
| `disciplined-research` | Deep problem understanding before design | `/Users/smtcprdx/.codex/skills/disciplined-research/SKILL.md` |
| `dns-record-analyzer` | DNS configuration audit/troubleshooting | `/Users/smtcprdx/.codex/skills/dns-record-analyzer/SKILL.md` |
| `launch-strategy` | Product/feature launch strategy | `/Users/smtcprdx/.codex/skills/launch-strategy/SKILL.md` |
| `marketing-ideas` | SaaS/product marketing idea frameworks | `/Users/smtcprdx/.codex/skills/marketing-ideas/SKILL.md` |
| `openvpn` | OpenVPN deployment and management | `/Users/smtcprdx/.codex/skills/openvpn/SKILL.md` |
| `performance-pass-ui` | UI performance regression pass | `/Users/smtcprdx/.codex/skills/performance-pass-ui/SKILL.md` |
| `pricing-strategy` | Pricing/packaging/monetization strategy | `/Users/smtcprdx/.codex/skills/pricing-strategy/SKILL.md` |
| `referral-program` | Referral/affiliate program optimization | `/Users/smtcprdx/.codex/skills/referral-program/SKILL.md` |
| `rust-development` | Idiomatic Rust implementation | `/Users/smtcprdx/.codex/skills/rust-development/SKILL.md` |
| `rust-performance` | Rust profiling and optimization | `/Users/smtcprdx/.codex/skills/rust-performance/SKILL.md` |
| `security-audit` | Rust/WASM security auditing | `/Users/smtcprdx/.codex/skills/security-audit/SKILL.md` |
| `state-modeling` | Complex UI state modeling | `/Users/smtcprdx/.codex/skills/state-modeling/SKILL.md` |
| `testing` | Test writing/execution/failure analysis | `/Users/smtcprdx/.codex/skills/testing/SKILL.md` |
| `ui-refactor-extract` | Behavior-preserving UI refactors | `/Users/smtcprdx/.codex/skills/ui-refactor-extract/SKILL.md` |
| `ux-ui-pro-skills` | UX/UI audit and redesign planning | `/Users/smtcprdx/.codex/skills/ux-ui-pro-skills/SKILL.md` |
| `web-design-guidelines` | UI guideline compliance checks | `/Users/smtcprdx/.codex/skills/web-design-guidelines/SKILL.md` |
| `wireguard` | WireGuard deployment and management | `/Users/smtcprdx/.codex/skills/wireguard/SKILL.md` |
| `xray` | Xray protocol deployment/configuration | `/Users/smtcprdx/.codex/skills/xray/SKILL.md` |
| `skill-creator` | Create/update Codex skills | `/Users/smtcprdx/.codex/skills/.system/skill-creator/SKILL.md` |
| `skill-installer` | Install curated/custom skills | `/Users/smtcprdx/.codex/skills/.system/skill-installer/SKILL.md` |

## 2. Practical Skill Mapping for This Project

Recommended skill choices by workstream:

1. Runtime regressions (`502`, auth failures, node sync drift):
   - `debugging` + `rust-development`
2. Data model and migration safety:
   - `testing` + `rust-development` (+ `security-audit` for auth/session changes)
3. Mini App UX and flows:
   - `ux-ui-pro-skills` + `state-modeling` + `performance-pass-ui`
4. Admin UI clarity and accessibility:
   - `ux-ui-pro-skills` + `a11y-semantics-pass`
5. Security-sensitive deployment controls:
   - `cybersecurity-hardening-pro` + `security-audit`
6. Pricing/referral/product mechanics:
   - `pricing-strategy` + `referral-program`

## 3. Agent Operating Rules to Preserve

These rules were consistently used in this development stream and should be preserved in other IDE agents:

1. Prefer `rg` for search and `rg --files` for file inventory.
2. Do not run destructive git commands (`reset --hard`, broad checkout reverts) unless explicitly requested.
3. Do not revert unrelated user changes.
4. Verify with `cargo check --workspace` after Rust changes.
5. For multi-file reads, parallelize when tool supports it.
6. For installer/runtime fixes, think in operational behavior first, then code shape.
7. Keep release flow aligned with tag-based workflow (`v*`).

## 4. Recommended Starter Prompt for Another IDE Agent

Use this as the first message in a new IDE assistant session:

```text
You are continuing work on CARAMBA at /Users/smtcprdx/Documents/caramba.
Read in this order:
1) docs/HANDOFF_MASTER_CONTEXT_2026-02-22.md
2) docs/HANDOFF_OPEN_WORKSTREAMS.md
3) docs/HANDOFF_RELEASE_TIMELINE_2026Q1.md
4) README.md
Then run cargo check --workspace and report current blockers.
Do not push or tag until explicitly requested.
Prioritize installer-first operations, auth stability, and subscription correctness.
```

## 5. Context Load Order (Fast)

If time-constrained, load only:

1. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_MASTER_CONTEXT_2026-02-22.md`
2. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_OPEN_WORKSTREAMS.md`
3. `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/main.rs`
4. `/Users/smtcprdx/Documents/caramba/apps/caramba-panel/src/api/client.rs`
5. `/Users/smtcprdx/Documents/caramba/apps/caramba-installer/src/main.rs`

## 6. Context Load Order (Full)

1. All files under `/Users/smtcprdx/Documents/caramba/docs/` with `HANDOFF_` prefix.
2. `README.md` and `scripts/install.sh`.
3. Panel services and sing-box generator files.
4. Mini App auth/subscription/promo/referral pages.
5. Recent git history from tags `v0.3.20+`.

