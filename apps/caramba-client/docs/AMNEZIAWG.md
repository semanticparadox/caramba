# AmneziaWG through the mihomo core

This explains how an AmneziaWG connection reaches the exarobot client, the exact
mihomo YAML the panel must emit, what has to run on the node for packets to pass,
and the operational flag that controls it.

> Nothing here has been compiled or run in this repo. The schema below was read
> from the pinned mihomo source (`metacubex/mihomo v1.19.27`,
> `adapter/outbound/wireguard.go`). If you bump the mihomo version, re-check the
> struct tags before trusting this document.

> Branding note: user-facing strings say exarobot. Code identifiers stay caramba.
> The mihomo selector constant `CARAMBA` is a panel to client contract and must
> not be renamed.

## The path: client, panel, node

1. The Flutter client asks the panel for `clash_url`
   (`GET /api/v2/app/subscription`). That URL is `/sub/<uuid>?client=clash`. The
   Go core (mihomo) pulls that, never the V2Ray or sing-box variants.
2. The panel renders the subscription as a clash/mihomo YAML
   (`subscription_generator.rs`). For an AmneziaWG inbound it emits a `wireguard`
   proxy with an `amnezia-wg-option` block (see the schema below).
3. The Go core overlays local policy on that YAML
   (`libs/caramba-core/profile/AssembleMihomoConfig`). It rewrites the top level
   sections (general, tun, dns, rules) and prepends the protocol group, but it
   never touches `proxies`, so the AmneziaWG parameters pass through unchanged.
4. Selecting protocol `AmneziaWG` in the client maps to clash type `wireguard`
   (`protocolClashType`) and groups every `wireguard` proxy into the
   `Caramba-Proto` url-test group, which is placed first inside the `CARAMBA`
   selector. mihomo then dials one of those proxies.
5. mihomo performs an AmneziaWG handshake against the node using the obfuscation
   parameters from `amnezia-wg-option`. Packets flow only if the node runs a real
   AmneziaWG server with matching parameters (see node requirement).

## The exact mihomo YAML

mihomo reads the obfuscation from a nested `amnezia-wg-option` map, not from flat
proxy keys and not from a key named `amnezia-wg`. Inside that map,
`jc/jmin/jmax/s1/s2/s3/s4` are integers and `h1/h2/h3/h4` are strings. Getting the
key name or the `h1..h4` type wrong makes mihomo drop the obfuscation and fall
back to a plain WireGuard handshake, which is DPI visible and fails against an
AmneziaWG-only server.

```yaml
proxies:
  - name: "NL AmneziaWG"
    type: wireguard
    server: node.example.com
    port: 51820
    ip: "10.10.0.2/32"        # client local address, /32
    private-key: <user awg private key>
    public-key: <server awg public key>
    udp: true
    mtu: 1280
    amnezia-wg-option:
      jc: 4                    # int
      jmin: 8                  # int
      jmax: 80                 # int
      s1: 15                   # int
      s2: 25                   # int
      s3: 0                    # int, optional (newer)
      s4: 0                    # int, optional (newer)
      h1: "1111111111"         # string, not a number
      h2: "2222222222"         # string
      h3: "3333333333"         # string
      h4: "4444444444"         # string
```

Source of truth, `metacubex/mihomo v1.19.27`,
`adapter/outbound/wireguard.go`:

```go
// in WireGuardOption
AmneziaWGOption *AmneziaWGOption `proxy:"amnezia-wg-option,omitempty"`

// AmneziaWGOption
JC   int    `proxy:"jc,omitempty"`
JMin int    `proxy:"jmin,omitempty"`
JMax int    `proxy:"jmax,omitempty"`
S1   int    `proxy:"s1,omitempty"`
S2   int    `proxy:"s2,omitempty"`
S3   int    `proxy:"s3,omitempty"`
S4   int    `proxy:"s4,omitempty"`
H1   string `proxy:"h1,omitempty"`
H2   string `proxy:"h2,omitempty"`
H3   string `proxy:"h3,omitempty"`
H4   string `proxy:"h4,omitempty"`
```

The node stores `h1..h4` as numbers (`AmneziaWgInbound` in `singbox/config.rs`
types them as `u32`), so the panel stringifies them on emission. That conversion
lives in `subscription_generator.rs` in the `"amneziawg"` clash branch.

## Node requirement: a real AmneziaWG server

The client side being correct is not enough. For traffic to pass, a genuine
AmneziaWG-capable WireGuard server must be listening on `node.address:listen_port`
and it must:

- speak the AmneziaWG wire protocol with the SAME `jc/jmin/jmax/s1/s2/h1..h4` the
  panel hands the client. The two ends must agree or the obfuscated handshake
  fails.
- have the user public key registered as a peer. The client private key is
  derived from the user uuid (`subscription_service::derive_awg_key`). Its public
  key must be a peer on the server, with allowed-ips covering the client
  `10.10.0.X/32` and routing `0.0.0.0/0` for return traffic.
- expose its own WireGuard public key as the proxy `public-key`.

Stock sing-box CANNOT be this server. The sing-box build installed from
`deb.sagernet.org` has no `wireguard` inbound and does not understand the
obfuscation fields. A server that CAN serve it is `amneziawg-go` /
`wireguard-go` with the AmneziaWG patch, an AmneziaVPN server, or an
AmneziaWG-patched sing-box fork. Without such a server running and peered, no
packets flow no matter how correct the client proxy is.

## The CARAMBA_ENABLE_AMNEZIAWG flags

Two flags gate AmneziaWG, for two different reasons.

`CARAMBA_ENABLE_AMNEZIAWG` (default off) is the strict node-side gate. When off,
the panel refuses to write an AmneziaWG inbound into a sing-box node config and
omits AmneziaWG from the sing-box and V2Ray subscriptions. This protects nodes:
an AmneziaWG inbound in a stock sing-box config makes `sing-box check` fail and
takes down the whole node. Leave this off unless a node actually runs an
AmneziaWG-capable sing-box fork.

`CARAMBA_ENABLE_AMNEZIAWG_CLIENT` (default off) gates only the clash/mihomo
subscription. The mihomo client speaks AmneziaWG natively, so it can receive a
`wireguard` proxy even while sing-box nodes stay gated. Turning this on adds the
proxy to the clash YAML only. It never writes an AmneziaWG inbound to a sing-box
node, so it cannot put a node into a failing state. The proxy is inert until a
real AmneziaWG server is running on that node.

`CARAMBA_ENABLE_AMNEZIAWG` implies the client flag: if the combined switch is on,
the clash emission is on too. Each accepts `1`, `true`, `yes`, or `on`.

Operational recipe to give the mihomo client a working AmneziaWG proxy without
touching sing-box nodes:

1. Stand up an AmneziaWG-capable WireGuard server on the node and register the
   user public key as a peer.
2. Make the node inbound carry the obfuscation parameters that match the server.
3. Set `CARAMBA_ENABLE_AMNEZIAWG_CLIENT=1` on the panel.
4. Leave `CARAMBA_ENABLE_AMNEZIAWG` off so the sing-box node config stays valid.

## Files

- `apps/caramba-panel/src/singbox/subscription_generator.rs` — clash `wireguard`
  proxy with the `amnezia-wg-option` block; node side stringifies `h1..h4`.
- `apps/caramba-panel/src/utils.rs` — `amneziawg_enabled` (node) and
  `amneziawg_client_enabled` (mihomo client).
- `libs/caramba-core/profile/profile.go` — overlay that preserves `proxies` and
  routes protocol `AmneziaWG` to the `wireguard` proxies via `Caramba-Proto`.
