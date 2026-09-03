# Running the Caramba Connect client UI

This guide runs the Caramba Connect Flutter client on each platform using mock
data. No VPN tunnel is started. The app drives its UI from `MockVpnConnection`
(wired by default on every platform in `lib/state/providers.dart`), so every
screen, the connect orb, the stat cards and the navigation all work end to end
without a real backend or a native tunnel.

> Brand note: the user-facing default brand is Caramba Connect. exarobot is
> tenant #1, one hosted instance of the same app. The brand shown at runtime is a
> value, not a build identity: the pre-login default comes from
> `--dart-define=CARAMBA_BRAND_NAME` (default `Caramba Connect`, see
> `lib/data/brand.dart`), and once a panel is selected the app themes from that
> panel's branding. Code identifiers stay `caramba` and are not rebranded.

The real VPN tunnel is wired through the `caramba_vpn` plugin and the
`com.caramba/vpn` channels. It needs a gomobile mihomo binding plus a native TUN
integration per platform (VpnService on Android, Network Extension on iOS and
macOS, WinTun on Windows, a TUN device on Linux). To build the binding, apply the
per-platform app level edits, and run with `--dart-define=USE_NATIVE_VPN=true`,
follow the runbook in `INTEGRATION.md`. Until you do that, the UI runs on mock
data and nothing leaves the device.

The native Go core is opt-in behind a build flag. By default `_useNativeVpn()`
returns false, so `flutter run` on Android and iOS uses `MockVpnConnection` too
and the connect orb animates everywhere. Pass
`--dart-define=USE_NATIVE_VPN=true` only once the gomobile binding and the
native `com.caramba/vpn` channels exist; without them the channels are
unimplemented and tapping connect throws `MissingPluginException`.

## What is already in the repo

`lib/` holds a compiling Flutter skeleton. The native runner folders
(`android/`, `ios/`, `macos/`, `windows/`, `linux/`) are not committed, because
Flutter was not available when the skeleton was written. You generate them once
with `flutter create .` (see below). That step is non-destructive: it only adds
the missing platform folders and leaves `lib/`, `pubspec.yaml` and
`analysis_options.yaml` untouched.

## One-time setup (every platform)

From `apps/caramba-client`:

```bash
flutter create . \
  --org com.caramba \
  --project-name caramba_client \
  --platforms=android,ios,macos,windows,linux
flutter pub get
```

You can pass a shorter `--platforms` list if you only target one platform.
After `flutter pub get`, list the devices Flutter can see:

```bash
flutter devices
```

Each entry prints a device id in its second column. That id is what you pass to
`flutter run -d <device>`.

## Prerequisites by platform

The client targets Dart SDK `>=3.6.0` and Flutter `>=3.29.0` (see
`pubspec.yaml` and `environment.flutter`). Install a Flutter release that ships
at least that version, then add the per-platform toolchain below. Run
`flutter doctor` after each install to confirm the toolchain is detected.

| Platform | Toolchain |
| --- | --- |
| macOS desktop | Xcode (full app from the App Store, not just the command line tools), then `sudo xcodebuild -runFirstLaunch`. CocoaPods (`sudo gem install cocoapods`). |
| iOS | Xcode plus CocoaPods, and an iOS Simulator or a provisioned physical device. |
| Android | Android Studio with the Android SDK, an SDK platform and build tools, plus a configured emulator (AVD) or a device with USB debugging on. Accept the licenses with `flutter doctor --android-licenses`. |
| Windows desktop | Visual Studio (not VS Code) with the "Desktop development with C++" workload, which installs MSVC, the Windows 10/11 SDK and CMake. |
| Linux desktop | clang, cmake, ninja-build, pkg-config and the GTK 3 dev headers. On Debian or Ubuntu: `sudo apt install clang cmake ninja-build pkg-config libgtk-3-dev`. |

## Build flags you must not forget

Use `scripts/build.sh` instead of calling `flutter build` or `flutter run`
directly. It injects the dart-defines the client needs:

```bash
scripts/build.sh run -d macos          # run with the native core
scripts/build.sh apk --debug           # Android APK
scripts/build.sh macos --release       # macOS app
USE_NATIVE_VPN=false scripts/build.sh run -d macos   # mock tunnel
```

`BUILD_EPOCH` is the one that bites. CSM enrollment anchors its clock
plausibility window to the build time, so a build without
`--dart-define=BUILD_EPOCH=<unix seconds>` treats the clock as unset and
refuses to establish first trust with an operator panel. That is the safe
direction of failure, and it looks exactly like a bug, so the script always
passes it. `USE_NATIVE_VPN=true` is the default in the script; pass
`USE_NATIVE_VPN=false` for a mock build. `CARAMBA_API_BASE` overrides the
tenant 1 panel when set in the environment.

The commands in the next section are the raw equivalents, kept for when you
need a flag the script does not pass.

## Run per platform

All commands run from `apps/caramba-client`. Find your exact device id with
`flutter devices` first; the ids below are the common defaults.

### macOS

```bash
flutter run -d macos
```

### Android

Start an emulator or plug in a device, confirm it shows in `flutter devices`,
then:

```bash
flutter run -d emulator-5554
```

Replace `emulator-5554` with the id from `flutter devices`. For a physical
device the id is its serial number.

### iOS

Open a simulator (`open -a Simulator`) or connect a device, then:

```bash
flutter run -d iphone
```

`iphone` matches the booted simulator. For a named simulator or a physical
device, use the exact id from `flutter devices`. A physical device needs a
signing team set in Xcode (open `ios/Runner.xcworkspace` once and pick your
team under Signing and Capabilities).

### Windows

```bash
flutter run -d windows
```

### Linux

```bash
flutter run -d linux
```

While the app runs, press `r` for hot reload and `R` for hot restart in the
terminal.

## Connection profiles: two kinds of source

The client is a universal client. A connection profile (`ConnectionProfile` in
`lib/data/models/connection_profile.dart`, stored by
`lib/data/connection_profiles_store.dart`) is one of two kinds:

- `panelAccount`: an account on a Caramba Connect panel (the exarobot tenant, or
  any self-hosted instance). The app logs in, the panel issues the subscription,
  and the tunnel authorizes against the panel before connecting.
- `rawSub`: an imported subscription that has no panel account behind it. The app
  parses the raw config into a mihomo config locally and connects without
  contacting any panel.

You can hold several profiles at once and switch the active one. The connections
screens live in `lib/features/connections/` (`connections_screen.dart`,
`connection_import_screen.dart`).

### Importing a raw subscription

The import screen accepts a subscription in several formats. The Go core
(`libs/caramba-core/subimport`) auto-detects and converts clash, sing-box, v2ray
base64 and bare proxy URIs, mapping the supported protocols (VLESS with
Reality / WS / HTTPUpgrade / gRPC / TCP, VMess, Trojan, Shadowsocks with
ShadowTLS, Hysteria2, TUIC v5, AmneziaWG / WireGuard, NaiveProxy) and
synthesizing the `CARAMBA` selector group.

- Paste: paste the raw text or a single proxy URI. Works on mock and native.
- URL: the app fetches the subscription URL and imports the body. Works on mock
  and native.
- QR and file: present as entry points but are stubs in this build. Adding the
  camera and file picker later will introduce OS permissions that require updates
  to the iOS privacy manifest and the Play Data Safety form before publishing.

On mock the import is parsed and a profile is created so you can exercise the UI;
no packets move. On native (`USE_NATIVE_VPN=true`, see `INTEGRATION.md`) a
`rawSub` profile connects through `connectRaw`, which imports the config into the
core (`mobile.ImportSubscription`) and raises the tunnel without a panel.

## Enrollment deeplink

A panel can hand a new user an invite as a `carambaconnect://` deeplink:

```
carambaconnect://enroll?panel=<https-panel-url>&code=<invite-code>
```

Opening it routes the app to the enroll screen with the panel and code
prefilled. In a `flutter run` session you can simulate the link without a real
OS handler:

```bash
# Android (adb), app already running
adb shell am start -a android.intent.action.VIEW \
  -d "carambaconnect://enroll?panel=https://exarobot.top&code=DEMO1234"

# iOS simulator, app already running
xcrun simctl openurl booted \
  "carambaconnect://enroll?panel=https://exarobot.top&code=DEMO1234"
```

For the OS to deliver the link to an installed build (not just a running debug
session) the scheme must be registered per platform. That registration is part of
the native runbook, see "register the carambaconnect:// enroll deeplink" in
`INTEGRATION.md`. The enroll path consumes the code on register and on
telegram-login only; carrying a code on login-by-code is a no-op the panel does
not implement.

## Runtime branding

Brand name, logo, accent color and support / bot links are not fixed in the
build. Before login the app uses the build-time defaults
(`--dart-define=CARAMBA_BRAND_NAME` and friends, default Caramba Connect with a
text wordmark). After a panel profile is active the app reads that panel's
branding from `GET /api/v2/app/branding` and themes the live UI from it, so a Pro
instance shows its own name, logo and accent without a per-tenant rebuild. A Free
instance shows an upsell and a "powered by" line. The accent is clamped: hues in
the purple / violet / indigo range are rejected and the accent never colors
status. None of this needs the native tunnel; it works on the mock run against a
reachable panel.

## Zero-toolchain fallback

If you cannot install Flutter, open the static HTML preview at
`apps/caramba-client/demo/caramba-demo.html` in any browser. It mirrors the
client screens and design tokens for review without a build step. It is a
visual mock only: there is no live state, no navigation logic and no mock
connection driver behind it.
