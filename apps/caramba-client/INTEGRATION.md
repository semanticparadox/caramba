# Caramba Connect client native VPN integration

This is the runbook that turns the Caramba Connect Flutter client from the mock
UI into a real tunnel that moves packets on Android, iOS, macOS, Windows and
Linux. It wires the Go core (mihomo) into the app through the `caramba_vpn`
plugin and the `com.caramba/vpn` platform channels.

Caramba Connect is the user-facing brand; exarobot is tenant #1, one hosted
instance. The native wiring in this runbook is identical for every tenant: the
brand is a runtime value, not a build artifact (see "Runtime branding" in
`RUNNING.md`), and the panel a profile points at selects the instance.

Read this top to bottom the first time. Each step is copy-pasteable.

> Status, honestly: Android ships. It is built and signed by CI and served from
> the panel. macOS builds a release `.app` and an **unsigned** DMG and runs in
> proxy mode. iOS compiles and links against the real core but has no signing and
> no Network Extension target. Windows and Linux exist as build definitions that
> have never been executed anywhere. The per-platform truth is the table in
> "Platform status" below; read it before believing any step in this runbook is
> free.
>
> Privileges you still need at run time: Android shows a system VPN consent
> dialog on first connect; a real system TUN needs administrator (Windows) or
> root / `CAP_NET_ADMIN` (Linux); iOS and macOS system TUN needs an Apple
> developer account, the Network Extension entitlement and a signed extension.
> macOS proxy mode is the one path that needs no privileges at all.

> Branding note: user-facing strings default to Caramba Connect and are themed
> at runtime per tenant. Code identifiers stay caramba and are never rebranded:
> the plugin package is `caramba_vpn`, the Dart package is `caramba_client`, the
> channels are `com.caramba/vpn`, and the mihomo selector constant `CARAMBA`
> (defined in `libs/caramba-core` as `profile.CarambaSelector`) is a panel to
> client contract. The binding artifacts are named `exarobot.aar` and
> `exarobot.xcframework` (build outputs, gitignored, not committed); those
> filenames are wired into the plugin podspecs, Gradle and CMake — and on Darwin
> the xcframework file name *is* the Swift module name — so keep them as-is. Do
> not rename any of these.

## Platform status

State as of 2026-09-07. "Verified" means observed on a real machine, not
described in a workflow file.

| Platform | Tunnel path | Build status | Signed | Verified on a device |
| --- | --- | --- | --- | --- |
| **Android** | `VpnService` + AAR binding (`io.caramba.core`) | production; CI builds a release APK per ABI | yes, release keystore | yes — served from the panel |
| **macOS** | `proxy`: `dart:ffi` loads `libcaramba_core.dylib` in-process, mixed inbound on `127.0.0.1:7890` | release `.app` (~165 MB) + unsigned DMG (~66 MB); core and `.app` universal `arm64 + x86_64` | **no** — Gatekeeper asks for "Open anyway" | app launches and stays up; no Network Extension |
| **iOS** | packet tunnel via Network Extension — the target does not exist yet | compiles and links against the real core on the simulator (`--no-codesign`); ~25 000 mihomo symbols and `_CarambaMobileNewClient` present in the plugin binary | **no** Apple certificate | **no** |
| **Windows** | C++ plugin + `libcaramba_core.dll` + `wintun.dll` | defined in CI, **never executed**: not in CI and not on a machine. The DLL itself cross-builds and was checked on macOS (PE32+, `Caramba*` exports) | **no** code-signing certificate | **no** |
| **Linux** | C++/GObject plugin + `libcaramba_core.so` | defined in CI, **never executed** | n/a | **no** |

Known holes that are not build problems:

- Windows: `windows/runner/runner.exe.manifest` is still `asInvoker`, but WinTun
  needs administrator rights to create the adapter. The window title and
  `ProductName` are still the Flutter default `caramba_client`.
- Linux: no `webview_flutter` and no `mobile_scanner` implementation exists for
  the platform. The QR screen already hides itself on desktop; a webview screen
  would throw `MissingPluginException`. There is no app icon in the repo and no
  package (deb/AppImage) — `linux/caramba-connect.desktop` is a template for a
  future packager.
- macOS: the DMG asset is named `caramba-connect-macos-arm64.dmg` for historical
  reasons; the bundle inside is universal. The name is wired into the installer
  and the panel, so it stays.

## What is and is not committed

- Committed: the Dart app (`lib/`), the Go core (`libs/caramba-core`), the build
  scripts, and the `caramba_vpn` plugin package with its native implementations.
- App platform folders: all five (`android/`, `ios/`, `macos/`, `windows/`,
  `linux/`) are present in the working tree with the app level edits from step 2
  already applied. `android/`, `ios/` and `macos/` are already committed;
  `windows/` and `linux/` were created in this round and land in the same commit
  as this document. Check with
  `git ls-files apps/caramba-client/{android,ios,macos,windows,linux} | wc -l`
  rather than trusting this paragraph. An earlier version of this document
  claimed the Apple and Android folders were not committed at all; that was
  wrong.
- Generated, not hand-authored: the platform folders originally came from
  `flutter create .`, and re-running it is still how you would regenerate one.
  Re-running it now **overwrites `.metadata`** (it keeps only `root` plus the
  platform you asked for) and silently bumps `pubspec.lock`. If you must run it,
  diff both files afterwards and restore them.
- Not committed: the gomobile and cgo artifacts (`exarobot.aar`,
  `exarobot.xcframework`, `libcaramba_core.{dylib,so,dll}`, `wintun.dll`). They
  are gitignored by `.gitignore` (`build/`) and
  `packages/caramba_vpn/.gitignore`. You produce them in step 0.

## The channel contract (do not break it)

The app talks to native through `com.caramba/vpn`. The plugin implements this on
every platform and bridges to the Go core. The contract is fixed by
`lib/vpn/vpn_service.dart`:

- MethodChannel `com.caramba/vpn`
  - `configure({panelUrl, subscriptionUuid, accessToken, refreshToken,
    accessExpiryUnix})`: authorize the core before the first connect. Maps to
    gomobile `NewClient(panelUrl, ...)` plus `SetSubscriptionID(subscriptionUuid)`
    and the whole JWT session. `subscriptionUuid` is the canonical key; every
    platform also accepts the legacy `subscriptionId`. The refresh token and the
    expiry are what let the core renew on its own: the access token lasts ~15
    minutes and the core outlives the app that handed it over (see the plugin
    README, "Auth and config seam").
  - `connect({serverId, serverName, countryCode})`: raise the tunnel. Maps to
    `SetTunFd(fd)` (mobile only) then `Up(serverId)`.
  - `disconnect()`: maps to `Down()`.
  - Generic mode (ABI v2, see `docs/CORE-ABI-v2.md`), none of which raise a
    tunnel; the first two return the core's JSON verbatim as a String:
    - `importSubscription({rawConfig, format})` -> `{name?, servers[]}` with
      `id`, `name`, `type`, `server`, `port`, `country` per node. `id` is the
      mihomo proxy name and doubles as the `serverId` for `connectRaw`.
    - `probe({timeoutMs})` -> `{servers[]}` with `latencyMs` (-1 on timeout).
    - `setPolicy({json})` -> the `CorePolicy` JSON, applied before the next `Up`.
    - `setTunnelMode({mode, port})` -> `tun` or `proxy` plus the local mixed
      inbound port, applied at the next `Up`.
- EventChannel `com.caramba/vpn/status`: emits `{stage, detail?,
  connectedSinceMs?, mode?, mixedPort?, activeProxy?}`. `stage` is one of
  `disconnected`, `connecting`, `connected`, `reconnecting`, `error`. The last
  three are ABI v2 additions and stay optional: a core that omits them parses to
  `null`.
- EventChannel `com.caramba/vpn/traffic`: emits `{downBps, upBps, downTotal,
  upTotal}` about once per second while connected.

Imported subscriptions (the `rawSub` profile kind, see "Connection profiles" in
`RUNNING.md`) use a second connect path instead of `configure` + `connect`:

- `connectRaw({rawConfig, format, label})`: import a raw subscription and raise
  the tunnel without a panel. Maps to `mobile.ImportSubscription(raw, format)`
  (the Go `subimport` parser) then `SetImportedConfig` and `Up`. There is no
  panel URL, subscription uuid or token on this path; `label` is display only.
  An optional `serverId` (ABI v2) pins the CARAMBA selector to one proxy of the
  imported config; empty keeps the automatic choice.
  Status and traffic events are emitted on the same two EventChannels.

The app picks the backend at runtime through
`createVpnConnection(native: ...)` (`lib/vpn/vpn_service.dart`, which wraps
`CarambaVpn.createConnection`): mock unless `--dart-define=USE_NATIVE_VPN=true`,
`FfiVpnConnection` on macOS (the core in the app process, see the macOS section)
and `MethodChannelVpnConnection` everywhere else. Without the binding artifacts the channels are
unimplemented and tapping connect throws `MissingPluginException`, so only pass
the flag once step 0 produced the artifacts for that platform.

On Apple platforms the same decision has to be made a second time, in Swift,
because `--dart-define` is invisible to it. The plugin podspec makes it during
`pod install` from two inputs: whether `darwin/Frameworks/<platform>/exarobot.xcframework`
exists, and the `USE_NATIVE_VPN` environment variable — which is why
`scripts/build.sh` *exports* it rather than only passing a dart-define. On iOS,
no framework and no explicit `USE_NATIVE_VPN=false` is a hard `#error`, not a
silent mock. See "iOS" in step 2.

---

## Step 0: build the Go binding

The core is built with `-tags mihomo` so the real engine is wired in (the default
build is a stub that does not raise a tunnel). The committed `go.sum` is
intentionally incomplete, so run `go mod tidy` once first to fetch mihomo
checksums (needs network and a module proxy). See `SETUP.md` for the toolchain
notes.

```bash
cd libs/caramba-core
go mod tidy          # completes go.sum for the mihomo graph (one time)
```

### Mobile (Android AAR + iOS xcframework)

One time gomobile setup:

```bash
go install golang.org/x/mobile/cmd/gomobile@latest
go install golang.org/x/mobile/cmd/gobind@latest
gomobile init
# platform SDKs: Android NDK for the AAR, Xcode for the xcframework
```

Build:

```bash
cd libs/caramba-core
scripts/build-mobile.sh android   # -> build/exarobot.aar
scripts/build-mobile.sh ios       # -> build/ios/exarobot.xcframework
scripts/build-mobile.sh macos     # -> build/macos/exarobot.xcframework
scripts/build-mobile.sh all       # all three
```

**The script vendors each result into the plugin itself** — there is no separate
`cp` step for these three any more, because "built it but forgot to copy" is how
you get a build against last week's core. Destinations are listed under "How the
plugin vendors the artifacts" below.

`ios` and `macos` write to *different* subdirectories of `build/` on purpose: the
output file name determines the Swift module name, so both must stay
`exarobot.xcframework` and in one directory they would overwrite each other.

Names on the native side, which are not obvious and were wrong in this document
before:

| | Value |
| --- | --- |
| Android Java package | `io.caramba.core` (from `-javapkg`) |
| Swift module | `Exarobot` — the base name of `exarobot.xcframework`, so `import Exarobot` |
| Swift class prefix | `CarambaMobile` — `-prefix Caramba` plus the Go package name `mobile` |
| Main class / constructor | `CarambaMobileClient` / `CarambaMobileNewClient` |
| Key-bridge protocol | `CarambaMobileDeviceKeyBridgeProtocol` |

There is no module `Caramba` and no class `CarambaClient`. Changing `-prefix` or
the output file name changes every one of these names in `darwin/Classes`.

One gomobile quirk worth knowing before you read the Swift: a Go method
returning `(string, error)` arrives as
`func f(_ x: String?, error: NSErrorPointer) -> String` and does **not** throw —
the return is `_Nonnull`, so there is nothing to signal failure with. Methods
returning `(bool, error)` do throw normally. `darwin/Classes/CarambaCoreCalls.swift`
wraps the first kind (`carambaCoreCall` / `carambaCoreTry`) so the call sites
read the same either way.

### Desktop (c-shared library)

Desktop platforms load the core as a native shared library `libcaramba_core`
(cgo `c-shared`). On desktop mihomo owns the TUN itself, so there is no fd to
pass; the plugin loads the library and calls the same configure / connect /
disconnect surface.

```bash
cd libs/caramba-core
scripts/build-desktop-lib.sh            # host platform
scripts/build-desktop-lib.sh macos      # explicit; universal arm64 + x86_64
scripts/build-desktop-lib.sh linux
```

The explicit target argument exists for CI, so a step reads as what it builds
instead of depending on the runner's `go env GOOS`.

| Platform | Artifact | Built by |
| --- | --- | --- |
| Linux   | `build/libcaramba_core.so` | `build-desktop-lib.sh linux` (natively) |
| macOS   | `build/libcaramba_core.dylib` | `build-desktop-lib.sh macos` — universal, `lipo`-merged |
| Windows | `build/libcaramba_core.dll` | `build-windows-lib.sh` (cross-builds from macOS/Linux with mingw-w64, or natively on Windows) |

The name is `libcaramba_core.dll` **with** the `lib` prefix on Windows too. The
old `caramba_core.dll` is gone from every path: the C++ plugin's `LoadLibraryW`,
the CMake bundling, `library_lookup.dart` and the build script all agree now.

macOS is built universal because the `.app` ships in an unsigned DMG: a
single-slice `arm64` dylib would fail to load at all on an Intel Mac, and the
Flutter runner would only report it at run time. Override the slices with
`CARAMBA_MACOS_ARCHS` (default `arm64 amd64`); a slice that fails to compile is
skipped with a warning instead of failing the whole build, so a runner without an
x86_64 SDK still produces something.

The build needs `CGO_ENABLED=1` and a system C toolchain (clang or gcc; mingw-w64
for Windows), because the mihomo graph (sing-tun, gvisor, quic-go, utls) uses cgo.

Windows also needs the WinTun runtime next to the DLL:

```bash
scripts/fetch-wintun.sh   # downloads wintun 0.14.1, verifies its SHA-256,
                          # extracts wintun/bin/amd64/wintun.dll and vendors it
```

> Smoke-testing the core without Flutter is `scripts/build-smoke.sh`
> (`cmd/caramba-smoke`): it raises the core in proxy mode on `127.0.0.1` and
> proves traffic really flows through a subscription, with no privileges at all.
> The desktop plugin needs the `libcaramba_core` shared library, not that binary.

### How the plugin vendors the artifacts

The `caramba_vpn` plugin expects the artifacts in its own platform folders so
that `flutter create .` (which only touches the app folders) never clobbers them.

| Artifact | Vendored to | Who copies it |
| --- | --- | --- |
| `exarobot.aar` | `packages/caramba_vpn/android/libs/caramba.aar` | `build-mobile.sh android` |
| `exarobot.xcframework` (iOS) | `packages/caramba_vpn/darwin/Frameworks/ios/` | `build-mobile.sh ios` |
| `exarobot.xcframework` (macOS) | `packages/caramba_vpn/darwin/Frameworks/macos/` | `build-mobile.sh macos` — **normally you do not want this**, see below |
| `libcaramba_core.dll` | `packages/caramba_vpn/windows/lib/` | `build-windows-lib.sh` |
| `wintun.dll` | `packages/caramba_vpn/windows/lib/` | `fetch-wintun.sh` |
| `libcaramba_core.dylib` | `packages/caramba_vpn/darwin/Libraries/` | **you**, by hand |
| `libcaramba_core.so` | `packages/caramba_vpn/linux/lib/` | **you**, by hand |

The two desktop shared libraries are the only manual copies left:

```bash
# from repo root
cp libs/caramba-core/build/libcaramba_core.dylib \
   apps/caramba-client/packages/caramba_vpn/darwin/Libraries/
cp libs/caramba-core/build/libcaramba_core.so \
   apps/caramba-client/packages/caramba_vpn/linux/lib/
```

Do it **before** the Flutter build. On macOS the podspec vendors that exact path
into the `.app`; on Linux the plugin CMake bundles it into `bundle/lib/`. Neither
platform *fails* when the file is absent — the build stays green and quietly
produces a mock bundle. That is precisely why `scripts/ci-desktop.sh` asserts the
core is inside the finished artifact.

> **Do not vendor the macOS xcframework** alongside the dylib. They are two
> independent Go runtimes in one process — two sets of signal handlers and about
> 100 MB of duplicated code. The macOS ffi path needs only the dylib; the macOS
> xcframework exists for a future Network/System Extension target and is left out
> until that target does. See
> `packages/caramba_vpn/darwin/Frameworks/macos/README.md`.

The plugin podspecs, Gradle and CMake files reference these vendored paths, so
once in place the binding is picked up by the normal Flutter build.

---

## Step 1: the platform folders

All five folders (`android/`, `ios/`, `macos/`, `windows/`, `linux/`) are in the
tree with the step 2 edits applied — `android/`, `ios/`, `macos/` from earlier
commits, `windows/` and `linux/` from this round's commit. For a fresh checkout
this step is just:

```bash
cd apps/caramba-client
flutter pub get
```

You only need `flutter create` when regenerating a platform from scratch:

```bash
flutter create . \
  --org com.caramba \
  --project-name caramba_client \
  --platforms=windows          # one platform at a time
```

> `flutter create` is **not** as non-destructive as it looks. It rewrites
> `.metadata`, keeping only `root` plus the platform you named and dropping every
> other `migration.platforms` entry; it runs an implicit `pub get` that bumps
> `pubspec.lock`; and it appends to `analysis_options.yaml`. After running it,
> diff those three files and restore what you did not mean to change —
> `.metadata` must list all five platforms plus `root`.

`flutter pub get` resolves the `caramba_vpn` plugin by path (it is listed in
`pubspec.yaml` as a path dependency). The plugin registers the `com.caramba/vpn`
channels on each platform through its federated plugin classes:

| Platform | Plugin class | Native package / language |
| --- | --- | --- |
| Android | `CarambaVpnPlugin` + `CarambaVpnService` | `com.caramba.caramba_vpn`, Kotlin |
| iOS     | `CarambaVpnPlugin` + `PacketTunnelProvider` | Swift, `sharedDarwinSource` (`darwin/Classes` + `darwin/Classes/ios`) |
| macOS   | `CarambaVpnPlugin` + `PacketTunnelProvider` (extension path), or no plugin class at all on the dart:ffi path — `FfiVpnConnection` loads `libcaramba_core.dylib` in process | Swift / Dart, same shared `darwin/` source |
| Windows | `CarambaVpnPluginCApi` | C++ loading `libcaramba_core.dll` + `wintun.dll` |
| Linux   | `CarambaVpnPlugin` | C++ / GObject loading `libcaramba_core.so`; registration symbol `caramba_vpn_plugin_register_with_registrar` |

There are no `packages/caramba_vpn/ios/` or `/macos/` directories: both Apple
platforms share one `darwin/` source tree (`sharedDarwinSource: true`) with
per-platform shims in `darwin/Classes/ios` and `darwin/Classes/macos`.

---

## Step 2: per platform app level edits

These edits go on top of the folders generated in step 1. The plugin carries the
channel and tunnel code; these are the host app capabilities the OS requires and
that a plugin cannot declare for you.

### Android

The VPN service and its permission must be declared in the app manifest, and the
app must request user consent before the first connect.

1. In `android/app/src/main/AndroidManifest.xml`, inside `<manifest>`:

   ```xml
   <uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
   <uses-permission android:name="android.permission.FOREGROUND_SERVICE_SPECIAL_USE" />
   <uses-permission android:name="android.permission.POST_NOTIFICATIONS" />
   ```

2. Inside `<application>`, register the plugin VPN service:

   ```xml
   <service
       android:name="com.caramba.caramba_vpn.CarambaVpnService"
       android:permission="android.permission.BIND_VPN_SERVICE"
       android:foregroundServiceType="specialUse"
       android:exported="false">
       <intent-filter>
           <action android:name="android.net.VpnService" />
       </intent-filter>
   </service>
   ```

3. Consent: the first connect triggers `VpnService.prepare`, which shows the
   system VPN consent dialog. The plugin returns the consent result to the app;
   if the user declines, connect resolves to the `error` stage. Nothing else is
   required from the app developer.
4. `minSdkVersion` must be at least 21 (the AAR is built against androidapi 21).
   Confirm it in `android/app/build.gradle.kts`, which is also where the release
   signing config lives (it falls back to the debug key when
   `android/key.properties` is absent, so an unsigned local build still runs).

### iOS

iOS routes packets through a Network Extension. The extension is a separate
target that a Flutter plugin cannot create for you, so add it once.

1. In Xcode (`open ios/Runner.xcworkspace`), add a new target of type
   Network Extension, Packet Tunnel. Name it for example `CarambaTunnel`. Add the
   extension's principal class by adding these plugin sources to the new target's
   Compile Sources (they live in the plugin package, so `flutter create .` never
   touches them):
   - `packages/caramba_vpn/darwin/Extension/PacketTunnelProvider.swift`
   - `packages/caramba_vpn/darwin/Classes/CarambaVpnShared.swift`

   Link the same `exarobot.xcframework` (the iOS slice) into this extension
   target as well as the Runner, so the extension can instantiate the Go core.
2. Capabilities on both the Runner target and the extension target:
   - Network Extensions (Packet Tunnel).
   - App Groups: add the same group id to both, for example
     `group.com.caramba.exarobot`. The app and the extension share status,
     traffic and the core work directory / token store through this group.
3. Info.plist keys (set identically on the Runner and the extension):
   - `CARAMBA_APP_GROUP` = your App Group id (e.g. `group.com.caramba.exarobot`).
     The plugin and the extension both read this to find the shared container; if
     it is missing they fall back to per-process defaults and status/traffic will
     not cross the process boundary.
   - On the Runner only, optionally `CARAMBA_VPN_EXTENSION_ID` = the extension's
     bundle id. The plugin defaults to `<app-bundle-id>.CarambaVpnExtension`; set
     this key if you named the extension target differently.
4. Signing: select your Apple developer team on both targets under Signing and
   Capabilities. A physical device and the Network Extension entitlement both
   require a provisioning profile from a paid Apple developer account.
5. The extension `Info.plist` must declare the packet tunnel provider principal
   class. Under `NSExtension`, set `NSExtensionPointIdentifier` to
   `com.apple.networkextension.packet-tunnel` and `NSExtensionPrincipalClass` to
   `$(PRODUCT_MODULE_NAME).PacketTunnelProvider`.
6. The extension target must compile `darwin/Extension/PacketTunnelProvider.swift`,
   `darwin/Classes/CarambaVpnShared.swift` and `darwin/Classes/CarambaCoreCalls.swift`,
   link the same `exarobot.xcframework`, and declare
   `SWIFT_ACTIVE_COMPILATION_CONDITIONS = CARAMBA_CORE`. The full checklist lives
   in `packages/caramba_vpn/darwin/Extension/README.md`. None of this exists yet —
   there is no extension target in the repository.

### How iOS chooses mock or native

`--dart-define` never reaches Swift, so `caramba_vpn.podspec` decides during
`pod install` and passes the result through `OTHER_SWIFT_FLAGS` (not
`SWIFT_ACTIVE_COMPILATION_CONDITIONS` — Flutter's podhelper overwrites that key
wholesale in `post_install`):

| `Frameworks/ios/exarobot.xcframework` | `USE_NATIVE_VPN` | Flag | Result |
| --- | --- | --- | --- |
| present | anything | `-DCARAMBA_CORE` | native build |
| absent | explicitly `false`/`0`/`no`/`off` | none | mock build |
| absent | anything else, including unset | `-DCARAMBA_CORE_REQUIRED` | `#error` at compile time |

The third row is the point: the old `#if canImport(Caramba)` made a build without
the core compile green and fail as `core_missing` on the device instead.

Two consequences for the build order:

- Build the xcframework **before** the Flutter build. The podspec's checksum does
  not change when the framework appears, so Flutter will not re-run `pod install`
  on its own if `Pods/` and `Podfile.lock` already exist. On a clean checkout
  there is no `ios/Pods` (gitignored) and it works out; if you cache `Pods/`, run
  `cd ios && pod install` explicitly after building the framework.
- A deliberate mock build needs `export USE_NATIVE_VPN=false` **before**
  `pod install`. `scripts/build.sh` exports it for you.

macOS has no such third row, and that is architecture rather than leniency: the
default native path there is `dart:ffi` over `libcaramba_core.dylib`, which Dart
loads without Swift's involvement, so a missing xcframework means "we go through
ffi", not "the core is missing". On iOS there is no other path to the core.

### macOS

macOS has TWO independent paths. Pick one; they do not interfere.

| | A. dart:ffi (no Xcode) | B. Network Extension |
| --- | --- | --- |
| Artifact | `libcaramba_core.dylib` in `packages/caramba_vpn/darwin/Libraries/` | `exarobot.xcframework` in `packages/caramba_vpn/darwin/Frameworks/macos/` (normally absent on purpose) |
| Where the core runs | in the app process | in a separate extension process |
| Traffic capture | `proxy`: local mixed inbound (SOCKS5 + HTTP) on 127.0.0.1:7890 | `tun`: system TUN, all traffic |
| Needs Xcode / signing / approval | no | yes |
| Dart class | `FfiVpnConnection` | `MethodChannelVpnConnection` |

#### A. dart:ffi path (development, no Xcode)

TUN on macOS needs root or a packet-tunnel Network Extension. The ffi path
sidesteps both: `FfiVpnConnection` loads `libcaramba_core.dylib` straight into
the app process, calls `CarambaSetTunnelMode(h, "proxy", 7890)` and raises the
core with a local mixed inbound instead of a TUN. That proves a real
subscription connection with zero privileges; traffic reaches it because the app
(or the macOS network proxy settings) points at 127.0.0.1:7890.

1. Build the library once (step 0, desktop section):

   ```bash
   cd libs/caramba-core
   scripts/build-desktop-lib.sh macos   # -> build/libcaramba_core.dylib, universal
   ```

2. Run. Nothing to copy for development: the lookup walks up from the working
   directory to `libs/caramba-core/build/`, so a repo checkout resolves on its
   own.

   ```bash
   cd apps/caramba-client
   flutter run -d macos --dart-define=USE_NATIVE_VPN=true
   ```

   Lookup order (`packages/caramba_vpn/lib/src/ffi/library_lookup.dart`, unit
   tested in `test/library_lookup_test.dart`):
   1. `$CARAMBA_CORE_LIB` (absolute path, wins over everything);
   2. `<executable dir>/../Frameworks/libcaramba_core.dylib` (packaged `.app`);
   3. `<executable dir>/libcaramba_core.dylib`;
   4. `<ancestor>/libs/caramba-core/build/libcaramba_core.dylib` for every
      ancestor of the working directory and of `Platform.script`.

3. For a packaged build, copy the dylib into the plugin so CocoaPods embeds it:

   ```bash
   cp libs/caramba-core/build/libcaramba_core.dylib \
      apps/caramba-client/packages/caramba_vpn/darwin/Libraries/
   ```

   The podspec declares `s.vendored_libraries =
   'Libraries/libcaramba_core.dylib'`, which lands it in
   `Contents/Frameworks/` — entry 2 of the lookup order. See
   `packages/caramba_vpn/darwin/Libraries/README.md`.

Threading: `CarambaUp` can block for up to 60 s and `CarambaProbe` for its
timeout, so both run in `Isolate.run`. Only sendable values cross the isolate
boundary (the library path, the integer handle, strings); the isolate reopens
the dylib by path, which `dlopen` resolves to the already-loaded image, so the
Go runtime and its handle table are shared and the handle stays valid. Status
and traffic are polled at 1 Hz on a `Timer` in the UI isolate — those calls
return immediately.

Symbols: `CarambaSetPolicy` and `CarambaProbe` are looked up LAZILY. A dylib
built before ABI v2 loads fine and only fails at the call site, with
`CarambaCoreMissingSymbol` naming the symbol and the library path, instead of
crashing at `DynamicLibrary.open`.

Smoke test against a real dylib:

```bash
cd apps/caramba-client
CARAMBA_CORE_LIB=$PWD/../../libs/caramba-core/build/libcaramba_core.dylib \
  flutter test test/ffi_smoke_test.dart
```

Without the env var the test skips itself.

##### Release build and DMG

This is the shipping macOS path today. `macos-dmg` is a target of
`scripts/build.sh` rather than a loose `hdiutil` command so that CI and a local
check run byte-for-byte the same sequence:

```bash
# from repo root
bash libs/caramba-core/scripts/build-desktop-lib.sh macos
cp libs/caramba-core/build/libcaramba_core.dylib \
   apps/caramba-client/packages/caramba_vpn/darwin/Libraries/
cd apps/caramba-client && bash scripts/build.sh macos-dmg
# -> build/macos/Build/Products/Release/caramba_client.app  (~165 MB, universal)
# -> build/caramba-connect-macos-arm64.dmg                  (~66 MB, UDZO, unsigned)
```

The copy is not optional: the podspec vendors that exact path, and without it the
release `.app` ships with no core and silently behaves as a mock.

`macos/Runner/Release.entitlements` carries `com.apple.security.network.server`
in addition to `network.client`. Proxy mode listens on `127.0.0.1:7890` inside
the app process, and under App Sandbox accepting inbound connections is denied
without that entitlement — debug worked (DebugProfile had it) while release would
have silently refused connections to the local proxy.

The DMG is **unsigned**: the project has no Apple Developer certificate, so
Gatekeeper on another Mac will require "Open anyway". Notarization is not
possible either. This is a known, accepted state of this round.

#### B. Network Extension path (system TUN, production)

macOS uses the same shared Swift sources as iOS (the plugin body and the
`PacketTunnelProvider`), but the extension packaging differs.

1. In `macos/Runner.xcworkspace`, add a Network Extension target. On macOS a
   packet tunnel ships as a System Extension (sandboxed app, distributed via the
   App Store or Developer ID) or, for development, an app extension. Add the same
   two plugin sources to its Compile Sources as on iOS:
   - `packages/caramba_vpn/darwin/Extension/PacketTunnelProvider.swift`
   - `packages/caramba_vpn/darwin/Classes/CarambaVpnShared.swift`

   Link the macOS slice of `exarobot.xcframework` into both the Runner and the
   extension.
2. Capabilities on both Runner and the extension:
   - Network Extensions: `com.apple.developer.networking.networkextension` with
     the `packet-tunnel-provider` value. A System Extension additionally needs
     `com.apple.developer.system-extension.install` on the app.
   - App Groups (`group.com.caramba.exarobot`) on both targets.
   - App Sandbox with outgoing/incoming network on both targets.
3. Info.plist keys: same as iOS, `CARAMBA_APP_GROUP` on both targets and the
   optional `CARAMBA_VPN_EXTENSION_ID` on the Runner. The extension declares
   `NSExtensionPointIdentifier` = `com.apple.networkextension.packet-tunnel` and
   `NSExtensionPrincipalClass` = `$(PRODUCT_MODULE_NAME).PacketTunnelProvider`.
4. Sign both targets with your Apple developer team. A System Extension on macOS
   requires signing and explicit user approval in System Settings on first run;
   the plugin reports the `connecting` stage until approval completes.

### Windows

Windows needs administrator rights to create the WinTun adapter, and the
`wintun.dll` runtime must ship next to the app.

Build (on a Windows host; the DLL alone also cross-builds on macOS/Linux with
mingw-w64):

```bash
bash libs/caramba-core/scripts/build-windows-lib.sh   # DLL, vendors itself
bash libs/caramba-core/scripts/fetch-wintun.sh        # wintun.dll, vendors itself
cd apps/caramba-client && bash scripts/build.sh windows --release
# -> build/windows/x64/runner/Release/
```

Both DLLs must be in `packages/caramba_vpn/windows/lib/` *before*
`flutter build windows`: the plugin CMake lists them in
`caramba_vpn_bundled_libraries` and the runner CMake installs them next to
`caramba_client.exe` via `install(FILES ${PLUGIN_BUNDLED_LIBRARIES})`. A missing
file fails CMake configuration outright. Confirm both are present in
`build/windows/x64/runner/<Config>/` afterwards.

Two things are **still open** and neither is a build problem:

1. Elevation. `windows/runner/runner.exe.manifest` is the Flutter default, i.e.
   `asInvoker`, but WinTun cannot create the adapter without administrator
   rights. Either set `requestedExecutionLevel` to `requireAdministrator` in that
   manifest, or relaunch elevated at connect time. Until then, run the built exe
   as administrator by hand.
2. Cosmetics: the window title and `ProductName` are still the Flutter default
   `caramba_client`, not `Caramba Connect`.

There is no Windows code-signing certificate, so SmartScreen will warn on the
downloaded ZIP.

### Linux

Linux needs root or the `CAP_NET_ADMIN` capability to open the TUN device.

Pick one:

- Grant the capability to the built binary once:

  ```bash
  sudo setcap cap_net_admin,cap_net_raw+ep \
    build/linux/x64/release/bundle/caramba_client
  ```

- Or launch with a privilege prompt at runtime via `pkexec`:

  ```bash
  pkexec build/linux/x64/release/bundle/caramba_client
  ```

Build:

```bash
bash libs/caramba-core/scripts/build-desktop-lib.sh linux
cp libs/caramba-core/build/libcaramba_core.so \
   apps/caramba-client/packages/caramba_vpn/linux/lib/
cd apps/caramba-client && bash scripts/build.sh linux --release
# -> build/linux/x64/release/bundle/
```

System packages needed on the build host: `clang cmake ninja-build pkg-config
libgtk-3-dev liblzma-dev libsecret-1-dev`. That is the full list — `libsecret`
comes from `flutter_secure_storage_linux`, GTK from the runner and the plugin.
No tray/appindicator package is needed; the project has no tray plugin.

The `cp` is easy to forget and, deliberately, does not break the build: the
plugin CMake wraps `caramba_vpn_bundled_libraries` in `if(EXISTS ...)`, because
the core is a gitignored artifact and a mock build must still configure. The
price is that a bundle without a core builds green, so verify explicitly:

```bash
test -f build/linux/x64/release/bundle/lib/libcaramba_core.so
nm -D --defined-only build/linux/x64/release/bundle/lib/libcaramba_core.so | grep Caramba
# expect CarambaNew / CarambaUp / CarambaDown / CarambaStatus / CarambaTraffic
```

Library lookup at run time does not rely on the runner's rpath. `$ORIGIN/lib` is
set on the executable, but `dlopen` is called from
`libcaramba_vpn_plugin.so`, which Flutter installs into `bundle/lib/` without
rewriting its rpath, and `DT_RUNPATH` is not inherited from the executable. The
plugin therefore tries, in order: `$CARAMBA_CORE_LIB`, the bare
`libcaramba_core.so` (the historical behaviour), `<binary dir>/lib/`, and
`<binary dir>/`.

Deep links on Linux go through `app_links_linux` over `package:gtk`, which
requires the runner to be single-instance: `linux/runner/my_application.cc` uses
`G_APPLICATION_HANDLES_COMMAND_LINE | G_APPLICATION_HANDLES_OPEN` and returns
`FALSE` from `local_command_line`. Scheme registration is packaging work:
`linux/caramba-connect.desktop` is the template (`MimeType=x-scheme-handler/caramba;x-scheme-handler/carambaconnect;`,
`Exec=... %u`, `StartupWMClass=com.caramba.caramba_client`). The packager
substitutes the real `Exec`, installs it into `/usr/share/applications` and runs
`update-desktop-database`. There is no deb/AppImage and no app icon in the repo
yet, and the tar.gz asset ships the raw bundle without the `.desktop` file.

Two Flutter plugins have no Linux implementation: `webview_flutter` and
`mobile_scanner`. The QR screen already hides itself on desktop; a webview screen
would throw `MissingPluginException`.

---

## Step 3: run native vs mock

Mock is the default and needs nothing native. Native requires step 0 artifacts in
place and the step 2 app edits applied for that platform.

Run from `apps/caramba-client`. Find the exact device id with `flutter devices`.

```bash
# Mock (UI only, no tunnel, no privilege): the default on every platform
flutter run -d <device>

# Native tunnel (real packets): requires the binding for that platform
flutter run -d <device> --dart-define=USE_NATIVE_VPN=true
```

Per platform notes for the native run:

- Android: `flutter run -d <emulator-or-serial> --dart-define=USE_NATIVE_VPN=true`.
  Accept the VPN consent dialog on first connect.
- iOS: `flutter run -d <device> --dart-define=USE_NATIVE_VPN=true`. Today this
  is a compile-and-link check only: there is no Network Extension target and no
  certificate, and the simulator cannot run a packet tunnel at all. Build the
  xcframework first, or the build fails on purpose with `#error` (see "How iOS
  chooses mock or native").
- macOS: `flutter run -d macos --dart-define=USE_NATIVE_VPN=true`. This takes
  the dart:ffi path by default (proxy mode, nothing to approve) as long as
  `libcaramba_core.dylib` is where the lookup can find it. Force the Network
  Extension path by passing `preferFfiOnMacOS: false` to
  `createVpnConnection`, and approve the extension in System Settings on first
  run.
- Windows: run an elevated shell (or the elevated exe) then
  `flutter run -d windows --dart-define=USE_NATIVE_VPN=true`.
- Linux: ensure the capability or `pkexec` from step 2, then
  `flutter run -d linux --dart-define=USE_NATIVE_VPN=true`.

If connect throws `MissingPluginException`, the binding for that platform is not
in place (step 0) or the channels did not register (step 1 / 2). Fall back to the
mock run to keep iterating on the UI.

---

## Step 3b: register the `carambaconnect://` enroll deeplink

The enrollment flow (P2) opens on the custom URL scheme
`carambaconnect://enroll?panel=<https-url>&code=<invite>`, alongside the newer
self-describing `caramba://connect?d=<armor>`. Intake (parse plus navigation to
`/enroll`) is wired in Dart (`lib/router/deep_links.dart`, on `app_links`). The
OS only delivers the link if the scheme is registered per platform.

**Already applied and committed** on Android, iOS and macOS — both schemes.
Windows and Linux are the remaining gaps: Linux ships a template that a packager
must install, Windows has no registration at all.

- Android (`android/app/src/main/AndroidManifest.xml`): add an intent-filter to
  the main activity.

  ```xml
  <intent-filter android:autoVerify="false">
    <action android:name="android.intent.action.VIEW" />
    <category android:name="android.intent.category.DEFAULT" />
    <category android:name="android.intent.category.BROWSABLE" />
    <data android:scheme="carambaconnect" android:host="enroll" />
  </intent-filter>
  ```

- iOS (`ios/Runner/Info.plist`) and macOS (`macos/Runner/Info.plist`): add the
  URL type.

  ```xml
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key>
      <string>com.caramba.connect.enroll</string>
      <key>CFBundleURLSchemes</key>
      <array><string>carambaconnect</string></array>
    </dict>
  </array>
  ```

- Windows: register the protocol under
  `HKCU\Software\Classes\carambaconnect` (per `app_links` Windows setup) so the
  launched exe receives the URI argument.
- Linux: `linux/caramba-connect.desktop` ships in the repo as a template with
  `MimeType=x-scheme-handler/caramba;x-scheme-handler/carambaconnect;` and
  `Exec=... %u` (the `%u` is mandatory — without it the URI never reaches `argv`
  and `app_links_linux` gets nothing). The packager substitutes the real `Exec`,
  installs it into `/usr/share/applications` and runs `update-desktop-database`.
  Verify with `xdg-mime query default x-scheme-handler/caramba`.

Scheme is per-platform brand neutral on purpose (`carambaconnect`, the
user-facing brand), independent of the `caramba_*` code identifiers. Tenant-1
(exarobot) reuses the same scheme; the panel URL in the link selects the
instance.

---

## Step 4: AmneziaWG and the node side

For AmneziaWG to actually obfuscate and move packets, both the node and the
client must be AmneziaWG capable. The full end to end (panel gate
`CARAMBA_ENABLE_AMNEZIAWG`, the AmneziaWG capable sing-box fork on the node, the
mihomo `wireguard` outbound on the client, protocol pinning and autotune) is
documented in `docs/AMNEZIAWG.md`. Read it before enabling AmneziaWG in
production, because a bare sing-box node fails `sing-box check` on an AmneziaWG
inbound and takes down the whole node config.

---

## Step 5: continuous integration

Two workflows build the client. Both fire on `push: tags: ['v*']` — assets are
appended to that tag's release, so their order relative to `release.yml` does not
matter — and on `workflow_dispatch`, where the artifacts stay in the run and
nothing is published.

| Workflow | Runner | Asset |
| --- | --- | --- |
| `.github/workflows/client-android.yml` | ubuntu-latest | `caramba-connect-arm64.apk`, `caramba-connect-armv7.apk` (signed) |
| `.github/workflows/client-desktop.yml`, job `macos` | macos-latest | `caramba-connect-macos-arm64.dmg` (unsigned) — plus the iOS compile check, which produces no asset |
| `.github/workflows/client-desktop.yml`, job `windows` | windows-latest, Git Bash | `caramba-connect-windows-x64.zip` |
| `.github/workflows/client-desktop.yml`, job `linux` | ubuntu-latest | `caramba-connect-linux-x64.tar.gz` |

The three desktop jobs are independent (no `needs`): a Windows failure must not
block publishing the DMG. Those asset names are a contract with the installer and
the panel (see "Distribution" in `README.md`) — renaming one breaks the download
button in the mini app.

All the logic lives in two scripts, not in YAML, so that a local check and CI run
the same steps:

```bash
bash apps/caramba-client/scripts/ci-android.sh          # → build/dist/*.apk
bash apps/caramba-client/scripts/ci-desktop.sh macos    # → build/dist/*.dmg
bash apps/caramba-client/scripts/ci-desktop.sh ios      # compile check, no asset
bash apps/caramba-client/scripts/ci-desktop.sh windows  # Windows host only
bash apps/caramba-client/scripts/ci-desktop.sh linux    # Linux host only
```

Useful variables: `CARAMBA_SKIP_CORE=1` / `CARAMBA_SKIP_AAR=1` (reuse an already
built core — CI sets these on a cache hit), `USE_NATIVE_VPN=false` (mock build),
`CARAMBA_MACOS_ARCHS`, `CARAMBA_GOMOBILE_VERSION`, `CARAMBA_NDK_VERSION`.

After every build `ci-desktop.sh` asserts that the core actually reached the
artifact — `Contents/Frameworks/libcaramba_core.dylib` plus `lipo` on macOS,
`bundle/lib/libcaramba_core.so` plus an export count on Linux, both DLLs next to
the `.exe` on Windows, and `nm` over the plugin binary on iOS. Without those
checks a green run can happily publish a mock bundle, because on desktop a
missing core does not fail the build. The checks downgrade to warnings when
`USE_NATIVE_VPN=false`.

Toolchains are pinned: Flutter `3.47.2`, Go `1.26` (`libs/caramba-core/go.mod`
requires `go 1.26.0`; an older toolchain refuses the module).

```bash
gh workflow run client-desktop.yml --ref main
gh workflow run client-android.yml --ref main
```

`workflow_dispatch` on a feature branch only becomes available once the workflow
file is on the default branch. That is a GitHub restriction, not ours.

---

## Reality check

Android is real: built, signed and served from the panel. Everything else has a
named gap. Before you expect a real tunnel elsewhere:

- The Go binding must be built with `-tags mihomo` (step 0); the stub engine does
  not raise a tunnel. Every script here already passes `mihomo,with_gvisor`.
- macOS today is **proxy mode**, not a system TUN: the core listens on
  `127.0.0.1:7890` and traffic reaches it only if the app or the system proxy
  settings point there. A system TUN on macOS needs path B — an Apple developer
  account, a signed Network/System Extension, and user approval.
- The macOS DMG is unsigned and un-notarized; Gatekeeper will require "Open
  anyway" on any Mac that is not this one.
- iOS has no Network Extension target and no certificate. It compiles and links
  against the core on the simulator, and that is the whole claim.
- Windows and Linux builds have never been executed — not in CI, not on a
  machine. Windows additionally needs administrator (and the manifest still says
  `asInvoker`) and has no code-signing certificate; Linux needs root or
  `CAP_NET_ADMIN`.
- Android needs the user to accept the VPN consent dialog on first connect.
- AmneziaWG needs an AmneziaWG capable node fork and the panel flag.

### What is needed from the owner

None of these can be solved in code:

| Need | Unblocks |
| --- | --- |
| Apple Developer Program membership | signing + notarizing the macOS DMG; the Network Extension target and entitlement for a real iOS/macOS TUN; any iOS distribution at all |
| A Windows code-signing certificate | SmartScreen not warning on the ZIP |
| A physical Windows PC | the first ever execution of the Windows build |
| A Linux machine | the first ever execution of the Linux build |
| A decision on Windows elevation | `requireAdministrator` in the manifest versus a UAC relaunch at connect |

---

## Document outline

- Caramba Connect client native VPN integration (intro, honest status warning,
  branding note)
- Platform status (per-platform table: tunnel path, build status, signing,
  device verification; known holes)
- What is and is not committed (all five platform folders are in the tree —
  Android/iOS/macOS committed earlier, Windows/Linux with this round; binding
  artifacts are not committed at all)
- The channel contract (do not break it; panelAccount configure/connect plus the
  rawSub connectRaw import path; how Apple platforms pick mock vs native)
- Step 0: build the Go binding (mobile AAR / xcframework and the `Exarobot` /
  `CarambaMobile*` names, desktop `libcaramba_core` + wintun, how the plugin
  vendors artifacts and why the macOS xcframework is deliberately absent)
- Step 1: the platform folders (all five in the tree; what `flutter create` breaks;
  plugin classes)
- Step 2: per platform app level edits (Android manifest + consent, iOS Network
  Extension + the podspec mock/native gate, macOS ffi proxy path + release DMG +
  entitlements, macOS Network Extension path, Windows wintun + open elevation
  question, Linux build + `.so` lookup + deep links)
- Step 3: run native vs mock (`--dart-define=USE_NATIVE_VPN=true` per platform)
- Step 3b: register the `carambaconnect://` enroll deeplink
- Step 4: AmneziaWG and the node side (pointer to `docs/AMNEZIAWG.md`)
- Step 5: continuous integration (both workflows, asset names, the local
  `ci-*.sh` scripts, `gh workflow run`)
- Reality check, and what is needed from the owner
- Document outline
