# exarobot.xcframework (macOS) goes here

This directory must contain `exarobot.xcframework`, the gomobile binding of the
Go package `libs/caramba-core/mobile` built for macOS. It is built with
`-prefix Caramba`, so the imported Swift module and class prefix are `Caramba`
(`CarambaNewClient`, `CarambaClient`); the Swift sources `import Caramba`.

It is gitignored as a build artifact. Build it with the Go + gomobile toolchain
and the `mihomo` build tag (real engine + AmneziaWG), then copy it here:

```bash
# build the macOS slice (extend scripts/build-mobile.sh with -target=macos,
# see INTEGRATION step 0), then:
cp -R libs/caramba-core/build/exarobot.xcframework \
   apps/caramba-client/packages/caramba_vpn/macos/Frameworks/
```

Both the plugin (app target) and the System/App Extension target link this same
xcframework.
