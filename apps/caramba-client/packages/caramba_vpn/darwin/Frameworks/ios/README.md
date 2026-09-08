# exarobot.xcframework (iOS) goes here

This directory must contain `exarobot.xcframework`, the gomobile binding of the
Go package `libs/caramba-core/mobile`.

## Names, and why they are not what the file is called

gobind derives two different names from two different places:

| Swift name | comes from | value here |
| --- | --- | --- |
| module | basename of the output file | `Exarobot` (`exarobot.xcframework`) |
| class prefix | `-prefix` + the Go package name | `CarambaMobile` (`Caramba` + `mobile`) |

So the Swift sources say `import Exarobot`, `CarambaMobileClient`,
`CarambaMobileNewClient`, `CarambaMobileDeviceKeyBridgeProtocol` — not
`Caramba`/`CarambaClient`, which is what an earlier revision of this file
claimed. Renaming the output file or changing `-prefix` renames these
identifiers in `darwin/Classes` and `darwin/Extension` too.

One more importer detail worth knowing before you read the call sites: a Go
method returning `(string, error)` becomes an Objective-C method with a
`_Nonnull` NSString return plus a trailing `NSError**`, and Swift does **not**
turn that into `throws` (a non-null return leaves nothing to signal failure
with). Those calls go through the `carambaCoreCall` / `carambaCoreTry` wrappers
in `Classes/CarambaCoreCalls.swift`. Methods returning `BOOL` do import as
`throws` and are called directly.

## Building it

It is gitignored as a build artifact. Build it on a machine with the Go +
gomobile toolchain and the `mihomo` build tag (so the real engine, with
AmneziaWG support, is linked rather than the stub):

```bash
# from the repo root
libs/caramba-core/scripts/build-mobile.sh ios
```

The script copies the result here itself — there is no manual `cp` step any
more, because "built it but forgot to copy" silently produced an app linked
against the previous core.

## What its presence and absence mean

`caramba_vpn.podspec` decides the build mode from this directory at
`pod install` time, because `--dart-define=USE_NATIVE_VPN` is a Dart-only flag
that Swift cannot see:

- framework here → `-DCARAMBA_CORE`, the native path compiles and links;
- framework missing and `USE_NATIVE_VPN=false` in the environment → mock build,
  the core calls answer `FlutterError("core_missing")`;
- framework missing otherwise → `-DCARAMBA_CORE_REQUIRED`, and the Swift sources
  stop the build with `#error` instead of shipping a mock as if it were native.

Both the plugin (app target) and the Network Extension target link this same
xcframework. The extension target must additionally declare
`SWIFT_ACTIVE_COMPILATION_CONDITIONS = CARAMBA_CORE` (see `../../Extension/README.md`).
