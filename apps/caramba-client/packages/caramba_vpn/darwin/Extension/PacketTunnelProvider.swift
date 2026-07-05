// PacketTunnelProvider — the Network Extension that owns the real tunnel.
//
// This class is the packet-tunnel-provider principal of the app's Network
// Extension target (NOT the plugin). It is documented for the user to add as an
// extension target after `flutter create .` (see INTEGRATION). It is compiled
// into the extension binary, links the vendored exarobot.xcframework (gomobile
// bind of the `mobile` package with -prefix Caramba, so the Swift module is
// `Caramba`), and runs in its own process.
//
// Flow:
//   1. startTunnel: read providerConfiguration (serverId + panel/sub URLs etc.)
//      that the app's CarambaVpnPlugin stored on the NETunnelProviderProtocol.
//   2. Apply NEPacketTunnelNetworkSettings so the OS routes packets to us.
//   3. Build the Go core (CarambaNewClient + Configure), hand it the tunnel file
//      descriptor (packetFlow's underlying utun fd) via SetTunFd, then Up(serverId).
//      mihomo reads/writes that same fd, so packets actually flow through the
//      AmneziaWG/VLESS/etc. proxy the panel's clash config selects.
//   4. Poll the Go core for stage + traffic and publish them into the App Group
//      shared store, which the app-process plugin reads and forwards to Flutter.
//
// CODE IDENTIFIERS stay `caramba`; user-facing strings say `exarobot`.

import Foundation
import NetworkExtension
import os.log

// The gomobile-bound Go core. `import Caramba` resolves the vendored
// exarobot.xcframework (gomobile bind of package `mobile` with -prefix Caramba,
// so the module + class prefix are `Caramba`). Guarded so the file still
// type-checks in tooling that lacks the framework; the real extension build links it.
#if canImport(Caramba)
import Caramba
#endif

@available(iOS 15.0, macOS 11.0, *)
final class PacketTunnelProvider: NEPacketTunnelProvider {
    private let log = OSLog(subsystem: "com.caramba.vpn", category: "tunnel")

    #if canImport(Caramba)
    private var core: CarambaClient?
    #endif

    private var pollTimer: DispatchSourceTimer?
    private let pollQueue = DispatchQueue(label: "com.caramba.vpn.poll")
    private var connectedSinceMs: Int64 = 0

    // MARK: - Start

    override func startTunnel(
        options: [String: NSObject]?,
        completionHandler: @escaping (Error?) -> Void
    ) {
        let conf = providerConfiguration()
        let serverId = conf[CarambaVpnKeys.serverId] as? String ?? ""
        // rawMode selects the imported-subscription path (connectRaw): the extension
        // imports the raw config instead of calling Configure, then raises with an
        // empty serverId. Any non-empty rawMode flag means raw.
        let rawMode = !((conf[CarambaVpnKeys.rawMode] as? String) ?? "").isEmpty

        publish(stage: CarambaStage.connecting,
                detail: rawMode ? "Importing profile" : "Securing tunnel")

        // 1. Apply network settings so the system hands packets to packetFlow. For a
        // raw import there is no panel serverName; use the display label if present.
        let remote = rawMode
            ? (conf[CarambaVpnKeys.rawLabel] as? String)
            : (conf[CarambaVpnKeys.serverName] as? String)
        let settings = makeNetworkSettings(remoteAddress: remote)
        setTunnelNetworkSettings(settings) { [weak self] settingsError in
            guard let self = self else { return }
            if let settingsError = settingsError {
                os_log("setTunnelNetworkSettings failed: %{public}@", log: self.log, type: .error,
                       settingsError.localizedDescription)
                self.publish(stage: CarambaStage.error, detail: "tunnel settings failed")
                completionHandler(settingsError)
                return
            }

            // 2. Build and start the Go core off the completion thread.
            self.pollQueue.async {
                do {
                    try self.startCore(serverId: serverId, rawMode: rawMode, conf: conf)
                    self.connectedSinceMs = Int64(Date().timeIntervalSince1970 * 1000)
                    self.publish(stage: CarambaStage.connected, detail: nil)
                    self.startPolling()
                    completionHandler(nil)
                } catch {
                    os_log("core start failed: %{public}@", log: self.log, type: .error,
                           error.localizedDescription)
                    self.publish(stage: CarambaStage.error, detail: error.localizedDescription)
                    completionHandler(error)
                }
            }
        }
    }

    // MARK: - Stop

    override func stopTunnel(
        with reason: NEProviderStopReason,
        completionHandler: @escaping () -> Void
    ) {
        os_log("stopTunnel reason=%d", log: log, type: .info, reason.rawValue)
        stopPolling()
        pollQueue.async {
            #if canImport(Caramba)
            // Down() shuts mihomo's listeners (including the TUN inbound).
            try? self.core?.down()
            self.core = nil
            #endif
            self.connectedSinceMs = 0
            CarambaSharedState.reset()
            completionHandler()
        }
    }

    // MARK: - App messages (optional control path from the app process)

    override func handleAppMessage(_ messageData: Data, completionHandler: ((Data?) -> Void)?) {
        // The app polls shared state for status; a direct ack keeps the channel
        // warm and lets the plugin probe liveness. Echo the current stage.
        let snap = CarambaSharedState.readStatus()
        completionHandler?(Data(snap.stage.utf8))
    }

    // MARK: - Core lifecycle

    private func startCore(serverId: String, rawMode: Bool, conf: [String: Any]) throws {
        #if canImport(Caramba)
        let panelUrl = conf[CarambaVpnKeys.panelUrl] as? String ?? ""
        let subUuid = conf[CarambaVpnKeys.subscriptionUuid] as? String ?? ""
        let accessToken = conf[CarambaVpnKeys.accessToken] as? String ?? ""

        // Use the App Group container so the extension and app agree on token
        // store + work dir on disk. The gomobile prefix is `Caramba` (see the
        // build script -prefix Caramba), so the type is CarambaClient and the
        // constructor is CarambaNewClient.
        let base = CarambaAppGroup.containerURL ?? FileManager.default.temporaryDirectory
        let workDir = base.appendingPathComponent("caramba", isDirectory: true).path
        let tokenPath = base.appendingPathComponent("caramba/token.json").path

        var initError: NSError?
        // gomobile maps Go `NewClient(panelURL,subURL,workDir,tokenPath) (*Client, error)`
        // to `CarambaNewClient(_,_,_,_, error:) -> CarambaClient?`. subURL is left
        // empty so the core uses the panel default. For a raw import panelUrl is
        // empty; NewClient still succeeds (it only wires the client, no network).
        guard let client = CarambaNewClient(panelUrl, "", workDir, tokenPath, &initError) else {
            throw initError ?? carambaError("core init failed")
        }
        self.core = client

        if rawMode {
            // rawSub path: import the raw subscription into a mihomo config instead
            // of calling Configure. gomobile maps Go
            // `ImportSubscription(raw, format string) (string, error)` to
            // `importSubscription(_ raw: String, format: String) throws -> String`
            // (a Go error surfaces as a thrown Swift error). We ignore the returned
            // metadata JSON here; a throw aborts to the error stage.
            let raw = conf[CarambaVpnKeys.rawConfig] as? String ?? ""
            let format = conf[CarambaVpnKeys.rawFormat] as? String ?? ""
            _ = try client.importSubscription(raw, format: format)
        } else {
            // Configure(panelURL, subscriptionID, accessToken): the binding's single
            // auth entry point. The extension runs in its own process, so the app
            // hands it the JWT + subscription uuid here rather than re-running login.
            try client.configure(panelUrl, subscriptionID: subUuid, accessToken: accessToken)
        }

        // Optional routing policy (applies to both paths).
        if let proto = conf[CarambaVpnKeys.protocolName] as? String, !proto.isEmpty {
            client.setProtocol(proto)
        }
        if let relay = conf[CarambaVpnKeys.relayCountry] as? String, !relay.isEmpty {
            client.setRelay(relay)
        }
        if let preset = conf[CarambaVpnKeys.presetId] as? String, !preset.isEmpty {
            try? client.applyPreset(preset)
        }

        // Hand mihomo the tunnel file descriptor. packetFlow's underlying utun
        // socket is the same fd mihomo's TUN inbound reads/writes, so once Up()
        // applies the (panel or imported) clash config, packets flow end to end.
        let fd = tunnelFileDescriptor()
        guard fd >= 0 else { throw carambaError("no tunnel file descriptor") }
        // gomobile maps Go `SetTunFd(fd int) error` to `setTunFd(_ fd: Int) throws`.
        try client.setTunFd(Int(fd))

        // Up raises the tunnel from the active config. On the panel path we pass the
        // selected serverId; on the raw path serverId is empty ("") since an
        // imported subscription has no panel node. gomobile maps
        // `Up(serverID string) (string, error)` to a throwing Swift method returning
        // the UpResult JSON; we only need its success/throw.
        _ = try client.up(rawMode ? "" : serverId)
        #else
        _ = serverId
        _ = rawMode
        _ = conf
        throw carambaError("exarobot.xcframework not linked")
        #endif
    }

    // MARK: - Polling (stage + traffic -> App Group)

    private func startPolling() {
        stopPolling()
        let timer = DispatchSource.makeTimerSource(queue: pollQueue)
        timer.schedule(deadline: .now() + 1.0, repeating: 1.0)
        timer.setEventHandler { [weak self] in self?.tick() }
        pollTimer = timer
        timer.resume()
    }

    private func stopPolling() {
        pollTimer?.cancel()
        pollTimer = nil
    }

    /// One ~1 Hz sample: read the Go core's stage + traffic and publish them.
    private func tick() {
        #if canImport(Caramba)
        guard let client = core else { return }

        // Stage: prefer the contract-shaped statusJSON() if the binding exposes
        // it; otherwise derive the stage from the engine Status() JSON.
        let stage = readStage(from: client)
        publish(stage: stage, detail: nil)

        // Traffic: prefer trafficJSON(); fall back to zeros if absent.
        if let t = readTraffic(from: client) {
            CarambaSharedState.writeTraffic(t)
        }
        #endif
    }

    #if canImport(Caramba)
    /// Reads the tunnel stage. Prefers the contract-shaped `StatusJSON()` from the
    /// go-binding surface (gomobile selector `statusJSON`), and falls back to the
    /// engine `Status()` JSON (`api.StatusResult`,
    /// engine.state = stopped|starting|connected|error) so the path still works on
    /// an older binding. Either way the result is normalized to a CHANNEL CONTRACT
    /// stage string.
    private func readStage(from client: CarambaClient) -> String {
        if let direct = try? client.statusJSON(),
           let stage = Self.stageFromContractJSON(direct) {
            return stage
        }
        if let raw = try? client.status() {
            return Self.stageFromEngineJSON(raw)
        }
        return CarambaStage.error
    }

    /// Reads traffic from the contract-shaped `TrafficJSON()` (gomobile selector
    /// `trafficJSON`). Returns nil when unavailable so the caller leaves the last
    /// counters untouched rather than zeroing a live tunnel.
    private func readTraffic(from client: CarambaClient) -> CarambaTrafficSnapshot? {
        guard let raw = try? client.trafficJSON() else { return nil }
        return Self.trafficFromJSON(raw)
    }
    #endif

    // MARK: - JSON normalization (static, testable, framework-free)

    /// Maps a contract-shaped `{ "stage": "...", ... }` payload to a stage string.
    static func stageFromContractJSON(_ json: String) -> String? {
        guard let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let stage = obj["stage"] as? String else { return nil }
        return normalizeStage(stage)
    }

    /// Maps `api.StatusResult` JSON (`engine.state`) to a CHANNEL CONTRACT stage.
    static func stageFromEngineJSON(_ json: String) -> String {
        guard let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let engine = obj["engine"] as? [String: Any],
              let state = engine["state"] as? String else {
            return CarambaStage.error
        }
        switch state {
        case "connected": return CarambaStage.connected
        case "starting": return CarambaStage.connecting
        case "error": return CarambaStage.error
        case "stopped": return CarambaStage.disconnected
        default: return CarambaStage.disconnected
        }
    }

    /// Coerces an arbitrary stage token to a known contract value.
    static func normalizeStage(_ s: String) -> String {
        switch s {
        case CarambaStage.connecting, CarambaStage.connected,
             CarambaStage.reconnecting, CarambaStage.error, CarambaStage.disconnected:
            return s
        default:
            return CarambaStage.disconnected
        }
    }

    /// Parses a traffic JSON payload `{downBps,upBps,downTotal,upTotal}`.
    static func trafficFromJSON(_ json: String) -> CarambaTrafficSnapshot? {
        guard let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        func n(_ k: String) -> Int64 { (obj[k] as? NSNumber)?.int64Value ?? 0 }
        return CarambaTrafficSnapshot(
            downBps: n("downBps"), upBps: n("upBps"),
            downTotal: n("downTotal"), upTotal: n("upTotal"))
    }

    // MARK: - Helpers

    /// The packet-tunnel file descriptor. NEPacketTunnelFlow does not publicly
    /// expose it, so we read the `tunnelFileDescriptor` of the utun socket the
    /// extension owns by scanning the process file descriptors for the one whose
    /// protocol is the system's `utun` control. This is the standard technique
    /// used by WireGuard/sing-box Apple tunnels.
    private func tunnelFileDescriptor() -> Int32 {
        var ctlInfo = ctl_info()
        withUnsafeMutablePointer(to: &ctlInfo.ctl_name) { ptr in
            ptr.withMemoryRebound(to: CChar.self, capacity: MemoryLayout.size(ofValue: ptr.pointee)) {
                _ = strcpy($0, "com.apple.net.utun_control")
            }
        }
        for fd: Int32 in 0...1024 {
            var addr = sockaddr_ctl()
            var len = socklen_t(MemoryLayout<sockaddr_ctl>.size)
            let ret = withUnsafeMutablePointer(to: &addr) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    getpeername(fd, $0, &len)
                }
            }
            if ret != 0 || Int32(addr.sc_family) != AF_SYSTEM { continue }
            if addr.ss_sysaddr != UInt16(AF_SYS_CONTROL) { continue }
            var ctlIdInfo = ctlInfo
            if ioctl(fd, CTLIOCGINFO, &ctlIdInfo) != 0 { continue }
            if ctlIdInfo.ctl_id == addr.sc_id { return fd }
        }
        return -1
    }

    /// Network settings for the tunnel. Routes all traffic into the tunnel and
    /// sets a sane DNS; the panel's clash config governs actual proxying.
    private func makeNetworkSettings(remoteAddress: String?) -> NEPacketTunnelNetworkSettings {
        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: remoteAddress ?? "127.0.0.1")

        let ipv4 = NEIPv4Settings(addresses: ["198.18.0.1"], subnetMasks: ["255.255.0.0"])
        ipv4.includedRoutes = [NEIPv4Route.default()]
        settings.ipv4Settings = ipv4

        let ipv6 = NEIPv6Settings(addresses: ["fd00::1"], networkPrefixLengths: [64])
        ipv6.includedRoutes = [NEIPv6Route.default()]
        settings.ipv6Settings = ipv6

        let dns = NEDNSSettings(servers: ["1.1.1.1", "8.8.8.8"])
        dns.matchDomains = [""]
        settings.dnsSettings = dns

        settings.mtu = 1500
        return settings
    }

    private func providerConfiguration() -> [String: Any] {
        guard let proto = protocolConfiguration as? NETunnelProviderProtocol,
              let conf = proto.providerConfiguration else { return [:] }
        return conf
    }

    private func publish(stage: String, detail: String?) {
        let since = (stage == CarambaStage.connected) ? connectedSinceMs : 0
        CarambaSharedState.writeStatus(
            CarambaStatusSnapshot(stage: stage, detail: detail, connectedSinceMs: since))
    }

    private func carambaError(_ message: String) -> NSError {
        NSError(domain: "com.caramba.vpn", code: -1,
                userInfo: [NSLocalizedDescriptionKey: message])
    }
}
