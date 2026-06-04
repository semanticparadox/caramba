# sing-box Configuration Reference

**Targets sing-box 1.13.x** — the engine version pinned on nodes (see
`apps/caramba-installer/src/install.rs`, apt pin `1.13.*`). The Caramba config
generator emits the 1.13 schema (`store_rdrc`, `download_detour`,
`independent_cache`). These vendored docs already include the 1.13.0 reference
content (e.g. `rule-set/headless-rule.md`, `rule-set/source-format.md`,
`dns/server/local.md`, `service/ocm.md`). They do **not** yet reflect the 1.14
field migrations (`download_detour` → `http_clients`, `store_rdrc` →
`store_dns`, removal of `independent_cache`); that migration is tracked in beads
and must land together with the engine bump past 1.13.

Local copy of [sing-box docs](https://sing-box.sagernet.org/configuration/).
Source: https://github.com/SagerNet/sing-box/tree/testing/docs/configuration

## Quick Reference for Caramba

### Inbounds we use (server-side)
- [VLESS Inbound](inbound/vless.md) — TCP+Reality, HTTPUpgrade, WS, gRPC
- [Hysteria2 Inbound](inbound/hysteria2.md) — UDP transport
- [Shadowsocks Inbound](inbound/shadowsocks.md) — relay transport
- [Mixed Inbound](inbound/mixed.md) — local SOCKS/HTTP proxy

### Outbounds we generate (client subscription configs)
- [VLESS Outbound](outbound/vless.md) — main proxy protocol
- [Hysteria2 Outbound](outbound/hysteria2.md) — UDP-based proxy
- [Selector](outbound/selector.md) — manual proxy selection
- [URLTest](outbound/urltest.md) — auto best-latency selection
- [Direct](outbound/direct.md) — bypass proxy
- [Block](outbound/block.md) — block traffic
- [DNS](outbound/dns.md) — DNS outbound

### Shared options (critical for config generation)
- [TLS / Reality](shared/tls.md) — TLS and Reality configuration
- [V2Ray Transport](shared/v2ray-transport.md) — WebSocket, gRPC, HTTPUpgrade, H2
- [Multiplex](shared/multiplex.md) — smux/yamux multiplexing
- [Dial](shared/dial.md) — dial options, detour (relay chaining)
- [Listen](shared/listen.md) — listen options for inbounds

---

## Full Index

### Core
- [Configuration Overview](index.md)
- [Log](log/index.md)
- [DNS](dns/index.md)
  - [DNS Rules](dns/rule.md)
  - [DNS Rule Actions](dns/rule_action.md)
  - [FakeIP](dns/fakeip.md)
  - DNS Servers: [index](dns/server/index.md) | [https](dns/server/https.md) | [tls](dns/server/tls.md) | [udp](dns/server/udp.md) | [tcp](dns/server/tcp.md) | [local](dns/server/local.md) | [dhcp](dns/server/dhcp.md) | [fakeip](dns/server/fakeip.md) | [hosts](dns/server/hosts.md)
- [NTP](ntp/index.md)
- [Certificate](certificate/index.md)

### Inbound
- [Overview](inbound/index.md)
- [VLESS](inbound/vless.md) | [VMess](inbound/vmess.md) | [Trojan](inbound/trojan.md)
- [Hysteria2](inbound/hysteria2.md) | [Hysteria](inbound/hysteria.md) | [TUIC](inbound/tuic.md)
- [Shadowsocks](inbound/shadowsocks.md) | [ShadowTLS](inbound/shadowtls.md)
- [Mixed](inbound/mixed.md) | [SOCKS](inbound/socks.md) | [HTTP](inbound/http.md) | [Direct](inbound/direct.md)
- [TUN](inbound/tun.md) | [Redirect](inbound/redirect.md) | [TProxy](inbound/tproxy.md)
- [NaiveProxy](inbound/naive.md) | [AnyTLS](inbound/anytls.md)

### Outbound
- [Overview](outbound/index.md)
- [VLESS](outbound/vless.md) | [VMess](outbound/vmess.md) | [Trojan](outbound/trojan.md)
- [Hysteria2](outbound/hysteria2.md) | [Hysteria](outbound/hysteria.md) | [TUIC](outbound/tuic.md)
- [Shadowsocks](outbound/shadowsocks.md) | [ShadowTLS](outbound/shadowtls.md)
- [WireGuard](outbound/wireguard.md) | [SSH](outbound/ssh.md) | [Tor](outbound/tor.md)
- [Selector](outbound/selector.md) | [URLTest](outbound/urltest.md)
- [Direct](outbound/direct.md) | [Block](outbound/block.md) | [DNS](outbound/dns.md)
- [HTTP](outbound/http.md) | [SOCKS](outbound/socks.md) | [NaiveProxy](outbound/naive.md)

### Route
- [Route](route/index.md)
- [Route Rules](route/rule.md)
- [Route Rule Actions](route/rule_action.md)
- [Sniff](route/sniff.md)
- [GeoIP](route/geoip.md) | [GeoSite](route/geosite.md)

### Rule Set
- [Overview](rule-set/index.md)
- [Source Format](rule-set/source-format.md)
- [Headless Rule](rule-set/headless-rule.md)
- [AdGuard](rule-set/adguard.md)

### Shared Options
- [TLS](shared/tls.md) — includes Reality config
- [V2Ray Transport](shared/v2ray-transport.md) — WS, gRPC, HTTPUpgrade, H2
- [Multiplex](shared/multiplex.md) — smux, yamux, h2mux
- [Dial](shared/dial.md) — detour, bind, routing mark
- [Listen](shared/listen.md) — listen address, port
- [TCP Brutal](shared/tcp-brutal.md)
- [UDP over TCP](shared/udp-over-tcp.md)
- [DNS-01 Challenge](shared/dns01_challenge.md)
- [Pre-match](shared/pre-match.md)
- [Wi-Fi State](shared/wifi-state.md)

### Experimental
- [Overview](experimental/index.md)
- [Cache File](experimental/cache-file.md)
- [Clash API](experimental/clash-api.md)
- [V2Ray API](experimental/v2ray-api.md)

### Endpoint
- [Overview](endpoint/index.md)
- [WireGuard](endpoint/wireguard.md)
- [Tailscale](endpoint/tailscale.md)

### Service
- [Overview](service/index.md)
- [DERP](service/derp.md)
- [CCM](service/ccm.md)
- [OCM](service/ocm.md)
- [Resolved](service/resolved.md)
- [SSM API](service/ssm-api.md)
