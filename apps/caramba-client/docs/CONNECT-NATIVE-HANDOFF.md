# New-session prompt — Caramba Connect native client (first working tunnel)

> Copy everything below the line into a fresh Claude Code session started in the
> repo root (`/Users/smtcprdx/Documents/Projects/caramba`). It is self-contained.

---

You are continuing the **Caramba Connect** native-client track in this repo. Work
carefully — the panel/bot/mini-app are in production with ~20 real paying users;
do not break them or the green CI. Use your skills (brainstorming before design,
systematic-debugging for any build failure, TDD for logic, verification-before-
completion before any "done" claim) and dynamic workflows for parallel audit.

## What Caramba is
A DPI-resistant VPN service. **In production:** a Rust workspace — `caramba-panel`
(Axum control plane, PostgreSQL/Redis), `caramba-bot` (Teloxide), `caramba-sub`
(subscription edge), `caramba-node` (VPN node agent), `caramba-installer` (the
`caramba` CLI) + `libs/caramba-db`, `libs/caramba-shared`; plus `apps/caramba-app`
(React/TS Telegram Mini App). sing-box is the server-side core.

**Caramba Connect** is the newer initiative: turn the single service into a
self-hostable white-label product with a **native cross-platform client**:
- `libs/caramba-core` — **Go** VPN engine on **mihomo/clash.meta** (NOT sing-box),
  with a stub engine by default and the real engine behind `-tags mihomo`.
- `apps/caramba-client` — **Flutter/Dart** app + the `packages/caramba_vpn` plugin
  (platform channels for android/ios/macos/linux/windows).
- `apps/caramba-cli` — Go desktop CLI.
- ed25519 licensing (`apps/caramba-license` signer, panel verifier), partner codes,
  white-label branding, enrollment.

## Current state (verified 2026-07-02 — do NOT redo)
- **CI is fully green** (`.github/workflows/ci.yml`, all 5 jobs pass). Don't regress it.
- **`libs/caramba-core` compiles end-to-end**: the `go.sum` had a corrupted
  `check.v1` checksum that blocked `go mod tidy`; it's fixed and the full mihomo
  v1.19.27 graph is committed. Locally verified (Go 1.26, CGO=1):
  `go build ./...`, `go build -tags mihomo ./...`, `go vet -tags mihomo ./...`,
  `go test -tags mihomo ./...` all pass. `engine/engine_mihomo.go` has no API drift.
- The whole graph maxes at the `go 1.22` directive, so CI's Go 1.22 builds it.
- `flutter analyze` is currently **allow-failure** in CI (the client is WIP, native
  core not yet wired, some screens on mocks). `dart format` is also allow-failure.
- The Flutter client runs on **`MockVpnConnection`** by default; the real native
  path is gated behind the `USE_NATIVE_VPN` dart-define (default off).

## Your objective
Bring Caramba Connect to a **first real end-to-end tunnel** and remove the WIP
caveats, in this order (each phase independently valuable — stop and report between):

### Phase A — Desktop native tunnel (fastest validation; do this first)
The desktop path is just a CGO c-shared lib (no NDK/Xcode/gomobile), and the
`-tags mihomo` build already compiles — highest-confidence first win.
1. Read `libs/caramba-core/scripts/build-desktop-lib.sh` (self-documented) and
   `libs/caramba-core/ffi/ffi.go` + `ffi/caramba_core.h` (C-ABI: `CarambaNew`,
   `CarambaConfigure`, `CarambaImportSubscription`, `CarambaSetTunFd`, `CarambaUp`,
   `CarambaDown`, `CarambaStatus`, `CarambaTraffic`, `CarambaFree`, `CarambaFreeString`).
2. Build the lib: `cd libs/caramba-core && CGO_ENABLED=1 ./scripts/build-desktop-lib.sh`
   → `build/libcaramba_core.{so|dylib}` (+ generated `.h`). Vendor per the script's
   header into `apps/caramba-client/{linux,macos}/caramba_vpn/`.
3. Wire the desktop side of `packages/caramba_vpn` (dart:ffi) to the C-ABI — inspect
   `apps/caramba-client/packages/caramba_vpn/{linux,macos}/` and `lib/`. Read
   `apps/caramba-client/INTEGRATION.md` (it documents the channel/FFI contract).
4. Run: `cd apps/caramba-client && flutter run -d macos --dart-define=USE_NATIVE_VPN=true`
   (the flag lives in `lib/state/providers.dart` → `_useNativeVpn()`). Bringing up
   TUN needs elevated privileges (root/CAP_NET_ADMIN on Linux, admin on macOS).
5. **Verify a REAL tunnel**, not the stub: confirm traffic actually routes through
   the core (check egress IP changes), status transitions, and `Traffic()` counters
   move. The stub engine currently reports `Connected` with no tunnel — see Phase E.

### Phase B — `flutter analyze` clean, then re-gate it
1. Install Flutter SDK; `cd apps/caramba-client && flutter pub get && flutter analyze`.
2. Fix all analyze findings (WIP client — expect a backlog: unused, nullability,
   deprecated APIs, mock leftovers). Also `dart format .`.
3. In `.github/workflows/ci.yml`, once analyze/format pass, remove the
   `continue-on-error: true` from the `flutter analyze` / `dart format` steps so
   they become real gates. Verify the whole CI stays green on push.

### Phase C — Mobile bindings (gomobile)
1. Prereqs: `go install golang.org/x/mobile/cmd/gomobile@latest` + `gobind`,
   `gomobile init`; Android NDK / Xcode.
2. `cd libs/caramba-core && ./scripts/build-mobile.sh android` → `build/exarobot.aar`,
   vendor to `apps/caramba-client/android/caramba_vpn/libs/`; `... ios` →
   `exarobot.xcframework` → `apps/caramba-client/ios/Frameworks/`. (Surface:
   `mobile.Client` — `NewClient/Configure/SetTunFd/Up/Down/StatusJSON/TrafficJSON` +
   `Login*/SetProtocol/SetRelay/ApplyPreset/SetSplitTunnel/ListPresets/AutoTune`,
   channel `com.caramba/vpn`.)
3. Wire Kotlin `VpnService` (Android) / Swift `PacketTunnelProvider` (iOS) to pass
   the platform TUN fd via `SetTunFd`. Test on a device/emulator: real tunnel + kill
   switch. Commit runner folders (they're currently uncommitted per the brief).

### Phase D — Licensing / enrollment / migrations end-to-end
1. Run the 8 new `libs/caramba-db/migrations/2026-06*` on a **copy of prod** (never
   prod directly) via the panel's sqlx runner; confirm no drift.
2. Wire the default-org membership hook on user creation (idempotent INSERT).
3. Generate a real ed25519 keypair; bake the real `DEFAULT_LICENSE_PUBKEY` into the
   installer; host the `apps/caramba-license` server. Test activate → verify → gate.
4. **Blocker to remember:** `libs/caramba-db/src/models/store.rs` `tg_id` is `i64`
   but the column is now nullable — email-only accounts read back as `tg_id=0`.
   Make it `Option<i64>` and fix every consumer (compiler surfaces them). Do NOT
   backfill `tg_id=0` (would hit the UNIQUE index). This is a Connect launch blocker,
   not a live-prod bug.

### Phase E — Security & correctness before any release (from the audit)
- **Stub engine false-positive:** `engine/engine_stub.go` reports `Connected` with no
  tunnel → Flutter shows "connected" while traffic leaks. Plumb a `stub:true` /
  `HasNativeEngine()` signal through `api`/`mobile`/`ffi` so the client hard-fails or
  warns instead of showing a fake connection.
- **Cleartext enroll:** `apps/caramba-client/lib/.../enrollment.dart` accepts `http://`
  and posts credentials without host confirmation → reject non-https + add a
  host-confirm step.
- **DPI-hostile bootstrap DNS:** the Go core `DefaultPolicy` hardcodes DoH
  1.1.1.1/8.8.8.8, blocked in RU/IR before the tunnel — route bootstrap DNS through a
  DPI-resistant resolver / per-region config.
- **Cold-start routing:** `geosite-ru`/`geoip-ru` rulesets download via
  `download_detour:"proxy"`, but they decide what stays DIRECT — on a fresh client RU
  traffic can exit abroad. Fetch RU rulesets via `download_detour:"direct"` + bundle a
  fallback.
- **Store compliance** (if publishing): iOS `PrivacyInfo.xcprivacy`, Play Data Safety,
  VPN entitlements, "no dynamic code" (guideline 2.5.2). See `docs/STORE-COMPLIANCE.md`.

## Constraints
- **Production safety:** panel/bot/sub/node/mini-app serve 20 real users. Don't change
  their runtime behavior for Connect work. Keep CI green — verify with a real push.
- **DPI resistance is sacred:** anything touching sing-box config generation or the
  Go core's routing/DNS must preserve censorship resistance. Read before editing.
- **Release build:** `.github/workflows/release.yml` does `cargo build --release
  --target x86_64-unknown-linux-musl` with LTO/opt-z (SLOW, memory-heavy) — only on
  `v*` tags. If you cut a release, watch for LTO OOM on the runner.
- Two Go modules have `go.sum` corruption history (single-char-off hashes). If a Go
  build hits a `SECURITY ERROR: checksum mismatch`, that's the class of bug — fix the
  hash from go's own "downloaded:" value, then `go mod tidy`.

## Verify success (evidence before claiming done)
- `cd libs/caramba-core && go build -tags mihomo ./... && go test -tags mihomo ./...` → pass.
- Desktop: `flutter run --dart-define=USE_NATIVE_VPN=true` establishes a **real** tunnel
  (egress IP changes; not the stub).
- `flutter analyze` → 0 issues; `dart format --set-exit-if-changed .` → clean.
- Push to a branch and confirm the **actual GitHub CI run** is green (all jobs), then
  flip the flutter steps to blocking.

## References (read, don't duplicate)
- `apps/caramba-client/{INTEGRATION,SETUP,RUNNING,DESIGN,ANTI-SLOP}.md`, root `BUILDING.md`.
- `docs/CARAMBA-CONNECT-{PLAN,STATUS}.md`, `docs/NEXT-SESSION.md`,
  `docs/CARAMBA-CONNECT-ULTRACODE-PROMPT.md` (predates the go.sum fix — treat its
  "won't compile" notes as resolved for the Go core).
- Build scripts are self-documented: `libs/caramba-core/scripts/build-{mobile,desktop-lib,desktop}.sh`.
- The full engineering brief with the audit findings: ask the user for `PROJECT-BRIEF.md`
  (it details money-safety, security, and Connect maturity).

Start by confirming the current state (run the `go build -tags mihomo` check), then
brainstorm the Phase A desktop approach with the user before writing code.
