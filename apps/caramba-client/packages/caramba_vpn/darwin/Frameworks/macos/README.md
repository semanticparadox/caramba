# exarobot.xcframework (macOS) goes here — but usually it should not

This directory is for the macOS slice of the gomobile binding of
`libs/caramba-core/mobile`. Build it with:

```bash
# from the repo root
libs/caramba-core/scripts/build-mobile.sh macos
```

The script vendors the result here itself. Names are the same as on iOS: module
`Exarobot`, classes `CarambaMobile*` (see `../ios/README.md` for why).

## Why this directory is normally empty

macOS has two ways to reach the core and they are NOT interchangeable:

| Path | Artifact | Lives in | Tunnel mode | Needs Xcode |
| --- | --- | --- | --- | --- |
| dart:ffi (default today) | `libcaramba_core.dylib` | `../../Libraries/` | `proxy` (mixed inbound on 127.0.0.1:7890) | no |
| Network/System Extension | `exarobot.xcframework` | here | `tun` (system TUN) | yes, plus a signed extension |

The shipping macOS build takes the first path: Dart loads the dylib in-process,
so the Swift plugin never needs the framework. Vendoring both at once would put
**two independent Go runtimes** in one process — two sets of signal handlers and
about 100 MB of duplicated code — so the framework is left out until the
extension target actually exists.

That is also why `caramba_vpn.podspec` never raises `CARAMBA_CORE_REQUIRED` on
macOS: an empty directory here means "we go through ffi", not "the core is
missing". On iOS there is no ffi path, so there the same emptiness is an error.

When the extension target is added, both it and the plugin link this same
xcframework, and the extension declares
`SWIFT_ACTIVE_COMPILATION_CONDITIONS = CARAMBA_CORE`.
