# AmneziaWG end to end (exarobot)

How AmneziaWG flows from a panel node to the native tunnel in the exarobot
client, and what has to be true on each side for packets to actually move.

AmneziaWG is WireGuard with extra obfuscation knobs (junk packets and header
magic) so that a censor cannot fingerprint the handshake. In exarobot it is one
of the protocols the client can pin (`AmneziaWG`, `VLESS-Reality`, `Hysteria2`,
`TUIC`, `Shadowsocks`). On the client it rides through the mihomo core as a
`wireguard` proxy with the amnezia fields set.

## The two sides

There are two independent halves and both must be AmneziaWG capable, or the
protocol degrades to plain WireGuard (still works, no obfuscation) or fails the
node config check.

### Node side (server inbound)

Stock sing-box has no `wireguard` inbound and does not understand the AmneziaWG
obfuscation fields, so an AmneziaWG inbound makes `sing-box check` fail and takes
down the whole node config. Because of that the panel hides AmneziaWG by default.

- Gate: the panel only writes the AmneziaWG sing-box inbound (and the share
  links and sing-box/V2Ray subscription entries) when `CARAMBA_ENABLE_AMNEZIAWG`
  is set to `1`, `true`, `yes` or `on` (`apps/caramba-panel/src/utils.rs`,
  `amneziawg_enabled()`). This is the strict NODE gate: it protects the node
  config, so leave it off unless an AmneziaWG capable sing-box fork is deployed.
- The mihomo/clash proxy emission is a SEPARATE gate (see Client side below):
  `amneziawg_client_enabled()`, satisfied by EITHER `CARAMBA_ENABLE_AMNEZIAWG`
  or `CARAMBA_ENABLE_AMNEZIAWG_CLIENT`. The inbound and share-link paths stay on
  `amneziawg_enabled()` alone, so turning on only the client flag never writes a
  sing-box inbound and cannot break a node.
- Requirement: the node must run an AmneziaWG capable sing-box fork before you
  flip `CARAMBA_ENABLE_AMNEZIAWG`. Until that fork is deployed, leave it off so a
  bad inbound does not break the node.
- When enabled, the panel writes the inbound from `AmneziaWgInbound`
  (`apps/caramba-panel/src/singbox/config.rs`) with the junk and header
  parameters (`jc`, `jmin`, `jmax`, and the header magics).

### Client side (mihomo outbound)

The client does not parse sing-box server configs. It pulls the mihomo (clash)
config the panel generates for the subscription, where the AmneziaWG node is
already expressed as a `wireguard` proxy. The mapping lives in
`libs/caramba-core/profile/profile.go` (`protocolClashType`): the friendly name
`AmneziaWG` maps to clash proxy `type: wireguard`.

- Gate: the clash `wireguard` proxy is emitted when `amneziawg_client_enabled()`
  is true (`subscription_generator.rs`, `apps/caramba-panel/src/utils.rs`),
  which is satisfied by EITHER `CARAMBA_ENABLE_AMNEZIAWG` (the combined node +
  client switch, which also un-gates the sing-box inbound) OR
  `CARAMBA_ENABLE_AMNEZIAWG_CLIENT` (client / mihomo subscription emission only,
  never writes a sing-box inbound). Each accepts `1`, `true`, `yes` or `on`.
- Use `CARAMBA_ENABLE_AMNEZIAWG_CLIENT=1` alone to hand the mihomo client a
  working AmneziaWG proxy without un-gating the node-breaking sing-box inbound:
  a real AmneziaWG server must already be listening on the node and peered, but
  the sing-box node config stays valid because no inbound is written.

The subscription generator
(`apps/caramba-panel/src/singbox/subscription_generator.rs`) fills the wireguard
outbound: `server`, `server_port`, `local_address`, `private_key`,
`peer_public_key`, `mtu`, and the amnezia obfuscation values carried alongside.
mihomo reads those and runs the obfuscated handshake.

## How the client pins it

1. UI / state calls `SetProtocol("AmneziaWG")` on the Go core (gomobile
   `Client.SetProtocol`, or the CLI flag `--protocol AmneziaWG`).
2. `applyProtocol` (`profile.go`) collects every `wireguard` proxy into a
   `Caramba-Proto` url-test group and puts it first in the `CARAMBA` selector, so
   the default pick is an AmneziaWG node. Other groups and nodes are untouched.
3. On `Up`, the mihomo engine applies the assembled config and the selector
   resolves to the AmneziaWG node. `Status.active_proxy` reflects the current
   pick from the `CARAMBA` group.

If no `wireguard` proxy exists in the subscription (AmneziaWG disabled on the
node), `applyProtocol` degrades softly and leaves the panel `Auto-All` choice in
place. Nothing breaks; you just do not get AmneziaWG.

## Auto tune

`AmneziaWG` is the highest priority protocol in autotune
(`libs/caramba-core/autotune/autotune.go`). When a server probes OK for
AmneziaWG, the recommendation prefers it. The recommendation only sets the
policy; the caller still raises the tunnel with `Up(serverId)`.

## Checklist to make AmneziaWG actually move packets

- [ ] A real AmneziaWG server is running on the node and the user public key is
      peered (stock sing-box cannot serve it).
- [ ] Panel started with the gate the deployment needs:
      `CARAMBA_ENABLE_AMNEZIAWG=1` (combined node + client; also writes the
      sing-box inbound, so only with an AmneziaWG capable sing-box fork), OR
      `CARAMBA_ENABLE_AMNEZIAWG_CLIENT=1` (client / mihomo subscription only;
      never writes a sing-box inbound, safe on stock nodes).
- [ ] The subscription mihomo config contains at least one `type: wireguard`
      proxy with the amnezia fields.
- [ ] Go core built with `-tags mihomo` (the stub engine does not raise a real
      tunnel). See `BUILDING.md`.
- [ ] Client pins the protocol with `SetProtocol("AmneziaWG")` (or leaves auto
      and lets autotune pick it).
- [ ] A real TUN is attached: mobile passes the platform fd via `SetTunFd`,
      desktop lets mihomo own the TUN. See `apps/caramba-client/INTEGRATION.md`.
