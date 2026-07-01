# Building Caramba Connect (caramba monorepo)

Local build and test steps for every layer of the monorepo. CI mirrors these in
`.github/workflows/ci.yml` (the single owner of the Rust, Go and Flutter
contours); release artifacts are produced by `release.yml`.

> The user-facing brand is Caramba Connect. The first hosted tenant is exarobot
> (tenant #1). The product is the same universal client and panel sold as a
> self-hosted instance; per-tenant brand, panel URL and bot are operator config,
> not build identity.

> **CI is expected red until the first-build fixups land.** The Rust job runs
> `cargo fmt --check` and `cargo clippy -- -D warnings` against code that has
> never been compiled or linted (see "nothing has been compiled yet" below), so
> the first runs will fail on formatting and lint warnings (unused imports,
> dead_code, needless clones, etc.). This is intentional: the gate is armed from
> day one. Work the checklist at the end of this file to green, then trust CI.
> If you need a non-blocking introduction first, temporarily drop `-D warnings`
> from the clippy step in `ci.yml` (and/or split fmt/clippy into a
> `continue-on-error` job) and re-arm once the build is green.

> Code identifiers, directories, the Go module path and the `CARAMBA`
> proxy-group contract stay `caramba`. The `CARAMBA` selector lives in
> `libs/caramba-core` (`profile.CarambaSelector`); it is a panel to client
> contract, not a user-facing string, and is never rebranded. User-facing strings
> default to `Caramba Connect`; the brand is a runtime value, set per tenant.

## Layout

| Layer    | Where                                   | Toolchain        |
| -------- | --------------------------------------- | ---------------- |
| Rust     | workspace root `Cargo.toml`             | Rust, edition 2024 |
| License  | `apps/caramba-license`                  | Rust, edition 2024 |
| Go core  | `libs/caramba-core`                     | Go 1.22          |
| Go CLI   | `apps/caramba-cli`                      | Go 1.22          |
| Flutter  | `apps/caramba-client`                   | Flutter stable (>=3.29), Dart >=3.6 |
| Mini App | `apps/caramba-app` (React/TS)           | Node 22          |

`apps/caramba-license` is the license control plane (the signer). It is a
workspace member, so the Rust commands below build and test it with everything
else. The panel is the verifier and holds only the public key. The detailed
keygen, issue and serve flow lives in the "License control plane" section.

## Toolchain versions

- **Rust**: stable toolchain, **edition 2024** (`apps/caramba-panel`,
  `libs/caramba-db`, etc.). Install via `rustup`. The release profile is heavily
  optimized (LTO, `opt-level = "z"`, strip, single codegen unit), so use
  `cargo check` during dev; release builds are slow.
- **Go**: **1.22** for both `libs/caramba-core` and `apps/caramba-cli`.
- **Flutter**: **stable** channel (SDK constraint `>=3.6.0 <4.0.0`, Flutter
  `>=3.29.0`). Install via the Flutter SDK or `subosito/flutter-action` in CI.

---

## Rust workspace

```bash
# from repo root
cargo fmt --all --check          # formatting gate
cargo check --workspace          # fast type/borrow check (use this in dev)
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

System deps on a fresh Ubuntu box (matches CI / release.yml):

```bash
sudo apt-get update
sudo apt-get install -y libssl-dev pkg-config
# release.yml additionally needs cmake musl-tools for the musl target
```

### sqlx: offline vs live DB

The panel uses sqlx at **runtime** (`sqlx::query` / `sqlx::query_as`), **not**
compile-time macros (`query!` / `query_as!`), and there is no `.sqlx` offline
cache. `libs/caramba-db` uses `sqlx::migrate!("./migrations")`, which only embeds
the `.sql` files at compile time and does **not** open a DB connection.

Therefore **`cargo check`/`cargo test` build without a running Postgres.** CI
sets `SQLX_OFFLINE=true` defensively (a no-op today; it guards against a future
`query!` macro being added without a live DB). A live Postgres is only needed to
actually run the panel, not to build/test it.

### Applying migrations (runtime)

Migrations live in `libs/caramba-db/migrations/` (`<timestamp>_name.sql`) and are
run by the panel at startup via the embedded `sqlx::migrate!` migrator against
the configured database. To apply them manually with the sqlx CLI:

```bash
cargo install sqlx-cli --no-default-features --features postgres,rustls
export DATABASE_URL=postgres://user:pass@localhost:5432/caramba
sqlx migrate run --source libs/caramba-db/migrations
```

Migrations are applied in timestamp order and are idempotent where they backfill.
The universal-client and self-hosted work added several, in order:

- `20260623000000_enrollment_codes` enrollment codes (code, max_uses,
  used_count, expires_at) with atomic `FOR UPDATE` redeem, plus the onboarding
  traffic setting and the one-time onboarding bonus column on subscriptions.
- `20260624000000_brand_settings` the `brand_*` key/value settings rows that hold
  the per-instance brand name, logo, accent and support/bot URLs.
- `20260625000000_license_state` the single-row license cache the panel writes
  after a verified activation (tier, limits, expiry, last verified time).
- `20260626000000_org_backfill` idempotent default-org seed (slug `exarobot`) and
  membership/subscription backfill. The migration aborts the transaction if the
  default org cannot be resolved after seed, so a failed seed surfaces as a
  migration error rather than a silent no-op. New users created after the
  migration do not yet get a default-org row automatically (deferred follow-up).

The newest migration is `20260626000000_org_backfill`.

---

## License control plane (`apps/caramba-license`)

`caramba-license` is the signer side of licensing. It serves `POST /v1/activate`,
signs activation responses with an ed25519 key, and issues license keys for a
tier and duration. Free instances need no key and never call this server, so this
crate is only relevant if you sell or self-issue Pro instances. The panel side
(verification, cache, grace, enforcement) builds with the normal Rust workspace
commands above and needs no extra steps to compile.

Build and test it with the workspace (it is a member of the root `Cargo.toml`):

```bash
cargo check -p caramba-license
cargo test  -p caramba-license
```

### Generate the signing key (run once)

The control plane and the panel share one ed25519 keypair: the license server
holds the private key and signs, the panel holds the public key and verifies.
Generate the pair once with the bundled CLI:

```bash
cargo run -p caramba-license -- keygen --out /etc/caramba/license_signing_key.pem
```

This writes the private key as PKCS#8 PEM (owner read/write only on unix) and
prints `CARAMBA_LICENSE_PUBKEY` as base64. Keep the private key off the repo. The
printed base64 public key is what each panel instance sets as
`CARAMBA_LICENSE_PUBKEY`. Print it again at any time with
`cargo run -p caramba-license -- pubkey --signing-key <path>`.

Until the real platform public key is baked into the installer
(`DEFAULT_LICENSE_PUBKEY` in `apps/caramba-installer/src/setup.rs`, currently
empty), a fresh install has no pubkey, which is unverifiable and fails safe to
the Free tier. Bake the real public key before shipping a Pro-capable build.

### Issue a key and run the server

```bash
# issue a key (random CRMB-XXXX-... unless --key is passed)
cargo run -p caramba-license -- issue \
  --store /etc/caramba/keystore.json \
  --tier pro --days 365 --seats 1 --note "acme corp"

# run the activation server
cargo run -p caramba-license -- serve \
  --signing-key /etc/caramba/license_signing_key.pem \
  --store /etc/caramba/keystore.json \
  --bind 0.0.0.0:8088
```

`--seats 1` binds the first activating instance id and refuses later ones;
`--seats 0` is unlimited. The keystore is written `0600` on unix; there is no
equivalent ACL hardening on non-unix hosts. Full flag and contract details,
including the trust model and what the signature does and does not stop, live in
`apps/caramba-license/README.md`.

### Panel-side license env

The panel reads four env vars and verifies the signed activation on startup,
re-verifies every 12 to 24 hours, and caches the result in the `license_state`
table. With no key it stays Free and never contacts the server.

| Env var | Meaning |
| --- | --- |
| `CARAMBA_LICENSE_KEY` | the `CRMB-...` key for this instance (unset = Free) |
| `CARAMBA_INSTANCE_ID` | stable instance id bound at first activation |
| `CARAMBA_LICENSE_SERVER_URL` | base URL of the activation server |
| `CARAMBA_LICENSE_PUBKEY` | base64 ed25519 public key that signs are checked against |

If the server is unreachable, the panel serves its last verified tier for a 14
day offline grace window, then soft-degrades new privileged actions to Free
limits. Existing users, traffic and active subscriptions are never blocked by a
license state change.

---

## Go core / CLI: default build (no native core)

The default build uses a stub engine and does **not** pull in mihomo. It needs no
CGO and builds against the partial `go.sum` (only `yaml.v3` is actually
imported). This is what CI gates on.

```bash
# core
cd libs/caramba-core
go vet ./...
go build ./...
go test ./...

# cli (replaces ../../libs/caramba-core)
cd apps/caramba-cli
go vet ./...
go build ./...
go test ./...
```

## Go core: native core build (`-tags mihomo`)

The `mihomo` build tag wires in the real sing-box/mihomo engine
(`engine/engine_mihomo.go`, `autotune/prober_mihomo.go`, `api/prober_mihomo.go`).
This pulls a large transitive graph (sing-tun, sing-box deps, gvisor, quic-go,
utls) and requires **CGO**. The committed `go.sum` is intentionally **incomplete**
(only `yaml.v3`), so a `go mod tidy` is required first to fetch mihomo's
checksums, otherwise the build fails with `missing go.sum entry for ...mihomo`.

```bash
cd libs/caramba-core
go mod tidy                          # needs network + module proxy; writes go.sum
CGO_ENABLED=1 go build -tags mihomo ./...
CGO_ENABLED=1 go test  -tags mihomo ./...
# commit the regenerated go.mod / go.sum afterwards
```

In CI this is a **separate `continue-on-error` job** (`go-mihomo`) so the
non-trivial native build does not block PRs while still surfacing mihomo API
drift (e.g. `adapter.ParseProxy`, `Proxy.URLTest`, `utils.NewUnsignedRanges`).

## Mobile bindings (gomobile, native core)

The Flutter client consumes the Go core as a native binding produced by
`gomobile bind` with `-tags mihomo`. Helper script:
`libs/caramba-core/scripts/build-mobile.sh`.

One-time setup:

```bash
go install golang.org/x/mobile/cmd/gomobile@latest
go install golang.org/x/mobile/cmd/gobind@latest
gomobile init
# platform SDKs: Android NDK (for AAR), Xcode (for xcframework)
```

Build:

```bash
cd libs/caramba-core
go mod tidy                              # complete go.sum for mihomo first
scripts/build-mobile.sh android          # -> build/exarobot.aar
scripts/build-mobile.sh ios              # -> build/exarobot.xcframework
scripts/build-mobile.sh all              # both
```

Then wire `build/exarobot.aar` into `apps/caramba-client/android` and
`build/exarobot.xcframework` into `apps/caramba-client/ios`. This path is **not**
run in CI (needs full toolchain + platform SDKs + network).

---

## Flutter client

```bash
cd apps/caramba-client
flutter pub get
dart format --output=none --set-exit-if-changed .
flutter analyze
flutter test            # may be empty for now (allow-failure in CI)
```

Models are hand-written (no `freezed`/`json_serializable` part files in `lib/`
today), so no `build_runner` codegen step is required to compile. If annotated
models are added later, run:

```bash
dart run build_runner build --delete-conflicting-outputs
```

### Build-time defines

The client compiles with a neutral default brand and the mock tunnel. Two sets of
`--dart-define` values change a build without touching code:

```bash
# brand a tenant build (all four are optional; brand name defaults to Caramba Connect)
flutter build apk \
  --dart-define=CARAMBA_BRAND_NAME="exarobot" \
  --dart-define=CARAMBA_BRAND_SUPPORT_URL="https://t.me/your_support" \
  --dart-define=CARAMBA_BRAND_BOT_URL="https://t.me/your_bot"

# wire the real native tunnel instead of the mock (needs the gomobile binding in place)
flutter run --dart-define=USE_NATIVE_VPN=true
```

These are static defaults only. At runtime the app also pulls brand name, logo
and accent from the active panel (`GET /api/v2/app/branding`), so a Pro instance
themes the app without a per-tenant rebuild. The build-time brand is the
pre-login default before any panel is selected. See
`apps/caramba-client/RUNNING.md` for the runtime brand and import flow and
`apps/caramba-client/INTEGRATION.md` for the native tunnel.

---

## Telegram Mini App (React/TS)

```bash
cd apps/caramba-app
npm ci
npm run build           # produces apps/caramba-app/dist (shipped in release.yml)
```

Root `package.json` uses Biome for JS/TS lint.

---

## Known caveat: nothing has been compiled yet

As of this writing **no layer has been built or tested**: the code was generated
without language toolchains available. Expect first-build fixups. Followup
checklist before relying on green CI:

- [ ] **Rust**: `cargo fmt --all`, then `cargo check --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings`; fix any
      type/borrow/lint errors. Then `cargo test --workspace`. This covers the
      panel verifier and `apps/caramba-license` (the signer) together.
- [ ] **License**: generate a keypair with `caramba-license keygen`, issue a test
      key, and confirm the panel verifies it (set the four `CARAMBA_LICENSE_*`
      env vars and watch the `license_state` row populate). With no key the panel
      must stay Free.
- [ ] **Go default**: `go vet ./... && go build ./... && go test ./...` in both
      `libs/caramba-core` and `apps/caramba-cli`.
- [ ] **Go mihomo**: `go mod tidy` in `libs/caramba-core` (needs network), then
      `CGO_ENABLED=1 go build -tags mihomo ./...`; **commit the completed
      go.sum**. Verify the three drift-prone mihomo symbols still resolve.
- [ ] **Flutter**: `flutter pub get`, `dart format`, `flutter analyze`; add and
      run real `flutter test`s once the gomobile binding is wired.
- [ ] **Mini App**: `npm ci && npm run build` in `apps/caramba-app`.
- [ ] **DB**: apply `libs/caramba-db/migrations` to a Postgres and smoke-test the
      panel `/api/v2/app/*` routes end to end.
- [ ] Once the default Go + Rust contours are green locally, re-run CI and remove
      this caveat.
