// CarambaVpnShared — types and IPC shared by the app-process plugin
// (CarambaVpnPlugin) and the packet-tunnel extension (PacketTunnelProvider).
//
// The Network Extension runs in a SEPARATE process from the Flutter app, so the
// two cannot share memory. They communicate through an App Group container:
// the extension writes the tunnel stage + traffic counters into the group's
// shared UserDefaults, and the plugin polls them at ~1 Hz and forwards the
// CHANNEL CONTRACT maps to the Flutter event channels.
//
// CODE IDENTIFIERS stay `caramba` (the user-facing brand is `exarobot`). These
// channel names and stage strings are the cross-platform federation contract
// consumed unchanged by apps/caramba-client/lib/vpn/vpn_service.dart.

#if os(iOS)
import Flutter
#elseif os(macOS)
import FlutterMacOS
#endif
import Foundation

/// Channel and key names shared across the Apple implementation.
///
/// `appGroup` is resolved at runtime (see `CarambaAppGroup.identifier`) so the
/// same source compiles for any bundle id the user picks during
/// `flutter create .`; it is not hardcoded here.
enum CarambaVpnChannels {
    static let method = "com.caramba/vpn"
    static let statusEvents = "com.caramba/vpn/status"
    static let trafficEvents = "com.caramba/vpn/traffic"
}

/// Keys written by the extension into the App Group shared UserDefaults.
enum CarambaVpnKeys {
    // Status snapshot (mirrors the EventChannel `com.caramba/vpn/status` map).
    static let stage = "caramba.vpn.stage"               // String, see CarambaStage
    static let detail = "caramba.vpn.detail"             // String?
    static let connectedSinceMs = "caramba.vpn.connectedSinceMs" // Int64 (epoch ms, 0 if n/a)

    // Traffic snapshot (mirrors the EventChannel `com.caramba/vpn/traffic` map).
    static let downBps = "caramba.vpn.downBps"           // Int64
    static let upBps = "caramba.vpn.upBps"               // Int64
    static let downTotal = "caramba.vpn.downTotal"       // Int64
    static let upTotal = "caramba.vpn.upTotal"           // Int64

    // The loopback service inbound of the current raise, credential included
    // (socks5://user:pass@127.0.0.1:port). Written by the extension after `up`,
    // read by the plugin and handed straight to the CSM core's rung R4. Never
    // forwarded to a Flutter sink.
    static let loopbackProxy = "caramba.vpn.loopbackProxy"

    // Configuration handed from the app to the extension via providerConfiguration.
    // The first three mirror the `configure` method-channel args; the rest carry
    // the connect args + optional policy.
    static let panelUrl = "panelUrl"            // Configure(panelURL, ...)
    static let subscriptionUuid = "subscriptionUuid" // Configure(_, subscriptionID, _)
    static let accessToken = "accessToken"      // Configure(_, _, accessToken) — JWT
    static let serverId = "serverId"
    static let serverName = "serverName"
    static let countryCode = "countryCode"
    static let protocolName = "protocol"
    static let relayCountry = "relay"
    static let presetId = "preset"

    // rawSub path (connectRaw): the imported subscription payload + its format and
    // a display label. Carried on the same providerConfiguration the connect path
    // uses; the extension imports the raw config instead of calling configure.
    // `rawMode` marks the provider profile as a raw import so the extension picks
    // the ImportSubscription path over the panel Configure path.
    static let rawConfig = "rawConfig"
    static let rawFormat = "format"
    static let rawLabel = "label"
    static let rawMode = "rawMode"

    // ABI v2. `policyJson` is the CorePolicy JSON the extension feeds to
    // `setPolicyJSON` before `up`; `tunnelMode` / `mixedPort` mirror
    // `SetTunnelMode(mode, port)`. All three ride providerConfiguration, so they
    // are stored as plist-safe values (String, String, String).
    static let policyJson = "policyJson"
    static let tunnelMode = "tunnelMode"
    static let mixedPort = "mixedPort"

    // Legacy alias for the subscription uuid. The app's VpnConfig.toArgs() and
    // the plugin facade both send `subscriptionUuid`; older callers sent
    // `subscriptionId`. Accept either on the `configure` channel.
    static let subscriptionId = "subscriptionId"

    // probe(timeoutMs) argument name on the method channel.
    static let timeoutMs = "timeoutMs"

    // CSM/1 device key arguments (ABI v3). The device key lives in the Secure
    // Enclave and never crosses this channel: only the message to sign, the peer
    // point and the results do.
    static let messageB64 = "messageB64"
    static let peerPubB64 = "peerPubB64"
    static let rkv = "rkv"

    // csmEnroll / csmSetLadder / csmAnswerCatalogChange carry one JSON string;
    // csmRefresh a timeout; csmSelectProfile a local profile key.
    static let timeoutSec = "timeoutSec"
    static let csmProfileKey = "profileKey"
}

/// Tunnel stage strings. These MUST stay identical to the Dart `VpnStage` names
/// (lowerCamel) the channel contract requires; do not localize or rename them.
enum CarambaStage {
    static let disconnected = "disconnected"
    static let connecting = "connecting"
    static let connected = "connected"
    static let reconnecting = "reconnecting"
    static let error = "error"
}

/// Resolves the App Group identifier shared by the app and the extension.
///
/// The group id is read from the host bundle's Info.plist key
/// `CARAMBA_APP_GROUP` (the user sets it in INTEGRATION.md, identically on the
/// app target and the extension target), so this code carries no hardcoded team
/// or bundle id. When the key is absent the helpers below degrade to standard
/// UserDefaults so a debug build without an App Group still runs (status/traffic
/// just will not cross the process boundary).
enum CarambaAppGroup {
    static var identifier: String? {
        Bundle.main.object(forInfoDictionaryKey: "CARAMBA_APP_GROUP") as? String
    }

    /// Shared defaults backed by the App Group container, or standard defaults
    /// as a fallback so the code path never crashes when the group is missing.
    static var defaults: UserDefaults {
        if let id = identifier, let shared = UserDefaults(suiteName: id) {
            return shared
        }
        return .standard
    }

    /// App Group container directory, used by the extension as the Go core
    /// `workDir` / token store so both processes agree on on-disk state.
    static var containerURL: URL? {
        guard let id = identifier else { return nil }
        return FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: id)
    }
}

/// A status snapshot in the exact shape of the `com.caramba/vpn/status` map.
struct CarambaStatusSnapshot {
    var stage: String
    var detail: String?
    var connectedSinceMs: Int64

    static let disconnected = CarambaStatusSnapshot(
        stage: CarambaStage.disconnected, detail: nil, connectedSinceMs: 0)

    /// Map matching the EventChannel contract consumed by VpnStatus.fromMap.
    var asMap: [String: Any] {
        var m: [String: Any] = [
            "stage": stage,
            "connectedSinceMs": connectedSinceMs,
        ]
        if let detail = detail { m["detail"] = detail }
        return m
    }
}

/// A traffic snapshot in the exact shape of the `com.caramba/vpn/traffic` map.
struct CarambaTrafficSnapshot {
    var downBps: Int64 = 0
    var upBps: Int64 = 0
    var downTotal: Int64 = 0
    var upTotal: Int64 = 0

    static let zero = CarambaTrafficSnapshot()

    var asMap: [String: Any] {
        [
            "downBps": downBps,
            "upBps": upBps,
            "downTotal": downTotal,
            "upTotal": upTotal,
        ]
    }
}

/// Reads/writes the snapshots through the App Group shared defaults. The
/// extension is the sole writer; the plugin is the sole reader (it polls).
enum CarambaSharedState {
    static func writeStatus(_ s: CarambaStatusSnapshot) {
        let d = CarambaAppGroup.defaults
        d.set(s.stage, forKey: CarambaVpnKeys.stage)
        if let detail = s.detail {
            d.set(detail, forKey: CarambaVpnKeys.detail)
        } else {
            d.removeObject(forKey: CarambaVpnKeys.detail)
        }
        d.set(s.connectedSinceMs, forKey: CarambaVpnKeys.connectedSinceMs)
    }

    static func readStatus() -> CarambaStatusSnapshot {
        let d = CarambaAppGroup.defaults
        let stage = d.string(forKey: CarambaVpnKeys.stage) ?? CarambaStage.disconnected
        let detail = d.string(forKey: CarambaVpnKeys.detail)
        let since = (d.object(forKey: CarambaVpnKeys.connectedSinceMs) as? NSNumber)?.int64Value ?? 0
        return CarambaStatusSnapshot(stage: stage, detail: detail, connectedSinceMs: since)
    }

    static func writeTraffic(_ t: CarambaTrafficSnapshot) {
        let d = CarambaAppGroup.defaults
        d.set(t.downBps, forKey: CarambaVpnKeys.downBps)
        d.set(t.upBps, forKey: CarambaVpnKeys.upBps)
        d.set(t.downTotal, forKey: CarambaVpnKeys.downTotal)
        d.set(t.upTotal, forKey: CarambaVpnKeys.upTotal)
    }

    static func readTraffic() -> CarambaTrafficSnapshot {
        let d = CarambaAppGroup.defaults
        func n(_ k: String) -> Int64 { (d.object(forKey: k) as? NSNumber)?.int64Value ?? 0 }
        return CarambaTrafficSnapshot(
            downBps: n(CarambaVpnKeys.downBps),
            upBps: n(CarambaVpnKeys.upBps),
            downTotal: n(CarambaVpnKeys.downTotal),
            upTotal: n(CarambaVpnKeys.upTotal))
    }

    /// The loopback service inbound address, credential included, of the raise
    /// that is up right now. Empty while the tunnel is down.
    ///
    /// It crosses the App Group for the same reason status does: the tunnel core
    /// lives in the network extension and the CSM core lives in the plugin
    /// process, and `up` is never called on the CSM core. Without the handoff,
    /// that core's rung R4 is permanently `not_configured` and the ladder
    /// silently degrades to R1 and R5 (02-SPEC.md 8.2).
    ///
    /// The value is a per-raise credential. It never reaches a Flutter sink: the
    /// plugin reads it only to hand it straight back to the core.
    static func writeLoopbackProxy(_ url: String) {
        let d = CarambaAppGroup.defaults
        if url.isEmpty {
            d.removeObject(forKey: CarambaVpnKeys.loopbackProxy)
        } else {
            d.set(url, forKey: CarambaVpnKeys.loopbackProxy)
        }
    }

    static func readLoopbackProxy() -> String {
        CarambaAppGroup.defaults.string(forKey: CarambaVpnKeys.loopbackProxy) ?? ""
    }

    /// Resets shared state to the disconnected baseline (called on stop).
    static func reset() {
        writeStatus(.disconnected)
        writeTraffic(.zero)
        writeLoopbackProxy("")
    }
}
