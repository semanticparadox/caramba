package com.caramba.caramba_vpn

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.annotation.NonNull
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import io.flutter.plugin.common.MethodChannel.MethodCallHandler
import io.flutter.plugin.common.MethodChannel.Result
import io.flutter.plugin.common.PluginRegistry
import org.json.JSONObject
import java.io.File

// CarambaVpnPlugin — the app-process Flutter plugin for Android.
//
// Registers the CHANNEL CONTRACT:
//   MethodChannel  com.caramba/vpn          configure / connect / connectRaw /
//                                           disconnect / status, plus the ABI v2
//                                           generic-mode calls importSubscription /
//                                           probe / setPolicy / setTunnelMode
//   EventChannel   com.caramba/vpn/status   { stage, detail?, connectedSinceMs }
//   EventChannel   com.caramba/vpn/traffic  { downBps, upBps, downTotal, upTotal }
//
// It does NOT run the tunnel itself. The tunnel lives in CarambaVpnService (an
// android.net.VpnService) which builds the TUN interface, hands its fd to the Go
// core (gomobile caramba.aar) and runs mihomo. This plugin:
//   * captures the `configure` auth/config seam and persists it for the service;
//   * triggers the system VPN-consent dialog (VpnService.prepare) before the
//     first connect, then starts the foreground service;
//   * subscribes to CarambaVpnBus and forwards status/traffic frames published
//     by the service to the two EventChannels.
//
// CODE IDENTIFIERS stay 'caramba' (the user-facing brand is 'exarobot').
class CarambaVpnPlugin :
    FlutterPlugin,
    ActivityAware,
    MethodCallHandler,
    PluginRegistry.ActivityResultListener {

    private companion object {
        const val VPN_REQUEST_CODE = 0x6361 // 'ca'
    }

    private lateinit var appContext: Context
    private lateinit var methodChannel: MethodChannel
    private lateinit var statusChannel: EventChannel
    private lateinit var trafficChannel: EventChannel

    private var statusSink: EventChannel.EventSink? = null
    private var trafficSink: EventChannel.EventSink? = null

    private var activity: Activity? = null

    // Main-thread handler: worker replies (importSubscription / probe) must be
    // delivered to the MethodChannel Result on the platform thread.
    private val mainHandler = Handler(Looper.getMainLooper())

    // Lazily built metadata-only core client for the generic-mode calls
    // (importSubscription / probe). Separate from the tunnel core, which lives
    // in CarambaVpnService.
    private var tools: CarambaCore? = null

    // The CSM/1 core client and the AndroidKeyStore holder of the device
    // identity. Both live in the plugin process for the lifetime of the engine:
    // the device identity is a long-lived identifier and must not be rebuilt per
    // call.
    private var csm: CarambaCore? = null

    /**
     * The profile whose CSM store is selected (02-SPEC.md 1.2). Empty means the
     * single store in the core work dir, as installs made before the second
     * operator have it.
     */
    private var csmProfileKey: String = ""

    /**
     * The loopback listener address with the credential of the current raise,
     * as reported by the tunnel core. Empty while the engine is down.
     */
    private var loopbackProxyUrl: String = ""
    private var device: CarambaDeviceKeys? = null

    // Pending connect args captured while the VPN-consent dialog is shown; the
    // service is started from onActivityResult once consent is granted.
    private var pendingConnectArgs: Map<String, String>? = null
    private var pendingResult: Result? = null

    // MARK: FlutterPlugin

    override fun onAttachedToEngine(@NonNull binding: FlutterPlugin.FlutterPluginBinding) {
        appContext = binding.applicationContext

        methodChannel = MethodChannel(binding.binaryMessenger, CarambaVpnChannels.METHOD)
        methodChannel.setMethodCallHandler(this)

        statusChannel = EventChannel(binding.binaryMessenger, CarambaVpnChannels.STATUS_EVENTS)
        statusChannel.setStreamHandler(object : EventChannel.StreamHandler {
            override fun onListen(arguments: Any?, events: EventChannel.EventSink?) {
                statusSink = events
                // Replay last known status so a fresh subscriber renders now.
                events?.success(CarambaVpnBus.currentStatus().asMap())
            }

            override fun onCancel(arguments: Any?) {
                statusSink = null
            }
        })

        trafficChannel = EventChannel(binding.binaryMessenger, CarambaVpnChannels.TRAFFIC_EVENTS)
        trafficChannel.setStreamHandler(object : EventChannel.StreamHandler {
            override fun onListen(arguments: Any?, events: EventChannel.EventSink?) {
                trafficSink = events
            }

            override fun onCancel(arguments: Any?) {
                trafficSink = null
            }
        })

        CarambaVpnBus.setListener(busListener)
        CarambaVpnBus.setLoopbackListener(::onLoopbackProxy)
    }

    override fun onDetachedFromEngine(@NonNull binding: FlutterPlugin.FlutterPluginBinding) {
        CarambaVpnBus.setListener(null)
        CarambaVpnBus.setLoopbackListener(null)
        tools?.close()
        tools = null
        csm?.close()
        csm = null
        device = null
        methodChannel.setMethodCallHandler(null)
        statusChannel.setStreamHandler(null)
        trafficChannel.setStreamHandler(null)
        statusSink = null
        trafficSink = null
    }

    // MARK: MethodCallHandler

    override fun onMethodCall(@NonNull call: MethodCall, @NonNull result: Result) {
        when (call.method) {
            "configure" -> {
                persistSeam(
                    panelUrl = call.argument<String>(CarambaVpnKeys.PANEL_URL) ?: "",
                    subUrl = call.argument<String>(CarambaVpnKeys.SUB_URL) ?: "",
                    subscriptionId = call.argument<String>(CarambaVpnKeys.SUBSCRIPTION_UUID)
                        ?: call.argument<String>(CarambaVpnKeys.SUBSCRIPTION_ID) ?: "",
                    accessToken = call.argument<String>(CarambaVpnKeys.ACCESS_TOKEN) ?: "",
                )
                result.success(null)
            }

            "connect" -> {
                val args = mapOf(
                    CarambaVpnKeys.SERVER_ID to (call.argument<String>(CarambaVpnKeys.SERVER_ID) ?: ""),
                    CarambaVpnKeys.SERVER_NAME to (call.argument<String>(CarambaVpnKeys.SERVER_NAME) ?: ""),
                    CarambaVpnKeys.COUNTRY_CODE to (call.argument<String>(CarambaVpnKeys.COUNTRY_CODE) ?: ""),
                )
                connect(args, result)
            }

            "connectRaw" -> {
                // rawSub path: carry the imported subscription + format + label
                // through the SAME args map / consent flow / service intent the
                // connect path uses. RAW_MODE flags the service to import instead of
                // using the panel seam; serverId (ABI v2) is optional and pins the
                // CARAMBA selector to one node of the imported config.
                val args = mapOf(
                    CarambaVpnKeys.RAW_MODE to "1",
                    CarambaVpnKeys.RAW_CONFIG to (call.argument<String>(CarambaVpnKeys.RAW_CONFIG) ?: ""),
                    CarambaVpnKeys.RAW_FORMAT to (call.argument<String>(CarambaVpnKeys.RAW_FORMAT) ?: ""),
                    CarambaVpnKeys.RAW_LABEL to (call.argument<String>(CarambaVpnKeys.RAW_LABEL) ?: ""),
                    // ABI v2: a non-empty serverId pins the CARAMBA selector to
                    // that proxy name inside the imported config; empty keeps the
                    // automatic choice.
                    CarambaVpnKeys.SERVER_ID to (call.argument<String>(CarambaVpnKeys.SERVER_ID) ?: ""),
                    // serverName drives the foreground notification / TUN session
                    // label; reuse the raw profile label there.
                    CarambaVpnKeys.SERVER_NAME to (call.argument<String>(CarambaVpnKeys.RAW_LABEL) ?: ""),
                )
                connect(args, result)
            }

            // --- generic mode (ABI v2) -------------------------------------------
            //
            // importSubscription and probe do NOT touch the VpnService: they run on
            // a lightweight core client inside the plugin process, on a background
            // thread, and reply on the main thread. Both return the core's JSON
            // verbatim; Dart parses it.

            "importSubscription" -> {
                val raw = call.argument<String>(CarambaVpnKeys.RAW_CONFIG) ?: ""
                val format = call.argument<String>(CarambaVpnKeys.RAW_FORMAT) ?: ""
                runOnWorker(result, "import_failed") {
                    toolsCore().importSubscription(raw, format)
                }
            }

            "probe" -> {
                val timeoutMs = (call.argument<Number>(CarambaVpnKeys.TIMEOUT_MS))?.toInt() ?: 5000
                runOnWorker(result, "probe_failed") {
                    toolsCore().probeJson(timeoutMs)
                }
            }

            "setPolicy" -> {
                // Persisted, not applied here: the core that matters is the one
                // CarambaVpnService builds, and it reads this seam before up().
                persistPolicy(call.argument<String>(CarambaVpnKeys.POLICY_JSON) ?: "")
                result.success(null)
            }

            "setTunnelMode" -> {
                persistTunnelMode(
                    mode = call.argument<String>(CarambaVpnKeys.TUNNEL_MODE) ?: "tun",
                    port = (call.argument<Number>(CarambaVpnKeys.MIXED_PORT))?.toInt() ?: 7890,
                )
                result.success(null)
            }

            // --- CSM/1 device keys (ABI v3) --------------------------------------
            //
            // These do NOT touch the Go core: the key lives in the AndroidKeyStore
            // and this class is its only holder. The core reaches the same key
            // through CarambaDeviceKeyBridge, so one identity serves both paths.
            // Signing and key agreement are keystore round trips, so they run on
            // the worker thread and reply on the main one.

            "deviceKeygen" -> {
                runOnWorker(result, "device_keygen_failed") {
                    deviceKeys().keygen("{}")
                }
            }

            "deviceSign" -> {
                val messageB64 = call.argument<String>(CarambaVpnKeys.MESSAGE_B64) ?: ""
                runOnWorker(result, "device_sign_failed") {
                    deviceKeys().sign(JSONObject().put("message_b64", messageB64).toString())
                }
            }

            "deviceAgree" -> {
                val peerB64 = call.argument<String>(CarambaVpnKeys.PEER_PUB_B64) ?: ""
                val rkv = (call.argument<Number>(CarambaVpnKeys.RKV))?.toInt() ?: 0
                runOnWorker(result, "device_agree_failed") {
                    deviceKeys().agree(
                        JSONObject().put("rkv", rkv).put("peer_pub_b64", peerB64).toString()
                    )
                }
            }

            "csmRequestSettings" -> {
                // The write goes through the core, never through a socket opened
                // here: a control plane with its own sockets bypasses the transport
                // ladder, and the app degenerates to rung R0 while the core is
                // still climbing for a configuration it can no longer change
                // (02-SPEC.md 8.9).
                val json = call.argument<String>(CarambaVpnKeys.POLICY_JSON) ?: "{}"
                runOnWorker(result, "csm_write_failed") {
                    csmCore().csmRequestSettings(json)
                }
            }

            "csmState" -> {
                // A read of what the core already verified. Off the main thread
                // all the same: the core takes its own lock, and blocking the
                // platform thread on it would stall the UI.
                runOnWorker(result, "csm_state_failed") { csmCore().csmState() }
            }

            "csmLadder" -> {
                runOnWorker(result, "csm_ladder_failed") { csmCore().csmLadder() }
            }

            "routeReport" -> {
                // What the LAST raise applied to routing. Also a read, also off
                // the main thread.
                //
                // Read from the core that RAISED — CarambaVpnService's — and not
                // from the plugin's own cores, on which up() is never called:
                // they would answer "no tunnel has been raised by this core
                // instance" for the rest of the install, which is true of them
                // and a lie about the device. Without it the settings screen has
                // no way to tell a working ad block from an enabled and dead one,
                // which is the whole reason the report exists.
                //
                // The fallback is the CSM core, reached only when nothing has
                // raised in this process yet. Its answer is the core's OWN
                // not_raised JSON: an empty string here would read on the Dart
                // side as "this build has no bridge" (AppliedRoute.unsupported),
                // and a report fabricated in Kotlin would be a fourth shape of
                // the contract nobody verifies.
                runOnWorker(result, "route_report_failed") {
                    CarambaCore.raisedCore()?.routeReport() ?: csmCore().routeReport()
                }
            }

            "csmEnroll" -> {
                // Enrolment goes THROUGH the core and up the ladder: it is the
                // one moment trust is created, and a socket opened here would
                // be a path to the operator the ladder cannot see.
                val json = call.argument<String>(CarambaVpnKeys.POLICY_JSON) ?: "{}"
                runOnWorker(result, "csm_enroll_failed") { csmCore().csmEnroll(json) }
            }

            "csmRefresh" -> {
                val timeout = (call.argument<Number>(CarambaVpnKeys.TIMEOUT_SEC))?.toInt() ?: 30
                runOnWorker(result, "csm_refresh_failed") { csmCore().csmRefresh(timeout) }
            }

            "csmSetLadder" -> {
                val json = call.argument<String>(CarambaVpnKeys.POLICY_JSON) ?: "{}"
                runOnWorker(result, "csm_set_ladder_failed") {
                    csmCore().csmSetLadder(json)
                    "{\"ok\":true}"
                }
            }

            "csmAnswerCatalogChange" -> {
                val json = call.argument<String>(CarambaVpnKeys.POLICY_JSON) ?: "{}"
                runOnWorker(result, "csm_catalog_answer_failed") {
                    csmCore().csmAnswerCatalogChange(json)
                }
            }

            "csmSelectProfile" -> {
                val key = call.argument<String>(CarambaVpnKeys.CSM_PROFILE_KEY) ?: ""
                runOnWorker(result, "csm_select_profile_failed") {
                    selectCsmProfile(key)
                    "{\"ok\":true}"
                }
            }

            "disconnect" -> {
                disconnect()
                result.success(null)
            }

            "status" -> {
                result.success(CarambaVpnBus.currentStatus().asMap())
            }

            else -> result.notImplemented()
        }
    }

    // MARK: Connect / Disconnect

    private fun connect(args: Map<String, String>, result: Result) {
        // Optimistic connecting frame so the UI reacts immediately.
        CarambaVpnBus.publishStatus(
            CarambaStatusSnapshot(CarambaStage.CONNECTING, "Securing tunnel", 0L)
        )

        // VpnService.prepare returns an Intent if the user has not yet granted VPN
        // consent (or it was revoked). Showing it requires an Activity; if none is
        // attached we still attempt to start (the service will surface an error if
        // consent is missing).
        val consent: Intent? = VpnService.prepare(appContext)
        if (consent != null) {
            val act = activity
            if (act == null) {
                CarambaVpnBus.publishStatus(
                    CarambaStatusSnapshot(
                        CarambaStage.ERROR,
                        "VPN permission required; no foreground screen to request it",
                        0L,
                    )
                )
                result.error("no_activity", "VPN consent needs a foreground Activity", null)
                return
            }
            pendingConnectArgs = args
            pendingResult = result
            act.startActivityForResult(consent, VPN_REQUEST_CODE)
            return
        }

        startService(args)
        result.success(null)
    }

    private fun disconnect() {
        val intent = Intent(appContext, CarambaVpnService::class.java).apply {
            action = CarambaVpnKeys.ACTION_DISCONNECT
        }
        // A normal startService is enough to deliver the stop command; the service
        // tears down the tunnel and stops itself from the foreground.
        appContext.startService(intent)
    }

    private fun startService(args: Map<String, String>) {
        val intent = Intent(appContext, CarambaVpnService::class.java).apply {
            action = CarambaVpnKeys.ACTION_CONNECT
            putExtra(CarambaVpnKeys.SERVER_ID, args[CarambaVpnKeys.SERVER_ID])
            putExtra(CarambaVpnKeys.SERVER_NAME, args[CarambaVpnKeys.SERVER_NAME])
            putExtra(CarambaVpnKeys.COUNTRY_CODE, args[CarambaVpnKeys.COUNTRY_CODE])
            // rawSub path extras (present only for connectRaw). RAW_MODE toggles the
            // service's import branch; the raw payload + format ride the same intent.
            if (args[CarambaVpnKeys.RAW_MODE] != null) {
                putExtra(CarambaVpnKeys.RAW_MODE, args[CarambaVpnKeys.RAW_MODE])
                putExtra(CarambaVpnKeys.RAW_CONFIG, args[CarambaVpnKeys.RAW_CONFIG])
                putExtra(CarambaVpnKeys.RAW_FORMAT, args[CarambaVpnKeys.RAW_FORMAT])
                putExtra(CarambaVpnKeys.RAW_LABEL, args[CarambaVpnKeys.RAW_LABEL])
            }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            appContext.startForegroundService(intent)
        } else {
            appContext.startService(intent)
        }
    }

    // MARK: Generic mode helpers (ABI v2)

    /**
     * Runs a blocking core call on a background thread and replies on the main
     * thread. importSubscription parses a whole subscription and probe dials
     * every node, so neither may sit on the platform thread.
     */
    private fun runOnWorker(result: Result, errorCode: String, body: () -> String) {
        Thread({
            val reply: Result = result
            try {
                val json = body()
                mainHandler.post { reply.success(json) }
            } catch (t: Throwable) {
                val message = t.message ?: errorCode
                mainHandler.post { reply.error(errorCode, message, null) }
            }
        }, "caramba-vpn-tools").start()
    }

    /**
     * Lazily builds the metadata-only core client shared by importSubscription
     * and probe. It lives in the plugin process and keeps its own work dir, so a
     * probe never disturbs a running tunnel (whose core lives in the service).
     *
     * Synchronized: it is called from the worker thread, and two concurrent
     * generic-mode calls must not build two clients over the same work dir.
     */
    @Synchronized
    private fun toolsCore(): CarambaCore {
        val existing = tools
        if (existing != null) return existing
        val dir = File(appContext.filesDir, "caramba-core-tools")
        if (!dir.exists()) dir.mkdirs()
        val created = CarambaCore.createTools(
            workDir = dir.absolutePath,
            tokenPath = File(dir, "tokens.json").absolutePath,
        )
        tools = created
        return created
    }

    /**
     * The AndroidKeyStore holder of the device identity. One instance: the
     * identity is established once and both the channel calls and the Go core's
     * bridge reach the same key through it.
     */
    @Synchronized
    private fun deviceKeys(): CarambaDeviceKeys {
        val existing = device
        if (existing != null) return existing
        val created = CarambaDeviceKeys(appContext)
        device = created
        return created
    }

    /**
     * The core client that owns the CSM/1 profile: enrollment state, the pinned
     * root, the monotonic marks and the transport ladder.
     *
     * It keeps a PERSISTENT work dir, unlike the tools client, because the CSM
     * store is the profile's identity and it must survive a restart. The device
     * key bridge is registered BEFORE any CSM call, so the core never falls back
     * to a software key it would then have to keep forever.
     */
    @Synchronized
    private fun csmCore(): CarambaCore {
        val existing = csm
        if (existing != null) return existing
        loopbackProxyUrl = CarambaVpnBus.currentLoopbackProxy()
        val dir = File(appContext.filesDir, "caramba-core-csm")
        if (!dir.exists()) dir.mkdirs()
        val prefs = appContext.getSharedPreferences(CarambaVpnKeys.PREFS, Context.MODE_PRIVATE)
        val created = CarambaCore.createCsm(
            panelUrl = prefs.getString(CarambaVpnKeys.PANEL_URL, "") ?: "",
            subUrl = prefs.getString(CarambaVpnKeys.SUB_URL, "") ?: "",
            workDir = dir.absolutePath,
            tokenPath = File(dir, "tokens.json").absolutePath,
            subscriptionId = prefs.getString(CarambaVpnKeys.SUBSCRIPTION_ID, "") ?: "",
            accessToken = prefs.getString(CarambaVpnKeys.ACCESS_TOKEN, "") ?: "",
            bridge = CarambaDeviceKeyBridge(deviceKeys()),
        )
        csm = created
        if (csmProfileKey.isNotEmpty()) {
            created.csmSelectProfile(csmProfileKey)
        }
        // The loopback rung of THIS core has to be told the listener address,
        // because `up` runs on the tunnel core and never on this one. Without
        // it R4 here is permanently not_configured and the ladder degrades to
        // R1 and R5 on the platform the listener exists for.
        applyLoopbackToCsm(created)
        return created
    }

    /**
     * Points the CSM store at one profile (02-SPEC.md 1.2). The store holds the
     * pinned root, the device registration, the monotonic marks and the attempt
     * history, and one store per app puts the second operator's on top of the
     * first operator's. Changing the key drops the current core so the next CSM
     * call rebuilds it against the new profile's directory.
     */
    @Synchronized
    private fun selectCsmProfile(key: String) {
        if (csmProfileKey == key) return
        csmProfileKey = key
        csm = null
    }

    /**
     * Hands the loopback listener address, credential included, to the CSM core.
     * An empty string means the engine is down and rung R4 has no path.
     */
    @Synchronized
    private fun applyLoopbackToCsm(target: CarambaCore? = csm) {
        val core = target ?: return
        try {
            core.csmSetLadder(
                JSONObject().put("tunnel_proxy", loopbackProxyUrl).toString()
            )
        } catch (_: Throwable) {
            // A core built before ABI v3 has no such symbol. The ladder then
            // keeps R4 unavailable, which is the honest side of the failure.
        }
    }

    /**
     * The tunnel core published a new loopback address (a raise) or an empty one
     * (a teardown). Pushed straight into the CSM core, which is the only place
     * that needs it: an empty string returns rung R4 to not_configured.
     */
    @Synchronized
    private fun onLoopbackProxy(url: String) {
        loopbackProxyUrl = url
        applyLoopbackToCsm()
    }

    private fun persistPolicy(json: String) {
        appContext.getSharedPreferences(CarambaVpnKeys.PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(CarambaVpnKeys.PREF_POLICY_JSON, json)
            .apply()
    }

    private fun persistTunnelMode(mode: String, port: Int) {
        appContext.getSharedPreferences(CarambaVpnKeys.PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(CarambaVpnKeys.PREF_TUNNEL_MODE, mode)
            .putInt(CarambaVpnKeys.PREF_MIXED_PORT, port)
            .apply()
    }

    private fun persistSeam(
        panelUrl: String,
        subUrl: String,
        subscriptionId: String,
        accessToken: String,
    ) {
        // Persist the seam so CarambaVpnService can read it even when the system
        // restarts the service in a fresh process. Stored in the app's private
        // prefs (MODE_PRIVATE) — not world-readable.
        appContext.getSharedPreferences(CarambaVpnKeys.PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(CarambaVpnKeys.PANEL_URL, panelUrl)
            .putString(CarambaVpnKeys.SUB_URL, subUrl)
            .putString(CarambaVpnKeys.SUBSCRIPTION_ID, subscriptionId)
            .putString(CarambaVpnKeys.ACCESS_TOKEN, accessToken)
            .apply()
        // The CSM core caches the seam it was built with, and the settings write
        // authorizes with the account token from that core's own store. A core
        // built before a token rotation would keep answering 401 until the app
        // restarts, so it is dropped and rebuilt on the next CSM call. The device
        // identity survives: it lives in the AndroidKeyStore, not in the core.
        dropCsmCore()
    }

    @Synchronized
    private fun dropCsmCore() {
        csm?.close()
        csm = null
    }

    // MARK: ActivityAware (for the VPN-consent dialog)

    override fun onAttachedToActivity(binding: ActivityPluginBinding) {
        activity = binding.activity
        binding.addActivityResultListener(this)
    }

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) {
        activity = binding.activity
        binding.addActivityResultListener(this)
    }

    override fun onDetachedFromActivityForConfigChanges() {
        activity = null
    }

    override fun onDetachedFromActivity() {
        activity = null
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != VPN_REQUEST_CODE) return false
        val args = pendingConnectArgs
        val result = pendingResult
        pendingConnectArgs = null
        pendingResult = null
        if (resultCode == Activity.RESULT_OK && args != null) {
            startService(args)
            result?.success(null)
        } else {
            CarambaVpnBus.publishStatus(
                CarambaStatusSnapshot(CarambaStage.ERROR, "VPN permission denied", 0L)
            )
            result?.error("denied", "VPN permission denied", null)
        }
        return true
    }

    // MARK: CarambaVpnBus.Listener (forward to the event sinks on the main thread)

    // Held as a private anonymous object rather than implemented by the plugin
    // class itself: CarambaVpnBus.Listener and the snapshot types are `internal`,
    // and a public member of this public class may not expose them (Kotlin
    // EXPOSED_PARAMETER_TYPE). The bus already posts to the main looper.
    private val busListener = object : CarambaVpnBus.Listener {
        override fun onStatus(snapshot: CarambaStatusSnapshot) {
            statusSink?.success(snapshot.asMap())
        }

        override fun onTraffic(snapshot: CarambaTrafficSnapshot) {
            trafficSink?.success(snapshot.asMap())
        }
    }
}
