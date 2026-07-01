# exarobot.xcframework (iOS) goes here

This directory must contain `exarobot.xcframework`, the gomobile binding of the
Go package `libs/caramba-core/mobile`. It is built with `-prefix Caramba`, so the
imported Swift module and class prefix are `Caramba` (e.g. `CarambaNewClient`,
`CarambaClient`), even though the framework file is named `exarobot`. That is why
the Swift sources `import Caramba` / `#if canImport(Caramba)`.

It is gitignored as a build artifact. Build it on a machine with the Go +
gomobile toolchain and the `mihomo` build tag (so the real engine, with
AmneziaWG support, is linked rather than the stub):

```bash
# from libs/caramba-core (see INTEGRATION step 0)
scripts/build-mobile.sh ios          # -> libs/caramba-core/build/exarobot.xcframework
```

Then copy it here:

```bash
cp -R libs/caramba-core/build/exarobot.xcframework \
   apps/caramba-client/packages/caramba_vpn/ios/Frameworks/
```

Both the plugin (app target) and the Network Extension target link this same
xcframework.
