#
# caramba_vpn — macOS plugin podspec.
#
# macOS counterpart of the iOS plugin. Same shared Swift body, same channel
# contract, same NETunnelProviderManager drive. On macOS the packet-tunnel
# extension is a System Extension (or a Developer ID app extension) and needs the
# networkextension entitlement plus user approval (see INTEGRATION). Links the
# vendored gomobile framework exarobot.xcframework (gomobile -prefix Caramba, so
# the Swift module is `Caramba`).
#
# CODE IDENTIFIERS stay `caramba`; user-facing brand is `exarobot`.
#
Pod::Spec.new do |s|
  s.name             = 'caramba_vpn'
  s.version          = '0.1.0'
  s.summary          = 'exarobot native VPN tunnel bridge (mihomo via gomobile) for macOS.'
  s.description      = <<-DESC
Federated Flutter plugin that registers the com.caramba/vpn method and event
channels and drives a NETunnelProviderManager. The packet path lives in a
NEPacketTunnelProvider system/app extension that links exarobot.xcframework.
                       DESC
  s.homepage         = 'https://exarobot.com'
  s.license          = { :type => 'Proprietary' }
  s.author           = { 'exarobot' => 'arrkotov@gmail.com' }
  s.source           = { :path => '.' }

  # Shared plugin body (darwin/Classes) + the macOS Flutter shim. The extension
  # source (darwin/Extension) is compiled into the System Extension target, not here.
  s.source_files = [
    'Classes/**/*',
    '../darwin/Classes/**/*',
  ]

  # Vendored gomobile xcframework (gomobile -prefix Caramba so the imported Swift
  # module + class prefix are `Caramba`), built for macOS and copied to
  # macos/Frameworks/ (see INTEGRATION step 0). canImport(Caramba) resolves it.
  s.vendored_frameworks = 'Frameworks/exarobot.xcframework'

  s.dependency 'FlutterMacOS'
  s.platform = :osx, '11.0'

  s.frameworks = 'NetworkExtension'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_VERSION' => '5.0',
    'OTHER_SWIFT_FLAGS' => '$(inherited)',
  }
  s.swift_version = '5.0'
end
