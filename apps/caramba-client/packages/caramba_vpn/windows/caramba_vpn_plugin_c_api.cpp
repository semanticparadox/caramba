// caramba_vpn_plugin_c_api.cpp — C entrypoint the Flutter Windows embedder calls.

#include "include/caramba_vpn/caramba_vpn_plugin_c_api.h"

#include <flutter/plugin_registrar_windows.h>

#include "caramba_vpn_plugin.h"

void CarambaVpnPluginCApiRegisterWithRegistrar(
    FlutterDesktopPluginRegistrarRef registrar) {
  caramba_vpn::CarambaVpnPlugin::RegisterWithRegistrar(
      flutter::PluginRegistrarManager::GetInstance()
          ->GetRegistrar<flutter::PluginRegistrarWindows>(registrar));
}
