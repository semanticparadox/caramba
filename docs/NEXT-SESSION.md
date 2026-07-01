# Caramba Connect: instructions for the next session

Last session date: 2026-06-21. Branch: test/payments-production. Everything is
uncommitted. Read this first, then the files it points to.

## Honest state right now

- The B2B pivot (P0 through P7 from docs/CARAMBA-CONNECT-PLAN.md) is written in
  source and adversarially reviewed, but it is STATIC ONLY. Nothing was compiled,
  bound, or signed. No cargo, no go, no flutter, no dart, no sqlx in the sandbox.
- The one thing actually executed: all 29 database migrations were run on a local
  throwaway Postgres 17. That found and fixed one real bug
  (libs/caramba-db/migrations/20260622000000_referral_money_reward.sql, a
  name[] vs text[] comparison that aborted the whole chain). The 4 new migrations
  are idempotent and org_backfill works on real data. The prod DB was NOT touched.
- The HTML file apps/caramba-client/demo/caramba-connect-demo.html is only a
  visual prototype. The user is not satisfied with it. It is NOT the real app and
  has no bearing on the actual Flutter client code under apps/caramba-client/lib.
- Full per-phase record and the complete honest checklist:
  docs/CARAMBA-CONNECT-STATUS.md (sections 4 and 6 are the live-work list).
  Store-review guidance: docs/STORE-COMPLIANCE.md.

## The single biggest next step: build it for real

The functionality has never met a compiler. Priority one is standing up the
toolchains and fixing the real errors that surface. Expect the first compile to
be red. Order:

1. Rust panel + workspace
   - cargo check --workspace, then cargo clippy, then cargo test.
   - Run all migrations on a real (ideally a copy of prod) Postgres through the
     panel sqlx runner, not just psql. Then sqlx prepare for the new queries
     (enrollment_codes, brand_settings, license_state, org_backfill, onboarding
     columns, license_repo).
   - New crate apps/caramba-license must build and join the workspace.
2. Go core
   - cd libs/caramba-core && go mod tidy (needs network and a module proxy).
   - go build ./... and go test ./... for the default (stub) build.
   - go build -tags mihomo and the gomobile / cgo paths to verify the new
     subimport package and ImportSubscription against the real mihomo graph.
3. Flutter client
   - cd apps/caramba-client && flutter create . then flutter pub get then
     flutter analyze. Fix analyzer errors in lib/ (the new connection_profiles,
     enroll, branding code has never been analyzed).
   - Run on at least one device with the mock VPN, then wire the native path per
     apps/caramba-client/INTEGRATION.md.

## Deferred code follow-ups (small, named, written down)

These were consciously left open last session. Each says where it lives.

- Enroll anti-phishing: carambaconnect://enroll routes straight to the credential
  form and accepts plaintext http. Add a confirm step that shows the panel host
  before sending credentials, and reject http. Lives in
  apps/caramba-client/lib/features/enroll/ (enroll_controller.dart, enroll_screen.dart)
  and lib/data/models/enrollment.dart (normalizePanelUrl).
- Login by code cannot carry an enroll code (panel LoginCodeRequest has no field).
  Reword the misleading Flutter docstrings or add panel support. Lives in
  apps/caramba-client/lib/data/api_client.dart and apps/caramba-panel/src/api/v2/app_auth.rs.
- Panel user-create org hook is deferred, so new users get no organization_members
  row until org_backfill is re-run. Add an idempotent default-org join on user
  create. Lives in the panel user-create path.
- License grace is wall-clock based; a clock rollback can extend the 14-day window.
  Add a monotonic or last-seen-floor guard. Lives in
  apps/caramba-panel/src/license/mod.rs.
- DEFAULT_LICENSE_PUBKEY ships empty by design. To exercise Pro locally you must
  run apps/caramba-license, generate an ed25519 keypair with its keygen CLI, and
  set CARAMBA_LICENSE_PUBKEY, CARAMBA_LICENSE_SERVER_URL, CARAMBA_INSTANCE_ID.

## The demo (only if the user still wants it)

The user did not like apps/caramba-client/demo/caramba-connect-demo.html. Do not
keep polishing the HTML prototype unless asked. If the goal is to see the product,
the better path is to run the real Flutter app (step 3 above) rather than iterate
the static demo. If the demo is still wanted, it now has in-app bottom navigation
(Главная / Подключения / Профиль) and the demo controls sit below the phone.

How to render the demo headless for review (the sandbox has no Chrome channel for
the playwright MCP, so drive bundled chromium directly):
- chromium was installed at ~/Library/Caches/ms-playwright/chromium_headless_shell-1228.
- Use a small node script that requires playwright from
  ~/.npm/_npx/<hash>/node_modules/playwright and launches with executablePath set
  to that chrome-headless-shell binary, then page.goto the file:// URL and
  page.screenshot. (Last session used /tmp/shoot*.js, now deleted.)

## How to resume

1. Read the memory note caramba-connect-platform (auto-loaded) for the full build
   record and the locked decisions.
2. Read docs/CARAMBA-CONNECT-PLAN.md (the executable spec) and
   docs/CARAMBA-CONNECT-STATUS.md (what is done vs not).
3. Decide with the user: commit the current uncommitted work first, or go straight
   to the toolchain build. Nothing is committed yet, so a checkpoint commit on the
   branch is reasonable before the first real build churns many files.

## Environment notes for next time

- Available in this sandbox: psql (Postgres 17), node, npm, openssl. NOT available:
  cargo, rustc, go, flutter, dart, sqlx. If the next session is on the same box,
  the real build still needs those installed first.
- The migration smoke test that found the bug: create a throwaway DB, apply every
  file in libs/caramba-db/migrations in filename order with psql -v ON_ERROR_STOP=1,
  then re-apply the new ones to check idempotency, then drop the DB. Do not run
  against prod.
