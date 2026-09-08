#
# caramba_vpn: shared Darwin podspec (iOS + macOS), Flutter `sharedDarwinSource`.
#
# One pod, one Swift body (Classes/*.swift) plus a per-platform Flutter shim
# (Classes/ios, Classes/macos). The packet-tunnel extension source
# (Extension/) is compiled into the app's Network Extension target, never here.
#
# ЯДРО. Биндинг лежит в Frameworks/<platform>/exarobot.xcframework (gomobile
# bind пакета libs/caramba-core/mobile). Имя файла задаёт имя Swift-модуля, а
# -prefix Caramba плюс имя Go-пакета `mobile` — префикс классов, поэтому в Swift
# это `import Exarobot` и `CarambaMobileClient` / `CarambaMobileNewClient`.
# macOS дополнительно вендорит libcaramba_core.dylib для пути dart:ffi
# (proxy-режим, без Network Extension) — это ДРУГОЙ артефакт, не xcframework.
#
# MOCK ПРОТИВ NATIVE. Флаг --dart-define=USE_NATIVE_VPN живёт только в Dart:
# Swift его не видит и увидеть не может. Поэтому решение принимается ЗДЕСЬ, во
# время `pod install`, и передаётся в Swift условием компиляции. На iOS:
#
#   xcframework на месте            → -DCARAMBA_CORE          (нативная сборка)
#   xcframework нет, USE_NATIVE_VPN
#     явно выключен (false/0/no/off) → ни одного флага        (mock-сборка)
#   xcframework нет, всё остальное  → -DCARAMBA_CORE_REQUIRED (#error в Swift)
#
# Умолчание «ядро требуется» повторяет умолчание scripts/build.sh
# (USE_NATIVE_VPN=true), и именно оно убирает молчаливую деградацию: раньше тут
# стоял `#if canImport(Caramba)`, и сборка без ядра проходила зелёной, а
# приложение отвечало core_missing уже на устройстве.
#
# На macOS третьей строки НЕТ, и это не послабление, а другая архитектура: там
# нативный путь по умолчанию — dart:ffi поверх libcaramba_core.dylib, который
# грузит сам Dart, минуя Swift. Отсутствие xcframework на macOS означает
# «работаем через ffi», а не «ядра нет», поэтому #error там был бы ложной
# тревогой. На iOS другого пути к ядру не существует: только этот фреймворк.
#
# Вендорить на macOS xcframework И dylib одновременно можно, но это два
# независимых рантайма Go в одном процессе (свои обработчики сигналов, ~100 МБ
# лишнего кода). Пока путь macOS — proxy через ffi, xcframework для macOS
# собирается только под цель Network/System Extension и по умолчанию не лежит.
#
# CODE IDENTIFIERS stay `caramba`; the user-facing brand is a runtime value.
#
core_dir  = File.expand_path('Frameworks', __dir__)
has_ios   = Dir.exist?(File.join(core_dir, 'ios',   'exarobot.xcframework'))
has_macos = Dir.exist?(File.join(core_dir, 'macos', 'exarobot.xcframework'))

# Пустая строка и «не задано» — это НЕ выключено: build.sh по умолчанию собирает
# нативно, и podspec обязан думать так же, иначе mock уедет в релиз молча.
native_off = %w[false 0 no off].include?((ENV['USE_NATIVE_VPN'] || '').strip.downcase)

ios_flags = if has_ios
              '-DCARAMBA_CORE'
            elsif native_off
              ''
            else
              '-DCARAMBA_CORE_REQUIRED'
            end
macos_flags = has_macos ? '-DCARAMBA_CORE' : ''

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
  # Резолвер Go (net.Resolver) зовёт res_9_ninit/res_9_nsearch/res_9_nclose из
  # libresolv, а Apple не линкует её по умолчанию. Без этой строки сборка с
  # вендоренным ядром падает на линковке приложения «Undefined symbol: _res_9_*».
  s.libraries = 'resolv'

  # Vendored artifacts (build outputs, gitignored; see INTEGRATION step 0).
  # Объявляются ТОЛЬКО когда файл действительно на месте: CocoaPods на
  # несуществующий vendored_frameworks ругается на этапе install, и сообщение у
  # него хуже, чем наш #error.
  s.ios.vendored_frameworks   = 'Frameworks/ios/exarobot.xcframework'   if has_ios
  s.osx.vendored_frameworks   = 'Frameworks/macos/exarobot.xcframework' if has_macos
  s.osx.vendored_libraries    = 'Libraries/libcaramba_core.dylib' \
    if File.exist?(File.expand_path('Libraries/libcaramba_core.dylib', __dir__))

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'SWIFT_VERSION' => '5.0',
    'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386',
  }
  # OTHER_SWIFT_FLAGS, а не SWIFT_ACTIVE_COMPILATION_CONDITIONS: podhelper
  # Flutter'а перезаписывает второй ключ целиком в post_install, и наш флаг
  # оттуда пропадал бы.
  s.ios.pod_target_xcconfig = { 'OTHER_SWIFT_FLAGS' => "$(inherited) #{ios_flags}" }
  s.osx.pod_target_xcconfig = { 'OTHER_SWIFT_FLAGS' => "$(inherited) #{macos_flags}" }
  s.swift_version = '5.0'
end
