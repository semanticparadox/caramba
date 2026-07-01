# CMakeLists.txt — Linux build for the caramba_vpn Flutter plugin.
#
# Builds the caramba_vpn GObject plugin and bundles the Go engine shared object
# next to the host binary:
#   * libcaramba_core.so — the cgo c-shared Go engine (mihomo). Built with
#     CGO_ENABLED=1 -buildmode=c-shared -tags mihomo for the host arch. mihomo
#     creates the tun device itself, so the plugin passes tunFd = -1.
#
# Place the .so under linux/lib/ before `flutter build linux` (see
# INTEGRATION.md step 0 vendoring). The host binary needs root or CAP_NET_ADMIN
# to create the tun device (pkexec at launch, or `setcap cap_net_admin+ep` on the
# binary) — see INTEGRATION.md.

cmake_minimum_required(VERSION 3.13)

# Project-level name used by the Flutter tooling to locate the plugin target.
set(PROJECT_NAME "caramba_vpn")
project(${PROJECT_NAME} LANGUAGES CXX)

# This value is the name of the native plugin library; Flutter links it.
set(PLUGIN_NAME "caramba_vpn_plugin")

# GTK / GLib for the GObject plugin + EventChannel sources.
find_package(PkgConfig REQUIRED)
pkg_check_modules(GTK REQUIRED IMPORTED_TARGET gtk+-3.0)

add_library(${PLUGIN_NAME} SHARED
  "caramba_vpn_plugin.cc"
)

apply_standard_settings(${PLUGIN_NAME})

set_target_properties(${PLUGIN_NAME} PROPERTIES
  CXX_VISIBILITY_PRESET hidden)
target_compile_definitions(${PLUGIN_NAME} PRIVATE FLUTTER_PLUGIN_IMPL)

target_include_directories(${PLUGIN_NAME} INTERFACE
  "${CMAKE_CURRENT_SOURCE_DIR}/include")
# The Go FFI header (caramba_core.h) and JSON helpers live alongside the source.
target_include_directories(${PLUGIN_NAME} PRIVATE
  "${CMAKE_CURRENT_SOURCE_DIR}"
  "${CMAKE_CURRENT_SOURCE_DIR}/include")

target_link_libraries(${PLUGIN_NAME} PRIVATE flutter)
target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::GTK)
# dlopen for libcaramba_core.so.
target_link_libraries(${PLUGIN_NAME} PRIVATE ${CMAKE_DL_LIBS})

# List of absolute paths to libraries that should be bundled with the plugin.
# Flutter installs these into the bundle's lib/ next to the host binary, so the
# dlopen("libcaramba_core.so") resolves via the rpath at runtime.
set(caramba_vpn_bundled_libraries
  "${CMAKE_CURRENT_SOURCE_DIR}/lib/libcaramba_core.so"
  PARENT_SCOPE
)
