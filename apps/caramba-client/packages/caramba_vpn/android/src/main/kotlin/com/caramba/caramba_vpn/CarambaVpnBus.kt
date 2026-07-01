package com.caramba.caramba_vpn

import android.os.Handler
import android.os.Looper

// CarambaVpnBus — process-wide event bus decoupling the writer (CarambaVpnService,
// which runs the Go core and polls status/traffic) from the reader
// (CarambaVpnPlugin, which owns the Flutter EventChannel sinks).
//
// The service and the plugin live in the same process, but their lifecycles are
// independent: the service can be (re)started by the system while no Flutter
// engine is attached, and the Flutter engine can detach/reattach (e.g. the
// activity is recreated) while the tunnel keeps running. The bus holds the last
// known status/traffic so a freshly attached sink renders the current state
// immediately, matching the Dart side's broadcast+cache expectation.
//
// All listener callbacks are delivered on the main thread (EventChannel sinks
// must be fed from the platform-thread / main looper).
internal object CarambaVpnBus {

    private val mainHandler = Handler(Looper.getMainLooper())

    @Volatile
    private var lastStatus: CarambaStatusSnapshot = CarambaStatusSnapshot.DISCONNECTED

    @Volatile
    private var lastTraffic: CarambaTrafficSnapshot = CarambaTrafficSnapshot.ZERO

    @Volatile
    private var listener: Listener? = null

    interface Listener {
        fun onStatus(snapshot: CarambaStatusSnapshot)
        fun onTraffic(snapshot: CarambaTrafficSnapshot)
    }

    /** The plugin registers its sink-forwarding listener and replays cached state. */
    fun setListener(l: Listener?) {
        listener = l
        if (l != null) {
            val s = lastStatus
            val t = lastTraffic
            mainHandler.post {
                l.onStatus(s)
                l.onTraffic(t)
            }
        }
    }

    /** Last status, for the synchronous MethodChannel `status` call. */
    fun currentStatus(): CarambaStatusSnapshot = lastStatus

    /** Called by the service (any thread). Caches and forwards on the main thread. */
    fun publishStatus(snapshot: CarambaStatusSnapshot) {
        lastStatus = snapshot
        val l = listener ?: return
        mainHandler.post { l.onStatus(snapshot) }
    }

    /** Called by the service (any thread). Caches and forwards on the main thread. */
    fun publishTraffic(snapshot: CarambaTrafficSnapshot) {
        lastTraffic = snapshot
        val l = listener ?: return
        mainHandler.post { l.onTraffic(snapshot) }
    }
}
