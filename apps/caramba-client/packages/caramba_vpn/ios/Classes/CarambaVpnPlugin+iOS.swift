// iOS Flutter shim for the shared CarambaVpnPlugin.
//
// The shared plugin body (darwin/Classes/CarambaVpnPlugin.swift) references
// `FlutterMethodChannel`, `FlutterEventChannel`, `FlutterPluginRegistrar` etc.
// without importing a Flutter module so it compiles for both iOS (`Flutter`) and
// macOS (`FlutterMacOS`). This file provides the iOS import and the one
// registrar difference (iOS exposes the messenger as `messenger()`).

import Flutter
import Foundation

/// Returns the binary messenger for the iOS registrar.
func pluginMessenger(_ registrar: FlutterPluginRegistrar) -> FlutterBinaryMessenger {
    registrar.messenger()
}
