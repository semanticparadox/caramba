# New-session prompt — Caramba + Paypalych provider rewrite

> Скопируй **всё ниже линии** в новый сеанс Claude Code в корне репо
> (`/Users/smtcprdx/Documents/Projects/caramba`). Это self-contained контекст.

---

You are fixing the **Paypalych (pal24.pro / pally.info) payment provider** in the
Caramba panel. The previous implementation (commit `475b839`) was written from
guesswork because the API docs were protected by DDoS-Guard. The user
extracted the full docs and saved them at
**`/Users/smtcprdx/apidocs.md`** (7106 lines of HTML — DO NOT re-parse;
use the cleaned-up spec at **`docs/PAYPALYCH-API-SPEC.md`** which has
the key facts in plain tables).

Your job: **rewrite `apps/caramba-panel/src/services/payment/paypalych.rs` to
match the real Pal24 API**, plus the few surrounding files that need
updating. Then ship as `v0.9.53`.

---

## 1. What Caramba is (so you don't break anything)

- Rust workspace in `/Users/smtcprdx/Documents/Projects/caramba`.
- `apps/caramba-panel` — Axum control plane (PostgreSQL + Redis).
- `apps/caramba-sub` — Axum frontend/worker.
- `apps/caramba-node` — VPN node agent.
- `apps/caramba-bot` — Teloxide Telegram bot.
- `apps/caramba-installer` — `caramba` CLI (install/upgrade/doctor/backup/uninstall).
- `libs/caramba-db`, `libs/caramba-shared` — shared crates.
- `apps/caramba-app` — React/TS Telegram Mini App (out of scope for this work).
- Production: ~20 real users, panel lives on `poland` (OVH Debian+sudo).
  Nodes: `germany`, `canada`, `veles`. **NEVER** build locally + scp — CI does
  release builds (musl, LTO, opt-level z). See `AGENTS.md` for the
  post-incident runbook.

## 2. Current state (verified 2026-07-21)

- **`v0.9.52` is deployed on poland** with the BROKEN paypalych.rs. The
  provider is in code but **would 100% fail in production** because the
  request format, response parsing, webhook field names, and signature
  scheme are all wrong. The user has not yet enabled it in admin UI
  (waiting for moderation on pally.info to pass), so no real damage yet.
- The settings UI for Paypalych exists in
  `apps/caramba-panel/templates/settings.html` (card with 3 fields:
  API Token, Shop ID, Webhook Secret). **Keep that UI as-is** but
  change the "Webhook Secret" label to make clear it's actually the
  API Token (since Pal24 uses the Bearer token as the signing key —
  there is no separate webhook secret).
- The marketplace service wires it up
  (`apps/caramba-panel/src/services/marketplace_service.rs` —
  `PaypalychProvider` is registered when `paypalych_api_token` is
  non-empty).
- The test-connection handler in
  `apps/caramba-panel/src/handlers/admin/payments.rs` has a
  `paypalych` arm (works against the current broken `create_invoice`).
- The webhook route in
  `apps/caramba-panel/src/api/webhooks.rs` extracts signature from
  header `["Sign", "sign", "Signature", "X-Sign", "x-sign"]`. **This
  is wrong — signature is in the body as `SignatureValue`.** Remove
  the header lookup for paypalych; the body itself is the signed
  payload.

## 3. The Pal24 API contract (the source of truth)

**Full spec:** `docs/PAYPALYCH-API-SPEC.md` (read this — it has all
the field tables).

**Critical facts you MUST get right (your code currently gets them all
wrong):**

1. **`POST /api/v1/bill/create` body is `multipart/form-data` (or
   `application/x-www-form-urlencoded`), NOT JSON.** Use `.form(&params)`
   on the reqwest builder, not `.json(&body)`.

2. **Field names** (multipart keys):
   - `amount` (decimal, required) — e.g. `100.05`
   - `shop_id` (string, required) — comes from the user's project in pally.info
   - `order_id` (string, optional) — pass our `PaymentSession::id` as string
   - `currency_in` (string, optional, default RUB) — **NOT** `currency`
   - `type` (string, optional) — `normal` (one-shot) or `multi`. Pass `normal`.
   - `description` (string, optional) — shown on payment form
   - `name` (string, optional) — also shown on form
   - `custom` (string, optional) — returned in webhook
   - DO NOT send `success_url`, `fail_url`, `hook_url`, `expire` —
     those are configured in the pally.info dashboard for the project.
     If you send them, Pal24 may use them to override the dashboard
     config (worse than nothing if you get them wrong).

3. **Response format** (JSON):
   ```json
   {
     "success": "true",       ← STRING, not boolean!
     "link_url": "https://pally.info/link/...",
     "link_page_url": "https://pally.info/transfer/...",
     "bill_id": "..."
   }
   ```
   Use `link_page_url` as the URL the user gets redirected to. Parse
   `success` as `String` and compare to `"true"`.

4. **Webhook (postback) is sent as
   `application/x-www-form-urlencoded` POST to the Result URL** with
   these field names (PascalCase, NOT snake_case):
   - `Status` — `"SUCCESS"` or `"FAIL"`
   - `InvId` — our `order_id` from the bill/create
   - `OutSum` — the amount
   - `CurrencyIn` — the currency
   - `Commission` — Pal24's commission
   - `TrsId` — Pal24's transaction id
   - `custom` — echoed from our request
   - `SignatureValue` — **signature is in the BODY, not a header!**

5. **Signature algorithm: `strtoupper(md5(OutSum + ":" + InvId + ":" + apiToken))`**
   - Plain MD5 (NOT HMAC-SHA256).
   - Key is the **Bearer token itself** (NOT a separate webhook secret).
   - Output: uppercase hex, 32 chars.
   - To verify: re-compute and compare constant-time.

## 4. What to change

### 4.1. `apps/caramba-panel/src/services/payment/paypalych.rs` — full rewrite

Keep the file structure and the `#[async_trait] impl PaymentProvider`
contract. The methods need:

**`create_invoice`**:
- Convert `session.amount` (kopecks) to rubles: `(session.amount as f64) / 100.0`
- Build a `Vec<(&str, String)>` of form fields:
  - `amount` = formatted rubles (e.g. `"100.05"`, not integer)
  - `shop_id` = `self.shop_id` (if empty, the docs say it falls back to
    the project's default — but the spec also says Success/Fail/Result
    URLs won't work without it. **Always send it** if the user
    provided one; error out clearly if empty.)
  - `order_id` = `session.id.to_string()`
  - `currency_in` = `"RUB"` (Pal24 is RU-only with RUB invoices;
    customer picks SBP/USDT on Pal24's side)
  - `type` = `"normal"` (one-shot per subscription)
  - `description` = `format!("VPN subscription (product {})", session.product_id)`
  - `name` = same as description or shorter
  - `custom` = `format!("plan:{}", session.product_id)` (so we can
    cross-check in the webhook)
- POST to `https://pal24.pro/api/v1/bill/create` with
  `.bearer_auth(&self.api_token).form(&params).send()`
- Parse response JSON: `success: Option<String>` (STRING!), then
  `data: { link_page_url: Option<String>, bill_id: Option<String> }` if
  `success == "true"`. Or directly `{ success, link_url, link_page_url, bill_id }`
  at the top level (the spec example shows top-level, not nested in
  `data` — so use top-level deserialization).
- Return `link_page_url` to the caller. If missing, error.

**`verify_webhook`**:
- The signature is in the **body**, not a header. But our handler
  passes `payload: &[u8]` and `signature: &str` — the
  `apps/caramba-panel/src/api/webhooks.rs` extracts the header.
  **You need to change webhooks.rs too** so that for "paypalych" it
  reads the signature from the parsed body's `SignatureValue` field.
- See section 4.3 below for the webhooks.rs change.
- For now, keep the trait signature; the body bytes still come
  through as `payload`. But since you can't parse it in
  `verify_webhook` without going against the trait design, the
  cleaner approach is:
  - Modify `webhooks.rs` to **parse the body as form-urlencoded
    upfront** for paypalych, and call `verify_webhook` with the
    parsed fields (or restructure to a new trait method).
  - **OR** add a new method to `PaymentProvider`:
    `async fn verify_form_webhook(&self, fields: &HashMap<String, String>) -> Result<bool>`
    and call that for paypalych. Default impl = `Ok(false)` for
    providers that don't override.
  - **Recommended approach** to minimize churn: extend
    `PaymentProvider` trait with a new optional method
    `verify_form_webhook`, have Lava / WATA / etc. inherit the
    default (no-op Ok(false) — they don't get the form path), and
    implement it for paypalych. Or keep it simple: just add a
    `verify_form_webhook` method (no default) and implement on the
    PaymentProvider trait. See step 4.3 for the simpler
    webhooks.rs-side approach.

  **Pick the simpler one:** Modify `webhooks.rs` so for
  provider=="paypalych" it does NOT read a signature header
  (empty), and instead **parses the body as form-urlencoded once**
  before calling `handle_webhook`. Then in
  `paypalych::verify_webhook`, do a parse of the body, extract
  `OutSum`/`InvId`/`SignatureValue`, and verify the MD5.

  Even simpler: change `verify_webhook` to ALWAYS parse the body
  for paypalych (since we know it's form-urlencoded, and the
  handler already passes the raw bytes). This keeps the trait
  stable.

  Pick whichever is cleanest — your call, but document it in a
  comment in the file.

**`handle_webhook`**:
- Parse body as form-urlencoded (`urlencoding` crate is already a
  transitive dep via `reqwest`; if not, add it to
  `apps/caramba-panel/Cargo.toml`).
- Extract: `Status`, `InvId` (this is our `order_id`),
  `SignatureValue` (optional).
- Map `Status`:
  - `"SUCCESS"` → `Completed { external_id: InvId }`
  - `"FAIL"` → `Failed { reason: "FAIL" }` (you can also include
    other reason text if Pal24 adds fields later, but spec only
    shows SUCCESS/FAIL)
  - anything else (NEW, MODERATING, etc.) → `Pending`

**`check_status`**:
- Now we can implement a real polling check via
  `GET /api/v1/bill/status?id={bill_id}`.
- If `bill_id` is in the session metadata (we should store it in
  `session.metadata` from `create_invoice` — see step 4.5), call
  that endpoint and return `"paid"` / `"failed"` / `"pending"`
  per the normalized contract in `provider.rs`.
- If `bill_id` is not in metadata (sessions created before this
  rewrite), fall back to returning `"pending"` so the poller
  doesn't break.
- The `MarketplaceService::handle_webhook` action flow
  already handles `Completed`, but if you want the
  under/over-pay guard you can also use
  `CompletedWithAmount { external_id, paid_amount_minor, paid_currency }`
  — set `paid_amount_minor = (OutSum parsed as f64 * 100.0) as i64`
  and `paid_currency = "RUB"`. This is a nice-to-have; `Completed`
  is fine for v0.9.53.

### 4.2. `apps/caramba-panel/src/services/payment/mod.rs`

Add `pub mod paypalych;` (already there — confirm).

### 4.3. `apps/caramba-panel/src/api/webhooks.rs`

In the `handle_payment_webhook` function, change the
`"paypalych" => header(...)` arm to return `String::new()` (no
header). Add a comment explaining that paypalych signs the body
itself, and the signature is verified inside the provider's
`verify_webhook` by parsing the form-urlencoded body.

### 4.4. `apps/caramba-panel/src/handlers/admin/payments.rs`

The existing `"paypalych" => { ... }` arm is structurally correct
(creates a `PaypalychProvider` and calls `create_invoice`). After
the rewrite, it should work as-is because `create_invoice` returns
`String` either way. Just confirm the arm still compiles.

### 4.5. MarketplaceService — store bill_id in session metadata

To enable polling via `check_status`, the bill_id returned by
`create_invoice` needs to be stored. The cleanest place is
`session.metadata` (a `serde_json::Value`). Look at how other
providers do it (search for `metadata` in
`apps/caramba-panel/src/services/marketplace_service.rs::create_session`).
Extend `create_session` to accept an optional `provider_metadata`
(serde_json::Map<String, Value>) that gets merged into the
session's metadata column. Then in `paypalych::create_invoice`,
you can save `{ "bill_id": "..." }` in the metadata. The
`check_status` impl reads it back.

If this is too invasive, skip it for v0.9.53 and have
`check_status` always return `"pending"` (current behavior). Add
a TODO comment. Polling-fallback is not on the critical path
(webhook works fine for the happy path); the user can live
without it for one release.

### 4.6. `apps/caramba-panel/src/handlers/admin/settings.html`

The card already exists with 3 fields. The label "Webhook Secret"
is now misleading — Pal24 uses the **API token** as the signing
key, so there's no separate secret. Options:

- Rename "Webhook Secret" to "API token (also used to verify
  webhooks)" — clearer.
- OR: keep the field, but rename label to "Webhook signing key
  (use API token)" and add a help text.

Either way, update `templates/payment_docs/paypalych.html` to
match.

### 4.7. `docs/PAYPALYCH-MODERATION.md` and `docs/PAYPALYCH-API-SPEC.md`

- `MODERATION.md` step 4 talks about "Webhook Secret" — update
  to clarify: the Bearer token is the signing key. There is no
  separate secret to set in pally.info.
- `API-SPEC.md` is the source of truth — keep it as-is, but
  double-check it matches your final implementation.

## 5. Quality gates (must pass before any commit)

```bash
cd /Users/smtcprdx/Documents/Projects/caramba
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If you add a new dep (e.g. `urlencoding`), check it's not already
transitive before adding — `cargo tree -i <crate>` will tell you.

## 6. Commit + ship

- One commit: `fix(payments): rewrite paypalych provider to match real pal24.pro API spec`
- Bump version: `0.9.52 → 0.9.53` in all 8 Cargo.toml files.
- One more commit: `chore: bump version to 0.9.53 for release`.
- Push to main, push tag `v0.9.53`. CI builds (~10-15 min on
  musl). Watch via `gh run list --workflow=release.yml --limit=1`.
- **Deploy only on poland** (the panel). The 3 nodes don't
  change.

**Important:** do **NOT** delete the v0.9.52 tag or rebase. Just
add 2 new commits on top of `82bdf8f` (current main HEAD).

## 7. Acceptance

- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is
  clean
- [ ] `cargo test --workspace` passes (no test regressions)
- [ ] When the user pastes a real Bearer token and shop_id in
  the admin UI, the "Проверить подключение" button returns
  `{"ok": true, "message": "..."}` (not an error).
- [ ] A real payment via Pal24 (the user will test once
  moderation passes) produces a webhook that the panel
  accepts (signature matches) and the subscription activates.

## 8. Files you'll likely touch

```
apps/caramba-panel/Cargo.toml                      (if adding urlencoding)
apps/caramba-panel/src/services/payment/paypalych.rs           (full rewrite)
apps/caramba-panel/src/services/payment/mod.rs                 (confirm mod is registered)
apps/caramba-panel/src/api/webhooks.rs                         (header lookup removal)
apps/caramba-panel/src/handlers/admin/settings.html            (label tweak)
apps/caramba-panel/templates/payment_docs/paypalych.html       (label tweak)
docs/PAYPALYCH-MODERATION.md                       (small clarification)
apps/*/Cargo.toml libs/*/Cargo.toml                (bump 0.9.52 → 0.9.53)
```

Optional (only if you implement the polling-fallback):

```
apps/caramba-panel/src/services/marketplace_service.rs          (accept provider_metadata in create_session)
apps/caramba-panel/src/services/payment/provider.rs             (CompletedWithAmount already exists)
```

## 9. After deploying v0.9.53

Give the user:
- 1-line confirmation that the rewrite shipped.
- A note that they should:
  1. Open `/admino4ka/settings` → Paypalych card → "Проверить
     подключение" (now using real API).
  2. When pally.info moderation passes, paste the real
     Bearer token + shop_id, save, test.
  3. If the test connection returns 4xx, share the error
     text — there might be a field name I still got wrong
     and we iterate.
- Reminder that the user does the actual deploy via
  `sudo caramba upgrade --to v0.9.53` on poland (per the
  caramba convention — you don't have admin auth).

---

That's it. Go.
