# CMakeLists.txt — Windows build for the caramba_vpn Flutter plugin.
#
# Builds the CarambaVpnPluginCApi plugin DLL and bundles the two runtime
# dependencies next to the host .exe:
#   * libcaramba_core.dll — the cgo c-shared Go engine (mihomo). Built with
#     scripts/build-desktop.sh equivalent (CGO_ENABLED=1, -buildmode=c-shared,
#     -tags mihomo) for GOOS=windows.
#   * wintun.dll          — the user-mode TUN driver mihomo opens to create the
#     tunnel interface. Ship the arch-matching build (amd64/arm64).
#
# Place both DLLs under windows/lib/ before `flutter build windows` (see
# INTEGRATION.md step 0 vendoring). The host app must run elevated
# (requestedExecutionLevel=requireAdministrator, or a UAC relaunch) so wintun can
# create the adapter — see INTEGRATION.md.

cmake_minimum_required(VERSION 3.14)

# Project-level name used by the Flutter tooling to locate the plugin target.
set(PROJECT_NAME "caramba_vpn")
project(${PROJECT_NAME} LANGUAGES CXX)

# This value is the name of the native plugin library; Flutter links it.
set(PLUGIN_NAME "caramba_vpn_plugin")

add_library(${PLUGIN_NAME} SHARED
  "caramba_vpn_plugin.cpp"
  "caramba_vpn_plugin_c_api.cpp"
)

apply_standard_settings(${PLUGIN_NAME})

set_target_properties(${PLUGIN_NAME} PROPERTIES
  CXX_VISIBILITY_PRESET hidden)
target_compile_definitions(${PLUGIN_NAME} PRIVATE FLUTTER_PLUGIN_IMPL)

target_include_directories(${PLUGIN_NAME} INTERFACE
  "${CMAKE_CURRENT_SOURCE_DIR}/include")
# The Go FFI header (caramba_core.h) lives under include/ and is consumed by the
# plugin sources directly.
target_include_directories(${PLUGIN_NAME} PRIVATE
  "${CMAKE_CURRENT_SOURCE_DIR}"
  "${CMAKE_CURRENT_SOURCE_DIR}/include")

target_link_libraries(${PLUGIN_NAME} PRIVATE flutter flutter_wrapper_plugin)

# List of absolute paths to libraries that should be bundled with the plugin.
# Flutter copies these next to the host .exe at install time, so libcaramba_core
# resolves them via LoadLibrary at runtime.
set(caramba_vpn_bundled_libraries
  "${CMAKE_CURRENT_SOURCE_DIR}/lib/libcaramba_core.dll"
  "${CMAKE_CURRENT_SOURCE_DIR}/lib/wintun.dll"
  PARENT_SCOPE
)
