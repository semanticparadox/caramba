# Vendored gomobile AAR

This directory holds `caramba.aar`, the gomobile-generated Android binding of
the Go core (`libs/caramba-core/mobile`). The plugin's `build.gradle` picks it up
via a `flatDir` repository and depends on it as `implementation(name: 'caramba',
ext: 'aar')`.

The AAR is a build artifact and is intentionally NOT committed. Build it on your
machine (Go + gomobile + Android NDK required) from the repo root:

```bash
cd libs/caramba-core
gomobile bind -target=android -androidapi 21 -tags mihomo \
  -o apps/caramba-client/packages/caramba_vpn/android/libs/caramba.aar \
  ./mobile
```

Notes:

- The `-tags mihomo` flag is REQUIRED. Without it the bundled engine is the stub
  (`engine_stub.go`), which tracks state but does not move packets. With the tag,
  `engine_mihomo.go` compiles the real mihomo core (TUN fd in-bound + statistics).
- `gomobile bind` camelCases the exported Go identifiers: `Mobile.newClient(...)`
  returns a `Client`; methods are `up`, `down`, `statusJSON`, `trafficJSON`,
  `setTunFd`, `configure`. `CarambaCore.kt` is the only file that imports the
  generated `mobile` package.
- Re-run the command whenever `libs/caramba-core/mobile` changes.
