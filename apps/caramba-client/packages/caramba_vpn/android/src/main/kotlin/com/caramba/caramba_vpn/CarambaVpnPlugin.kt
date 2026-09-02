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
    PluginRegistry.ActivityResultListener,
    CarambaVpnBus.Listener {

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

        CarambaVpnBus.setListener(this)
    }

    override fun onDetachedFromEngine(@NonNull binding: FlutterPlugin.FlutterPluginBinding) {
        CarambaVpnBus.setListener(null)
        tools?.close()
        tools = null
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

    override fun onStatus(snapshot: CarambaStatusSnapshot) {
        statusSink?.success(snapshot.asMap())
    }

    override fun onTraffic(snapshot: CarambaTrafficSnapshot) {
        trafficSink?.success(snapshot.asMap())
    }
}
