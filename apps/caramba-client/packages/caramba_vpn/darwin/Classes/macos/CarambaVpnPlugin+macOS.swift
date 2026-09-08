// macOS Flutter shim for the shared CarambaVpnPlugin.
//
// Mirrors the iOS shim but imports `FlutterMacOS` and exposes the one registrar
// difference: on macOS the binary messenger is the `messenger` property (not a
// method as on iOS). The shared plugin body is otherwise identical across the
// two platforms; both drive a NETunnelProviderManager and a PacketTunnelProvider.
//
// macOS note: a packet-tunnel Network Extension on macOS ships as a System
// Extension (or an App Extension for Mac-Catalyst / Developer ID) and needs the
// `com.apple.developer.networking.networkextension` entitlement plus user
// approval in System Settings. The Swift code is shared; only the target type
// and entitlements differ (documented in INTEGRATION).

import FlutterMacOS
import Foundation

/// Returns the binary messenger for the macOS registrar.
func pluginMessenger(_ registrar: FlutterPluginRegistrar) -> FlutterBinaryMessenger {
    registrar.messenger
}
