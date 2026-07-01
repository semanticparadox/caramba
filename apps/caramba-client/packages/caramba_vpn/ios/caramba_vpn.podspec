#
# caramba_vpn — iOS plugin podspec.
#
# Bridges the Flutter app to the caramba-core Go engine (mihomo) on iOS. The
# plugin process registers the com.caramba/vpn channels and drives a
# NETunnelProviderManager; the actual tunnel runs in a Network Extension
# (PacketTunnelProvider) that the user adds as a separate target (see
# INTEGRATION). Both link the vendored gomobile framework exarobot.xcframework
# (gomobile bind -prefix Caramba, so the Swift module is `Caramba`).
#
# CODE IDENTIFIERS stay `caramba`; user-facing brand is `exarobot`.
#
Pod::Spec.new do |s|
  s.name             = 'caramba_vpn'
  s.version          = '0.1.0'
  s.summary          = 'exarobot native VPN tunnel bridge (mihomo via gomobile) for iOS.'
  s.description      = <<-DESC
Federated Flutter plugin that registers the com.caramba/vpn method and event
channels and drives a NETunnelProviderManager. The packet path lives in a
NEPacketTunnelProvider extension that links the vendored exarobot.xcframework.
                       DESC
  s.homepage         = 'https://exarobot.com'
  s.license          = { :type => 'Proprietary' }
  s.author           = { 'exarobot' => 'arrkotov@gmail.com' }
  s.source           = { :path => '.' }

  # Shared plugin body (darwin/Classes) + the iOS Flutter shim. The extension
  # source (darwin/Extension) is intentionally NOT compiled here: it belongs to
  # the app's Network Extension target, not the plugin/app target.
  s.source_files = [
    'Classes/**/*',
    '../darwin/Classes/**/*',
  ]

  # Vendored gomobile xcframework (Go package `mobile`, gomobile -prefix Caramba
  # so the imported Swift module + class prefix are `Caramba`). Built by
  # libs/caramba-core/scripts/build-mobile.sh ios and copied to ios/Frameworks/
  # (see INTEGRATION step 0). canImport(Caramba) resolves it.
  s.vendored_frameworks = 'Frameworks/exarobot.xcframework'

  s.dependency 'Flutter'
  s.platform = :ios, '15.0'

  s.frameworks = 'NetworkExtension'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_VERSION' => '5.0',
    # Allow `#if canImport(Caramba)` to resolve the vendored framework headers.
    'OTHER_SWIFT_FLAGS' => '$(inherited)',
  }
  s.swift_version = '5.0'
end
