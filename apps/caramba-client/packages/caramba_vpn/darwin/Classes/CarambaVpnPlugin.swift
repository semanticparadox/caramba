// CarambaVpnPlugin — the app-process Flutter plugin for Apple platforms.
//
// Registers the CHANNEL CONTRACT on both iOS and macOS:
//   MethodChannel  com.caramba/vpn          configure / connect / connectRaw /
//                                           disconnect / status, plus the ABI v2
//                                           generic-mode calls importSubscription /
//                                           probe / setPolicy / setTunnelMode
//   EventChannel   com.caramba/vpn/status   { stage, detail?, connectedSinceMs }
//   EventChannel   com.caramba/vpn/traffic  { downBps, upBps, downTotal, upTotal }
//
// It does NOT run the tunnel itself. The tunnel lives in a Network Extension
// (PacketTunnelProvider) in a separate process; this plugin drives that
// extension through a NETunnelProviderManager (load/save the provider profile,
// startTunnel/stopTunnel, pass serverId + config via providerConfiguration) and
// polls the App Group shared store the extension writes, forwarding ~1 Hz
// status + traffic frames to the two event channels.
//
// Flutter imports differ per platform (FlutterMacOS on macOS, Flutter on iOS).
// The per-platform shim files (ios/Classes/CarambaVpnPlugin+iOS.swift and
// macos/Classes/CarambaVpnPlugin+macOS.swift) each `import` the right Flutter
// module — which makes its symbols (FlutterMethodChannel, FlutterPlugin, etc.)
// available across the single pod module — and provide `pluginMessenger(_:)` for
// the one registrar API difference (method on iOS, property on macOS). This body
// is otherwise shared verbatim.
//
// CODE IDENTIFIERS stay `caramba`; user-facing strings say `exarobot`.

#if os(iOS)
import Flutter
#elseif os(macOS)
import FlutterMacOS
#endif
import Foundation
import NetworkExtension

// The gomobile-bound Go core, used ONLY for the metadata-only generic-mode calls
// (importSubscription / probe) that must not raise a tunnel. The packet path
// still lives in the Network Extension. Guarded so the plugin compiles without
// the vendored framework; the calls then answer FlutterError("core_missing").
#if canImport(Caramba)
import Caramba
#endif

public final class CarambaVpnPlugin: NSObject, FlutterPlugin, FlutterStreamHandler {

    // MARK: Registration

    public static func register(with registrar: FlutterPluginRegistrar) {
        let messenger = pluginMessenger(registrar)
        let instance = CarambaVpnPlugin()

        let method = FlutterMethodChannel(name: CarambaVpnChannels.method, binaryMessenger: messenger)
        registrar.addMethodCallDelegate(instance, channel: method)

        let statusChannel = FlutterEventChannel(name: CarambaVpnChannels.statusEvents, binaryMessenger: messenger)
        statusChannel.setStreamHandler(instance.statusStream)

        let trafficChannel = FlutterEventChannel(name: CarambaVpnChannels.trafficEvents, binaryMessenger: messenger)
        trafficChannel.setStreamHandler(instance.trafficStream)
    }

    // MARK: State

    private let statusStream = CarambaEventStream()
    private let trafficStream = CarambaEventStream()

    private var manager: NETunnelProviderManager?
    private var pollTimer: Timer?
    private var statusObserver: NSObjectProtocol?

    /// Last status emitted, to dedupe identical frames (stage + detail + since).
    private var lastEmittedStatusKey: String = ""

    /// App-supplied connection config (panel/sub URLs, subscription id, policy)
    /// captured by `configure` and threaded to the extension on connect.
    private var pendingConfig: [String: Any] = [:]

    /// Lazily built metadata-only core client for the generic-mode calls
    /// (importSubscription / probe). It runs IN THE APP PROCESS and never raises
    /// a tunnel; the packet path stays in the extension.
    #if canImport(Caramba)
    private var tools: CarambaClient?
    #endif

    /// The Secure Enclave holder of the device identity. One instance: the
    /// identity is established once, and both the channel calls and the Go
    /// core's bridge reach the same key through it.
    private let deviceKeys = CarambaDeviceKeys()

    /// The core client that owns the CSM/1 profile. Separate from `tools`
    /// because its work dir is PERSISTENT: the CSM store is the profile's
    /// identity and it must survive a restart.
    #if canImport(Caramba)
    private var csm: CarambaClient?

    /// The profile whose CSM store is selected (02-SPEC.md 1.2). Empty means the
    /// single store in the core work dir, as installs made before the second
    /// operator have it.
    private var csmProfileKey: String = ""
    #endif

    /// Serial queue for the blocking core calls (import parses a whole
    /// subscription, probe dials every node) so they never sit on the main thread.
    private let toolsQueue = DispatchQueue(label: "com.caramba.vpn.tools")

    // MARK: FlutterPlugin

    public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        switch call.method {
        case "configure":
            let args = call.arguments as? [String: Any] ?? [:]
            // Capture the auth + endpoint params the extension needs to call
            // Configure(panelURL, subscriptionID, accessToken) on the Go core, plus
            // optional policy. The values are stored on the provider configuration
            // (an App Group plist) so the out-of-process extension can read them.
            for key in [CarambaVpnKeys.panelUrl, CarambaVpnKeys.subscriptionUuid,
                        CarambaVpnKeys.accessToken, CarambaVpnKeys.protocolName,
                        CarambaVpnKeys.relayCountry, CarambaVpnKeys.presetId] {
                if let v = args[key] { pendingConfig[key] = v }
            }
            // Wire-key compatibility: the canonical key is `subscriptionUuid`, but
            // older callers send `subscriptionId`. Accept either, store the canonical.
            if pendingConfig[CarambaVpnKeys.subscriptionUuid] == nil,
               let legacy = args[CarambaVpnKeys.subscriptionId] {
                pendingConfig[CarambaVpnKeys.subscriptionUuid] = legacy
            }
            // The CSM core caches the seam it was built with, and the settings
            // write authorizes with the account token from that core's own
            // store. A core built before a token rotation would keep answering
            // 401 until the app restarts, so it is dropped and rebuilt on the
            // next CSM call. The device identity survives: it lives in the
            // Secure Enclave, not in the core.
            #if canImport(Caramba)
            csm = nil
            #endif
            result(nil)

        case "connect":
            let args = call.arguments as? [String: Any] ?? [:]
            connect(args: args, result: result)

        case "connectRaw":
            let args = call.arguments as? [String: Any] ?? [:]
            connectRaw(args: args, result: result)

        case "disconnect":
            disconnect(result: result)

        case "status":
            result(CarambaSharedState.readStatus().asMap)

        // --- generic mode (ABI v2) -------------------------------------------

        case "importSubscription":
            let args = call.arguments as? [String: Any] ?? [:]
            let raw = args[CarambaVpnKeys.rawConfig] as? String ?? ""
            let format = args[CarambaVpnKeys.rawFormat] as? String ?? ""
            importSubscription(raw: raw, format: format, result: result)

        case "probe":
            let args = call.arguments as? [String: Any] ?? [:]
            let timeoutMs = (args[CarambaVpnKeys.timeoutMs] as? NSNumber)?.intValue ?? 5000
            probe(timeoutMs: timeoutMs, result: result)

        case "setPolicy":
            // Stored, not applied here: the core that raises the tunnel lives in
            // the extension, which reads this through providerConfiguration and
            // calls setPolicyJSON before up.
            let args = call.arguments as? [String: Any] ?? [:]
            pendingConfig[CarambaVpnKeys.policyJson] = args["json"] as? String ?? ""
            result(nil)

        case "setTunnelMode":
            let args = call.arguments as? [String: Any] ?? [:]
            pendingConfig[CarambaVpnKeys.tunnelMode] = args["mode"] as? String ?? "tun"
            let port = (args["port"] as? NSNumber)?.intValue ?? 7890
            // providerConfiguration must stay plist-safe; keep it a String.
            pendingConfig[CarambaVpnKeys.mixedPort] = String(port)
            result(nil)

        // --- CSM/1 device keys (ABI v3) --------------------------------------
        //
        // These do NOT touch the Go core: the key lives in the Secure Enclave and
        // CarambaDeviceKeys is its only holder. The core reaches the same key
        // through CarambaGoDeviceKeyBridge, so one identity serves both paths.
        // Keychain round trips run off the main thread.

        case "deviceKeygen":
            let keys = deviceKeys
            toolsQueue.async {
                let json = keys.keygen("{}")
                DispatchQueue.main.async { result(json) }
            }

        case "deviceSign":
            let args = call.arguments as? [String: Any] ?? [:]
            let messageB64 = args[CarambaVpnKeys.messageB64] as? String ?? ""
            let keys = deviceKeys
            toolsQueue.async {
                let json = keys.sign(CarambaDeviceKeys.request(["message_b64": messageB64]))
                DispatchQueue.main.async { result(json) }
            }

        case "deviceAgree":
            let args = call.arguments as? [String: Any] ?? [:]
            let peerB64 = args[CarambaVpnKeys.peerPubB64] as? String ?? ""
            let rkv = (args[CarambaVpnKeys.rkv] as? NSNumber)?.intValue ?? 0
            let keys = deviceKeys
            toolsQueue.async {
                let json = keys.agree(
                    CarambaDeviceKeys.request(["rkv": rkv, "peer_pub_b64": peerB64])
                )
                DispatchQueue.main.async { result(json) }
            }

        case "csmRequestSettings":
            // The write goes through the core, never through a socket opened
            // here: a control plane with its own sockets bypasses the transport
            // ladder, and the app degenerates to rung R0 while the core is still
            // climbing for a configuration it can no longer change
            // (02-SPEC.md 8.9).
            let args = call.arguments as? [String: Any] ?? [:]
            csmRequestSettings(json: args["json"] as? String ?? "{}", result: result)

        case "csmState":
            // A read of what the core already verified: no socket, nothing
            // applied. It carries the trusted catalog's resource projection,
            // without which the client cannot notice the narrowing that arrives
            // in the catalog rather than in a setting (02-SPEC.md 7.7.1).
            csmRead(code: "csm_state_failed", ladder: false, result: result)

        case "csmLadder":
            // The attempt history is local and never reported to the operator
            // (02-SPEC.md 7.10); the transport screen must show it (INV-17).
            csmRead(code: "csm_ladder_failed", ladder: true, result: result)

        case "csmEnroll":
            // Enrolment goes through the core and up the ladder: it is the one
            // moment trust is created, and a socket opened here would be a path
            // to the operator the ladder cannot see.
            let args = call.arguments as? [String: Any] ?? [:]
            csmCall(kind: .enroll, json: args["json"] as? String ?? "{}",
                    code: "csm_enroll_failed", result: result)

        case "csmRefresh":
            let args = call.arguments as? [String: Any] ?? [:]
            let timeout = (args[CarambaVpnKeys.timeoutSec] as? NSNumber)?.intValue ?? 30
            csmCall(kind: .refresh, json: "\(timeout)",
                    code: "csm_refresh_failed", result: result)

        case "csmSetLadder":
            // A write that stops in the Dart layer only changes the picture:
            // the user reorders the ladder, the screen shows the new order, and
            // the fetch keeps walking the old one.
            let args = call.arguments as? [String: Any] ?? [:]
            csmCall(kind: .setLadder, json: args["json"] as? String ?? "{}",
                    code: "csm_set_ladder_failed", result: result)

        case "csmAnswerCatalogChange":
            // Until this answer arrives the core holds the PREVIOUS resource set
            // in force, which is what makes "keep the previous ones" true
            // (02-SPEC.md 7.7.1).
            let args = call.arguments as? [String: Any] ?? [:]
            csmCall(kind: .answerCatalogChange, json: args["json"] as? String ?? "{}",
                    code: "csm_catalog_answer_failed", result: result)

        case "csmSelectProfile":
            // 02-SPEC.md 1.2: every profile state store MUST be keyed by pid.
            let args = call.arguments as? [String: Any] ?? [:]
            let key = args[CarambaVpnKeys.csmProfileKey] as? String ?? ""
            selectCsmProfile(key: key, result: result)

        default:
            result(FlutterMethodNotImplemented)
        }
    }

    /// Sends the settings write through the core and returns its state snapshot.
    private func csmRequestSettings(json: String, result: @escaping FlutterResult) {
        #if canImport(Caramba)
        toolsQueue.async { [weak self] in
            guard let self = self else { return }
            do {
                let client = try self.csmClient()
                let out = try client.csmRequestSettings(json)
                DispatchQueue.main.async { result(out) }
            } catch {
                DispatchQueue.main.async {
                    result(FlutterError(code: "csm_write_failed",
                                        message: error.localizedDescription, details: nil))
                }
            }
        }
        #else
        result(FlutterError(code: "core_missing",
                            message: "exarobot.xcframework not linked", details: nil))
        #endif
    }

    /// The CSM calls that take one JSON string and give one back.
    private enum CsmCallKind {
        case enroll
        case refresh
        case setLadder
        case answerCatalogChange
    }

    /// Runs one CSM write call on the tools queue.
    ///
    /// Enrolment and refresh climb the transport ladder and can take the whole
    /// budget of a cycle, so none of this runs on the platform thread.
    private func csmCall(kind: CsmCallKind, json: String, code: String,
                         result: @escaping FlutterResult) {
        #if canImport(Caramba)
        toolsQueue.async { [weak self] in
            guard let self = self else { return }
            do {
                let client = try self.csmClient()
                let out: String
                switch kind {
                case .enroll:
                    out = try client.csmEnroll(json)
                case .refresh:
                    out = try client.csmRefresh(Int(json) ?? 30)
                case .setLadder:
                    try client.csmSetLadder(json)
                    out = "{\"ok\":true}"
                case .answerCatalogChange:
                    out = try client.csmAnswerCatalogChange(json)
                }
                DispatchQueue.main.async { result(out) }
            } catch {
                DispatchQueue.main.async {
                    result(FlutterError(code: code,
                                        message: error.localizedDescription, details: nil))
                }
            }
        }
        #else
        result(FlutterError(code: "core_missing",
                            message: "exarobot.xcframework not linked", details: nil))
        #endif
    }

    /// Points the CSM store at one profile and drops the current core so the
    /// next CSM call rebuilds it against that profile's directory.
    private func selectCsmProfile(key: String, result: @escaping FlutterResult) {
        #if canImport(Caramba)
        toolsQueue.async { [weak self] in
            guard let self = self else { return }
            if self.csmProfileKey == key {
                DispatchQueue.main.async { result("{\"ok\":true}") }
                return
            }
            self.csmProfileKey = key
            self.csm = nil
            DispatchQueue.main.async { result("{\"ok\":true}") }
        }
        #else
        result(FlutterError(code: "core_missing",
                            message: "exarobot.xcframework not linked", details: nil))
        #endif
    }

    /// Runs a read-only CSM call on the tools queue and returns its JSON.
    ///
    /// The client type only exists when the framework is linked, so the call
    /// itself lives under the same guard as every other core call here and the
    /// selector is a flag rather than a closure over that type.
    private func csmRead(code: String, ladder: Bool, result: @escaping FlutterResult) {
        #if canImport(Caramba)
        toolsQueue.async { [weak self] in
            guard let self = self else { return }
            do {
                let client = try self.csmClient()
                let out: String
                if ladder {
                    out = try client.csmLadder()
                } else {
                    out = try client.csmState()
                }
                DispatchQueue.main.async { result(out) }
            } catch {
                DispatchQueue.main.async {
                    result(FlutterError(code: code,
                                        message: error.localizedDescription, details: nil))
                }
            }
        }
        #else
        result(FlutterError(code: "core_missing",
                            message: "exarobot.xcframework not linked", details: nil))
        #endif
    }

    // MARK: Connect / Disconnect

    private func connect(args: [String: Any], result: @escaping FlutterResult) {
        // Optimistic connecting frame so the UI reacts immediately.
        emitStatus(CarambaStatusSnapshot(stage: CarambaStage.connecting,
                                         detail: "Securing tunnel", connectedSinceMs: 0))

        loadOrCreateManager { [weak self] mgr, err in
            guard let self = self else { return }
            if let err = err {
                self.emitStatus(CarambaStatusSnapshot(stage: CarambaStage.error,
                                                      detail: err.localizedDescription,
                                                      connectedSinceMs: 0))
                result(FlutterError(code: "manager", message: err.localizedDescription, details: nil))
                return
            }
            guard let mgr = mgr else {
                result(FlutterError(code: "manager", message: "no tunnel manager", details: nil))
                return
            }

            // Build providerConfiguration: serverId + policy + endpoints. Strings
            // only (NETunnelProviderProtocol requires a plist-safe dictionary).
            var providerConf: [String: Any] = self.pendingConfig
            providerConf[CarambaVpnKeys.serverId] = args["serverId"] as? String ?? ""
            providerConf[CarambaVpnKeys.serverName] = args["serverName"] as? String ?? ""
            providerConf[CarambaVpnKeys.countryCode] = args["countryCode"] as? String ?? ""

            let proto = (mgr.protocolConfiguration as? NETunnelProviderProtocol) ?? NETunnelProviderProtocol()
            proto.providerBundleIdentifier = self.extensionBundleIdentifier()
            // Required non-empty server address; the real endpoint is selected by
            // the panel's clash config inside the extension.
            proto.serverAddress = (args["serverName"] as? String) ?? "exarobot"
            proto.providerConfiguration = providerConf
            mgr.protocolConfiguration = proto
            mgr.localizedDescription = "exarobot"
            mgr.isEnabled = true

            mgr.saveToPreferences { saveErr in
                if let saveErr = saveErr {
                    self.emitStatus(CarambaStatusSnapshot(stage: CarambaStage.error,
                                                          detail: saveErr.localizedDescription,
                                                          connectedSinceMs: 0))
                    result(FlutterError(code: "save", message: saveErr.localizedDescription, details: nil))
                    return
                }
                // Reload to pick up the saved configuration before starting.
                mgr.loadFromPreferences { _ in
                    do {
                        try mgr.connection.startVPNTunnel()
                        self.observeConnection(mgr)
                        self.startPolling()
                        result(nil)
                    } catch {
                        self.emitStatus(CarambaStatusSnapshot(stage: CarambaStage.error,
                                                              detail: error.localizedDescription,
                                                              connectedSinceMs: 0))
                        result(FlutterError(code: "start", message: error.localizedDescription, details: nil))
                    }
                }
            }
        }
    }

    /// rawSub path: mirror `connect` but carry an imported subscription instead of
    /// a panelAccount server. The extension imports the raw config
    /// (ImportSubscription) instead of calling Configure, then raises the tunnel
    /// with an empty serverId. Everything downstream (save provider profile, start
    /// the tunnel, observe, poll) is identical to the connect path.
    private func connectRaw(args: [String: Any], result: @escaping FlutterResult) {
        // Optimistic connecting frame so the UI reacts immediately.
        emitStatus(CarambaStatusSnapshot(stage: CarambaStage.connecting,
                                         detail: "Importing profile", connectedSinceMs: 0))

        loadOrCreateManager { [weak self] mgr, err in
            guard let self = self else { return }
            if let err = err {
                self.emitStatus(CarambaStatusSnapshot(stage: CarambaStage.error,
                                                      detail: err.localizedDescription,
                                                      connectedSinceMs: 0))
                result(FlutterError(code: "manager", message: err.localizedDescription, details: nil))
                return
            }
            guard let mgr = mgr else {
                result(FlutterError(code: "manager", message: "no tunnel manager", details: nil))
                return
            }

            // Build providerConfiguration for the raw path. We intentionally do NOT
            // include pendingConfig's panel/sub/token seam: a raw import raises the
            // tunnel from the imported config alone (no panel Configure). rawMode
            // flags the extension to take the ImportSubscription branch, and
            // serverId stays empty (a raw source has no subscription node).
            let label = args[CarambaVpnKeys.rawLabel] as? String ?? ""
            var providerConf: [String: Any] = [:]
            providerConf[CarambaVpnKeys.rawMode] = "1"
            providerConf[CarambaVpnKeys.rawConfig] = args[CarambaVpnKeys.rawConfig] as? String ?? ""
            providerConf[CarambaVpnKeys.rawFormat] = args[CarambaVpnKeys.rawFormat] as? String ?? ""
            providerConf[CarambaVpnKeys.rawLabel] = label
            // ABI v2: a non-empty serverId pins the CARAMBA selector to that proxy
            // name inside the imported config; empty keeps the automatic choice.
            providerConf[CarambaVpnKeys.serverId] = args[CarambaVpnKeys.serverId] as? String ?? ""
            // Policy + capture mode apply to BOTH paths, so copy them over even
            // though the raw path skips the panel seam.
            for key in [CarambaVpnKeys.policyJson, CarambaVpnKeys.tunnelMode,
                        CarambaVpnKeys.mixedPort] {
                if let v = self.pendingConfig[key] { providerConf[key] = v }
            }

            let proto = (mgr.protocolConfiguration as? NETunnelProviderProtocol) ?? NETunnelProviderProtocol()
            proto.providerBundleIdentifier = self.extensionBundleIdentifier()
            // Required non-empty server address; the real endpoint is selected by
            // the imported clash config inside the extension.
            proto.serverAddress = label.isEmpty ? "exarobot" : label
            proto.providerConfiguration = providerConf
            mgr.protocolConfiguration = proto
            mgr.localizedDescription = "exarobot"
            mgr.isEnabled = true

            mgr.saveToPreferences { saveErr in
                if let saveErr = saveErr {
                    self.emitStatus(CarambaStatusSnapshot(stage: CarambaStage.error,
                                                          detail: saveErr.localizedDescription,
                                                          connectedSinceMs: 0))
                    result(FlutterError(code: "save", message: saveErr.localizedDescription, details: nil))
                    return
                }
                // Reload to pick up the saved configuration before starting.
                mgr.loadFromPreferences { _ in
                    do {
                        try mgr.connection.startVPNTunnel()
                        self.observeConnection(mgr)
                        self.startPolling()
                        result(nil)
                    } catch {
                        self.emitStatus(CarambaStatusSnapshot(stage: CarambaStage.error,
                                                              detail: error.localizedDescription,
                                                              connectedSinceMs: 0))
                        result(FlutterError(code: "start", message: error.localizedDescription, details: nil))
                    }
                }
            }
        }
    }

    // MARK: Generic mode (ABI v2), in-process, no tunnel

    /// Lazily builds the metadata-only core client. Its work dir is separate from
    /// the extension's so a probe never disturbs a live tunnel.
    #if canImport(Caramba)
    private func toolsClient() throws -> CarambaClient {
        if let existing = tools { return existing }
        let base = CarambaAppGroup.containerURL ?? FileManager.default.temporaryDirectory
        let workDir = base.appendingPathComponent("caramba-tools", isDirectory: true).path
        let tokenPath = base.appendingPathComponent("caramba-tools/token.json").path
        var initError: NSError?
        // Empty panel URL: NewClient only wires the client (no network), and the
        // generic path never talks to a panel.
        guard let client = CarambaNewClient("", "", workDir, tokenPath, &initError) else {
            throw initError ?? NSError(domain: "com.caramba.vpn", code: -1,
                                       userInfo: [NSLocalizedDescriptionKey: "core init failed"])
        }
        tools = client
        return client
    }

    /// Builds the CSM/1 core client, registering the device key bridge BEFORE any
    /// CSM call. 02-SPEC.md 9.4 puts the device signing key in the Secure
    /// Enclave, and a Go implementation would by definition put it in a file,
    /// which is the software tier; registering the bridge late would leave the
    /// core with a software identity it then has to keep, because `dtp` has
    /// already gone to the operator.
    private func csmClient() throws -> CarambaClient {
        if let existing = csm { return existing }
        let base = CarambaAppGroup.containerURL ?? FileManager.default.temporaryDirectory
        let workDir = base.appendingPathComponent("caramba-csm", isDirectory: true).path
        let tokenPath = base.appendingPathComponent("caramba-csm/token.json").path
        let panelUrl = pendingConfig[CarambaVpnKeys.panelUrl] as? String ?? ""
        var initError: NSError?
        guard let client = CarambaNewClient(panelUrl, "", workDir, tokenPath, &initError) else {
            throw initError ?? NSError(domain: "com.caramba.vpn", code: -1,
                                       userInfo: [NSLocalizedDescriptionKey: "core init failed"])
        }
        try client.setDeviceKeyBridge(CarambaGoDeviceKeyBridge(keys: deviceKeys))
        let subscription = pendingConfig[CarambaVpnKeys.subscriptionUuid] as? String ?? ""
        let token = pendingConfig[CarambaVpnKeys.accessToken] as? String ?? ""
        if !subscription.isEmpty || !token.isEmpty {
            try client.configure(panelUrl, subscriptionID: subscription, accessToken: token)
        }
        if !csmProfileKey.isEmpty {
            try client.csmSelectProfile(csmProfileKey)
        }
        // The loopback listener belongs to the tunnel core, which lives in the
        // network extension; `up` is never called on THIS core. Without the
        // handoff its rung R4 stays not_configured forever and the ladder
        // degrades to R1 and R5 (02-SPEC.md 8.2). An empty string means the
        // tunnel is down and R4 genuinely has no path.
        try? client.csmSetLadder(
            CarambaDeviceKeys.request(["tunnel_proxy": CarambaSharedState.readLoopbackProxy()])
        )
        csm = client
        return client
    }
    #endif

    /// Parses a raw subscription and returns the metadata JSON verbatim, WITHOUT
    /// raising a tunnel. Dart parses the JSON into ImportResult.
    private func importSubscription(raw: String, format: String,
                                    result: @escaping FlutterResult) {
        #if canImport(Caramba)
        toolsQueue.async { [weak self] in
            guard let self = self else { return }
            do {
                let client = try self.toolsClient()
                let json = try client.importSubscription(raw, format: format)
                DispatchQueue.main.async { result(json) }
            } catch {
                DispatchQueue.main.async {
                    result(FlutterError(code: "import_failed",
                                        message: error.localizedDescription, details: nil))
                }
            }
        }
        #else
        result(FlutterError(code: "core_missing",
                            message: "exarobot.xcframework not linked", details: nil))
        #endif
    }

    /// Measures the latency of every node of the currently loaded config and
    /// returns the ABI v2 JSON verbatim. Blocking, so it runs off the main thread.
    private func probe(timeoutMs: Int, result: @escaping FlutterResult) {
        #if canImport(Caramba)
        toolsQueue.async { [weak self] in
            guard let self = self else { return }
            do {
                let client = try self.toolsClient()
                let json = try client.probeJSON(timeoutMs)
                DispatchQueue.main.async { result(json) }
            } catch {
                DispatchQueue.main.async {
                    result(FlutterError(code: "probe_failed",
                                        message: error.localizedDescription, details: nil))
                }
            }
        }
        #else
        result(FlutterError(code: "core_missing",
                            message: "exarobot.xcframework not linked", details: nil))
        #endif
    }

    private func disconnect(result: @escaping FlutterResult) {
        manager?.connection.stopVPNTunnel()
        // Do not tear down polling immediately; the extension transitions to
        // disconnected and we mirror that frame, then stop.
        emitStatus(.disconnected)
        emitTraffic(.zero)
        stopPolling()
        result(nil)
    }

    // MARK: NETunnelProviderManager

    private func loadOrCreateManager(_ completion: @escaping (NETunnelProviderManager?, Error?) -> Void) {
        if let mgr = manager { completion(mgr, nil); return }
        NETunnelProviderManager.loadAllFromPreferences { [weak self] managers, err in
            guard let self = self else { return }
            if let err = err { completion(nil, err); return }
            let mgr = managers?.first ?? NETunnelProviderManager()
            self.manager = mgr
            completion(mgr, nil)
        }
    }

    /// Observes the system connection state and mirrors it into the status
    /// channel. The extension is the source of truth for stage detail (it writes
    /// the shared store the poller forwards); this just catches fast transitions.
    private func observeConnection(_ mgr: NETunnelProviderManager) {
        if let prev = statusObserver {
            NotificationCenter.default.removeObserver(prev)
            statusObserver = nil
        }
        statusObserver = NotificationCenter.default.addObserver(
            forName: .NEVPNStatusDidChange,
            object: mgr.connection, queue: .main
        ) { [weak self] _ in
            guard let self = self else { return }
            switch mgr.connection.status {
            case .connecting, .reasserting:
                let stage = mgr.connection.status == .reasserting
                    ? CarambaStage.reconnecting : CarambaStage.connecting
                self.emitStatus(CarambaStatusSnapshot(stage: stage, detail: nil, connectedSinceMs: 0))
            case .disconnected, .invalid:
                self.emitStatus(.disconnected)
                self.emitTraffic(.zero)
                self.stopPolling()
            case .connected:
                // Defer stage/since/traffic to the shared-store poller, which has
                // the extension's authoritative connectedSinceMs.
                break
            case .disconnecting:
                break
            @unknown default:
                break
            }
        }
    }

    // MARK: Polling (shared App Group store -> event channels)

    private func startPolling() {
        stopPolling()
        let timer = Timer(timeInterval: 1.0, repeats: true) { [weak self] _ in
            guard let self = self else { return }
            self.emitStatus(CarambaSharedState.readStatus())
            // Only stream traffic while connected; otherwise the contract wants zeros.
            let snap = CarambaSharedState.readStatus()
            if snap.stage == CarambaStage.connected {
                self.emitTraffic(CarambaSharedState.readTraffic())
            } else {
                self.emitTraffic(.zero)
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        pollTimer = timer
    }

    private func stopPolling() {
        pollTimer?.invalidate()
        pollTimer = nil
    }

    // MARK: Emit helpers (dedupe + main-thread delivery)

    private func emitStatus(_ snap: CarambaStatusSnapshot) {
        let key = "\(snap.stage)|\(snap.detail ?? "")|\(snap.connectedSinceMs)"
        if key == lastEmittedStatusKey { return }
        lastEmittedStatusKey = key
        statusStream.send(snap.asMap)
    }

    private func emitTraffic(_ t: CarambaTrafficSnapshot) {
        trafficStream.send(t.asMap)
    }

    // MARK: Bundle ids

    /// The packet-tunnel extension bundle id. By convention it is the app bundle
    /// id plus `.CarambaVpnExtension`; overridable via Info.plist
    /// `CARAMBA_VPN_EXTENSION_ID` for users who name the target differently.
    private func extensionBundleIdentifier() -> String {
        if let override = Bundle.main.object(forInfoDictionaryKey: "CARAMBA_VPN_EXTENSION_ID") as? String,
           !override.isEmpty {
            return override
        }
        let appId = Bundle.main.bundleIdentifier ?? "com.caramba.caramba_client"
        return appId + ".CarambaVpnExtension"
    }

    // MARK: FlutterStreamHandler (plugin itself is unused as a stream handler;
    // the two channels use CarambaEventStream. This conformance is kept so the
    // plugin can serve as a fallback handler if needed.)

    public func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? {
        nil
    }

    public func onCancel(withArguments arguments: Any?) -> FlutterError? {
        nil
    }
}

/// A thread-safe FlutterStreamHandler that buffers the last value and replays it
/// to a new subscriber, matching the Dart side's broadcast+cache expectation.
final class CarambaEventStream: NSObject, FlutterStreamHandler {
    private var sink: FlutterEventSink?
    private var last: [String: Any]?
    private let lock = NSLock()

    func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? {
        lock.lock()
        sink = events
        let cached = last
        lock.unlock()
        if let cached = cached { events(cached) }
        return nil
    }

    func onCancel(withArguments arguments: Any?) -> FlutterError? {
        lock.lock()
        sink = nil
        lock.unlock()
        return nil
    }

    func send(_ value: [String: Any]) {
        lock.lock()
        last = value
        let s = sink
        lock.unlock()
        guard let s = s else { return }
        if Thread.isMainThread {
            s(value)
        } else {
            DispatchQueue.main.async { s(value) }
        }
    }
}
