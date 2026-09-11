# AmneziaWG end to end (exarobot)

How AmneziaWG flows from a panel node to the native tunnel in the exarobot
client, and what has to be true on each side for packets to actually move.

AmneziaWG is WireGuard with extra obfuscation knobs (junk packets and header
magic) so that a censor cannot fingerprint the handshake. In exarobot it is one
of the protocols the client can pin (`AmneziaWG`, `VLESS-Reality`, `Hysteria2`,
`TUIC`, `Shadowsocks`). On the client it rides through the mihomo core as a
`wireguard` proxy with the amnezia fields set.

**This doc describes the shipped architecture** (node runs a real AmneziaWG
server; no sing-box fork). It is not an env-flag toggle over a stub anymore —
read the checklist at the bottom before flipping anything in production.

## The two sides

There are two independent halves and both must be AmneziaWG capable, or the
protocol degrades softly to "AmneziaWG not offered" (see "If AWG isn't
offered" below) rather than a hard failure.

### Node side (server) — a real, separate process

Stock sing-box has no `wireguard` inbound and does not understand the
AmneziaWG obfuscation fields (the upstream PR for it,
[sing-box#2670](https://github.com/SagerNet/sing-box/pull/2670), was closed
without merging). Forking sing-box to add it would mean maintaining a second
binary on top of the node's existing self-update pipeline. Instead, AmneziaWG
on the node is **`amneziawg-go`, an independent userspace process with its own
`awg0` interface**, managed by a dedicated module in `apps/caramba-node`
(`apps/caramba-node/src/awg/`: `download.rs`, `uapi.rs`, `iface.rs`,
`stats.rs`, `mod.rs`). sing-box never sees an AmneziaWG inbound and is not
restarted when peers change.

- **Binary delivery** (`awg/download.rs`): `amneziawg-go` has no upstream
  GitHub Releases at all — only source tags
  (`github.com/amnezia-vpn/amneziawg-go`). The node downloads a prebuilt
  `linux/amd64` binary from **the caramba release** instead, the same
  mechanism `ensure_singbox_with_v2ray_api` already uses for sing-box
  (`apps/caramba-node/src/main.rs:22-130`): one asset per agent version, same
  GitHub repo, same trust boundary. `AWG_GO_VERSION` pins the upstream source
  tag the asset was built from (`v3.1.20260828` at the time of writing —
  upstream ships new obfuscation revisions specifically in response to new RU
  blocking signatures, so this constant is expected to move with node
  releases, not sit for years). `AWG_GO_SHA256` pins the asset hash and is
  **fail-closed by design**: while it is empty, the node does not download or
  install the binary at all — there is no "trust it once" path. It gets filled
  in the same commit that teaches the release pipeline to publish the asset
  (`sha256sum` of the built binary). For a one-off manual check on a live node
  before that pipeline exists, `CARAMBA_AWG_SHA256` (env) supplies the
  expected hash instead of the constant — it does **not** disable the check,
  it only sources the expected value differently, and using it is logged as a
  warning.
- **Interface control — UAPI, not the `awg` CLI.** No dependency on
  `wireguard-tools`/`amneziawg-tools` being installed on the node: the module
  talks the text WireGuard UAPI protocol (`get=1`/`set=1`) directly over a Unix
  socket. The socket path is **not** a single constant — `awg/uapi.rs` tries,
  in order: `/var/run/amneziawg`, `/run/amneziawg`, `/var/run/wireguard`,
  `/run/wireguard` (+ `<iface>.sock`). Upstream `amneziawg-go`/`wireguard-go`
  bake the directory in at link time and it differs across builds, so probing
  beats hard-coding one path. `set=1` is built as a diff against the last
  `get=1` snapshot (`iface.rs`/`mod.rs::apply`) — unchanged peers are not
  re-sent, so live handshakes and counters are not disturbed when only one
  peer is added or removed.
- **Keys in transit vs. on the wire.** The panel sends `private_key` and every
  peer's `public_key` as X25519, standard base64 (the wg convention). The node
  accepts both the standard and URL-safe alphabets, padded or not, and
  converts to hex itself before talking UAPI (`uapi::key_base64_to_hex`).
- **NAT / forwarding.** IPv4 forwarding, MASQUERADE for the AWG pool
  (`10.66.0.0/16`, derived from `address_cidr` — no hard-coded subnet in the
  node) and opening the UDP port are all applied idempotently on every check,
  the same pattern as the existing "Firewall synced" logic. Two backends,
  auto-selected (`iface.rs`): **iptables** where present; otherwise **nft**,
  recreating a dedicated `caramba_awg` table in one atomic transaction (does
  not touch other tables/chains). This split is load-bearing, not
  belt-and-suspenders: at least one production node (veles) has **no
  `iptables` binary at all**, only `nft` — a single-backend implementation
  would have silently left AWG without NAT on that node. Where `ufw` is
  active, the UDP listen port is additionally opened via `ufw allow` (same as
  `sync_firewall` already does for other inbounds).
- **The AmneziaWG process itself** is spawned without `-f` (it daemonizes) and
  is deliberately not a child of the agent process: the node self-updates and
  restarts its agent regularly, and user tunnels must not drop when it does.

### Panel side — source of truth for peers and parameters

The panel is the only place server keys, obfuscation parameters (`jc`, `jmin`,
`jmax`, `s1`, `s2`, `h1`-`h4`) and the peer list are generated or decided.
`apps/caramba-panel/src/services/awg_service.rs` is the single owner:

- **`node_awg`** (one row per node): server keypair + obfuscation parameters,
  generated **once** when a node's AWG config is first requested and never
  regenerated after (changing them would break the handshake for every peer
  already issued a client config). Listen port comes from a dedicated band,
  `17400`-`17499` (right after TUIC's `16400`-`16499`), allocated against the
  same `inbounds` port bookkeeping sing-box uses, so the two can never collide.
- **`subscription_awg_keys`** (one row per subscription): a *journal* of
  issued client keys, not the only place the key lives. The client private key
  is **derived deterministically** from the subscription UUID
  (`derive_client_private_key` = clamped-X25519(`sha256(uuid ||
  "amneziawg-key-salt")`)) — byte-for-byte the same function
  `subscription_service::generate_amneziawg_key` already used. This matters
  because the synchronous subscription/mihomo-config generator cannot hit the
  database mid-request; deriving the key from data it already has (the
  subscription UUID) keeps the node's peer list and the client's outbound
  config from ever disagreeing, without a DB round trip on the hot path.
  Likewise the peer address is deterministic:
  `awg_allowed_ip(subscription_id) = 10.66.{1 + n/250}.{2 + n%250}`,
  `n = subscription_id.rem_euclid(63000)` over the `10.66.0.0/16` pool
  (edge octets `.0`/`.1`/`.255` never handed out).
- **The `awg` section of the node config** (`GET /api/v2/node/config`, built
  by `awg_service::build_section`) travels **next to** the sing-box config,
  not inside it — see "The config contract" below.
- **A mirror row in `inbounds`** (`protocol = "amneziawg"`, tag
  `amneziawg-awg0`) exists purely so the existing synchronous subscription
  generators (which read `node.inbounds` and cannot hit the DB) can find the
  node's public key/port/obfuscation params without a new code path, and so
  the port stays reserved in the shared port allocator. It carries the public
  key and parameters but **never the server's private key** — the `awg`
  section on the config endpoint is the only place that travels. The mirror's
  `enable` flag is `global_toggle AND per_node_toggle`; the source of truth
  for everything else stays `node_awg`.

### The config contract (`libs/caramba-shared`)

Both sides import the same types from `caramba_shared::config` and
`caramba_shared::api` — there is exactly one definition of the wire shapes,
not one per side.

```jsonc
// GET /api/v2/node/config
{
  "hash": "<md5 of `content` ONLY>",
  "content": { /* sing-box config, unchanged */ },
  "awg": {                                   // optional: absent = panel doesn't manage AWG on this node
    "enabled": true,
    "listen_port": 17400,
    "private_key": "<X25519, base64>",       // node converts to hex itself
    "address_cidr": "10.66.0.1/16",
    "jc": 5, "jmin": 50, "jmax": 700, "s1": 30, "s2": 40,
    "h1": 1111, "h2": 2222, "h3": 3333, "h4": 4444,
    "peers": [                               // FULL set, not a delta
      { "public_key": "<X25519 base64>", "allowed_ip": "10.66.1.9/32", "user_tag": "user_42" }
    ]
  },
  "awg_hash": "<md5 of the awg section, independent of `hash`>"
}
```

`hash` is derived **only** from `content` and is still the sole trigger for
rewriting `/etc/sing-box/config.json` and restarting sing-box — exactly as
before AWG existed. `awg`/`awg_hash` are additive
(`serde(default, skip_serializing_if = Option::is_none)`): an old node never
sees them, an old panel never sends them, and neither side treats that as an
error. **Adding or removing one AWG peer changes `awg_hash` only** — it is
applied through UAPI on the node without touching sing-box or its restart
counter at all. This is the reason peer changes and sing-box's own
config-versioning ACK path are completely decoupled.

The node validates the whole `awg` section **before** applying it, and applies
it **fail-closed**: any violation and the section is skipped entirely (not
partially) until the panel sends a valid one. Checked, beyond basic
parseability of keys/CIDRs:

- `listen_port != 0`.
- `h1..h4` are each `> 4` (1-4 are WireGuard's own packet types — colliding
  with them breaks parsing on both sides) **and pairwise distinct**.
- `jmin <= jmax`.
- every peer has a non-empty `user_tag` and no `public_key` repeats.

> **Known sharp edge, not yet closed as of this writing:** the panel's
> `generate_params()` (`awg_service.rs`) draws `h1..h4` as independent
> `u32` values with no distinctness/`>4` check on the generating side — it
> relies on the ~4×10⁻⁹ collision probability being negligible in practice
> rather than on an explicit guarantee. If it ever does collide, the node's
> `validate()` above rejects the **entire** `awg` section for that node (the
> node keeps whatever it last had applied; nothing crashes), and the fix is a
> narrow one in `awg_service.rs::generate_params` — regenerate on collision —
> not anything in this crate. Flagged here so it isn't mistaken for dead code
> when someone eventually reads `validate()` and asks "can this actually
> fail?".

### Traffic accounting and online status (heartbeat)

`POST /api/v2/node/heartbeat` carries a new optional field,
`active_users: [{ tag, rx_delta, tx_delta, online }]`
(`caramba_shared::api::ActiveUser`), populated from **two independent
sources merged into one list, deduplicated by tag**:

- sing-box users: the existing v2ray-stats delta path (`online` = delta over
  the interval `> 0`).
- AWG peers: UAPI `get=1` (`online` = `last_handshake` fresher than
  `awg::ONLINE_WINDOW_SECS` = **180 seconds**, a value fixed by the contract —
  WireGuard re-handshakes roughly every 120s under active traffic, so 180s is
  the minimum window that doesn't flicker a live peer offline).

One person on AWG from a phone and sing-box from a laptop at the same time is
**one row**: deltas from both sources are summed, `online` is the OR of both.
A peer that is online but silent shows up with zero deltas; a peer that is
offline and silent does not show up at all. `user_usage` (the field that
already fed `app_traffic_daily`) keeps its old meaning — sum of both
directions, for quotas — and **now includes AWG traffic too**; the schema did
not change. `active_users` deltas are informational only (`node_user_activity`
via the panel's `record_activity`) — nothing double-counts traffic from it.
`None`/absent `active_users` means "old agent", not "nobody is online".

## Gates — one toggle now, not two envs

`CARAMBA_ENABLE_AMNEZIAWG` and `CARAMBA_ENABLE_AMNEZIAWG_CLIENT` are **gone**
— neither env var is read anywhere anymore. There is exactly one source of
truth, a boolean panel setting:

- **Global**: admin **Settings → AmneziaWG** toggle
  (`settings.amneziawg_enabled`, default `false`). Mirrored into an
  in-process atomic (`utils::amneziawg_enabled()`) because the subscription
  generators are synchronous and cannot hit the DB; the mirror is refreshed on
  every node heartbeat and immediately when Settings is saved.
- **Per node**: a toggle in the node's admin card
  (`node_awg.enabled`, default `false` when the row is first created), flipped
  via its own route, `POST /nodes/{id}/awg/toggle` — deliberately **not**
  folded into the existing `/nodes/{id}/update`, which calls `reset_inbounds`
  and would regenerate every inbound (and every live client's ports/keys) on
  a node just to flip one switch.
- **Effective state** = global toggle AND per-node toggle. Both are required:
  a node cannot serve AWG the operator hasn't opted it into, and no node
  serves AWG while the feature is off panel-wide.
- `amneziawg_client_enabled()` still exists as a call site for the client
  paths (CSM catalog, `api/v2/app.rs`, the clash/subscription generator) but
  is now a plain alias for `amneziawg_enabled()` — there is no longer a
  separate "client-only, doesn't touch the node" state, because there is no
  node-breaking sing-box inbound left to protect against.

### What each side emits, now that AWG is a real server

- **sing-box body of the subscription**: AmneziaWG is emitted **nowhere** in
  it anymore (previously gated, now removed outright). A stock sing-box
  client doesn't understand the obfuscation fields and would build a bare
  WireGuard outbound against an AmneziaWG server — a connection that is
  guaranteed dead on arrival, not a degraded one, so there is no reason to
  keep emitting it even behind a flag.
- **clash/mihomo body**: unchanged in shape — still a `type: wireguard` proxy
  with the amnezia fields (mihomo understands `amnezia-wg-option`), still
  gated by the one toggle above. This is what the exarobot client actually
  consumes.
- **`wireguard://` share links**: unchanged, same gate, for third-party
  clients (the official Amnezia apps on Android/iOS/macOS/Windows are
  confirmed to read this format; Hiddify compatibility specifically was not
  verified — check before advertising AWG subscriptions to non-caramba
  clients).
- **sing-box node config validation** (`config_validation_service.rs`): the
  `amneziawg` inbound schema is now a read-only **mirror description**, not a
  real inbound spec — `users` is always empty (peers travel in the `awg`
  section, not here) and no private key is ever required or accepted in it.
  The gate check itself still exists but means something different than
  before: it used to protect the node from a config that would fail
  `sing-box check`; now it just means "the operator hasn't turned AmneziaWG on
  in Settings yet".

## How the client pins it

1. UI / state calls `SetProtocol("AmneziaWG")` on the Go core (gomobile
   `Client.SetProtocol`, or the CLI flag `--protocol AmneziaWG`).
2. `applyProtocol` (`libs/caramba-core/profile/profile.go`) collects every
   `wireguard` proxy into a `Caramba-Proto` url-test group and puts it first
   in the `CARAMBA` selector, so the default pick is an AmneziaWG node. Other
   groups and nodes are untouched.
3. On `Up`, the mihomo engine applies the assembled config and the selector
   resolves to the AmneziaWG node. `Status.active_proxy` reflects the current
   pick from the `CARAMBA` group.

### If AWG isn't offered

If no `wireguard` proxy exists in the subscription (AWG off globally, or off
on every node the subscription currently offers), `applyProtocol` returns
before touching anything (`len(names) == 0` guard) and leaves the panel
`Auto-All` choice in place. Nothing breaks; you just do not get AmneziaWG.
This is what makes it safe that **autotune's own fallback path does not check
catalog availability** (see next section) — its worst case is a
recommendation that this guard silently ignores, not a broken connection.

## Auto tune

`AmneziaWG` is the highest priority protocol in autotune
(`libs/caramba-core/autotune/autotune.go`, `ProtocolPriority`). When a server's
probed `OKProtocols` include it, the recommendation prefers it — that path is
correctly scoped to what the current subscription actually offers, because
`OKProtocols` for a reachable candidate comes from `Candidate.Protocols`,
which `aggregateCandidates` (`libs/caramba-core/api/api.go`) builds from the
live subscription's server list.

The **two fallback branches** of `Recommend` — "nothing reachable, but a relay
is configured" and "nothing reachable at all, no relay" — are different: both
unconditionally return `ProtocolPriority[0]` ("AmneziaWG") as the recommended
protocol, with no way to know from a `ProbeResult` alone whether AmneziaWG was
ever declared for any candidate, reachable or not (unreachable probes carry no
`OKProtocols`). **This was checked deliberately for this doc update** and is
**not being changed**: fixing it properly needs "declared protocols
regardless of reachability" threaded from `libs/caramba-core/api/api.go`
through `prober.go`/`prober_mihomo.go` into `Recommend`, none of which are
files this pass owns, and — per "If AWG isn't offered" above — a wrong
recommendation here already degrades softly at `applyProtocol` instead of
breaking anything, so it is a low-value fix to rush. Left as a known limitation
for whoever next touches the autotune package.

The recommendation only sets the policy; the caller still raises the tunnel
with `Up(serverId)`.

## Checklist to make AmneziaWG actually move packets

- [ ] Admin **Settings → AmneziaWG** is turned on for the panel.
- [ ] The specific node's AmneziaWG toggle (node card) is also turned on —
      both are required.
- [ ] `amneziawg-go` actually downloaded and running on that node: node logs
      show it fetched the release asset and the hash check passed (empty
      `AWG_GO_SHA256` blocks this silently until the release pipeline
      publishes the asset hash — check for that first if nothing is
      happening).
- [ ] The node's `awg0` interface is up (`ip addr show awg0` over SSH,
      read-only) and NAT/forwarding rules are in place (`iptables -S` or
      `nft list table ip caramba_awg`, also read-only).
- [ ] The subscription mihomo config contains at least one `type: wireguard`
      proxy with the amnezia fields — confirms the client-visible gate is on.
- [ ] Go core built with `-tags mihomo` (the stub engine does not raise a real
      tunnel). See `BUILDING.md`.
- [ ] Client pins the protocol with `SetProtocol("AmneziaWG")` (or leaves auto
      and lets autotune pick it, once it has actually been probed as OK).
- [ ] A real TUN is attached: mobile passes the platform fd via `SetTunFd`,
      desktop lets mihomo own the TUN. See `apps/caramba-client/INTEGRATION.md`.
