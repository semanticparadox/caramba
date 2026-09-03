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

    /**
     * The loopback service inbound address, credential included, of the current
     * raise. Empty while the engine is down.
     *
     * It travels the bus for the same reason status does: the tunnel core lives
     * in CarambaVpnService and the CSM core lives in the plugin, and the CSM
     * core never has `up` called on it. Without this handoff its rung R4 stays
     * not_configured forever and the ladder quietly degrades to R1 and R5 on
     * the one platform the loopback listener was added for.
     *
     * The value is a credential and it is never published to a Flutter sink:
     * only the plugin reads it, and only to hand it back to the core.
     */
    @Volatile
    private var lastLoopbackProxy: String = ""

    @Volatile
    private var loopbackListener: ((String) -> Unit)? = null

    /** The plugin registers a hook and gets the current value replayed. */
    fun setLoopbackListener(l: ((String) -> Unit)?) {
        loopbackListener = l
        if (l != null) {
            val v = lastLoopbackProxy
            mainHandler.post { l(v) }
        }
    }

    /** Current loopback address, for a CSM core built after the raise. */
    fun currentLoopbackProxy(): String = lastLoopbackProxy

    /** Called by the service after up() and after teardown. */
    fun publishLoopbackProxy(url: String) {
        lastLoopbackProxy = url
        val l = loopbackListener ?: return
        mainHandler.post { l(url) }
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
