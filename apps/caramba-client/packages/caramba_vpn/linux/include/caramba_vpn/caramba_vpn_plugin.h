// caramba_vpn_plugin.h — public registrant for the Linux caramba_vpn plugin.
//
// caramba_vpn_plugin_register_with_registrar is the symbol the Flutter Linux
// embedder calls (derived from pluginClass: caramba_vpn in pubspec). It creates
// the GObject plugin which registers the com.caramba/vpn MethodChannel and the
// status + traffic EventChannels and bridges to libcaramba_core.so.

#ifndef FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_H_
#define FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_H_

#include <flutter_linux/flutter_linux.h>

G_BEGIN_DECLS

#ifdef FLUTTER_PLUGIN_IMPL
#define FLUTTER_PLUGIN_EXPORT __attribute__((visibility("default")))
#else
#define FLUTTER_PLUGIN_EXPORT
#endif

// G_DECLARE_FINAL_TYPE generates CarambaVpnPlugin, CARAMBA_VPN_PLUGIN() and
// caramba_vpn_plugin_get_type(); the full instance struct lives in the .cc.
G_DECLARE_FINAL_TYPE(CarambaVpnPlugin, caramba_vpn_plugin, CARAMBA, VPN_PLUGIN,
                     GObject)

FLUTTER_PLUGIN_EXPORT void caramba_vpn_plugin_register_with_registrar(
    FlPluginRegistrar* registrar);

G_END_DECLS

#endif  // FLUTTER_PLUGIN_CARAMBA_VPN_PLUGIN_H_
