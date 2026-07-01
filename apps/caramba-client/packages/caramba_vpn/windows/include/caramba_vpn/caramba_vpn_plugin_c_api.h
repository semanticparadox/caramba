// caramba_vpn_plugin_c_api.h — C registrant for the Windows caramba_vpn plugin.
//
// CarambaVpnPluginCApiRegisterWithRegistrar is the symbol the Flutter Windows
// embedder calls (derived from pluginClass: CarambaVpnPluginCApi in pubspec).
// It bridges into the C++ CarambaVpnPlugin which registers the com.caramba/vpn
// MethodChannel and the status + traffic EventChannels.

#ifndef FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_C_API_H_
#define FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_C_API_H_

#include <flutter_plugin_registrar.h>

#ifdef FLUTTER_PLUGIN_IMPL
#define FLUTTER_PLUGIN_EXPORT __declspec(dllexport)
#else
#define FLUTTER_PLUGIN_EXPORT __declspec(dllimport)
#endif

#if defined(__cplusplus)
extern "C" {
#endif

FLUTTER_PLUGIN_EXPORT void CarambaVpnPluginCApiRegisterWithRegistrar(
    FlutterDesktopPluginRegistrarRef registrar);

#if defined(__cplusplus)
}  // extern "C"
#endif

#endif  // FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_C_API_H_
