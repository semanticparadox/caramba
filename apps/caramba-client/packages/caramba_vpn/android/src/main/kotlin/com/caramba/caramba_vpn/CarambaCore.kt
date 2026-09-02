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
        ): CarambaCore {
            val client = GoMobile.newClient(panelUrl, subUrl, workDir, tokenPath)
            // Configure injects the JWT + subscription UUID so the core is
            // authenticated before Up. panelUrl is also echoed (the core keeps the
            // NewClient URL; the arg is part of the documented plugin contract).
            client.configure(panelUrl, subscriptionId, accessToken)
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
         * Builds a core client for the metadata-only calls of generic mode
         * (importSubscription / probe). No panel, no auth, no TUN: the client is
         * only used to parse a subscription and to measure node latency, then
         * closed. Runs in the PLUGIN process, not in CarambaVpnService.
         *
         * @param workDir scratch dir for the parsed config (separate from the
         *        tunnel work dir so a probe never disturbs a live session).
         * @param tokenPath token-store file path (unused on this path).
         *
         * @throws Exception if the Go core fails to initialize.
         */
        fun createTools(workDir: String, tokenPath: String): CarambaCore {
            // NewClient only wires the client (no network), so an empty panel URL
            // is fine here — the generic path never talks to a panel.
            val client = GoMobile.newClient("", "", workDir, tokenPath)
            return CarambaCore(client)
        }
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

    /** Releases the client (used by the short-lived tools instance). */
    fun close() {
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
    fun up(serverId: String): String = client.up(serverId)

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
