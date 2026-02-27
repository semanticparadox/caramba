# IDE Prompt Pack (Antigravity / Claude Code / Cursor)

Purpose: ready-to-use prompts for continuing CARAMBA development in another IDE agent without losing context.

Use these after reading:

1. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_MASTER_CONTEXT.md`
2. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_OPEN_WORKSTREAMS.md`
3. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_RELEASE_TIMELINE.md`
4. `/Users/smtcprdx/Documents/caramba/docs/HANDOFF_SKILLS_AND_AGENT_OPERATIONS.md`

---

## Global starter prompt (any IDE)

```text
Project: /Users/smtcprdx/Documents/caramba

Before coding, load and summarize:
1) docs/HANDOFF_MASTER_CONTEXT.md
2) docs/HANDOFF_OPEN_WORKSTREAMS.md
3) docs/HANDOFF_RELEASE_TIMELINE.md
4) docs/HANDOFF_SKILLS_AND_AGENT_OPERATIONS.md
5) README.md

Rules:
- Do not push, tag, or release unless I explicitly request it.
- Prefer installer-first operational behavior over local-only fixes.
- Preserve compatibility paths (/api/client and /caramba-api/client).
- Run cargo check --workspace after Rust changes.
- If touching miniapp, run npm run build in apps/caramba-app.

Then report:
1) current blocker list
2) exact files to change first
3) smallest safe implementation plan
```

---

## Antigravity Prompts

### Antigravity: Debug mode

```text
Act as a Rust production debugger for CARAMBA.
Workspace: /Users/smtcprdx/Documents/caramba

Goal: reproduce and fix this issue: <PASTE ISSUE>.

Process:
1) Trace request path from router to handler/service/repository.
2) Identify root cause, not symptom.
3) Propose minimal fix with low regression risk.
4) Implement patch.
5) Run cargo check --workspace.
6) Provide verification commands for server-side runtime.

Constraints:
- No destructive git commands.
- No push/release.
- Keep backward-compatible API behavior unless explicitly approved.

Output format:
1) Root cause
2) Code changes
3) Validation results
4) Follow-up risks
```

### Antigravity: Feature mode

```text
Implement feature in CARAMBA: <PASTE FEATURE>.
Workspace: /Users/smtcprdx/Documents/caramba

Requirements:
- Respect existing installer-first architecture.
- Keep admin path and /caramba-api compatibility behavior intact.
- Update docs in /docs when behavior changes.
- Add tests when practical for core logic.

Execution:
1) Identify exact modules and API contracts touched.
2) Implement end-to-end (backend + UI + miniapp if needed).
3) Run cargo check --workspace.
4) If miniapp changed: cd apps/caramba-app && npm run build.
5) Return migration/ops notes if deployment behavior changes.
```

### Antigravity: Refactor mode

```text
Perform a behavior-preserving refactor in CARAMBA.
Workspace: /Users/smtcprdx/Documents/caramba
Scope: <PASTE FILES OR MODULES>.

Rules:
- No functional changes.
- Preserve routes, payload shapes, and installer commands.
- Prefer small atomic commits (local only; do not push).

Deliver:
1) What was extracted/renamed/reorganized
2) Why behavior is unchanged
3) Validation run (cargo check --workspace)
```

### Antigravity: Release prep mode

```text
Prepare CARAMBA release candidate locally (no push until I confirm).
Workspace: /Users/smtcprdx/Documents/caramba

Checklist:
1) Verify clean git status for intended changes.
2) cargo fmt --all
3) cargo check --workspace
4) cd apps/caramba-app && npm ci && npm run build
5) Summarize release notes from local commits.
6) Propose tag version based on existing tags.

Stop before push/tag. Ask me for final confirmation.
```

---

## Claude Code Prompts

### Claude Code: Debug mode

```text
You are continuing CARAMBA work at /Users/smtcprdx/Documents/caramba.
Use a strict root-cause debugging approach.

Issue:
<PASTE ISSUE + logs + screenshots context>

First load:
- docs/HANDOFF_MASTER_CONTEXT_2026-02-22.md
- docs/HANDOFF_OPEN_WORKSTREAMS.md

Then:
1) map execution path
2) identify failing assumptions
3) patch with smallest safe change
4) run cargo check --workspace
5) return exact runtime verification commands

Do not push/tag/release.
```

### Claude Code: Feature mode

```text
Build this feature in CARAMBA: <PASTE FEATURE>.
Repo: /Users/smtcprdx/Documents/caramba

Design constraints:
- Maintain installer-first operations.
- Preserve compatibility for legacy endpoints where present.
- Keep topology (hub/distributed) behavior coherent.

Tasks:
1) list touched files
2) implement end-to-end
3) run cargo check --workspace
4) run miniapp build if frontend changed
5) update docs handoff files if behavior changed
```

### Claude Code: Refactor mode

```text
Refactor CARAMBA module(s) without behavior changes:
<PASTE MODULES>.

Repo: /Users/smtcprdx/Documents/caramba

Must keep:
- API contracts
- installer CLI behavior
- subscription generation outputs

Validation:
- cargo check --workspace
- show before/after structure summary

No push/release.
```

### Claude Code: Release mode

```text
Prepare release for CARAMBA (local prep only).
Repo: /Users/smtcprdx/Documents/caramba

Steps:
1) collect commits since last tag
2) produce concise release notes draft
3) run:
   - cargo fmt --all
   - cargo check --workspace
   - (if needed) apps/caramba-app npm run build
4) propose next tag and risk list

Do not push tag or branch until I approve.
```

---

## Cursor Prompts

### Cursor: Debug mode

```text
Project root: /Users/smtcprdx/Documents/caramba

Debug production issue:
<PASTE ISSUE>

Instructions:
- Read docs/HANDOFF_MASTER_CONTEXT_2026-02-22.md first.
- Trace from route -> handler -> service -> DB.
- Patch minimally with low blast radius.
- Run cargo check --workspace.
- Provide exact commands to verify fix on server.

Do not push or tag.
```

### Cursor: Feature mode

```text
Project root: /Users/smtcprdx/Documents/caramba

Implement feature:
<PASTE FEATURE>

Requirements:
- Keep architecture modular (panel/node/sub/bot/installer).
- Keep /api/client and /caramba-api/client compatible.
- Update docs under /docs for operational changes.

After coding:
- cargo check --workspace
- if miniapp touched: cd apps/caramba-app && npm run build
```

### Cursor: Refactor mode

```text
Project root: /Users/smtcprdx/Documents/caramba

Refactor target:
<PASTE PATHS>

Goal:
- readability and maintainability improvements
- zero behavior change

Guardrails:
- keep route signatures and payloads
- keep installer command semantics
- run cargo check --workspace
```

### Cursor: Release mode

```text
Project root: /Users/smtcprdx/Documents/caramba

Prepare release candidate:
1) summarize changes since latest tag
2) run quality checks
3) draft release note markdown
4) recommend semantic version bump

Important:
- stop before git push / tag push
- wait for my explicit confirmation
```

---

## Optional one-line handoff for teammates

```text
Use /Users/smtcprdx/Documents/caramba/docs/HANDOFF_MASTER_CONTEXT_2026-02-22.md as source of truth, then execute only installer-first compatible changes, validate with cargo check --workspace, and do not push without explicit approval.
```

