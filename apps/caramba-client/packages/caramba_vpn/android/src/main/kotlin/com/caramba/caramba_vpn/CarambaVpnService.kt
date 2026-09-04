package com.caramba.caramba_vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean

// CarambaVpnService — the Android VPN tunnel.
//
// Lifecycle on ACTION_CONNECT:
//   1) build the VPN interface (VpnService.Builder: addAddress / addDnsServer /
//      addRoute, optional per-app allow/disallow) and establish().detachFd();
//   2) start as a foreground service with a persistent exarobot notification;
//   3) hand the fd to the Go core (caramba.aar: newClient + configure +
//      setTunFd(fd) + up(serverId)) which runs mihomo over the TUN;
//   4) poll status()/traffic() ~1 Hz on a background thread and publish frames to
//      CarambaVpnBus (the plugin forwards them to the event channels).
// On ACTION_DISCONNECT: down() the core, stop foreground, close the fd, stop self.
//
// CODE IDENTIFIERS stay 'caramba' (the user-facing brand is 'exarobot').
class CarambaVpnService : VpnService() {

    private companion object {
        const val NOTIF_CHANNEL_ID = "caramba_vpn"
        const val NOTIF_ID = 0x6361
        const val POLL_INTERVAL_MS = 1000L

        // TUN interface parameters. The mihomo core terminates the tunnel; these
        // are the local virtual-interface settings (private CGNAT-range address,
        // catch-all routes, DNS handled inside the core's fake-ip stack).
        const val TUN_ADDRESS = "172.19.0.1"
        const val TUN_PREFIX = 30
        const val TUN_MTU = 1500
        const val TUN_DNS = "1.1.1.1"
        const val TUN_DNS_FALLBACK = "8.8.8.8"
    }

    private var tunInterface: ParcelFileDescriptor? = null
    private var core: CarambaCore? = null
    private val running = AtomicBoolean(false)
    private var pollThread: Thread? = null

    @Volatile
    private var connectedSinceMs: Long = 0L

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            CarambaVpnKeys.ACTION_DISCONNECT -> {
                stopTunnel(CarambaStage.DISCONNECTED, null)
                return START_NOT_STICKY
            }

            CarambaVpnKeys.ACTION_CONNECT -> {
                val serverId = intent.getStringExtra(CarambaVpnKeys.SERVER_ID) ?: ""
                val serverName = intent.getStringExtra(CarambaVpnKeys.SERVER_NAME) ?: ""
                // rawSub path: non-empty RAW_MODE means import the raw config
                // instead of using the panel seam, then raise with an empty serverId.
                val rawMode = !intent.getStringExtra(CarambaVpnKeys.RAW_MODE).isNullOrEmpty()
                val rawConfig = intent.getStringExtra(CarambaVpnKeys.RAW_CONFIG) ?: ""
                val rawFormat = intent.getStringExtra(CarambaVpnKeys.RAW_FORMAT) ?: ""
                startTunnel(serverId, serverName, rawMode, rawConfig, rawFormat)
            }
        }
        // Do not auto-restart with a null intent: a VPN tunnel must be explicitly
        // re-established (consent + fresh config seam), not silently revived.
        return START_NOT_STICKY
    }

    private fun startTunnel(
        serverId: String,
        serverName: String,
        rawMode: Boolean = false,
        rawConfig: String = "",
        rawFormat: String = "",
    ) {
        if (running.get()) {
            // Already up: a second connect replaces the session. Tear down first.
            stopTunnel(CarambaStage.CONNECTING, "Switching server")
        }
        publishStatus(CarambaStage.CONNECTING, if (rawMode) "Importing profile" else "Securing tunnel")

        // Foreground BEFORE heavy work so the system does not kill us mid-setup.
        startInForeground(serverName)

        val seam = readSeam()

        // Build the local TUN interface and detach its fd for the Go core.
        val pfd: ParcelFileDescriptor
        try {
            pfd = buildInterface(serverName)
        } catch (t: Throwable) {
            publishStatus(CarambaStage.ERROR, "Failed to create tunnel interface: ${t.message}")
            stopForegroundCompat()
            stopSelf()
            return
        }
        // Дескриптор ОТДАЁТСЯ ядру, а не одалживается.
        //
        // Раньше сервис оставлял себе ParcelFileDescriptor и передавал ядру
        // pfd.fd. Владельцев становилось два: ядро закрывает дескриптор в Down(),
        // а stopTunnel закрывал его же второй раз — и Android убивал процесс
        // целиком («fdsan: double-close of file descriptor», SIGABRT). То есть
        // приложение падало каждый раз, когда человек нажимал «отключить».
        //
        // detachFd снимает владение с ParcelFileDescriptor: дальше дескриптор
        // закрывает ровно тот, кому его отдали.
        val fd = pfd.detachFd()
        tunInterface = null

        // Hand off to the Go core on a background thread (network + handshake).
        running.set(true)
        pollThread = Thread(
            { runCore(serverId, fd, seam, rawMode, rawConfig, rawFormat) },
            "caramba-vpn-core",
        ).also { it.start() }
    }

    private fun runCore(
        serverId: String,
        fd: Int,
        seam: Seam,
        rawMode: Boolean,
        rawConfig: String,
        rawFormat: String,
    ) {
        try {
            val c = if (rawMode) {
                // rawSub path: import the raw config directly (no panel Configure).
                CarambaCore.createRaw(
                    panelUrl = seam.panelUrl,
                    subUrl = seam.subUrl,
                    workDir = workDir().absolutePath,
                    tokenPath = File(workDir(), "tokens.json").absolutePath,
                    raw = rawConfig,
                    format = rawFormat,
                )
            } else {
                CarambaCore.create(
                    panelUrl = seam.panelUrl,
                    subUrl = seam.subUrl,
                    workDir = workDir().absolutePath,
                    tokenPath = File(workDir(), "tokens.json").absolutePath,
                    subscriptionId = seam.subscriptionId,
                    accessToken = seam.accessToken,
                )
            }
            core = c
            // ABI v2: policy and capture mode are applied BEFORE up() so the
            // assembled mihomo config already carries them.
            if (seam.policyJson.isNotEmpty()) {
                c.setPolicyJson(seam.policyJson)
            }
            if (seam.tunnelMode.isNotEmpty() && seam.tunnelMode != "tun") {
                c.setTunnelMode(seam.tunnelMode, seam.mixedPort)
            }
            // fd MUST be set before up(); on Android the OS owns routing, so the
            // core leaves auto-route off and uses this descriptor as the TUN.
            c.setTunFd(fd)
            // Raise the tunnel. Both paths pass serverId: on the panel path it is
            // the subscription node, on the raw path it is the ABI v2 pin of the
            // CARAMBA selector to one proxy of the imported config (empty = auto).
            c.up(serverId) // blocks until applied; throws on failure.
            // The loopback listener exists only while the engine is up, and its
            // credential is minted per raise. The CSM core lives in the plugin
            // and never has up() called on it, so the address is handed across
            // the bus; without it that core's rung R4 is permanently
            // not_configured (02-SPEC.md 8.2).
            CarambaVpnBus.publishLoopbackProxy(
                try {
                    c.loopbackProxyUrl()
                } catch (_: Throwable) {
                    ""
                }
            )
            connectedSinceMs = System.currentTimeMillis()
            pollLoop(c)
        } catch (t: Throwable) {
            publishStatus(CarambaStage.ERROR, t.message ?: "tunnel failed to start")
            stopTunnel(CarambaStage.ERROR, t.message)
        }
    }

    private fun pollLoop(c: CarambaCore) {
        while (running.get()) {
            try {
                val status = c.status()
                // Prefer the core's connectedSinceMs; fall back to the local mark
                // so the UI uptime timer starts even if the core reports 0.
                val since = if (status.connectedSinceMs > 0) status.connectedSinceMs else connectedSinceMs
                publishStatusSnapshot(
                    CarambaStatusSnapshot(status.stage, status.detail, since)
                )

                if (status.stage == CarambaStage.CONNECTED) {
                    publishTraffic(c.traffic())
                } else {
                    publishTraffic(CarambaTrafficSnapshot.ZERO)
                }
            } catch (t: Throwable) {
                // A transient read error should not kill the tunnel; keep polling.
                publishTraffic(CarambaTrafficSnapshot.ZERO)
            }
            try {
                Thread.sleep(POLL_INTERVAL_MS)
            } catch (ie: InterruptedException) {
                Thread.currentThread().interrupt()
                break
            }
        }
    }

    private fun stopTunnel(stage: String, detail: String?) {
        running.set(false)
        pollThread?.interrupt()
        pollThread = null

        try {
            core?.down()
        } catch (_: Throwable) {
            // Best-effort teardown.
        }
        core = null
        // The listener is gone with the engine, so R4 has no path again.
        CarambaVpnBus.publishLoopbackProxy("")
        connectedSinceMs = 0L

        // Закрывать дескриптор здесь НЕЛЬЗЯ: владение ушло вместе с detachFd,
        // и ядро уже закрыло его в down() выше. Второй close ронял процесс.
        tunInterface = null

        publishTraffic(CarambaTrafficSnapshot.ZERO)
        publishStatus(stage, detail)

        stopForegroundCompat()
        stopSelf()
    }

    override fun onDestroy() {
        // Revoked from settings, swiped away, or system reclaim: tear down cleanly.
        // `tunInterface` больше не признак поднятого туннеля: дескриптор отдан
        // ядру через detachFd и здесь всегда null. Живость определяет `running`.
        if (running.get() || core != null) {
            stopTunnel(CarambaStage.DISCONNECTED, null)
        }
        super.onDestroy()
    }

    override fun onRevoke() {
        // The user (or another VPN app) revoked our permission.
        stopTunnel(CarambaStage.DISCONNECTED, "VPN permission revoked")
        super.onRevoke()
    }

    // MARK: VPN interface

    private fun buildInterface(serverName: String): ParcelFileDescriptor {
        val builder = Builder()
            .setSession(if (serverName.isNotEmpty()) "exarobot — $serverName" else "exarobot")
            .setMtu(TUN_MTU)
            .addAddress(TUN_ADDRESS, TUN_PREFIX)
            // Catch-all routes: send all IPv4 (and IPv6) traffic into the tunnel.
            // The mihomo core's rule engine then decides direct vs proxy per the
            // panel config + client policy (split tunnel is applied inside rules).
            .addRoute("0.0.0.0", 0)
            .addRoute("::", 0)
            .addDnsServer(TUN_DNS)
            .addDnsServer(TUN_DNS_FALLBACK)

        // Keep our own app out of the tunnel so config/subscription fetches and
        // the management plane never loop back through mihomo while connecting.
        try {
            builder.addDisallowedApplication(packageName)
        } catch (_: Exception) {
            // packageName always installed; guard is defensive only.
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            builder.setMetered(false)
        }

        return builder.establish()
            ?: throw IllegalStateException("VpnService.establish() returned null (consent missing?)")
    }

    // MARK: Foreground notification (exarobot-branded, no emoji)

    private fun startInForeground(serverName: String) {
        ensureNotificationChannel()
        val notification = buildNotification(serverName)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIF_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
            )
        } else {
            startForeground(NOTIF_ID, notification)
        }
    }

    private fun ensureNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val mgr = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (mgr.getNotificationChannel(NOTIF_CHANNEL_ID) != null) return
        val channel = NotificationChannel(
            NOTIF_CHANNEL_ID,
            "VPN connection",
            NotificationManager.IMPORTANCE_LOW, // no sound; persistent status only
        ).apply {
            description = "Shows the exarobot tunnel while it is active"
            setShowBadge(false)
        }
        mgr.createNotificationChannel(channel)
    }

    private fun buildNotification(serverName: String): Notification {
        // Tapping the notification opens the app's launcher activity.
        val launch = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val pendingFlags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        } else {
            PendingIntent.FLAG_UPDATE_CURRENT
        }
        val contentIntent = if (launch != null) {
            PendingIntent.getActivity(this, 0, launch, pendingFlags)
        } else {
            null
        }

        val title = "exarobot"
        val text = if (serverName.isNotEmpty()) "Connected via $serverName" else "Tunnel active"

        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, NOTIF_CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        builder
            .setContentTitle(title)
            .setContentText(text)
            // Use the app's own icon; falls back to a system icon if absent so the
            // build never breaks on a missing drawable.
            .setSmallIcon(resolveSmallIcon())
            .setOngoing(true)
            .setOnlyAlertOnce(true)
        if (contentIntent != null) {
            builder.setContentIntent(contentIntent)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
            builder.setVisibility(Notification.VISIBILITY_PUBLIC)
        }
        return builder.build()
    }

    private fun resolveSmallIcon(): Int {
        // Prefer the host app's launcher icon (ic_launcher); fall back to a stock
        // system icon so a fresh `flutter create .` without a custom icon builds.
        val appIcon = applicationInfo.icon
        if (appIcon != 0) return appIcon
        // Plugin-owned fallback. android.R.drawable.stat_sys_vpn_ic is a hidden
        // framework id (not in the public SDK), so it cannot be referenced here.
        return R.drawable.ic_caramba_vpn
    }

    private fun stopForegroundCompat() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            stopForeground(STOP_FOREGROUND_REMOVE)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(true)
        }
    }

    // MARK: Seam (config) + workDir

    private data class Seam(
        val panelUrl: String,
        val subUrl: String,
        val subscriptionId: String,
        val accessToken: String,
        // ABI v2 policy + capture mode, written by setPolicy() / setTunnelMode()
        // on the plugin. Applied to the core BEFORE up().
        val policyJson: String,
        val tunnelMode: String,
        val mixedPort: Int,
    )

    private fun readSeam(): Seam {
        val p = getSharedPreferences(CarambaVpnKeys.PREFS, Context.MODE_PRIVATE)
        return Seam(
            panelUrl = p.getString(CarambaVpnKeys.PANEL_URL, "") ?: "",
            subUrl = p.getString(CarambaVpnKeys.SUB_URL, "") ?: "",
            subscriptionId = p.getString(CarambaVpnKeys.SUBSCRIPTION_ID, "") ?: "",
            accessToken = p.getString(CarambaVpnKeys.ACCESS_TOKEN, "") ?: "",
            policyJson = p.getString(CarambaVpnKeys.PREF_POLICY_JSON, "") ?: "",
            // Android owns the TUN fd, so "tun" stays the default here; "proxy"
            // is honoured for the rare no-TUN debugging case.
            tunnelMode = p.getString(CarambaVpnKeys.PREF_TUNNEL_MODE, "tun") ?: "tun",
            mixedPort = p.getInt(CarambaVpnKeys.PREF_MIXED_PORT, 7890),
        )
    }

    private fun workDir(): File {
        // Private app storage for the assembled mihomo config + token store.
        val dir = File(filesDir, "caramba-core")
        if (!dir.exists()) dir.mkdirs()
        return dir
    }

    // MARK: Publish helpers (to the bus, which forwards to the event channels)

    private fun publishStatus(stage: String, detail: String?) {
        publishStatusSnapshot(CarambaStatusSnapshot(stage, detail, if (stage == CarambaStage.CONNECTED) connectedSinceMs else 0L))
    }

    private fun publishStatusSnapshot(snapshot: CarambaStatusSnapshot) {
        CarambaVpnBus.publishStatus(snapshot)
    }

    private fun publishTraffic(snapshot: CarambaTrafficSnapshot) {
        CarambaVpnBus.publishTraffic(snapshot)
    }
}
