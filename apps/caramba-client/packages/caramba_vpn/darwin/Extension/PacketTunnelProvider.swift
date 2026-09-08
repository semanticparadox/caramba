// PacketTunnelProvider — the Network Extension that owns the real tunnel.
//
// This class is the packet-tunnel-provider principal of the app's Network
// Extension target (NOT the plugin). It is documented for the user to add as an
// extension target after `flutter create .` (see INTEGRATION). It is compiled
// into the extension binary, links the vendored exarobot.xcframework (gomobile
// bind of the `mobile` package with -prefix Caramba, so the Swift module is
// `Exarobot` and the classes are `CarambaMobile*`), and runs in its own process.
//
// Flow:
//   1. startTunnel: read providerConfiguration (serverId + panel/sub URLs etc.)
//      that the app's CarambaVpnPlugin stored on the NETunnelProviderProtocol.
//   2. Apply NEPacketTunnelNetworkSettings so the OS routes packets to us.
//   3. Build the Go core (CarambaMobileNewClient + Configure), hand it the tunnel file
//      descriptor (packetFlow's underlying utun fd) via SetTunFd, then Up(serverId).
//      mihomo reads/writes that same fd, so packets actually flow through the
//      AmneziaWG/VLESS/etc. proxy the panel's clash config selects.
//   4. Poll the Go core for stage + traffic and publish them into the App Group
//      shared store, which the app-process plugin reads and forwards to Flutter.
//
// CODE IDENTIFIERS stay `caramba`; user-facing strings say `exarobot`.

// Darwin, а не только Foundation: дескриптор utun ищется через ctl_info /
// sockaddr_ctl / AF_SYS_CONTROL из <sys/kern_control.h>, и они видны Swift
// только из модуля Darwin.
import Darwin
import Foundation
import NetworkExtension
import os.log

// The gomobile-bound Go core. `import Exarobot` resolves the vendored
// exarobot.xcframework: имя Swift-модуля берётся из имени файла, а префикс
// классов — из -prefix Caramba плюс имя Go-пакета `mobile`, отсюда
// CarambaMobileClient / CarambaMobileNewClient.
//
// Этот файл компилируется НЕ подом, а целью Network Extension в приложении
// (её создаёт владелец в Xcode, см. INTEGRATION). Значит условие CARAMBA_CORE
// должна поставить та цель: SWIFT_ACTIVE_COMPILATION_CONDITIONS = CARAMBA_CORE.
// Без него расширение собралось бы пустышкой, которая молча не поднимает
// туннель, — поэтому здесь #error, а не тихая деградация.
#if CARAMBA_CORE
import Exarobot
#else
#error("PacketTunnelProvider: цель Network Extension должна линковать exarobot.xcframework и объявлять SWIFT_ACTIVE_COMPILATION_CONDITIONS = CARAMBA_CORE")
#endif

@available(iOS 15.0, macOS 11.0, *)
final class PacketTunnelProvider: NEPacketTunnelProvider {
    private let log = OSLog(subsystem: "com.caramba.vpn", category: "tunnel")

    #if CARAMBA_CORE
    private var core: CarambaMobileClient?
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
            #if CARAMBA_CORE
            // Down() shuts mihomo's listeners (including the TUN inbound).
            try? self.core?.down()
            // Republished AFTER down and BEFORE the core is dropped: the report
            // survives teardown (it describes the raise that just ended), and
            // re-reading it here is what makes the `tunnel_up` inside it follow
            // the tunnel down instead of freezing on the value it had while the
            // engine was still up.
            if let client = self.core {
                self.publishRouteReport(from: client)
            }
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
        #if CARAMBA_CORE
        let panelUrl = conf[CarambaVpnKeys.panelUrl] as? String ?? ""
        let subUuid = conf[CarambaVpnKeys.subscriptionUuid] as? String ?? ""
        let accessToken = conf[CarambaVpnKeys.accessToken] as? String ?? ""
        // The long-lived half of the same session, plus when the access half
        // dies. This extension is a separate process with no Dart in it: once
        // the 15-minute access token expires it has nobody to ask for a new one,
        // so without these the core goes permanently 401 while the tunnel is
        // still up. Expiry rides the plist as a String; 0 means "unknown" and
        // the core falls back to the JWT's own exp claim.
        let refreshToken = conf[CarambaVpnKeys.refreshToken] as? String ?? ""
        let accessExpiryUnix = Int64(conf[CarambaVpnKeys.accessExpiryUnix] as? String ?? "") ?? 0

        // Use the App Group container so the extension and app agree on token
        // store + work dir on disk. The gomobile prefix is `Caramba` (see the
        // build script -prefix Caramba), so the type is CarambaMobileClient and the
        // constructor is CarambaMobileNewClient.
        let base = CarambaAppGroup.containerURL ?? FileManager.default.temporaryDirectory
        let workDir = base.appendingPathComponent("caramba", isDirectory: true).path
        let tokenPath = base.appendingPathComponent("caramba/token.json").path

        var initError: NSError?
        // gomobile maps Go `NewClient(panelURL,subURL,workDir,tokenPath) (*Client, error)`
        // to `CarambaMobileNewClient(_,_,_,_, error:) -> CarambaMobileClient?`. subURL is left
        // empty so the core uses the panel default. For a raw import panelUrl is
        // empty; NewClient still succeeds (it only wires the client, no network).
        guard let client = CarambaMobileNewClient(panelUrl, "", workDir, tokenPath, &initError) else {
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
            _ = try carambaCoreCall { client.importSubscription(raw, format: format, error: $0) }
        } else {
            // Configure(panelURL, subscriptionID, accessToken): the binding's single
            // auth entry point. The extension runs in its own process, so the app
            // hands it the JWT + subscription uuid here rather than re-running login.
            try client.configure(panelUrl, subscriptionID: subUuid, accessToken: accessToken,
                                 refreshToken: refreshToken, accessExpiryUnix: accessExpiryUnix)
        }

        // ABI v2 policy: the whole CoreConfig arrives as one JSON blob and is
        // applied BEFORE up, so the assembled mihomo config already carries it.
        if let policyJson = conf[CarambaVpnKeys.policyJson] as? String, !policyJson.isEmpty {
            try client.setPolicyJSON(policyJson)
        }
        // Capture mode. Apple owns the utun fd, so "tun" is the norm here; the
        // key exists for parity with desktop and for no-TUN debugging.
        if let mode = conf[CarambaVpnKeys.tunnelMode] as? String, !mode.isEmpty, mode != "tun" {
            let port = Int(conf[CarambaVpnKeys.mixedPort] as? String ?? "") ?? 7890
            // Метка аргумента — mixedPort:, как в CarambaMobile.objc.h
            // (setTunnelMode:mixedPort:error:); `port:` не компилировался бы,
            // но раньше этого никто не замечал: файл не входит ни в одну цель.
            try client.setTunnelMode(mode, mixedPort: port)
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

        // Up raises the tunnel from the active config. Both paths pass serverId:
        // on the panel path it is the subscription node, on the raw path it is the
        // ABI v2 pin of the CARAMBA selector to one proxy of the imported config
        // (empty means automatic). gomobile maps
        // `Up(serverID string) (string, error)` to a throwing Swift method returning
        // the UpResult JSON; we only need its success/throw.
        _ = try carambaCoreCall { client.up(serverId, error: $0) }
        // The loopback service inbound exists only while the engine is up, and
        // its credential is minted per raise. The CSM core lives in the app
        // process and never has `up` called on it, so the address travels the
        // App Group; without it that core's rung R4 is permanently
        // not_configured and the ladder degrades to R1 and R5 on the one
        // platform the listener was added for (02-SPEC.md 8.2).
        CarambaSharedState.writeLoopbackProxy(client.loopbackProxyURL())
        // The routing report is taken by the core at the moment the engine
        // starts, and only this process has that core. Publishing it here is
        // what lets the app answer "is the ad block actually cutting anything"
        // at all; without it the app-process core answers "nothing raised" for
        // the life of the install.
        publishRouteReport(from: client)
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
        #if CARAMBA_CORE
        guard let client = core else { return }

        // Stage: prefer the contract-shaped statusJSON() if the binding exposes
        // it; otherwise derive the stage from the engine Status() JSON.
        let stage = readStage(from: client)
        publish(stage: stage, detail: nil)

        // Traffic: prefer trafficJSON(); fall back to zeros if absent.
        if let t = readTraffic(from: client) {
            CarambaSharedState.writeTraffic(t)
        }

        // The report also rides the poll, so a reconnect that re-applies a
        // different config does not leave the app reading the previous raise.
        // Written only when the core's answer actually changed: this is a whole
        // JSON document and the tick is 1 Hz.
        publishRouteReport(from: client)
        #endif
    }

    #if CARAMBA_CORE
    /// Last report handed to the App Group, so the 1 Hz tick writes only on a
    /// real change.
    private var lastRouteReport: String = ""

    /// Publishes the core's routing report into the App Group.
    ///
    /// The payload crosses verbatim: nothing here parses or reshapes it, so the
    /// app reads exactly what `api.RouteReport` produced. A throw or an empty
    /// answer leaves the previous report standing — the last raise is a better
    /// answer than the empty string, which the app reads as "no bridge".
    private func publishRouteReport(from client: CarambaMobileClient) {
        guard let json = carambaCoreTry({ client.routeReport($0) }), !json.isEmpty else { return }
        guard json != lastRouteReport else { return }
        lastRouteReport = json
        CarambaSharedState.writeRouteReport(json)
    }
    #endif

    #if CARAMBA_CORE
    /// Reads the tunnel stage. Prefers the contract-shaped `StatusJSON()` from the
    /// go-binding surface (gomobile selector `statusJSON`), and falls back to the
    /// engine `Status()` JSON (`api.StatusResult`,
    /// engine.state = stopped|starting|connected|error) so the path still works on
    /// an older binding. Either way the result is normalized to a CHANNEL CONTRACT
    /// stage string.
    private func readStage(from client: CarambaMobileClient) -> String {
        if let direct = carambaCoreTry({ client.statusJSON($0) }),
           let stage = Self.stageFromContractJSON(direct) {
            return stage
        }
        if let raw = carambaCoreTry({ client.status($0) }) {
            return Self.stageFromEngineJSON(raw)
        }
        return CarambaStage.error
    }

    /// Reads traffic from the contract-shaped `TrafficJSON()` (gomobile selector
    /// `trafficJSON`). Returns nil when unavailable so the caller leaves the last
    /// counters untouched rather than zeroing a live tunnel.
    private func readTraffic(from client: CarambaMobileClient) -> CarambaTrafficSnapshot? {
        guard let raw = carambaCoreTry({ client.trafficJSON($0) }) else { return nil }
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

    /// Уровень протокола и опция сокета utun: SYSPROTO_CONTROL и
    /// UTUN_OPT_IFNAME из <sys/kern_control.h> и <net/if_utun.h>.
    ///
    /// Числа, а не имена: эти заголовки не входят ни в один модуль Swift
    /// (ни Darwin, ни Foundation их не реэкспортируют), поэтому прежняя версия
    /// этой функции — со структурами ctl_info / sockaddr_ctl — не
    /// компилировалась вообще. Заметить это было негде: файл не входит ни в одну
    /// цель, его собирает только цель Network Extension, которой пока нет.
    /// Значения зафиксированы в ABI ядра Darwin и не менялись.
    private static let sysprotoControl: Int32 = 2
    private static let utunOptIfname: Int32 = 2

    /// The packet-tunnel file descriptor. NEPacketTunnelFlow does not publicly
    /// expose it, so we scan the process file descriptors for the utun socket
    /// the extension owns: only that socket answers getsockopt(UTUN_OPT_IFNAME)
    /// with an "utunN" name. This is the technique the WireGuard Apple app uses.
    private func tunnelFileDescriptor() -> Int32 {
        var name = [CChar](repeating: 0, count: Int(IFNAMSIZ))
        for fd: Int32 in 0...1024 {
            var len = socklen_t(name.count)
            let ok = getsockopt(fd, Self.sysprotoControl, Self.utunOptIfname, &name, &len) == 0
            guard ok, String(cString: name).hasPrefix("utun") else { continue }
            return fd
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
