# Caramba — Post-incident improvement roadmap (session brief)

> **Use this doc as the FIRST message of a new session** if you want the agent
> to do everything from "what broke / what's pending" → committed & deployed.
>
> Everything below assumes the new session is **blank** — no knowledge of the
> July 21 incident. The doc is the whole context.

---

## 0. Who you are, what this is

- **Repo**: `semanticparadox/caramba` (private; GitHub Actions CI on
  `.github/workflows/release.yml`).
- **Maintainer**: Artur Kotov (`semanticparadox@gmail.com`, `@semanticparadox`).
  One-person project. ~20 real users on production VPN.
- **Stack** (Rust workspace, `Cargo.toml` at root):
  - `apps/caramba-panel` — Axum admin/orchestration, PostgreSQL, Redis
  - `apps/caramba-sub` — Axum frontend/worker
  - `apps/caramba-node` — agent on each VPN node
  - `apps/caramba-bot` — Teloxide Telegram bot
  - `apps/caramba-installer` — `caramba` CLI (install/upgrade/doctor/backup/uninstall)
  - `apps/caramba-app` — Telegram Mini App (TS)
  - `apps/caramba-client` — Flutter super-app client (WIP, kept locally; lives outside the Rust release)
  - `libs/caramba-db`, `libs/caramba-shared`
- **Ship flow** (NEVER build locally + scp — that's not how this project ships):
  1. Edit code on a branch.
  2. `cargo check --workspace` must be clean locally.
  3. `git push origin <branch>`, then `git tag v0.9.XX && git push origin v0.9.XX`.
  4. CI builds `x86_64-unknown-linux-musl` binaries and publishes them as
     GitHub release assets (panel, sub, node, bot, installer, app dist).
  5. On each server: `sudo caramba upgrade` downloads the new binary and
     restarts the relevant systemd unit.
- **Release profile is heavy** (`LTO`, `opt-level = "z"`, `strip`, single codegen
  unit). Release builds are SLOW — use `cargo check` for dev loops. `cargo build
  --workspace` locally is wasteful; trust CI.
- **Server fleet** (managed in `~/.ssh/config`, key-based via `id_ed25519`):
  - `poland` → 57.128.240.245 — OVH, debian+sudo, **panel + sub live here**
  - `germany` → 85.215.196.151 — IONOS, root
  - `canada` → 158.69.213.88 — OVH, debian+sudo
  - `veles` → 141.98.191.214 — relay, root, named by user in Slavic style
  - `usa` → 209.46.123.89 — IONOS, root, **NOT in caramba panel** (legacy host)
  - Panel admin path is `ADMIN_PATH` env var, currently `/admino4ka`
    (NOT `/admin`).
  - `~/.ssh/config` has these entries; new ones can be added with the same
    pattern (no passwords in config — use one-time password bootstrap, install
    key, use key auth).
- **Production state on July 21**: recovered from a `sing-box` crash on every
  node (root cause: `experimental.cache_file.path` in `13bcfe4` pointed to
  `/etc/sing-box/cache.db` which `User=sing-box` cannot write; fixed in
  v0.9.49). All 3 nodes (germany, canada, veles) on v0.9.49. caramba-node
  self-update also fixed (was string-equal instead of semver, kept "updating"
  to older versions and SIGTERM'd itself).

---

## 1. The mission

Ship the next minor release (target **`v0.9.50`** or **`v0.10.0`** depending on
scope you take) with **two confirmed tickets** plus as much of the long-term
list as fits in scope. Don't bloat: 1 release = 1 coherent theme, otherwise we
can't bisect or roll back cleanly.

### Already-decided tickets (must ship in this cycle)

#### T1. Add Paypalych (`pally.info` → `pal24.pro` API) as a payment provider

- **Why**: user wants SBP / USDT TRC20 acceptance for the ~20 Russian users.
  Existing 18 providers (cryptomus, aaio, lava, nowpayments, oxapay, wata,
  plisio, tribute, crystalpay, balance, btcpay, cryptobot, coinbase_commerce,
  telegram_stars, stripe, manual, etc.) don't cover the SBP channel well.
- **Status on user side (as of last contact)**:
  - pally.info account exists (`ayertag@gmail.com`).
  - Project creation form was being filled. The "Add Project" dialog accepts:
    name, activity type, description, language, **URL магазина**, **Success URL**,
    **Fail URL**, **Result URL**, **Refund URL**, **Chargeback URL**.
  - **Tariffs (committed by user)**: USDT TRC20 in ₽ — 3% + 1 USDT (min 400₽,
    max 1,000,000₽). СБП — 6.5% + 2₽ (min 10₽, max 50,000₽).
  - **The dashboard's `app.exarobot.top` is currently broken** (DNS points to
    veles which only runs sing-box, no HTTP). For pally.info the user will use
    `panel.exarobot.top` (panel serves `/app`, `/sub/{uuid}`, `/api/...`).
- **Code pattern**: model new file after
  `apps/caramba-panel/src/services/payment/lava.rs` (HMAC-SHA256 signature,
  snake_case JSON, simple webhook). Cryptomus.rs is the heavier reference.
- **API contract to implement** (from public sources, NOT confirmed against
  pally.info's `/merchant/api` page — the user only sent screenshots of the
  dashboard, not the API reference). Re-verify on the first run if user has
  not given you the live reference text:
  - Base URL: `https://pal24.pro/api/v1/`
  - Auth header: `Authorization: Bearer <token>` (the `72|xxxxx` form)
  - Create bill: `POST /bill/create` — body has `amount`, `currency`,
    `order_id`, `shop_id?`, `hook_url?`, `success_url?`, `fail_url?`,
    `comment?`, `custom_fields?`, `expire?` (minutes). Response returns
    `link_page_url` (or `link` — TBD; both are referenced in similar APIs).
  - Status: `GET /bill/status?id=<bill_id>` or `POST /bill/status` with body.
  - Webhook (postback): configurable URL in project settings. Typical body
    has `id`, `order_id`, `status` (e.g. `new` / `paid` / `canceled` / `expired`),
    `amount`, `currency`, `custom_fields`. May or may not be signed.
- **Webhooks the project form asks for**: `Result URL`, `Refund URL`,
  `Chargeback URL` — point all three to the same path for now, let the handler
  branch on payload:
  `https://panel.exarobot.top/api/webhooks/payment/paypalych`
- **What user must hand you before you can ship**:
  1. Bearer token (from `pally.info` → "API интеграция" after moderation
     passes).
  2. Project/shop ID (visible in project URL after creation).
  3. Confirmation whether postback is HMAC-signed and with which header.
  4. Whether they want both СБП and USDT TRC20 (likely yes, user picks).
- **Wiring checklist** (these are the 4 touch points per provider):
  1. New file `apps/caramba-panel/src/services/payment/paypalych.rs`
     implementing `PaymentProvider` trait.
  2. Add `pub mod paypalych;` to `apps/caramba-panel/src/services/payment/mod.rs`.
  3. Add to `apps/caramba-panel/src/handlers/admin/payments.rs` (the
     `payments::test_provider_connection` switch — copy the pattern for the
     other providers).
  4. Add to `apps/caramba-panel/src/handlers/admin/payment_pricing.rs` so the
     admin UI can enable/disable it.
  5. Document env vars in `.env.example`: `PAYPALYCH_API_TOKEN`,
     `PAYPALYCH_SHOP_ID` (optional), `PAYPALYCH_WEBHOOK_SECRET` (optional).
- **Don't** modify the `PaymentSession` schema or add a migration. All 18
  existing providers use string-keyed `provider` field — `"paypalych"` fits.

#### T2. Admin UI cleanup (duplicates, dead code, consistency)

- **Why**: user explicitly complained "там сейчас я замечаю дублирование" and
  asked to remove extra buttons/menus and improve the interface.
- **Audit findings (July 21)** — the big one is **the bottom OS-style dock
  duplicates the top sub-nav**. Both show the same 6 sections. Pick one, kill
  the other. Specifics in `apps/caramba-panel/templates/base.html:244-370`
  (sub-nav) vs `:317-370` (dock).
- **Confirmed cleanups** (low risk):
  1. **Delete the bottom dock** entirely (sub-nav already context-sensitive).
     Frees ~50 lines + 6 lucide icon imports.
  2. **Remove dead commented routes** in
     `apps/caramba-panel/src/main.rs:891` (`/tools`) and `:896` (`/traffic`).
     `/traffic`'s handler `get_traffic_analytics` is still used at `:1137`,
     so the comment is misleading — drop the comment line, keep the handler
     registration at `:1137`.
  3. **Delete the legacy `/nodes` route** if `get_nodes` (the combined view)
     is not used by the new Exit/Relay split. Verify in
     `apps/caramba-panel/templates/` — only `/nodes/exit` and `/nodes/relay`
     appear in nav-pills now.
  4. **Rename URL `/api-keys` → `/enrollment-keys`** for label consistency
     (UI says "Enrollment Keys" but URL says api-keys). Includes route
     definition, nav-pill href, and any handler-relative links.
- **Decide-and-execute (medium risk, ASK USER before doing)**:
  5. **Marketplace vs Plans/Products/Categories** — is `Marketplace` a separate
     concept or the same thing with a different name? If same, merge into
     "Commerce" sub-nav and drop the `/marketplace` page.
  6. **Profiles / Groups / Templates / Frontends** under "Servers" sub-nav
     — 7 items, can any be merged or hidden behind a "Advanced" toggle?
  7. **Dashboard / Analytics / Logs** — three separate pages, can Analytics
     be a dashboard widget? (UX, not duplicate)
- **Don't** change the dashboard's data model or restructure the
  statusbar (`templates/partials/statusbar.html`).

### Nice-to-have if you have bandwidth (one line each, decide together)

- **N1. `sing-box` validation gate that fails loud on panel side** — install
  `sing-box` binary on poland, change the `WARN Skipping Sing-box config
  validation` in `apps/caramba-panel/src/singbox/generator.rs::validate_config`
  to an error when validation can't run. (5-10 lines, ~30 min)
- **N2. caramba-node runtime smoke after writing config** — after `sing-box
  check`, briefly start sing-box in dry-run / with timeout, refuse to restart
  service if it would die immediately. Closes the same class of bug we just
  fixed. (~50 lines, ~1-2 hours)
- **N3. Rollback flag** in installer — `caramba upgrade --to v0.9.49` to
  pin version. (~20 lines, 30 min)
- **N4. AGENTS.md updates** — append a "runbook" section covering the
  patterns we just discovered (sing-box `User=sing-box` `StateDirectory`,
  per-node reload-all trigger, version-comparison fix). Pure doc, no risk.
- **N5. `app.exarobot.top` DNS fix** — change A record from veles to poland
  (the panel's /app route serves the Mini App). Trivial DNS edit; user has
  to do it at the registrar. (5 min, but on the user's side)

### Explicitly out of scope (defer)

- External monitoring (healthchecks.io, UptimeRobot) — needs user account.
- Staging environment — needs separate VM, billing decision.
- Canary deploy — design discussion, not a code change.
- sing-box AmneziaWG support — needs a fork or removal from UI.
- Caramba Connect / exa_robot (B2B pivot, native client, license server) —
  lives in a separate repo; this repo only ships the core Rust stack and the
  Telegram Mini App.

---

## 2. Pre-flight checklist (do these first)

```bash
cd /Users/smtcprdx/Documents/Projects/caramba
git status                     # clean tree?
git branch --show-current      # which branch are you on?
ls ~/.ssh/config               # confirm fleet aliases (germany, canada, veles, poland)
ssh -o ConnectTimeout=5 poland 'systemctl is-active caramba-panel'  # sanity
cargo check --workspace 2>&1 | tail -3   # must end in "Finished" — if not, fix first
```

If `cargo check --workspace` is not clean on main, **stop and ask the user** —
there's a regression between sessions. Do not proceed on a broken tree.

Then read:
- `apps/caramba-panel/src/services/payment/lava.rs` (simplest reference)
- `apps/caramba-panel/src/services/payment/cryptomus.rs` (heavier reference)
- `apps/caramba-panel/src/services/payment/provider.rs` (the trait)
- `apps/caramba-panel/src/handlers/admin/payments.rs` (test-connection endpoint)
- `apps/caramba-panel/src/handlers/admin/payment_pricing.rs` (enable toggle)
- `apps/caramba-panel/templates/base.html` (sub-nav and dock — the duplication)

---

## 3. How to work — use parallel sub-agents where it pays off

This session is doing 1–2 confirmed tickets + maybe 0–2 nice-to-haves. The
**confirmed** tickets are largely independent in the code:

- T1 (Paypalych provider) → 1 new file `paypalych.rs` + 3-4 touch points in
  existing files (`mod.rs`, `payments.rs`, `payment_pricing.rs`,
  `.env.example`).
- T2 (UI cleanup) → 1 file `base.html` (remove dock) + 2 lines in `main.rs`
  (delete dead routes) + a URL rename across 3-4 files.

You can do these **sequentially yourself** in one session — they're each small
enough that orchestrating sub-agents costs more than it saves. But:

- **If T1 starts blocking on user input** (token, webhook secret), pivot to T2
  or N1/N2 — don't idle waiting.
- **If a "nice-to-have"** (N1 install sing-box on panel) is one-liner, do it
  in the same commit, don't split.
- **N4 (runbook in AGENTS.md)** is pure docs and can be done anytime — save
  it for end-of-session cleanup.

Pattern for the T1 provider file: read `lava.rs` (290 lines, simple), copy as
a template, rename `Lava` → `Paypalych`, swap URL/auth/fields. Aim for the
same shape. **Do not over-engineer** — match the existing 18 providers'
simplicity.

---

## 4. Quality gates (must pass before any commit)

```bash
# 1. Linter
cargo clippy --workspace --all-targets -- -D warnings

# 2. Unit tests
cargo test --workspace

# 3. Workspace check (catches missing pub mods)
cargo check --workspace

# 4. rustfmt
cargo fmt --all -- --check
```

If clippy complains about your new code, **fix it**. If a pre-existing
warning shows up in code you didn't touch, ignore it (don't expand scope).

If a test fails, **fix the test or the code** — do not `--release` skip.

---

## 5. Ship procedure (exactly the way the project ships)

```bash
cd /Users/smtcprdx/Documents/Projects/caramba

# Branching: keep working on a feature branch, only merge to main when ready.
git checkout -b feature/paypalych-and-ui-cleanup

# Commit granularity: 2-3 commits is fine, but each commit must build.
# Suggested split:
#   1. "feat(payments): add Paypalych provider"
#   2. "fix(admin): remove dead routes + duplicate dock navigation"
#   3. "chore: bump version to 0.9.50 for release"
# (combine 1+2 if they're both small enough — your call)

git add <files>
git commit -m "<conventional commit style matching existing log>"

# Bump version in all 8 Cargo.toml files (apps + libs):
#   0.9.49 → 0.9.50
sed -i.bak 's/version = "0.9.49"/version = "0.9.50"/' \
  apps/*/Cargo.toml libs/*/Cargo.toml
find apps libs -name "*.bak" -delete
git add apps/*/Cargo.toml libs/*/Cargo.toml
git commit -m "chore: bump version to 0.9.50 for release"

# Fast-forward merge to main (after rebase) + push:
git checkout main
git rebase feature/paypalych-and-ui-cleanup    # if needed
git push origin main

# Tag triggers release.yml on .github/workflows:
git tag v0.9.50
git push origin v0.9.50
```

If `git push origin main` is rejected because remote has commits you don't
have locally, do `git fetch origin && git rebase origin/main` first. This
happens when the user pushed a hotfix in another session.

**DO NOT** try to build release locally and scp binaries. The CI does it.

---

## 6. Watch CI, deploy when ready

CI typically takes 5-15 minutes for the musl build + npm ci for the Mini App.
Set a cron self-reminder to check every 5 min until the release is published
at `https://github.com/semanticparadox/caramba/releases/tag/v0.9.50`.

When published, run **on each server in this order**:

```bash
# Panel + sub first (they live on the same host)
ssh poland 'sudo caramba upgrade'

# Then each VPN node
ssh germany 'sudo caramba upgrade'
ssh canada 'sudo caramba upgrade'
ssh veles 'sudo caramba upgrade'

# Re-enable on hosts where the agent was disabled for hotfix reasons
ssh germany 'sudo systemctl enable --now caramba-node'
ssh veles 'sudo systemctl enable --now caramba-node'
```

After all four upgrade + the node-agent re-enable, give the user a 2-line
status report per host (what version, sing-box state, caramba-node state).
**Do not** try to invoke `/admino4ka/nodes/reload-all` from curl — auth is
cookie+CSRF based and you won't have credentials; the panel regenerates
configs on heartbeat anyway. Tell the user to click the button in admin UI
once for cleanliness.

**Don't** run `sudo caramba upgrade` on `usa` — that host is not part of the
caramba panel and shouldn't be touched.

---

## 7. Acceptance criteria — release is "done" when

- [ ] All quality gates (clippy, test, check, fmt) green.
- [ ] `v0.9.50` tag published, all 6 release assets present.
- [ ] All 4 in-scope servers (`poland`, `germany`, `canada`, `veles`) on
      v0.9.50.
- [ ] On each node: `sing-box` active, all expected protocols listening,
      `caramba-node` active.
- [ ] `usa` left alone.
- [ ] User receives a single final report with: what's in the release,
      what's deployed, where to verify, and any known follow-ups.
- [ ] Cron self-reminder is deleted (don't leave scheduled tasks running).
- [ ] No more pending async operations.

---

## 8. If you hit a wall

- **CI fails on musl build** (e.g. new dep needs glibc) — message user, don't
  try to work around locally. They know their infra.
- **`cargo check --workspace` was already broken on main** before you started
  — stop, ask user. Likely a hotfix was pushed in parallel.
- **Paypalych moderation still pending** and user can't give you a token —
  ship T2 only as a UI-cleanup release (`v0.9.50-cleanup` or similar) and
  leave T1 for the next cycle.
- **Panel admin auth blocks you from `/admino4ka/nodes/reload-all`** —
  expected, don't try to bypass. Tell the user to click it once.
- **User isn't responding to messages** — check your last message timestamp;
  if > 30 min, they may be away. Set a longer cron and wait, don't retry
  destructively.

---

## 9. Out-of-band references (read only if you need them)

- `docs/CONFIGURATION.md` — env vars
- `docs/API.md` — public API surface
- `docs/DEPLOYMENT.md` — install/upgrade flow
- `docs/MODULES.md` — module map
- `docs/CURRENT_STATE.md` — feature history
- `apps/caramba-panel/src/handlers/admin/auth.rs` — admin auth middleware
  (you'll touch this if you need a way to call admin endpoints from CLI; you
  don't, for this release)
- `~/.minimax/memory/user.md` — user profile and working style (read this
  before your first message)
