# PacketTunnelProvider: Network Extension source

`PacketTunnelProvider.swift` here is the principal class of the app's Network
Extension target. It is NOT compiled into the plugin/app target (the podspecs
deliberately exclude `darwin/Extension`), because a Network Extension is a
separate binary with its own bundle id, entitlements and process.

When you add the extension target after `flutter create .` (see the repo's
`INTEGRATION.md`), add these source files to that target's Compile Sources:

- `darwin/Extension/PacketTunnelProvider.swift` (this file)
- `darwin/Classes/CarambaVpnShared.swift` (the App Group IPC + stage/traffic
  types, shared with the app-process plugin)

And link the same `exarobot.xcframework` the plugin links (the iOS or macOS
build, matching the extension platform).

The extension and the app must share the SAME App Group id, declared in each
target's Info.plist under `CARAMBA_APP_GROUP`, so status + traffic cross the
process boundary.
