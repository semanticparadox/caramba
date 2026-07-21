# Project workflow

Use Beads for task tracking.
Use TaskWing for planning.

Workflow:

1. Check available tasks:
   bd ready

2. Claim a task:
   bd update <id> --claim

3. If task requires planning:
   call TaskWing to create subtasks.

4. Implement only the current task.

5. If new work appears:
   create follow-up tasks with bd create.

6. Close task only when tests pass.

<!-- BEGIN BEADS INTEGRATION v:1 profile:full hash:d4f96305 -->
## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Git-friendly: Dolt-powered version control with native sync
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" --description="Detailed context" -t bug|feature|task -p 0-4 --json
bd create "Issue title" --description="What this issue is about" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update <id> --claim --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task atomically**: `bd update <id> --claim`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" --description="Details about what was found" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`

### Auto-Sync

bd automatically syncs via Dolt:

- Each write auto-commits to Dolt history
- Use `bd dolt push`/`bd dolt pull` for remote sync
- No manual export/import needed!

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems

For more details, see README.md and docs/QUICKSTART.md.

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

<!-- END BEADS INTEGRATION -->

---

## Post-incident runbook (added 2026-07-21, after the v0.9.48 sing-box outage)

Patterns discovered while fixing the 2026-07-21 incident. Read this if you're
touching `apps/caramba-node`, `sing-box` config generation, the panel's
`MarketplaceService`, or the installer's upgrade flow.

### 1. `sing-box` runs as a system user, not root

The systemd unit on every node is `User=sing-box` (`Group=sing-box`). The
package's `/etc/sing-box/` is owned by root with mode 0755 and cannot be
written by `sing-box`. Anything that needs a writable path (cache files,
runtime state, generated TLS material) MUST live under `StateDirectory=`
(`/var/lib/sing-box/`), NOT under `/etc/sing-box/`. The 0.9.48 bug was
`experimental.cache_file.path = /etc/sing-box/cache.db` — fix is to point at
`/var/lib/sing-box/cache.db`. When generating sing-box config, default any
new writable path to `/var/lib/sing-box/<thing>`; never reintroduce
`/etc/sing-box` for state.

### 2. Per-node reload-all trigger

The panel regenerates configs on heartbeat (every ~30s per node). The admin
"Reload all" button is a force-flush of the cache for nodes that haven't
checked in yet — use it after a panel-side config change that needs to
land fast, but expect a 30s lag without it. Do NOT call
`/admino4ka/nodes/reload-all` from curl/CLI: auth is cookie+CSRF and you
won't have credentials. The heartbeat regeneration is the safe path for
the agent anyway.

### 3. `caramba-node` self-update version comparison

`is_newer_version(target, current)` in `apps/caramba-node/src/main.rs`
parses `vX.Y.Z` and compares the first three numeric components. A
string-equality fallback used to be in the same call site and caused
the agent to "update" to older versions, then SIGTERM itself on restart
and trip `systemd`'s `StartLimitBurst` — leaving the node dark. The fix
is the semver-style compare. If you change the version comparison, keep
the unit-test coverage (`is_newer_version_*` cases in the same file) and
verify the 3-segment minimum guard. Do NOT use a lexicographic compare
(`"0.10.0" < "0.9.0"` in lexicographic order — the seg-fault path of
this whole incident class).

### 4. Panel-side `sing-box` validation gate (added 0.9.50)

`apps/caramba-panel/src/singbox/generator.rs::validate_config` used to
`warn!` and skip validation if the panel couldn't run `sing-box check`.
After 0.9.50 it `error!`s and returns Err — the panel must have the
`sing-box` binary installed (same version as the nodes) so a broken
config is caught on the panel BEFORE it ships. On a fresh panel host,
install `sing-box` from the same package source as the nodes, then
verify with `sing-box version`. Don't ship a release that disables
this gate.

### 5. Installer rollback / version pinning

`caramba upgrade --to v0.9.49` (alias of `--version`) is the supported
rollback path. The flow downloads the older release asset from GitHub,
writes the binary, and restarts — same as a forward upgrade. The
version marker at `<install_dir>/.caramba-version` is updated on
success, so `caramba diagnose` always shows the actual version. If a
release ships broken, the playbook is: `sudo caramba upgrade --to
<last-good-tag>`, then file the post-mortem.

### 6. Payment provider add checklist (use for new providers)

When adding a new PSP to `apps/caramba-panel/src/services/payment/`:

1. New file `<provider>.rs` implementing `PaymentProvider`
   (`create_invoice`, `verify_webhook`, `handle_webhook`, `check_status`,
   `supports_polling` if off-chain).
2. `pub mod <provider>;` in `mod.rs`.
3. Add to `MarketplaceService::new` signature + provider registration in
   `services/marketplace_service.rs` (add the env-var fields, plumb them
   through, and register the provider with a `if !key.is_empty()` guard).
4. Add a case in `handlers/admin/payments.rs::invoke_provider_test` so
   the admin "Test connection" button covers it.
5. Add a `provider_label` entry in `api/client.rs` (with emoji + suffix).
6. Add a `provider_enable_setting` entry in `marketplace_service.rs`
   (default off for opt-in RU providers, default on for legacy).
7. Add a `header(...)` case in `api/webhooks.rs::handle_payment_webhook`
   for the provider's signature header (or empty string if signed in
   body — see Cryptomus, AAIO).
8. Document the env vars in `.env.example`.
9. Run `cargo check --workspace` and `cargo test --workspace`.

The 0.9.50 cycle added `paypalych` as a worked example; cross-check
against that file if the list above is ambiguous.
