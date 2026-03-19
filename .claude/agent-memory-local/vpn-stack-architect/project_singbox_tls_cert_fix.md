---
name: sing-box TLS cert fix (2026-03-17)
description: Fix for FATAL crash on startup due to missing TLS certs for non-Reality inbounds
type: project
---

Fixed sing-box startup FATAL: `open /etc/sing-box/certs/cert.pem: no such file or directory`.

**Root causes:**
1. `VlessTlsConfig.reality` was always serialized (even when `enabled: false`), causing sing-box to reject configs with `private_key: ""`
2. Hysteria2/VLESS+TLS inbounds referenced `/etc/sing-box/certs/cert.pem` but the file was never created on the node
3. TLS server_name defaulted to `reality_sni` (e.g. `timecard365.de`) instead of the node's actual domain/IP

**Fix locations:**
- `apps/caramba-panel/src/singbox/config.rs`: Changed `VlessTlsConfig.reality` from `RealityConfig` to `Option<RealityConfig>` with `skip_serializing_if = "Option::is_none"`
- `apps/caramba-panel/src/singbox/generator.rs`:
  - Reality inbounds: `reality: Some(RealityConfig { enabled: true, ... })`
  - TLS inbounds: `reality: None` (field omitted from JSON entirely)
  - TLS server_name: uses `node.domain` (fallback: `node.ip`) instead of `node.reality_sni`
  - Validates Reality private key BEFORE constructing config (early bail)
- `apps/caramba-node/src/main.rs`: After `save_config`, auto-generates self-signed cert via `rcgen` if `/etc/sing-box/certs/cert.pem` or `key.pem` don't exist. CN/SAN taken from config's TLS server_name. Valid 10 years. Permissions 0o600 on key.
- `apps/caramba-node/Cargo.toml`: Added `rcgen = "0.13"` dependency

**Architecture note:** Config is generated on the PANEL and pushed to nodes via pubsub. The NODE agent (caramba-node) writes the JSON to disk and generates certs locally. This is correct — certs live on the node, not the panel.

**Why:** Sing-box refuses to start when `private_key: ""` is present in a reality block, or when cert files are referenced but missing on disk.
