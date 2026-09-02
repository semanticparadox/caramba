# macos/Libraries

Drop `libcaramba_core.dylib` here. It is the cgo `c-shared` build of the Go core
and it powers the **dart:ffi path** on macOS: `FfiVpnConnection` loads it into
the app process and runs mihomo there, with no Network Extension, no Xcode
extension target and no System Extension approval.

The file is not committed (38 MB, machine specific). Build and copy it:

```bash
cd libs/caramba-core
go mod tidy                       # one time, completes go.sum for mihomo
scripts/build-desktop-lib.sh      # -> libs/caramba-core/build/libcaramba_core.dylib

cp libs/caramba-core/build/libcaramba_core.dylib \
   apps/caramba-client/packages/caramba_vpn/macos/Libraries/
```

`macos/caramba_vpn.podspec` declares
`s.vendored_libraries = 'Libraries/libcaramba_core.dylib'`, so once the file is
here CocoaPods embeds it into the built app under
`Contents/Frameworks/libcaramba_core.dylib` — which is the second entry in the
lookup order below.

## How the dylib is found at runtime

`FfiVpnConnection` tries these in order (see
`lib/src/ffi/library_lookup.dart`, covered by `test/library_lookup_test.dart`):

1. `$CARAMBA_CORE_LIB` — an explicit absolute path, wins over everything;
2. `<executable dir>/../Frameworks/libcaramba_core.dylib` — the packaged `.app`;
3. `<executable dir>/libcaramba_core.dylib` — a flat bundle layout;
4. `<ancestor>/libs/caramba-core/build/libcaramba_core.dylib` for every ancestor
   of the working directory and of `Platform.script` — the repo dev path, which
   is what `flutter run -d macos` hits without any copying.

So for day-to-day development you do **not** need to copy anything: build the
dylib in `libs/caramba-core/build/` and run the app from the repo. The copy step
above matters for a packaged build.

## Two different macOS paths, two different artifacts

| Path | Artifact | Lives in | Tunnel mode | Needs Xcode |
| --- | --- | --- | --- | --- |
| dart:ffi (this folder) | `libcaramba_core.dylib` | `macos/Libraries/` | `proxy` (mixed inbound on 127.0.0.1:7890) | no |
| Network Extension | `exarobot.xcframework` | `macos/Frameworks/` | `tun` (system TUN) | yes |

TUN on macOS needs root or a packet-tunnel Network Extension, so the ffi path
defaults to `TunnelMode.proxy`: the core opens a local SOCKS5 + HTTP inbound and
traffic is steered into it by the app or by the system proxy settings. That is
enough to prove a real subscription connection without any signing.
