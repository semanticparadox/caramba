package com.caramba.caramba_vpn

import io.caramba.core.mobile.Client as GoClient
import io.caramba.core.mobile.Mobile as GoMobile
import org.json.JSONObject

// CarambaCore — thin Kotlin wrapper over the gomobile-generated Go core.
//
// gomobile bind on libs/caramba-core/mobile produces, under the Java package
// declared by the module's `javapkg` (io.caramba.core), the classes
// `io.caramba.core.mobile.Mobile` (static factory) and `.Client` (instance),
// aliased here as GoMobile / GoClient. Method names
// are camelCased by gomobile: NewClient -> Mobile.newClient, Up -> up, Down ->
// down, StatusJSON -> statusJSON, TrafficJSON -> trafficJSON, SetTunFd ->
// setTunFd, Configure -> configure. Go errors surface as thrown Exceptions.
//
// Isolating the gomobile import here keeps the rest of the module free of the
// generated package and gives a single place to map the JSON contract shapes
// returned by statusJSON()/trafficJSON() into the snapshot types.
//
// The wrapper is intentionally stateful-but-simple: one Client per tunnel
// session, created on connect() and released on close(). The Go core keeps one
// api.Core for the Client's lifetime, so up/down/status stay consistent.
internal class CarambaCore private constructor(
    private val client: GoClient,
) {

    companion object {
        /**
         * Builds the core and applies the auth/config seam.
         *
         * @param panelUrl base panel URL (required by the Go core's NewClient).
         * @param subUrl subscription service URL (empty -> defaults to panelUrl).
         * @param workDir on-disk dir for the assembled mihomo config + cache.
         * @param tokenPath token-store file path.
         * @param subscriptionId subscription UUID; passed through configure.
         * @param accessToken live JWT injected so Up fetches the clash config
         *        (which already carries amnezia-wg) without a re-login.
         * @param refreshToken the long-lived half of the same session. Without
         *        it the core is authenticated for ~15 minutes and then dead: it
         *        has nothing to renew with, and this service outlives the app.
         * @param accessExpiryUnix when accessToken expires; 0 lets the core read
         *        the JWT's own exp claim.
         *
         * @throws Exception if the Go core fails to initialize or configure.
         */
        fun create(
            panelUrl: String,
            subUrl: String,
            workDir: String,
            tokenPath: String,
            subscriptionId: String,
            accessToken: String,
            refreshToken: String,
            accessExpiryUnix: Long,
        ): CarambaCore {
            val client = GoMobile.newClient(panelUrl, subUrl, workDir, tokenPath)
            // Configure injects the whole session + subscription UUID so the core
            // is authenticated before Up AND can stay that way. panelUrl is also
            // echoed (the core keeps the NewClient URL; the arg is part of the
            // documented plugin contract).
            client.configure(panelUrl, subscriptionId, accessToken, refreshToken, accessExpiryUnix)
            return CarambaCore(client)
        }

        /**
         * Builds the core for the rawSub path: no panel auth (configure) — the raw
         * subscription [raw] in [format] is imported into a mihomo config directly.
         *
         * @param panelUrl base URL (may be empty for a pure raw import; NewClient
         *        only wires the client, it does not touch the network here).
         * @param subUrl subscription service URL (empty -> defaults to panelUrl).
         * @param workDir on-disk dir for the assembled mihomo config + cache.
         * @param tokenPath token-store file path.
         * @param raw the raw subscription payload (clash/singbox/v2ray/uri/auto).
         * @param format the payload format hint ("" / "auto" -> autodetect).
         *
         * @throws Exception if the Go core fails to initialize or the import fails.
         */
        fun createRaw(
            panelUrl: String,
            subUrl: String,
            workDir: String,
            tokenPath: String,
            raw: String,
            format: String,
        ): CarambaCore {
            val client = GoMobile.newClient(panelUrl, subUrl, workDir, tokenPath)
            // ImportSubscription parses the raw payload into a mihomo config and
            // stores it as the imported source. gomobile maps Go
            // `ImportSubscription(raw, format string) (string, error)` to
            // `importSubscription(raw, format): String` (throws on Go error). The
            // returned metadata JSON is not needed by the channel contract.
            client.importSubscription(raw, format)
            return CarambaCore(client)
        }

        /**
         * Builds the core client that owns the CSM/1 profile.
         *
         * Differs from [create] in one thing that matters: the device key bridge
         * is registered BEFORE any CSM call. 02-SPEC.md 9.4 puts the device
         * signing key in StrongBox or the TEE, and a Go implementation would by
         * definition put it in a file, which is the software tier; registering
         * the bridge late would leave the core with a software identity it then
         * has to keep, because `dtp` has already gone to the operator.
         *
         * @param bridge the AndroidKeyStore holder of the device identity.
         *
         * @throws Exception if the Go core fails to initialize.
         */
        fun createCsm(
            panelUrl: String,
            subUrl: String,
            workDir: String,
            tokenPath: String,
            subscriptionId: String,
            accessToken: String,
            refreshToken: String,
            accessExpiryUnix: Long,
            bridge: io.caramba.core.mobile.DeviceKeyBridge,
        ): CarambaCore {
            val client = GoMobile.newClient(panelUrl, subUrl, workDir, tokenPath)
            client.setDeviceKeyBridge(bridge)
            if (subscriptionId.isNotEmpty() || accessToken.isNotEmpty()) {
                client.configure(panelUrl, subscriptionId, accessToken, refreshToken, accessExpiryUnix)
            }
            return CarambaCore(client)
        }

        /**
         * Builds the core client for the metadata-only calls (importSubscription
         * / probe). No TUN: it parses a subscription and measures node latency,
         * and runs in the PLUGIN process, not in CarambaVpnService.
         *
         * It DOES carry the panel seam. It used to be built as
         * `newClient("", "", ...)` with no auth at all, on the reasoning that
         * "the generic path never talks to a panel" — true of an imported
         * subscription and false of the panel path, which is the other caller of
         * the very same `probe`. `Core.Probe` measures the nodes of the config
         * the core has loaded, a panel-path core loads that config by fetching
         * the subscription, and a core with no panel URL and no token cannot
         * fetch anything. So it measured nothing and returned `{"servers":[]}`,
         * which the app read as "the operator gave us no nodes" — the autotune's
         * "Ядро не вернуло ни одного узла", and the reason a panel user never
         * saw a latency of their own.
         *
         * @param panelUrl base panel URL; empty keeps the pure-import behaviour.
         * @param subUrl subscription service URL (empty -> defaults to panelUrl).
         * @param workDir scratch dir for the parsed config (separate from the
         *        tunnel work dir so a probe never disturbs a live session).
         * @param tokenPath token-store file path.
         * @param subscriptionId subscription UUID; passed through configure.
         * @param accessToken live JWT so the fetch needs no re-login.
         * @param refreshToken the long-lived half of the same session, so the
         *        probe still works on a phone that has been sitting for hours.
         * @param accessExpiryUnix when accessToken expires; 0 lets the core
         *        read the JWT's own exp claim.
         *
         * @throws Exception if the Go core fails to initialize.
         */
        fun createTools(
            workDir: String,
            tokenPath: String,
            panelUrl: String = "",
            subUrl: String = "",
            subscriptionId: String = "",
            accessToken: String = "",
            refreshToken: String = "",
            accessExpiryUnix: Long = 0L,
        ): CarambaCore {
            // NewClient only wires the client (no network), so an empty panel URL
            // stays valid: that is the imported-subscription case.
            val client = GoMobile.newClient(panelUrl, subUrl, workDir, tokenPath)
            // Guarded exactly as in createCsm: configure with an empty pair would
            // write emptiness over a seam the store may already hold.
            if (subscriptionId.isNotEmpty() || accessToken.isNotEmpty()) {
                client.configure(panelUrl, subscriptionId, accessToken, refreshToken, accessExpiryUnix)
            }
            return CarambaCore(client)
        }

        /**
         * The core whose [up] last succeeded in this process, or null when none
         * has.
         *
         * The routing report is held by the core that RAISED, and on Android
         * that core belongs to CarambaVpnService. The plugin keeps two cores of
         * its own (tools and CSM) and never calls `up` on either, so asking one
         * of them for the report would answer "no tunnel has been raised by
         * this core instance" forever — an answer that is true of that core and
         * a lie about the device. The service and the plugin already share this
         * process (that is how CarambaVpnBus works), so one process-wide handle
         * is all the report needs to cross.
         *
         * Deliberately NOT cleared by [down]: the report answers "what did the
         * last raise apply", which is exactly the question asked after the
         * tunnel is taken down. The Go core keeps its own `lastRoute` across
         * Down for the same reason. Exactly one core is retained, and the next
         * successful raise replaces it.
         */
        @Volatile
        private var raised: CarambaCore? = null

        /** @see raised */
        fun raisedCore(): CarambaCore? = raised
    }

    /**
     * Parses a raw subscription into a mihomo config and returns the metadata
     * JSON (ABI v2: `{"name":...,"servers":[{id,name,type,server,port,country}]}`).
     * Does NOT raise a tunnel.
     *
     * @throws Exception on a parse failure (Go error).
     */
    fun importSubscription(raw: String, format: String): String =
        client.importSubscription(raw, format)

    /**
     * Measures the latency of every proxy in the currently loaded config
     * (ABI v2 `Client.ProbeJSON`). Returns
     * `{"servers":[{...,"latencyMs":42}]}` with -1 on timeout. Blocking: call
     * from a background thread.
     *
     * gomobile maps Go `int` to Java `long`.
     *
     * @throws Exception if the core has no config loaded.
     */
    fun probeJson(timeoutMs: Int): String = client.probeJSON(timeoutMs.toLong())

    /**
     * Applies the app-side CoreConfig before up (ABI v2 `Client.SetPolicyJSON`).
     * The JSON shape is the one produced by the Dart `CorePolicy.toJson()`.
     *
     * @throws Exception if the JSON is malformed.
     */
    fun setPolicyJson(json: String) {
        client.setPolicyJSON(json)
    }

    /**
     * Switches the traffic capture mode: "tun" (system TUN inbound, the default
     * on Android where the VpnService owns the fd) or "proxy" (local mixed
     * inbound on 127.0.0.1:port). Applied at the next up().
     *
     * @throws Exception if the mode is unknown.
     */
    fun setTunnelMode(mode: String, port: Int) {
        client.setTunnelMode(mode, port.toLong())
    }

    /**
     * Sends a settings change as a signed directive request and takes the signed,
     * sealed response as the new directive (ABI v3 `Client.CsmRequestSettings`).
     * Blocking: the request climbs the transport ladder. Returns the CSM state
     * snapshot JSON.
     *
     * @throws Exception if the write is refused or the response does not verify.
     */
    fun csmRequestSettings(json: String): String = client.csmRequestSettings(json)

    /**
     * Returns the verified CSM state snapshot as JSON (ABI v3 `Client.CsmState`).
     * A read: it touches no socket and applies nothing. Carries the trusted
     * catalog's `resources` and `routes` projection, without which the client
     * cannot notice the posture narrowing that arrives in the catalog rather
     * than in a setting (02-SPEC.md 7.7.1).
     */
    fun csmState(): String = client.csmState()

    /**
     * Returns the rung states and the local attempt history as JSON (ABI v3
     * `Client.CsmLadder`). The history is local and is never reported to the
     * operator (02-SPEC.md 7.10); this call lifts it into the app because the
     * transport screen must show every attempt with its outcome (INV-17).
     */
    fun csmLadder(): String = client.csmLadder()

    /**
     * Returns what the LAST raise actually applied to routing as JSON (ABI v3
     * `Client.RouteReport`): the preset, the fate of each of its external rule
     * sources, the GEOSITE tags it depends on, whether the GeoSite.dat base is
     * there at all, and the fate of the requested entry country.
     *
     * A read: it touches no socket and applies nothing. Without it the settings
     * screen has no way to tell a working ad block from an enabled and dead
     * one — config assembly silently drops an unreachable rule source together
     * with the rules that stepped on it, and a GEOSITE tag without the base
     * means nothing.
     */
    fun routeReport(): String = client.routeReport()

    /**
     * Enrols the profile: a bootstrap blob, or an origin with a code and a
     * dictated pin (02-SPEC.md 9). Blocking: it climbs the ladder. Returns the
     * verified state snapshot JSON, from which the app takes the `pid`, the
     * root fingerprint and the time floor and anchors the profile.
     *
     * Without this call a profile never leaves stage `pinned`, and everything
     * gated on a verified catalog stays off.
     */
    fun csmEnroll(json: String): String = client.csmEnroll(json)

    /**
     * One fetch cycle: directive, and the catalog when it is needed. A failure
     * does NOT mean a lost configuration: the profile stays on its cached
     * documents and keeps connecting (INV-16).
     */
    fun csmRefresh(timeoutSec: Int): String = client.csmRefresh(timeoutSec.toLong())

    /**
     * Applies the user's rung order, switches and proxy addresses to the CORE.
     * A write that stops in the Dart layer only changes the picture: the user
     * reorders the ladder, the screen shows the new order, and the fetch keeps
     * walking the old one.
     */
    fun csmSetLadder(json: String) {
        client.csmSetLadder(json)
    }

    /**
     * The user's answer to the resource-set change card (02-SPEC.md 7.7.1).
     * Until it arrives the core holds the PREVIOUS set in force, which is what
     * makes "keep the previous ones" mean what it says.
     */
    fun csmAnswerCatalogChange(json: String): String = client.csmAnswerCatalogChange(json)

    /**
     * Points the CSM store at one profile (02-SPEC.md 1.2: every profile state
     * store MUST be keyed by `pid`). One store per app would put the second
     * operator's pinned root, device registration, monotonic marks and attempt
     * history on top of the first operator's.
     */
    fun csmSelectProfile(key: String) {
        client.csmSelectProfile(key)
    }

    /**
     * The loopback service inbound address together with the credential minted
     * for this raise, or an empty string when the engine is down.
     *
     * The CSM profile lives in a SECOND core here, and `up` is never called on
     * it: without handing this across, that core's rung R4 stays
     * `not_configured` forever and the ladder degrades to R1 and R5 on exactly
     * the platform the loopback listener was added for.
     */
    fun loopbackProxyUrl(): String = client.loopbackProxyURL()

    /** Releases the client (used by the short-lived tools instance). */
    fun close() {
        // A closed client must not be called again, so it stops being the one
        // the route report is read from. Only this core is dropped: a close of
        // some other instance must not blind the report.
        if (raised === this) raised = null
        try {
            client.down()
        } catch (_: Throwable) {
            // Best-effort: the tools client never raised a tunnel.
        }
    }

    /** Pass the platform TUN fd to the engine BEFORE up(). */
    fun setTunFd(fd: Int) {
        client.setTunFd(fd.toLong())
    }

    /**
     * Raise the tunnel to [serverId] (empty -> panel/auto choice). The Go core
     * fetches the panel's mihomo/clash config itself; only serverId + policy go
     * across. Returns the UpResult JSON (not needed by the channel contract).
     *
     * @throws Exception if the tunnel fails to start.
     */
    fun up(serverId: String): String {
        val out = client.up(serverId)
        // Registered HERE and not in create(): the Go core takes its routing
        // snapshot only after the engine actually started, so a core that threw
        // out of up() has nothing to report and must not displace the one that
        // does. See the `raised` doc for why the report has to cross from the
        // service's core to the plugin at all.
        raised = this
        return out
    }

    /** Stop the tunnel. */
    fun down() {
        client.down()
    }

    /** Status snapshot in the channel-contract shape. */
    fun status(): CarambaStatusSnapshot {
        val json = client.statusJSON()
        val o = JSONObject(json)
        val detail = if (o.isNull("detail")) null else o.optString("detail", "").ifEmpty { null }
        return CarambaStatusSnapshot(
            stage = o.optString("stage", CarambaStage.CONNECTING),
            detail = detail,
            connectedSinceMs = o.optLong("connectedSinceMs", 0L),
        )
    }

    /** Traffic snapshot in the channel-contract shape. */
    fun traffic(): CarambaTrafficSnapshot {
        val json = client.trafficJSON()
        val o = JSONObject(json)
        return CarambaTrafficSnapshot(
            downBps = o.optLong("downBps", 0L),
            upBps = o.optLong("upBps", 0L),
            downTotal = o.optLong("downTotal", 0L),
            upTotal = o.optLong("upTotal", 0L),
        )
    }
}
