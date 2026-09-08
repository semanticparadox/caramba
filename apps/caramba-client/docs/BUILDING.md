# Caramba Connect — developer guide

The user-facing VPN client. Flutter UI (`lib/`) on top of the `caramba_vpn`
plugin (`packages/caramba_vpn/`), which bridges the `com.caramba/vpn` channels
to the Go core in `libs/caramba-core` (mihomo engine). Current app version:
`1.0.0+105` (`pubspec.yaml`).

Caramba Connect is the user-facing brand; exarobot is tenant #1, one hosted
instance. The brand is a runtime value, not a build identity. Code identifiers
stay `caramba` everywhere and are never rebranded.

| Doc | What it covers |
| --- | --- |
| `INTEGRATION.md` | The native runbook: build the Go binding, materialize platform folders, per-platform app edits, CI. Read it before touching anything native. |
| `RUNNING.md` | Running the UI on mock data, runtime branding, connection profiles. |
| `SETUP.md` | Dart-side project setup and toolchain notes. |
| `DESIGN.md`, `ANTI-SLOP.md` | UI design language and the writing rules for it. |
| `docs/` | Protocol specs (CSM), phase design records, AmneziaWG. |

## Platform support

Published `client-v1.0.0+105` assets include Android APKs, a macOS DMG, a Windows ZIP and a Linux archive. Artifact availability does not imply device verification for every platform. Read [desktop verification](DESKTOP-VERIFICATION-2026-09-08.md) for the checks actually performed on the recovered desktop implementation.

macOS currently uses proxy mode; an iOS Network Extension and Apple distribution signing are not in place. Signing and notarization require the owner's developer credentials. The Windows runner does not yet request administrator elevation; WinTun adapter creation needs elevated rights.

## Distribution

Android builds are served by the panel as static files: the installer drops
release assets into `<install_dir>/apps/caramba-panel/downloads/`, the panel
serves that directory at `/downloads`, and `GET /api/client/app/downloads`
returns `{panel_url}/downloads/<file>` when the `app_download_url_<platform>`
setting is empty (the setting, when set, wins). The asset names are a contract
between CI, the installer and the panel — do not rename them:

```
caramba-connect-arm64.apk
caramba-connect-armv7.apk
caramba-connect-macos-arm64.dmg
caramba-connect-windows-x64.zip
caramba-connect-linux-x64.tar.gz
```

`caramba-connect-macos-arm64.dmg` is historically named: the bundle inside is
actually universal (`arm64 + x86_64`). The name is wired into the installer and
the panel, so it stays.

## Build

Never call `flutter build` directly for a release: `scripts/build.sh` supplies
the mandatory `--dart-define`s (`USE_NATIVE_VPN`, `BUILD_EPOCH`) and exports
`USE_NATIVE_VPN` so the Darwin podspec sees the same mock/native decision. The
core must be built and vendored *before* the Flutter build on every platform.

```bash
# from the repo root

# Android (release APK, both ABIs) — one script does everything
bash apps/caramba-client/scripts/ci-android.sh

# macOS (universal core + release .app + unsigned DMG)
bash libs/caramba-core/scripts/build-desktop-lib.sh macos
cp libs/caramba-core/build/libcaramba_core.dylib \
   apps/caramba-client/packages/caramba_vpn/darwin/Libraries/
cd apps/caramba-client && bash scripts/build.sh macos-dmg

# iOS (compile check only — no signing, no Network Extension target)
bash libs/caramba-core/scripts/build-mobile.sh ios   # vendors itself
cd apps/caramba-client && flutter build ios --simulator --debug \
  --dart-define=USE_NATIVE_VPN=true --no-codesign

# Windows (on Windows; the DLL also cross-builds on macOS with mingw-w64)
bash libs/caramba-core/scripts/build-windows-lib.sh   # vendors itself
bash libs/caramba-core/scripts/fetch-wintun.sh        # vendors itself
cd apps/caramba-client && bash scripts/build.sh windows --release

# Linux (on Linux)
bash libs/caramba-core/scripts/build-desktop-lib.sh linux
cp libs/caramba-core/build/libcaramba_core.so \
   apps/caramba-client/packages/caramba_vpn/linux/lib/
cd apps/caramba-client && bash scripts/build.sh linux --release
```

Or run the exact CI path locally, which additionally asserts that the core
really landed inside the artifact:

```bash
bash apps/caramba-client/scripts/ci-desktop.sh macos    # → build/dist/*.dmg
bash apps/caramba-client/scripts/ci-desktop.sh ios      # compile check, no asset
bash apps/caramba-client/scripts/ci-desktop.sh windows  # Windows host only
bash apps/caramba-client/scripts/ci-desktop.sh linux    # Linux host only
```

Mock UI, no native anything: `flutter run -d <device>` (mock is the default) or
`USE_NATIVE_VPN=false bash scripts/build.sh <target>`.

## CI

| Workflow | Runner(s) | Produces |
| --- | --- | --- |
| `.github/workflows/client-android.yml` | ubuntu-latest | signed `caramba-connect-{arm64,armv7}.apk` |
| `.github/workflows/client-desktop.yml` | macos-latest / windows-latest / ubuntu-latest, three independent jobs | unsigned DMG, Windows ZIP, Linux tar.gz; plus the iOS compile check inside the macOS job |

Both trigger on `push: tags: ['v*']` (assets are appended to that tag's release)
and on `workflow_dispatch` (artifacts stay in the run). Toolchains are pinned:
Flutter `3.47.2`, Go `1.26` (`libs/caramba-core/go.mod` requires `go 1.26.0`;
older toolchains refuse the module outright).

```bash
gh workflow run client-desktop.yml --ref main
gh workflow run client-android.yml --ref main
```

`workflow_dispatch` on a feature branch only works once the workflow file is on
the default branch — a GitHub restriction, not ours.

## Distribution credentials

Apple signing/notarization and Windows code signing require the owner's certificates. Never commit these credentials. Test Windows and Linux builds on their target systems before claiming device verification.
