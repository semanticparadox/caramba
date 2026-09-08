# Caramba Connect status

Date: 2026-06-21

This document records the current state of the Caramba Connect work. It is honest about scope: every phase below was written and reviewed statically. Nothing in this run was compiled, no Go bindings were built, no migrations were run, no binary was signed, and no store submission was made. Section 4 lists everything that still requires the user to run live toolchains, hardware, signing, license-server hosting, or store publication.

Naming rule used throughout: the user-facing default brand is Caramba Connect. The first tenant is exarobot. Per-instance brand is operator config. Code identifiers stay `caramba`, and the routing contract `profile.CarambaSelector = "CARAMBA"` is untouched.

## 1. Summary

Caramba Connect is the B2B form of the existing product. The same codebase is sold as a self-hosted panel instance that an operator runs for their own users. Three parts make this work:

- Universal client (Flutter + the Go `caramba-core` engine). It connects from two sources: a raw subscription (paste, URL, QR stub, file stub) or a panel account (enroll or login against a Caramba Connect panel). Import covers 14 protocol mappings and several subscription formats.
- Self-hosted licensed panel (Rust). The panel runs Free by default and never blocks existing users. A signed ed25519 license raises the panel to Pro limits (more nodes, more users, end-user billing, custom branding). License verification is offline-tolerant with a 14-day grace and a soft degrade that does not cut off live users.
- Per-instance Telegram bot (Rust). The bot is env-driven (one token, one panel URL per instance) and resolves its brand at render time from the panel `brand_name` setting, defaulting to Caramba Connect.

B2B pivot state: the seams exist and are wired end to end in source. The single live tenant is exarobot with roughly 20 active users. The org model is single-tenant v1 (one default org, all users backfilled into it); there is no per-org scoping yet. To actually sell instances the user must complete the live steps in Section 4 (host the license server, generate keys, build and sign the client, publish or sideload).

## 2. Per-phase build record (P0..P7)

Each phase was recon, then build, then adversarial review, then fix. All blocking findings were closed in source. Static only.

### P0 Rebrand
Built: `apps/caramba-client/lib/data/brand.dart` with `kBrandName` defaulting to Caramba Connect via dart-define, exarobot tenant-1 constants, and a `LicenseTier`/`Limits` matrix (Free 2 nodes / 100 users, Pro 1000 / unlimited). App title, splash, login, home, and referrals screens were rewired to `kBrandName`. The naming section was added to `apps/caramba-client/ANTI-SLOP.md`.
Known gaps: none blocking.

### P1 Universal client
Built (Go): `libs/caramba-core/subimport/` (`subimport.go`, `clash.go`, `singbox.go`, `v2ray.go`, `uri.go`, `detect.go`, `common.go`, `subimport_test.go`). `Import(raw, format)` auto-detects clash / singbox / v2ray / uri, with 14 protocol mappings (VLESS Reality/WS/HTTPUpgrade/gRPC/TCP, VMess, Trojan, Shadowsocks + ShadowTLS, Hysteria2, TUIC v5, AmneziaWG/WireGuard, NaiveProxy). `marshalClash` synthesizes the CARAMBA selector group. `libs/caramba-core/api/api.go` adds a raw-config source. `libs/caramba-core/mobile/mobile.go` adds `ImportSubscription` plus the ffi entry.
Built (Flutter): `ConnectionProfile` model (`lib/data/models/connection_profile.dart`), store (`lib/data/connection_profiles_store.dart`), connection screens (`lib/features/connections/connection_import_screen.dart`, `connections_screen.dart`), and vpn-wire `connectRaw` plus the Mock path for both source types.
Known gaps: import-by-URL fetch is wired in Flutter; import-by-QR and import-by-file are stubs (no scanner/picker dependency, no camera or storage permission requested). The audit confirmed `subimport` treats subscriptions strictly as config data: no `os/exec`, `plugin`, `reflect`, `unsafe`, `syscall`, or eval, and it never runs the mihomo core. URL import fetches with plain response type and feeds the body to the parser as text only.

### P2 Enrollment and onboarding
Built: migration `libs/caramba-db/migrations/20260623000000_enrollment_codes.sql` (`enrollment_codes` with code / max_uses / used_count / expires_at, atomic `FOR UPDATE` redeem), a `settings` onboarding-traffic value, and a `subscriptions.onboarding_bonus_bytes` one-time grant that leaves existing users unaffected. Public validate endpoint `GET /api/v2/app/enroll/{code}` in `apps/caramba-panel/src/api/v2/app_enroll.rs`. The `enroll_code` is threaded into register atomically (pre-validate, then single transaction). Flutter `carambaconnect://` deeplink, enroll screen, and register/login flows in `lib/features/enroll/enroll_controller.dart` and `enroll_screen.dart`, with the model in `lib/data/models/enrollment.dart`.
Known gaps: the enroll deeplink has no panel-host confirmation step before sending credentials (anti-phishing follow-up; see Section 5). `EnrollLink.normalizePanelUrl` accepts both http and https, so a deeplink can target a cleartext host. The login/code path cannot carry an enroll code; only telegram-login and register consume it.

### P3 Branding
Built: public `GET /api/v2/app/branding` in `apps/caramba-panel/src/api/v2/app_branding.rs` (enabled / brand_name / logo_url / accent_hex / support_url / bot_url / upstream_ads), gated by license limits. License seam at `apps/caramba-panel/src/license/mod.rs`. Brand stored as `settings` keys `brand_*`. Bot admin brand commands write them through panel `POST /api/v2/bot/settings/{key}` with a 6-key write allowlist plus cache update, and a fixed read allowlist. Flutter runtime theming from the active panel profile (`lib/features/branding/branding.dart`, `brand_wordmark.dart`, `powered_by.dart`), with an accent clamp that rejects purple/violet/indigo (hue >= 240) and never colors status. The Free tier shows a first-party "Powered by Caramba Connect" upsell card with one quiet learn-more link; it hides on Pro.
Known gaps: none blocking. The audit confirmed the storage-to-response key rename is consistent end to end (`brand_logo_url` -> `logo_url`, etc.) and the Free upsell is first-party with no third-party ad or tracker SDK.

### P4 Licensing
Built (shared): `libs/caramba-shared/src/license.rs` (`LicenseTier`, `LicenseLimits`, `Activation` types, instance-bound `canonical_message`, ed25519-dalek sign/verify).
Built (server): `apps/caramba-license/` binary (`main.rs`, `activate.rs`, `keys.rs`, `state.rs`, `store.rs`, `cli.rs`, `README.md`) with `POST /v1/activate`, key store, instance binding, expiry, signed responses, and an issue-key CLI.
Built (panel): `apps/caramba-panel/src/license/activation.rs` and `mod.rs` reading env `CARAMBA_LICENSE_KEY` / `CARAMBA_INSTANCE_ID` / `CARAMBA_LICENSE_SERVER_URL` / `CARAMBA_LICENSE_PUBKEY`, verifying signatures, caching to a `license_state` table (migration `20260625000000_license_state.sql`), re-verifying every 12 to 24 hours, with a 14-day offline grace and a soft degrade that never blocks existing users. Effective tier comes from the verified state, falling back to Free when no key is set. Enforcement covers `max_nodes`, `max_users`, end-user billing on purchase, branding via limits, and `manual_approval` wired into live promo / gift / paid issuance plus an approve-subscription admin route.
Known gaps: a self-hoster can self-issue Pro by running their own license server with their own keypair (inherent to a signed, self-hosted model with no central enforcement; documented). `DEFAULT_LICENSE_PUBKEY` ships empty by design, so with no key the panel runs Free. The 14-day grace is wall-clock based, so a clock rollback could extend it (blast radius limited by soft degrade). The security review found 0 critical issues (no signature bypass, replay, or tamper).

### P5 Per-instance bot
Built: `apps/caramba-bot/` is env-driven (`BOT_TOKEN`, `PANEL_URL`). Brand is resolved from the `brand_name` setting (default Caramba Connect) by render-time substitution in `apps/caramba-bot/src/bot/translations.rs` and `apps/caramba-bot/src/bot/handlers/callback.rs`. The bot uses a `{brand}` named placeholder.
Known gaps: the P5 note about a "technical CARAMBA contract label left intact in the bot" is slightly misplaced. The audit confirmed no `CARAMBA` literal exists in `apps/caramba-bot/src`; the CARAMBA selector contract lives only in `libs/caramba-core` (`profile.CarambaSelector`), which P5 correctly did not touch. Future audits should look there, not in the bot.

### P6 Org backfill
Built: migration `libs/caramba-db/migrations/20260626000000_org_backfill.sql`, an idempotent DO block that seeds a default org with slug exarobot, backfills `organization_members` for all users, and sets `subscriptions.organization_id` where NULL. Single-tenant v1, no per-org scoping.
Known gaps: the panel user-create hook that adds new users to the default org is deferred, so new users get no org row until the migration is re-run.

### P7 Adversarial audit pass and fixes
This phase ran the cross-layer, store-compliance, and security reviews and applied the small, additive, low-risk fixes. Fixes applied in source:
- `apps/caramba-license/src/store.rs`: the keystore is now written 0600 on unix (temp file chmod before rename, mirroring `keys.rs`), so the committed inode is never group/world-readable. Unix-only; no equivalent ACL hardening on non-unix hosts.
- `apps/caramba-bot/src/bot/handlers/callback.rs` (get_config render site): `brand_name` is now trimmed and falls back to the default brand on empty or whitespace, matching the guard already in `app_branding.rs`.
- `libs/caramba-db/migrations/20260626000000_org_backfill.sql`: the block now raises an exception if the default org id is NULL after the seed, so a failed seed surfaces as a migration error instead of a silent no-op. The whole block is one transaction, so the raise rolls back any partial work.
Audit verdict: PASS with documentation gaps. No new critical findings. The remaining store-compliance work is documentation-only (see Section 4 and Section 5).
Known gaps deferred from this pass: enroll anti-phishing host-confirm gate (and rejecting plaintext http panel URLs); login/code enroll path; panel user-create org hook; grace clock monotonic guard; the iOS privacy manifest, Play Data Safety, and store positioning docs do not exist yet.

## 3. Contracts pinned

These are the new surfaces. The audit verified each is aligned across layers.

Endpoints (panel):
- `GET /api/v2/app/enroll/{code}` (public): validates an enrollment code. Returns a generic `reason = "invalid"` on failure by design, to avoid leaking expired vs exhausted vs unknown through enumeration.
- `GET /api/v2/app/branding` (public): returns enabled / brand_name / logo_url / accent_hex / support_url / bot_url / upstream_ads, gated by license limits. Storage keys `brand_logo_url` / `brand_accent_hex` / `brand_support_url` / `brand_bot_url` are renamed to `logo_url` / `accent_hex` / `support_url` / `bot_url` in the response; `brand_name` maps 1:1.
- `POST /api/v2/bot/settings/{key}`: bot-authenticated brand write, 6-key write allowlist plus cache update, fixed read allowlist.
- Register accepts an `enroll_code` field, redeemed atomically inside the register transaction.

Endpoint (license server):
- `POST /v1/activate`: instance-bound activation, signed ed25519 response. Canonical message binds the instance id so a response cannot be replayed onto another instance.

Env (license, identical names across installer writer, panel reader, and the panel `ENV_*` constants):
- `CARAMBA_LICENSE_KEY`, `CARAMBA_INSTANCE_ID`, `CARAMBA_LICENSE_SERVER_URL`, `CARAMBA_LICENSE_PUBKEY` (panel side). `DEFAULT_LICENSE_PUBKEY` ships empty.

Deeplink (client):
- `carambaconnect://enroll?panel=<host>&code=<code>`: opens the enroll flow. The panel host is displayed on the account step but is not yet behind a deliberate confirm gate (see Section 5).

Import (client to core):
- `ImportSubscription(raw, format)` in `libs/caramba-core/mobile/mobile.go` equals the FFI entry equals the native `importRawProfile` equals the Flutter call. Format is always `auto` from the client.

Contract drift recorded (doc-level only, no behavioral break for live users):
- Flutter `loginCode` (`apps/caramba-client/lib/data/api_client.dart`) sends `enroll_code` and its docstring claims the panel redeems it, but the panel `LoginCodeRequest` DTO has no such field and the handler never reads it. There is no `deny_unknown_fields`, so the extra field is silently dropped. The enroll-on-login-code path is a no-op. The docstrings in `api_client.dart` and `lib/features/enroll/enroll_controller.dart` should be reworded to say only telegram-login and register consume enroll codes.
- `lib/data/models/enrollment.dart` documents reason values `expired` / `exhausted` / `unknown`, but the panel only ever emits `invalid`. The client tolerates any string. The documented enum is unreachable by design.

## 4. Honest list: what still requires the user

Nothing below was done in this run. This is the live work, as a checklist.

Rust build and database:
- [ ] `cargo check`, `cargo clippy`, `cargo test` across the workspace.
- [x] Validated locally: the full 29-migration chain applies clean on a throwaway Postgres 17, the 4 new migrations (`enrollment_codes`, `brand_settings`, `license_state`, `org_backfill`) are idempotent on re-run, and `org_backfill` correctly makes every user a member of the default org. This run found and fixed one real bug in `20260622000000_referral_money_reward.sql` (a `name[] = text[]` comparison that aborted the chain; `att.attname` is now cast to text). NOTE: this used psql directly on a fresh DB, not the sqlx runner against prod.
- [ ] Run the migrations on the live prod database through the sqlx runner (the panel runs `sqlx::migrate!` on startup), against the existing ~20-user schema.
- [ ] `sqlx prepare` for the new queries against that database.

Go bindings:
- [ ] `cd libs/caramba-core && go mod tidy`.
- [ ] `gomobile bind` and cgo c-shared with `-tags mihomo` to verify the new `subimport`, `ImportSubscription`, and ffi against the real mihomo graph.

Flutter:
- [ ] `flutter create .`, `flutter pub get`, `flutter analyze`.
- [ ] Per-platform edits in `apps/caramba-client/INTEGRATION.md`, then build and run.

Signing, consent, and platform privileges (all documented in `apps/caramba-client/INTEGRATION.md`):
- [ ] iOS: Apple developer account, Network Extension Packet Tunnel target, Network Extensions capability, provisioning.
- [ ] macOS: System Extension and networkextension entitlement.
- [ ] Android: VPN consent, `FOREGROUND_SERVICE_SPECIAL_USE` with a Play justification, `foregroundServiceType=specialUse`.
- [ ] Windows: requireAdministrator and wintun.dll. Linux: root or CAP_NET_ADMIN.

Store compliance docs:
- [x] Written: `docs/STORE-COMPLIANCE.md` covers iOS `PrivacyInfo.xcprivacy` (no tracking, nothing collected, required-reason API reasons), Play Data Safety (no collection or sharing, no analytics or ad SDK), the no-dynamic-code argument, the VPN entitlement summary, the first-party upsell, the future camera/file permission note, and the fallback distribution plan.
- [ ] The operator still applies the actual `PrivacyInfo.xcprivacy` file and fills the live Play Data Safety form from that doc at submission time, and confirms the required-reason API set against the final linked dependencies.

License server hosting:
- [ ] Run `apps/caramba-license`, generate an ed25519 keypair with its keygen CLI, and set `CARAMBA_LICENSE_PUBKEY`, `CARAMBA_LICENSE_SERVER_URL`, and `CARAMBA_INSTANCE_ID` in the installer and `.env`. With no key the panel stays Free and the live users are unaffected.

Operator nodes and protocols:
- [ ] Stand up nodes with the real protocols. For AmneziaWG, the node needs an AmneziaWG-capable server (see `docs/AMNEZIAWG.md`).

Publication:
- [ ] Store publication, accepting the review risk for a universal panel client, with sideload or AltStore as a fallback.

## 5. Known limitations and deferred follow-ups

Each item lists where it lives.

- Enroll anti-phishing host confirmation (top security follow-up). A `carambaconnect://enroll` deeplink routes straight to the credential form, which POSTs email and password to the panel host from the link. The host is shown but there is no deliberate confirm gate, and `EnrollLink.normalizePanelUrl` accepts plaintext http. A clean fix needs a new enroll stage between valid and credential entry plus http rejection, which is more than a small additive tweak. Lives in `apps/caramba-client/lib/features/enroll/enroll_controller.dart`, `enroll_screen.dart`, and `lib/data/models/enrollment.dart` (`normalizePanelUrl` at the http/https acceptance point).
- Login/code cannot carry an enroll code. The panel `LoginCodeRequest` has no field for it and the handler ignores the one Flutter sends. Doc reword needed in `apps/caramba-client/lib/data/api_client.dart` and `lib/features/enroll/enroll_controller.dart`. Lives in `apps/caramba-panel/src/api/v2/app_auth.rs`.
- Enroll reason enum is collapsed server-side to `invalid` to prevent state enumeration. The Flutter doc in `lib/data/models/enrollment.dart` still lists the unreachable values.
- Panel user-create org hook is deferred. New users get no `organization_members` row until `org_backfill` is re-run. Lives in the panel user-create path and `libs/caramba-db/migrations/20260626000000_org_backfill.sql`.
- License self-issue is inherent to the self-hosted signed model with no central enforcement. Documented; the control is contractual, not technical. Lives in `apps/caramba-license` and `apps/caramba-panel/src/license/`.
- License grace is wall-clock based; a clock rollback could extend the 14-day window. Soft degrade limits the impact. A monotonic or last-seen-floor guard is the harden step. Lives in `apps/caramba-panel/src/license/mod.rs`.
- License keystore is 0600 on unix only. No equivalent ACL hardening on non-unix hosts. Lives in `apps/caramba-license/src/store.rs`.
- `brand_name` written through `POST /api/v2/bot/settings` is not trimmed or empty-guarded at write time, unlike the bot brand command handler which trims at write. Two render sites now trim (`app_branding.rs`, `callback.rs` get_config). Other `get_or_default("brand_name", ...)` call sites (`client.rs`, `status.rs`, `app_enroll.rs`) were not reviewed for the empty-stored-value case in this pass.
- QR and file import are stubs. Store-safe now (no camera or storage permission requested). When implemented they will add permissions that must be declared in the iOS privacy manifest and the Play Data Safety form. Lives in `apps/caramba-client/lib/features/connections/connection_import_screen.dart`.
- Org model is single-tenant v1. No per-org scoping; one default org, all users backfilled. Lives in `libs/caramba-db/migrations/20260626000000_org_backfill.sql`.

## 6. What is verified vs not

Verified statically (source read, cross-layer alignment checked, anti-slop applied):
- Contract alignment across panel, client, bot, license server, and core for enroll, branding, license env and types, and import.
- No dynamic code execution in subscription import; no third-party analytics or ad SDK in the client.
- VPN entitlements for all five platforms are documented in `apps/caramba-client/INTEGRATION.md`.

Not done in this run (requires the user):
- No compile, no `cargo check`, no `sqlx prepare`.
- No Go bind, no cgo build against mihomo.
- No `flutter analyze`, no per-platform build or run.
- Migrations validated on a local throwaway Postgres 17 (full chain applies clean after one fix, the 4 new ones idempotent, org_backfill functional). NOT yet run on the live prod DB through the sqlx runner, and no sqlx prepare.
- No signing, no store submission, no license server hosting or keygen, no real nodes.
