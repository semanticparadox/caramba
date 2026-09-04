package com.caramba.caramba_vpn

// CarambaVpnContract — channel names, stage strings and intent keys shared by the
// plugin (CarambaVpnPlugin) and the VPN service (CarambaVpnService).
//
// The channel names and stage strings are the cross-platform federation contract
// consumed unchanged by apps/caramba-client/lib/vpn/vpn_service.dart. They MUST
// stay identical across all platforms.
//
// CODE IDENTIFIERS stay 'caramba' (the user-facing brand is 'exarobot').

internal object CarambaVpnChannels {
    const val METHOD = "com.caramba/vpn"
    const val STATUS_EVENTS = "com.caramba/vpn/status"
    const val TRAFFIC_EVENTS = "com.caramba/vpn/traffic"
}

// Tunnel stage strings. These MUST match the Dart VpnStage names (lowerCamel).
internal object CarambaStage {
    const val DISCONNECTED = "disconnected"
    const val CONNECTING = "connecting"
    const val CONNECTED = "connected"
    const val RECONNECTING = "reconnecting"
    const val ERROR = "error"
}

// Keys for the Intent extras the plugin passes to CarambaVpnService on start, and
// for the SharedPreferences-backed config seam written by `configure`.
internal object CarambaVpnKeys {
    // Intent action(s) for the service.
    const val ACTION_CONNECT = "com.caramba.caramba_vpn.action.CONNECT"
    const val ACTION_DISCONNECT = "com.caramba.caramba_vpn.action.DISCONNECT"

    // connect() args (MethodChannel contract).
    const val SERVER_ID = "serverId"
    const val SERVER_NAME = "serverName"
    const val COUNTRY_CODE = "countryCode"

    // connectRaw() args (rawSub path). The service imports the raw config
    // (ImportSubscription) instead of using the panel seam, then raises with an
    // empty serverId. RAW_MODE marks the CONNECT intent as a raw import.
    const val RAW_CONFIG = "rawConfig"
    const val RAW_FORMAT = "format"
    const val RAW_LABEL = "label"
    const val RAW_MODE = "rawMode"

    // importSubscription() / probe() args (generic mode, ABI v2). Both run in the
    // plugin process on a lightweight core client, without the VpnService.
    const val TIMEOUT_MS = "timeoutMs"

    // deviceSign() / deviceAgree() args (CSM/1, ABI v3). The device key lives in
    // the AndroidKeyStore and never crosses this channel: only the message to
    // sign, the peer point, and the results.
    const val MESSAGE_B64 = "messageB64"
    const val PEER_PUB_B64 = "peerPubB64"
    const val RKV = "rkv"

    // csmEnroll() / csmSetLadder() / csmAnswerCatalogChange() carry one JSON
    // string under POLICY_JSON, the same shape the ABI v3 symbols take on every
    // one of the five bridges. csmRefresh() takes a timeout, csmSelectProfile()
    // a local profile key.
    const val TIMEOUT_SEC = "timeoutSec"
    const val CSM_PROFILE_KEY = "profileKey"

    // setPolicy() / setTunnelMode() args (ABI v2). Persisted in the seam prefs so
    // CarambaVpnService applies them to the core before up().
    const val POLICY_JSON = "json"
    const val TUNNEL_MODE = "mode"
    const val MIXED_PORT = "port"

    // configure() seam (auth/config handed from the app before connect).
    const val PANEL_URL = "panelUrl"
    const val SUB_URL = "subUrl"
    // The app's VpnConfig.toArgs() sends subscriptionUuid; the plugin facade may
    // send subscriptionId. Accept either on the same channel (read UUID first).
    const val SUBSCRIPTION_UUID = "subscriptionUuid"
    const val SUBSCRIPTION_ID = "subscriptionId"
    const val ACCESS_TOKEN = "accessToken"

    // The rest of the session. The access token lives ~15 minutes, and the core
    // had nothing to renew it with: fifteen minutes after connect every call it
    // made to the panel got a 401 it could not recover from. The refresh token
    // has to reach the core because the core is what keeps running once the app
    // is gone — this service is restartable by the system in a process with no
    // Flutter engine in it, so "ask Dart for a fresh token" has nobody to ask.
    // ACCESS_EXPIRY is unix seconds; 0 means "unknown" and lets the core read
    // the JWT's own exp claim.
    const val REFRESH_TOKEN = "refreshToken"
    const val ACCESS_EXPIRY = "accessExpiryUnix"

    // SharedPreferences file holding the most recent configure() seam so the
    // service can read it even if started fresh by the system. It also carries
    // the policy JSON and tunnel mode written by setPolicy() / setTunnelMode().
    //
    // The refresh token joins the access token already stored here, in the app's
    // private prefs (MODE_PRIVATE). That is a deliberate trade, not an
    // oversight: the alternative is that a system-restarted service comes back
    // with no session at all, and the Go core it builds persists the same pair
    // to its own tokens.json in this same app-private directory anyway — next
    // to the assembled mihomo config, which carries the subscription's private
    // keys. Nothing here is readable by another app; everything here is
    // readable by anyone who has already rooted the device.
    const val PREFS = "caramba_vpn_seam"

    // Seam pref keys for the ABI v2 policy / tunnel mode (namespaced so they do
    // not collide with the method-call arg names above).
    const val PREF_POLICY_JSON = "policy.json"
    const val PREF_TUNNEL_MODE = "tunnel.mode"
    const val PREF_MIXED_PORT = "tunnel.mixedPort"
}

// A status snapshot in the exact shape of the com.caramba/vpn/status map.
internal data class CarambaStatusSnapshot(
    val stage: String,
    val detail: String?,
    val connectedSinceMs: Long,
) {
    fun asMap(): Map<String, Any?> {
        val m = HashMap<String, Any?>(3)
        m["stage"] = stage
        m["detail"] = detail
        m["connectedSinceMs"] = connectedSinceMs
        return m
    }

    companion object {
        val DISCONNECTED = CarambaStatusSnapshot(CarambaStage.DISCONNECTED, null, 0L)
    }
}

// A traffic snapshot in the exact shape of the com.caramba/vpn/traffic map.
internal data class CarambaTrafficSnapshot(
    val downBps: Long = 0,
    val upBps: Long = 0,
    val downTotal: Long = 0,
    val upTotal: Long = 0,
) {
    fun asMap(): Map<String, Any?> = mapOf(
        "downBps" to downBps,
        "upBps" to upBps,
        "downTotal" to downTotal,
        "upTotal" to upTotal,
    )

    companion object {
        val ZERO = CarambaTrafficSnapshot()
    }
}
