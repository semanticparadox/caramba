#
# caramba_vpn: shared Darwin podspec (iOS + macOS), Flutter `sharedDarwinSource`.
#
# One pod, one Swift body (Classes/*.swift) plus a per-platform Flutter shim
# (Classes/ios, Classes/macos). The packet-tunnel extension source
# (Extension/) is compiled into the app's Network Extension target, never here.
#
# iOS links the vendored gomobile xcframework (Frameworks/ios/exarobot.xcframework,
# module `Caramba`); the plugin body compiles without it (`#if canImport(Caramba)`).
# macOS additionally vendors libcaramba_core.dylib for the in-process dart:ffi
# path (proxy mode, no Network Extension); the xcframework is optional there.
#
# CODE IDENTIFIERS stay `caramba`; the user-facing brand is a runtime value.
#
Pod::Spec.new do |s|
  s.name             = 'caramba_vpn'
  s.version          = '0.2.0'
  s.summary          = 'Caramba Connect native VPN bridge (mihomo core) for iOS and macOS.'
  s.description      = <<-DESC
Federated Flutter plugin registering the com.caramba/vpn method and event
channels. iOS drives a NETunnelProviderManager; macOS can run the core
in-process through dart:ffi (libcaramba_core.dylib) or use the same extension.
                       DESC
  s.homepage         = 'https://github.com/semanticparadox/caramba'
  s.license          = { :type => 'Proprietary' }
  s.author           = { 'Caramba' => 'arrkotov@gmail.com' }
  s.source           = { :path => '.' }

  s.source_files      = 'Classes/*.swift'
  s.ios.source_files  = 'Classes/*.swift', 'Classes/ios/*.swift'
  s.osx.source_files  = 'Classes/*.swift', 'Classes/macos/*.swift'

  s.ios.deployment_target = '15.0'
  s.osx.deployment_target = '12.0'

  s.ios.dependency 'Flutter'
  s.osx.dependency 'FlutterMacOS'
  s.frameworks = 'NetworkExtension'

  # Vendored artifacts (build outputs, gitignored; see INTEGRATION step 0).
  s.ios.vendored_frameworks = 'Frameworks/ios/exarobot.xcframework'
  s.osx.vendored_frameworks = 'Frameworks/macos/exarobot.xcframework'
  s.osx.vendored_libraries  = 'Libraries/libcaramba_core.dylib'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_VERSION' => '5.0',
    'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386',
  }
  s.swift_version = '5.0'
end
