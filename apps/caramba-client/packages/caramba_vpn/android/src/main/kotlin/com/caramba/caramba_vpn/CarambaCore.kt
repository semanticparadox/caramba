package com.caramba.caramba_vpn

import org.json.JSONObject

// CarambaCore — thin Kotlin wrapper over the gomobile-generated Go core.
//
// gomobile bind on libs/caramba-core/mobile produces the Java/Kotlin class
// `mobile.Mobile` (static factory) and `mobile.Client` (instance). Method names
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
    private val client: mobile.Client,
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
            val client = mobile.Mobile.newClient(panelUrl, subUrl, workDir, tokenPath)
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
            val client = mobile.Mobile.newClient(panelUrl, subUrl, workDir, tokenPath)
            // ImportSubscription parses the raw payload into a mihomo config and
            // stores it as the imported source. gomobile maps Go
            // `ImportSubscription(raw, format string) (string, error)` to
            // `importSubscription(raw, format): String` (throws on Go error). The
            // returned metadata JSON is not needed by the channel contract.
            client.importSubscription(raw, format)
            return CarambaCore(client)
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
