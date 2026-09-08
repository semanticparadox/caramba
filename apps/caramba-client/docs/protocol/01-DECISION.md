# Caramba Connect Protocol: Decision Record

Status: decided, 2026-09-02. Supersedes nothing. Consumes `00-DESIGN-BRIEF.md` (all `[section]` and `Rn` references below point into it).

Audience: two implementation teams. This session owns the Flutter client, the Go core and the specification. A second session owns the Rust panel. Section 7 is written as the request to the panel team.

Inputs: four candidate designs (A minimal-delta, B control-plane, C sealed-bundle, D diversity-first), twelve adversarial stress reports (three lenses per design: cryptography and security, censorship resistance in Russia 2026, implementability and store compliance), and three independent judge verdicts.

---

## 1. The decision

**Design A wins. The protocol is CSM/1, the Caramba Signed Manifest, with the grafts in section 3 folded in before P1 ships.** A wins on the fatal-weighting rule that all three judges applied independently: a fatal finding a design can answer with a deleted field or a written paragraph is a bug, and a fatal finding that voids the design's own central justification is a different design. A is the only candidate whose three adversarial lenses all returned `fixable`, and each of its three crypto fatals has a stated, bounded repair that does not touch the design's shape (write the doc_type to role to threshold authorization table; delete `cfg` from the directive, because its own worked example carries the raw subscription uuid, which per `services/subscription_service.rs:191-205` IS the VLESS uuid, the Trojan password, the TUIC uuid and part of the Hysteria2 password; carry a sha256 per rule-set and geo file in the signed catalog, because `routing/routing.go:186-192` emits providers with no hash and no pinning and those payloads decide what leaves the tunnel). B, C and D each carry fatals of the second kind: B's catalog cannot represent the panel's plan-scoped multi-inbound node model without breaking one of its own three load-bearing claims, and its device-key-only grant issuance has no bridge to the account JWT that all 32 preserved `/api/v2/app/*` endpoints still use, so an ordinary reinstall becomes a manual operator action per user in a market where the operator's support channel is blocked; C puts `flags` and `plainlen` outside the signing pre-image, so one bit flip reclassifies a live grant as an out-of-band import and switches nonce binding off, which is a break of the exact primitive the design is named after, and its device binding is cosmetic because the grant signature covers plaintext with no recipient binding while its own type-6 transfer feature demonstrates the re-seal; D's per-device per-epoch wildcard rendezvous manufactures a textbook DGA signature that is cheaper to block than the fixed hostname it replaces, defends fields TSPU cannot see because they sit inside the TLS record, and publishes the tenant seed on `GET /api/v2/app/branding`, which is verified public and unauthenticated in `api/v2/mod.rs`. The commercial half of the decision is decisive for a team with twenty paying users: A is the only design whose first three panel releases change nothing any existing client sees, the only one that scoped `caramba-sub` work (verified: `apps/caramba-sub/src/main.rs` registers `/health`, `/sub/{uuid}`, `/app`, `/app/`, `/app/{*path}` and `/api/{*path}` with no fallback and no wildcard, so B's `/connect/v1`, C's `/c/*` and D's derived paths are all unreachable through the live edge tier), and it scores highest on the two axes that decide whether anything reaches a user this quarter, operator experience and honesty. What A must import is real and non-trivial, and it is listed in section 3: capability negotiation, per-node connection material, a device keypair, uTLS on the control plane, per-tenant transport diversity, and a handshake-inclusive byte budget. Those are additive grafts onto a shippable core, which is the correct direction of travel. The reverse, grafting shippability onto B or D, is not available.

---

## 2. Scores

Nine criteria, three judges, scored 0 to 10. Cells read `J1 / J2 / J3`.

| Criterion | A (Sidecar, CSM/1) | B (Caravan/1) | C (CCP/1) | D (MOSAIC) |
|---|---|---|---|---|
| 1. Trust model and security | 6 / 6 / 6 | 6 / 7 / 5 | 4 / 5 / 4 | 4 / 5 / 4 |
| 2. Censorship resistance, RU 2026 | 5 / 6 / 6 | 5 / 5 / 5 | 6 / 6 / 5 | 3 / 3 / 3 |
| 3. Store and legal survivability | 7 / 8 / 7 | 5 / 6 / 5 | 4 / 5 / 5 | 5 / 5 / 4 |
| 4. Operator experience | 8 / 8 / 8 | 4 / 5 / 4 | 5 / 5 / 5 | 3 / 3 / 2 |
| 5. User experience | 7 / 6 / 6 | 5 / 7 / 5 | 7 / 6 / 6 | 4 / 4 / 4 |
| 6. Privacy | 6 / 6 / 6 | 6 / 7 / 7 | 8 / 7 / 6 | 6 / 7 / 6 |
| 7. Implementability | 7 / 8 / 7 | 3 / 4 / 3 | 5 / 5 / 5 | 4 / 3 / 2 |
| 8. Evolvability and versioning | 6 / 7 / 7 | 8 / 8 / 8 | 6 / 6 / 5 | 6 / 5 / 5 |
| 9. Honesty about weaknesses | 9 / 9 / 9 | 7 / 8 / 7 | 8 / 6 / 7 | 6 / 5 / 6 |
| **Judge totals** | **61 / 64 / 62** | **49 / 57 / 49** | **53 / 51 / 48** | **41 / 40 / 36** |
| **Aggregate (max 270)** | **187** | **155** | **152** | **117** |

Lens outcomes, which carried more weight than the totals:

| Design | Crypto and security | Censorship RU 2026 | Implementability and store |
|---|---|---|---|
| A | fixable | fixable | fixable |
| B | fixable | fixable | **fatal** |
| C | fixable | fixable | fixable |
| D | **fatal** | **fatal** | fixable |

A is the only clean sweep. C's three `fixable` verdicts are why it outscores B on one judge's card and outranks it on another's; it still loses because two of its defects sit in the frame rather than in a field, and because a single fixed-offset signature slot makes the dual-signed root rotation it mandates inexpressible.

Where the judges disagreed, and why it does not change the outcome: judge 1 ranked C second on the strength of its floor concept and its privacy construction; judges 2 and 3 ranked B second on the strength of its trust model and evolvability. Every judge ranked A first and D last, and no judge ranked A first by fewer than four points.

---

## 3. Grafts

Every item here is now part of CSM/1 and is normative. Each carries its source and the reason it beat A's own answer.

### 3.1 From B (Caravan/1, control-plane)

**B1. Capability negotiation.** A signed per-operator capability bitfield in the catalog, intersected with client capabilities and echoed in the per-device directive. Named by all three judges. A's own compatibility section concedes it cannot express "this operator has no relays", and a subscription URL structurally cannot say it either: it can only return a config without relays, which the client cannot distinguish from a config where relays were filtered out. Without this, every licensed operator's client renders controls that lie to some users. This is B's single best structural idea and A has no substitute. It is also what makes the relay picker honestly dark rather than a placebo while R13 is open.

**B2. Per-field setting provenance.** Every setting carries `user`, `operator` or `default`. When the operator changes a field the user set explicitly, the client raises a Keep or Revert card rather than applying silently. Widened per B's own crypto lens: the card fires on ANY narrowing of the user's security posture (kill switch off, split mode off, DNS repointed, enabled transport set shortened) regardless of provenance. This is the cheapest real difference between a control plane and a remote control.

**B3. UI hints as `{enum kind, inert text}`.** Operator-supplied text can never carry a URL the app will open, in any storefront. URL-shaped substrings are stripped at render. Operator text never renders on the same surface as the verification chrome. This replaces A's storefront-conditional `msg.url` drop, which depends on A's own unresolved open question about determining the storefront on Android. One mechanism simultaneously bounds a hostile operator and answers Apple 3.1.3, and it is the strongest store-compliance idea anywhere in the material.

**B4. Device identity as a keypair thumbprint.** P-256 in Secure Enclave and StrongBox, replacing IP plus User-Agent counting. Two corrections the lenses forced: the hardware claim does not hold below Android 12 (`PURPOSE_AGREE_KEY` is API 31), so the software tier is explicit, recorded in the enrollment and visible to both the user and the operator; and A's rule that `/sub/m1` must not call `track_access` fixes only the metadata half, because the device limit is enforced on the config fetch at `subscription.rs:203-262` via `get_active_ips`, so every ladder rung with a different apparent egress still burns a slot and under fetch-through-tunnel the apparent IP is the exit node's, shared by every user of that node. R9 is not retired without this.

**B5. Write proofs bound to the body.** Every write carries a proof over `sha256(request body)` plus a fresh nonce inside the body, and the signed echo covers every field the write can set, not a subset. A's `PUT /api/v2/app/preferences` currently has only `If-Match`, which under A's own state-CA premise is rewritable in flight. B's own failure is instructive and must not be repeated: its `Caravan-Proof` covered method, path, nonce and session but not the body, and `PATCH /settings` had no nonce at all, so a state-CA MITM rewrites `killSwitch`, `split.mode`, `dns` and `transport_order`, and three of those four were absent from the signed echo.

**B6. Settings as one round trip with three-way semantics.** `want` in, authoritative signed `sel` out. Absent means unchanged, explicit null means reset to operator default, a value means set. `If-Match` with 409 and the current state on stale. Vocabulary is the `CorePolicy` string set (`packages/caramba_vpn/lib/src/core_policy.dart:41-176`), never `CoreConfig` indices. A's queued PUT is close; B's shape is better and costs nothing extra. Note the ABI consequence flagged by B's implementability lens: `CorePolicy.toJson` omits nulls and Go's `policyPatch` uses pointers, so explicit null is not representable today. Use a sentinel value (`"default"`) rather than changing the two-way contract, or ship the third state as a named deliverable with its own vectors. Do not change it silently.

**B7. Signed, dated deprecations.** `{surface, sunset}` in the catalog with a minimum 180 day notice, surfaced in Settings. Nothing is ever withdrawn without an announcement that predates it and survives caching and mirrors. A has no deprecation channel at all and is about to license this protocol to third parties who upgrade on their own schedule.

**B8. The "What this app sends" diagnostics screen.** Renders the decoded fields of the last request with a copy button. It is simultaneously the Apple 2.3.1 evidence that nothing is hidden, the Apple 5.1.1(i) disclosure artifact, and the only realistic support tool. It is only cheap because the request is small, which is itself an argument for keeping it small.

### 3.2 From C (CCP/1, sealed-bundle)

**C1. One armored capsule identity.** The same bytes are a QR, a paste, a file and an HTTP body, with a defined multi-frame QR chunking format (`CARCAP1.<i>/<n>.<chunk>`). A's boot blob is a separate artifact with a separate encoding. Unifying the framing lets the out-of-band rung carry anything, up to a full offline snapshot, and it is the only thing that survives a mobile shutdown or whitelist mode.

**C2. Offline origination for enrollment.** A's rung 6 has the same defect C's does, from the opposite direction: A's boot blob carries mirrors but no enrollment credential, and the Dart `ApiClient` baseUrl is fixed at `'{panel}/api/v2/app'`, so a blocked new user cannot enroll from a QR at all. C's enrollment cannot originate offline either, because redemption requires POSTing a device key to the live panel. The resolution: put the code, the mirror set, the DoH list and the pinned root key in the blob, and make the enrollment path itself ladder-aware. The escape kit must not live behind the host being escaped.

**C3. Integrity for rule-sets and geo databases.** A sha256 per rule-set and per geo file inside the signed catalog, with those fetches routed through the same ladder. This is the only integrity anyone provides for the data that actually decides which packets enter the tunnel. Verified: `routing/routing.go:186-192` emits providers as `{type, behavior, format, url, interval}` with no hash, no signature and no pinning, and `component/geodata/init.go:71-88` downloads direct from GitHub. A widens `rsbase` into a pool without hashing anything, which turns a compromised online key from "can re-target among existing nodes" into "can move named domains from proxy to DIRECT so traffic leaves the device in cleartext with the tunnel showing connected". That breaches the constraint that a malicious operator must not harm the user beyond the VPN service they signed up for.

**C4. Every framing byte inside the signing pre-image.** Normatively: a verifier MUST reject trailing bytes and MUST require total length to equal exactly `7 + payload_len + 1 + 76 * nsigs`. C's fatal is A's latent bug: A signs the magic, the type byte, the length field and the payload, but never says the frame is exact-length or that `nsigs` cannot be inflated.

**C5. Expiry is the revocation, stated as an operator dial.** The offline grace window is an explicit operator setting with its consequence printed beside it: a longer window buys blackout tolerance and buys the same amount of un-revocable service. This sits alongside A's correct invariant that an expired document never disconnects a user and only stops it accepting new instructions.

**C6. Closed-vocabulary persistence.** Generalize A's PII whitelist test into C's rule: the client persists and echoes only values it can validate against a closed vocabulary (stable node ids matching a fixed charset and length, `CorePolicy` enum values), never an operator-supplied opaque string. Announce text is render-only, never persisted, never echoed. This closes the channel C's own reserved-key-range trick left open, where the abuse simply moves into a permitted free-text field.

**C7. The four-phase migration.** Shadow (fetch and verify, discard the result, build config the old way, verification failures logged and never fatal), then verify-and-compare (build both ways, diff, report), then cut over, then steady state. This is the best process idea anywhere in the material and it is where three-language canonicalization drift and generator divergence get caught on real traffic before they can break a paying user. It is strictly better than A's three-release plan for the specific risk that client-side generation introduces.

**C8. Three issuers for enrollment codes.** An admin endpoint, a bot command, and a shell subcommand. A has the first two. The shell path is the one that works when Telegram is blocked, which is the market. It must be a `caramba-panel` subcommand, not a new binary, because no `caramba-cli` crate exists (verified: the only `[[bin]]` declarations in the workspace are `caramba-installer` and `caramba-sub`).

### 3.3 From D (MOSAIC, diversity-first)

**D1. The "source list" reframing, plus the accurate review note.** A must delete Apple 2.3.1 from its anti-Tor rationale, because R6, R7 and R8 stand alone and are sufficient, and must stop claiming nothing is failure-triggered while R2 and R3 visibly run on failure. The correct note describes an ordered, individually toggleable, geography-independent list of places the app looks for its configuration, which progresses automatically on failure and is fully visible on one screen. An accurate note describing that is a stronger posture than a note that misdescribes the app.

**D2. uTLS on the control plane.** Selected from mihomo's already-vendored roster (`component/tls/utls.go`), randomized per connection rather than per epoch, with explicit modern hello IDs rather than the vendored `Auto` aliases which resolve to a Chrome build that no longer ships. Constructed in BOTH `api.NewCore` and `api.SetPanelURL`. Verified: neither Go HTTP surface sets a `Transport` today (`auth/client.go:75-86`, `subscription/subscription.go:99-109`), so Go's own ClientHello is a free, permanent, cross-tenant fingerprint that A does not currently remove. Forgetting `SetPanelURL` silently reverts a re-enrolled tenant, which is the exact bug that ships and is found six months later.

**D3. Per-tenant defaults for the ladder, plus randomized padding.** The signed catalog carries the tenant's default rung order and default enabled set, and padding buckets are drawn per request from a per-tenant range rather than always landing on the floor. A pads every directive to 512 bytes and the directive is the dominant flow, so 512 in and 512 out becomes a cross-tenant constant in a design that is otherwise careful about traffic shape. This is the cheap, store-safe half of D's thesis: diversity lives in signed data, never in app logic, and never in a derived rendezvous.

**D4. TLS SPKI pinning for manifest hosts.** Pins carried in the signed catalog, enforced with mihomo's existing `component/ca` pinning and a private root pool. A demotes TLS to confidentiality only and then relies on it for confidentiality with no pinning, against an adversary the brief documents as holding a trusted CA in 28.6 percent of RuStore apps by count and 81.4 percent by weighted download share.

**D5. Count the TLS handshake inside the per-connection byte budget.** With the provisioning rule that follows: ECDSA leaf, shortest chain, no stapled OCSP on manifest hosts. That is 3 to 4 KB of the freeze budget recovered for free, and A's cold-start numbers currently assume roughly 1.2 KB of TLS with session resumption on a connection that by definition has no session ticket. Budget packets as well as bytes, because the brief's trigger is roughly 25 packets OR 15 to 20 KB and A tracks only bytes. Re-derive the 8192 byte connection-reuse threshold against the honest number and make it a signed catalog field rather than a compiled constant.

**D6. Mirror ASN diversity enforced by resolution, not by label.** The panel resolves each mirror hostname server-side at save time, looks up the real ASN, and rejects a pool with fewer than three distinct ASNs and two distinct countries. A's admin UI checks an operator-typed label, which its own censorship lens defeated in one sentence: six Hetzner boxes typed `eu-1` through `eu-6` pass.

**D7. Per-cohort subsets of a larger pool.** Mirrors and, where the operator's fleet allows, nodes are drawn per cohort from a pool larger than any one cohort sees. This is D's diversity instinct without D's mechanism: one enrolled adversary burns a slice and identifies himself, instead of taking the whole fleet with one purchase. It also gives the operator a canary channel for a new mirror before it is exposed to everyone.

### 3.4 From B and C jointly

**BC1. Per-node connection material in the signed catalog, and the client builds its own mihomo config.** Host, port, protocol, SNI, Reality public key, short id, flow, alpn, fingerprint, port-hopping range, obfs. Judge 2 calls this the single most important graft, and it is the one that resolves A's own internal contradiction: A's size budget says the client always requests `?node_id=sel.exit` and receives a single-node config, while A's settings section says tapping Amsterdam is a local `CARAMBA` selector move with no network. Both cannot hold, because a single-node config has one proxy in the group. Beyond the contradiction, this is the only version in which changing servers still works when the operator origin is unreachable, which is the case the product exists for: a user in Moscow whose exit is blocked and whose operator origin is blocked at the same time. Size it against the panel's real `StreamInfo` (`singbox/subscription_generator.rs:91-114`), not against a 100-byte sketch: the honest entry is 220 to 280 bytes, so a 40-node catalog is roughly 7 to 11 KB and chunking is required in v1, not reserved for later. The cost, stated plainly: relay chaining and outbound rendering now exist in Go as well as in the Rust generator that serves Hiddify and v2rayNG. That is a permanent two-renderer tax and it belongs in the cost list, mitigated by C7's verify-and-compare phase and by restructuring the panel so the catalog is one node model with several renderers.

**BC2. HPKE-seal the per-device directive, and remove `cfg` from it entirely.** A's directive carries the subscription uuid, which is simultaneously the VLESS uuid, the Trojan password, the TUIC uuid and part of the Hysteria2 password, and A deliberately exposes that document to mirrors and to `caramba-sub`. Until the credential is out of the directive, the locator derivation buys nothing and the mirror pool can only ever be hosts the operator fully controls, which is the opposite of the diversity the design needs and forecloses the only mirror strategy the RU evidence supports. Sealing is the second half: it lets the directive ride a CDN, an onion front or a third-party mirror while that host holds only a ciphertext and a thumbprint.

### 3.5 Cross-cutting grafts (raised by the lenses, owned by no design)

**X1. Strict decode profile and strict signature verification, enforced by a three-language CI gate.** CBOR: definite lengths only, unsigned integer keys, ascending key order, shortest-form heads, no duplicate keys, no tags, no floats, no bignums, no simple values other than true and false, trailing bytes rejected. Ed25519: reject small-order and non-canonical public keys at ingest, reject non-canonical S, cofactorless RFC 8032 verification in all three implementations. A's framing rule retires encode-side drift and leaves decode drift and signature-boundary divergence fully open, and with two independent verifiers every divergence becomes a split-brain between what the UI shows and what the tunnel dials rather than a crash. A negative-fixture corpus (wrong-role signing pairs, duplicate map keys, non-minimal integers, indefinite lengths, trailing bytes, inflated `nsigs`, small-order keys, non-canonical S) runs as a merge gate in `cargo test`, `go test` and `flutter test`. Vectors must be computed independently, not generated from the Rust signer alone, or all three implementations agree on the same wrong value and CI stays green.

**X2. Move `GET /api/v2/app/branding` under the signed surface.** Verified in source: it is registered in the public group in `api/v2/mod.rs` before `route_layer(require_app_jwt)`, it is unauthenticated and unsigned, and `powered_by.dart:126` opens operator-supplied `support_url` and `bot_url` with `LaunchMode.externalApplication`. That is a pin bypass on the one field an attacker most wants and an Apple 3.1.3 exposure that lands on Webq Pro, not on the operator. Every design inherited it by promising to preserve all 32 endpoints. It moves first.

**X3. One fetcher, one verifier, one monotonic store, in the app process.** Verified: on iOS `PacketTunnelProvider.swift:147,166` builds the Go core and calls `configure(panelUrl, subscriptionID:, accessToken:)` inside the Network Extension, and `CarambaVpnPlugin.swift:313-318` builds a second client with a separate work dir (`caramba-tools/` versus `caramba/`). Two cores with separate work directories means two monotonic high-water-mark stores, which is a rollback hole and not defence in depth. Name one fetcher and one verifier, keep them in the app process (the 50 MiB NE ceiling requires it), and hand the extension a rendered config plus a validity window. The precedent exists in the same file: `rawMode` already has the app fetch and hand bytes to the NE.

**X4. Say what fetch-through-tunnel actually does on each platform.** Verified: `CarambaVpnService.kt:253-256` excludes the app from its own tunnel on Android by design, with the comment stating why. So the fetch-through-tunnel rung, which every design lists and two call the highest-success rung, does not exist there without a mihomo local listener plus a proxy-capable client. Either build it (a local listener plus a SOCKS-capable Dio adapter and Go dialer) or state it, but do not ship a Diagnostics screen that reports success on iOS and failure on Android for the same tenant on the same network.

---

## 4. Rejected outright

Consciously dropped. Do not re-propose without new evidence of the kind named in the trigger column of section 8.

**4.1 Embedded Tor as a v1 rung, in any form (Arti, C-tor, public Snowflake, obfs4, WebTunnel).** Rejected on R6, R7 and R8, which stand alone: gomobile allows one framework and the merge with mihomo is unverified for Go version alignment, cgo flags and duplicate quic-go and pion dependencies; the iOS Network Extension is capped at 50 MiB and headroom with mihomo alone is unmeasured; Arti on mobile is pinned at 1.7.0 against upstream 2.6.0, is described by Tor's own guide as not actively maintained, supports only managed PTs which are dead on mobile, and may call `exit(1)` on an obsolete consensus. And the market evidence says it would not work anyway: directory authorities and default bridges blocked since 2021-12-01, obfs4 largely blocked, most WebTunnel bridges enumerated since June 2025, Snowflake blocked by a shared DTLS fingerprint since 2026-03-30, BridgeDB shut down. Explicitly NOT rejected on Apple 2.3.1 grounds: a visible, documented, individually toggleable Tor rung would be equally compliant, and leaving that wrong framing in place would mis-price the next transport decision. The onion address stays in the catalog schema at zero binary cost, usable the day a user installs Orbot and points the custom-proxy rung at it.

**4.2 Derived per-device per-epoch rendezvous (D's core mechanism).** Rejected because it is a net censorship regression, not a tuning problem. Maximum-entropy 10-character labels under a shared wildcard apex with near-zero reuse is the canonical DGA heuristic, visible in plaintext to the ISP resolver on RU mobile and again as SNI, blockable by one apex-scoped rule that hits every tenant at once, while the registrable apex, which is the actual blockable unit, stays stable and shared. It manufactures R16 rather than answering it. Related and rejected with it: path template, HTTP method and Content-Type derivation, all of which sit inside the TLS record and are invisible to the adversary they were designed against; the wildcard cover site, which answers at every random subdomain and so defeats the active-probing story without a valid request being sent; bare-IP mirrors, which require disabling certificate validation because Let's Encrypt does not issue IP-SAN certificates; and port rotation into Cloudflare alternate ports on self-hosted panels, which has close to zero legitimate Russian baseline.

**4.3 A dormant feature whose parameters arrive later in signed data (C's slot 3.5).** A visible, empty, disabled "domestic relay carrier" toggle that becomes functional when a signed catalog populates it is a dormant feature activated by remote data: Apple 2.3.1 plus 2.5.2 plus the Play VpnService policy, at the account-termination tier. The distinction that survives is between data that reconfigures an existing, reviewable, exercisable code path (mirror hostnames, DoH URLs, node parameters) and data that activates a code path that was inert at review. The `transport.Carrier` interface may exist in the Go core; a shipped toggle for a carrier the reviewer cannot exercise may not.

**4.4 The transfer capsule (C type 6) and any portable entitlement token.** A signed, sealed, re-encryptable bearer object a user moves between devices by scanning a screen, with no network involved, is the most license-key-shaped construct in the whole material and raises R3 rather than lowering it. It is also cryptographically hollow as specified: the grant signature covers plaintext with no recipient binding, so one subscriber mints unlimited grants offline and the `transferable` boolean is an entitlement enforced by the party it constrains.

**4.5 DNS TXT as the whitelist-mode bootstrap (C source 4).** Refuted twice over: the record name `_cap.<operator-host>` is inside the zone that was just blocked, and the recommended resolver is a domestic, order-compellable one, so routing bootstrap through it does not evade the block, it asks the censor for the answer and hands him the query stream. The answer it returns points at non-whitelisted hosts anyway. DNS TXT stays available as one transport among several outside whitelist mode; it is not the whitelist answer and must not be described as one.

**4.6 An unauthenticated public document carrying the operator's full node fleet (B's `GET /catalog`, C's `GET /c/k`).** One anonymous curl per operator hands RKN every exit hostname, port, Reality public key and short id, refreshed daily, from a URL both designs encourage putting on public CDNs. Today enumeration at least costs a purchase. Node IPs are what actually get blocked. The catalog in CSM/1 is content-addressed and cacheable, and it is authorized: see section 5.2.

**4.7 Grant issuance bound only to a device key with no bridge to the account JWT (B).** All 32 authenticated `/api/v2/app/*` endpoints keep account JWTs. A design where a directive is issued only to a device key registered at code redemption makes an ordinary reinstall a manual operator action per user, in a market where the operator's support channel is blocked. Enrollment must be an act an authenticated account can perform on itself.

**4.8 Long-poll push with a fixed-interval keepalive (B's `/watch`).** A one-byte TLS record every 60 seconds indefinitely is a near-ideal flow-classifier feature and strictly more regular than the two-minute poll it replaces. It also cannot be held on iOS, where the app is suspended within seconds of backgrounding, and it creates a per-device presence oracle at 240 second granularity for the operator and any CDN in front of it. Refresh is jittered polling with conditional GETs.

**4.9 ECH, and classic domain fronting.** ECH is dropped by TSPU when a ClientHello carries both ECH and SNI (since 2024-11-05), RKN formally recommended Russian site owners disable it, and enabling it makes traffic stand out. Classic fronting is closed on Cloudflare, Google, AWS, Azure and Fastly. Both are named as dead in the spec so nobody re-proposes them as an oversight.

**4.10 Oblivious HTTP and a transparency log, in v1.** OHTTP does not support stateful auth and targets infrequent requests, and it needs a non-colluding relay that either a small operator cannot credibly provide or Webq Pro must run, which reintroduces the central chokepoint. A transparency log detects an operator equivocating between users and publishes an observable operator-activity record that is itself a censorship target. Both are reserved, neither ships. Consequence stated honestly: per-user equivocation on the catalog is undetectable in v1.

**4.11 A mandatory Webq Pro countersignature or Webq-Pro-held operator root keys.** Requiring it makes Webq Pro a single compromise and censorship target for every tenant at once. The countersignature exists as an optional, non-load-bearing badge and an opt-in recovery lane; when it is absent or fails, nothing breaks. Root keys are operator-held, generated by a panel subcommand that prints once and refuses to write into the panel working directory.

**4.12 Telegram as a dependable bootstrap or payment channel in Russia.** The bot is a convenience issuer and a convenience payment path, never a dependency. Enrollment codes are dictatable over a phone call by construction, and payment state is signed data rendered as a neutral status line.

**4.13 Removing the node auto-pin (`subscription.rs:617-634`) before Connect clients are the majority.** The comment there is explicit that it prevents dumping 40 or more outbounds on first fetch. Removing it early pushes every Hiddify, v2rayNG and stock-Clash user in Russia from a payload that fits into an 18 to 25 KB YAML squarely inside the freeze window, for months, with no benefit to them. B, C and D all did this early. See section 7 for the sequencing that replaces it.

**4.14 Decoy requests.** D's own weakest item, priced honestly by D itself: it costs bandwidth and code and buys little against a resource-limited adversary. Padding buckets and jitter carry the traffic-shape work.

**4.15 Renaming `exarobot.aar` and `exarobot.xcframework`, and any other cosmetic de-branding, inside this protocol's scope.** They are wired into podspecs, Gradle and CMake. Real, but not this document's problem, and not worth the churn while the protocol is landing.

---

## 5. Resolved answers to the brief's open questions (section 4)

Each statement below is normative for the specification. MUST, MUST NOT, SHOULD and MAY carry their RFC 2119 meanings.

### 5.1 Identity and signing (brief 4.1)

1. **A tenant is a key, addressed by an origin.** The protocol MUST NOT introduce a `tenant_id` column or a panel identifier in JWT claims. Tenant identity is `pid = sha256(root ed25519 public key)[0..8]`, carried in every signed payload. The base URL remains the address; the pinned root key is the identity. A directive signed by operator A MUST be rejected by a profile pinned to operator B even if both profiles are held on one device, which they already can be (`enrollApiClientProvider` is an autoDispose family keyed by panel URL).

2. **Two tiers, threshold kept in the format.** An offline root key signs only the key document. An online signing key signs catalogs and directives. `thr` carries a threshold per role even though single-operator reality means threshold 1, because keeping the field costs 20 bytes and means a future multi-signer root does not need a `spec_version` bump.

3. **The authorization rule is normative and is the one A omitted.** A verifier MUST resolve the required role from the document type, and MUST read the key set and the threshold for that role from the PREVIOUSLY TRUSTED key document, never from the document being verified. There MUST be no API path that returns a key without its role. The doc_type to role to threshold table is a required table in the wire-format spec. Without this rule, an attacker holding the online signing key mints a key document at version N+1 containing only their own key with role `root`, the client's high-water mark advances, and the operator can never recover; that voids the design's headline compromise-recovery property.

4. **Root key custody is operator-held.** Generated by `caramba-panel csm keygen root`, printed once as a BIP39-style mnemonic so paper backup is realistic, derived deterministically from it, with the fingerprint printed alongside so a restore can be verified. The tool MUST refuse to write the private half into the panel working directory and SHOULD refuse to run when it detects it is on the panel host. Root loss is unrecoverable in band; the optional Webq Pro countersignature is the opt-in recovery lane and defaults off.

5. **The signing input is the transmitted bytes inside a fixed frame.** `magic || doc_type || u16be(payload_len) || payload`, signed as received. No implementation ever re-serializes, re-orders, normalizes or parses before verifying. The magic plus doc_type is the domain separator, so a catalog signature can never be replayed as a directive signature. RFC 8785 JCS and TUF canonical JSON are both rejected, because both require every implementation to agree on a re-encoding, which is exactly R10. A verify-side re-encode-and-compare step is also rejected, because it silently re-imposes an encoder in Go and Dart as a rejection criterion (D's mistake). Canonicity is enforced instead by the strict decode profile in X1, which is a local predicate over the incoming bytes.

6. **Pinning is by truncated keyid in the enrollment artifact.** `link_pin = base32_crockford(sha256(root_pubkey)[0..12])`, 96 bits, carried as `k=` on the enroll link and in the bootstrap blob. On mismatch the client MUST refuse enrollment with a hard error, not a warning. This is trust on first use and the spec MUST say so. The manual-entry path, which is the one that survives when Telegram is blocked, MUST carry at least the first 40 bits of the pin as a required dictated field, and MUST NOT offer a "continue anyway" affordance for any code-based enrollment. Fold the pin into the code so there is one string to dictate.

7. **Root rotation follows TUF.** Version exactly N+1, signed by both the old and the new root key sets, meeting the threshold under both, with "old set" meaning the client's currently trusted set. Clients MUST refuse to skip a version and MUST be able to walk intermediates in one request (`GET /sub/k1?since=N`). An imported out-of-band bundle MAY carry a full intermediate chain, using the multi-frame armored form, so the offline rung survives rotation.

8. **`http://` is rejected.** `normalizePanelUrl` (`lib/data/models/enrollment.dart:60-71`) accepts plain http today and MUST stop, with a documented `.onion` exception because onion addresses are self-authenticating. `fetchSubscriptionBody` (`lib/data/subscription_fetch.dart:22-49`) gets `followRedirects: false` with per-hop scheme and origin validation and a body size cap, in the same change. One hop MAY be followed when, and only when, the target host equals the tenant's configured `subscription_domain`, because `subscription.rs:113-137` issues that redirect unconditionally today; the profile URL is then normalized to the target so the hop disappears.

### 5.2 Manifest schema (brief 4.2)

1. **Three documents.** A key document (root-signed, 7 day expiry, carries the key set, thresholds and the revocation list). A catalog (online-signed, content-addressed, 30 day expiry, nonce-free, identical bytes for every subscriber on a plan tier, therefore cacheable and mirrorable). A per-device directive (online-signed, 1 hour expiry, nonce-bound, HPKE-sealed to the device key). This is the Uptane image and director split reduced to its minimum useful form.

2. **The envelope is the frame.** `"CSM1" || doc_type(1) || payload_len(2, big-endian, max 49152) || payload || nsigs(1) || nsigs * {keyid_trunc(12) || sig(64)}`. Verifiers MUST reject trailing bytes and MUST require the total length to equal exactly `7 + payload_len + 1 + 76 * nsigs`. Payloads are CBOR with unsigned integer keys under the strict profile in X1.

3. **Three independent freshness mechanisms, and they are not interchangeable.** Monotonic version per `(pid, doc_type, locator)`, persisted; expiry with 300 seconds of skew tolerance; and a client nonce echoed inside the signed directive payload. The nonce is the only one that survives a wrong clock, which is the normal case after a factory reset and common when DNS blocking prevents NTP. On first run, before any trusted time exists, the client MUST anchor on the enrollment-time server date and the freshest `iat` seen during enrollment, and MUST NOT accept a document whose `iat` is below that floor; the floor is monotonic and never decreases.

4. **The catalog is authorized, not public.** Rejecting 4.6 has a consequence: the catalog is fetched under the same locator-scoped surface as the directive, and its content-addressed URL is served only to a caller presenting a valid locator. It remains nonce-free and byte-identical per tier, so it is still cacheable and still safe to mirror, and a mirror still learns only an IP and a timestamp. The catalog root hash for each tier MUST be published in the root-signed key document, so a per-user catalog cannot be minted without a root signature; this closes the tracking-beacon channel that content addressing otherwise hides.

5. **Nodes carry a stable id alongside the verbatim display name.** `pn` is the exact mihomo proxy name, byte-identical, preserving `Server.ID == Server.Name == the Clash proxy name` and the flag-emoji country decoding. `id` and `cc` are first-class fields alongside it. This is the brief's own prescription (preserve it while introducing stable ids alongside) and it is what finally makes a display-name change possible without breaking server pinning, the prober and autotune at once. The Clash generator's missing uniquifier (`format!("{} {}{}", ...)` with no dedup, unlike the sing-box path's `unique_tag`) MUST be fixed in the same pass, and the Go renderer MUST reuse the same dedup algorithm, with a fixture asserting Rust and Go emit identical proxy names for the same node set.

6. **Relays are a first-class list, and the picker is capability-gated.** Three orthogonal lists (exits, relays, routes) in the catalog. The relay control is hidden until the capability bit is set, and the bit is set by the panel only when the Clash generator actually emits relay chains. Shipping the data field early and the control late is the only way to avoid selling a placebo (R13).

7. **The size budget is a protocol constraint.** No single response above 4 KB. No single TCP connection carrying more than 8192 bytes of body, counted with the handshake included until measurement says otherwise, and with a packet ceiling alongside the byte ceiling. Both thresholds are signed catalog fields, not compiled constants. A catalog payload above 12288 bytes MUST be chunked; the panel MUST refuse to sign one above 49152 bytes. Chunking ships in v1, not as a reserved field, because BC1 makes the honest node entry 220 to 280 bytes.

8. **Refusal reasons and operator metadata are signed fields, not headers.** `st` (status enum: `pending_approval`, `onboarding`, `active`, `expired`, `revoked`, `suspended`, `quota_exceeded`, `device_limit`) plus a machine reason code, replacing today's bare 403 text body (`subscription.rs:169-265`). `announce`, `support-url` and `profile-web-page-url` become signed fields, because signed fields survive caching and mirrors in a way headers do not. Free text is capped at 80 characters, rendered as inert text under B3, and is render-only under C6.

9. **Revocation is a root-signed list inside the key document.** Revoked keyids, revoked node ids. A client that sees a keyid in `rev` MUST reject that key and every document it signed, including cached ones on disk, immediately. Propagation is bounded by the key document's 7 day expiry in the worst case and by one refresh in the normal case. Node revocation additionally MUST be honored against the cached catalog, so a seized node is dropped even while the client is running offline.

10. **The engine-capability signal is mandatory.** The client MUST NOT display a connected state that the engine cannot back. `engine/engine_stub.go` reports `Connected` with no tunnel; the capability intersection from B1 covers this on the client side and the spec states it as an invariant (section 6).

### 5.3 Transports and the fallback ladder (brief 4.3)

1. **The ladder is a source list, ordered, individually toggleable, geography-independent.** Seven rungs: R0 cached signed documents; R1 direct HTTPS to the enrolled origin; R2 signed mirrors, ASN-diverse, per-cohort; R3 DoH-resolved address with an explicit per-mirror SNI field under operator control; R4 through the app's own tunnel; R5 user-entered SOCKS5 or HTTP proxy, used for manifest and config fetch only, never for tunnel traffic; R6 out of band (QR, file, paste), always on, never disableable. The default order and the default enabled set come from the signed catalog per tenant (D3). The user may reorder and toggle. A rung the user has switched off is never tried, ever, including on failure.

2. **Nothing is conditional on geography and nothing is dormant.** Every compiled rung is enumerated on one screen, including rungs unavailable on the current device or not offered by the current operator, which render visible and disabled with the reason, never hidden. Review notes describe the list rung by rung and state accurately that the list progresses automatically on failure (D1).

3. **The ladder is implemented once, in Go, behind `HTTPDoer`.** Wired at `auth.WithHTTPClient` and `subscription.WithHTTPClient`, constructed in BOTH `NewCore` and `SetPanelURL`. The Dart control plane MUST NOT open its own sockets to an operator: `ApiClient` and `fetchSubscriptionBody` route through the core over the existing primitive FFI boundary as JSON strings, following the `SetPolicyJSON` pattern. Otherwise enrollment, login, token refresh and preferences all bypass the ladder, the locator cannot be re-read after a token expiry, and the app degrades to R0 permanently while the core happily climbs a ladder for a config it can no longer be told to change. Ship `transport_mihomo.go` and `transport_default.go` twins per the `engine_mihomo.go` and `engine_stub.go` discipline.

4. **`FetchProfile` becomes the ladder loop.** Per-attempt timeouts, `io.LimitReader`, ETag caching, connection hygiene (no reuse of a connection that has already carried more than the signed threshold), and a last-good on-disk document as the final rung. `component/resource/vehicle.go:87-183` is the working reference for the caching and limiting parts.

5. **uTLS, pinning and padding are mandatory on the control plane.** Per D2, D3 and D4.

6. **Refresh cadence comes from the signed `ttl` in explicit seconds with signed jitter.** `Profile-Update-Interval` keeps emitting the bare string `"2"` forever for Hiddify and sing-box; Caramba Connect reads `ttl`. This settles the hours-versus-minutes ambiguity (`subscription/subscription.go:183-187`) without either consumer changing, and it takes a connected device from roughly 720 fetches a day to roughly 12 at unpredictable moments. The mirror set refreshes on its own faster cadence, independent of `ttl`, so the rescue channel is not slowed by the privacy win.

7. **Whitelist mode is not solved and the spec says so.** Only R0 and R6 survive it. The `transport.Carrier` interface exists in the Go core; no carrier ships in v1 and no toggle for one appears in the UI (4.3).

8. **Bootstrap de-blocking is part of the ladder, not a separate track.** Generalize `{BASE}` (`routing/presets.go:24-58`) into an ordered pool shared by the manifest fetch, rule-set providers and geo databases; emit `proxy:` on rule-providers; call `SetGeoIpUrl`, `SetGeoSiteUrl`, `SetMmdbUrl` and `SetASNUrl` at catalog mirrors; move bootstrap DoH off the hardcoded 1.1.1.1 and 8.8.8.8 into the signed catalog; replace the `gstatic.com` probe. Every one of those fetches carries a sha256 from C3 and traverses the same `HTTPDoer`.

### 5.4 Settings sync (brief 4.4)

1. **Vocabulary is `CorePolicy` strings, never `CoreConfig` indices.** `corePolicyFrom` is the single translation point, and its inverse MUST be added in `core_policy_mapping.dart` so a fetched selection repopulates the pickers. Without the inverse, settings sync is write-only and a second device shows stale UI over correct behavior, which reads as a bug forever.

2. **One writer, one round trip.** `want` in the request, authoritative signed `sel` in the response, per B6, with body-bound proofs and fresh nonces per B5. Absent means unchanged, explicit null (or the `"default"` sentinel) means reset to operator default. `If-Match` with 409 and the current state on stale.

3. **Local first, then queued.** The change applies locally and immediately, because the client holds the catalog and builds its own config (BC1). The signed write queues and drains over whatever rung is available, with a visible "not yet synced to your provider" state. No setting change ever blocks on the network.

4. **Preferences are per subscription, except split rules.** Exit, relay, routing preset, protocol and variant propagate across a user's devices, because that is the product goal. `split` is per device, because app lists are platform-specific. A cross-device write MUST NOT be able to set `killSwitch`, `dns`, `split` or the enabled transport set on a sibling device without that device raising the Keep or Revert card unconditionally.

5. **The config fetch becomes a pure read.** The `UPDATE subscriptions SET relay_country` inside the GET handler (`subscription.rs:745-751`) and the auto-pin persist (`:617-634`) are both removed from the app path, and both query parameters keep filtering exactly as they do today so a pasted URL sees no change. Sequencing and the mini-app dependency are in section 7.

6. **No client-side state report beyond the version high-water mark.** The client reports the highest directive version it has accepted, which gives server-side rollback detection at a privacy cost of one integer whose upper bound the operator already knows. The client MUST NOT report which transport rung carried the request, per request, to the operator: that is a live map of which circumvention rungs still work, per device and per ASN, volunteered to a party who may be compelled. Rung telemetry, if it ships at all, is opt-in, coarse-bucketed, aggregated over 24 hours, sent only over an established tunnel, and never on the same request that carries the device identity.

### 5.5 Device binding (brief 4.5)

1. **A device is a keypair thumbprint.** P-256, generated non-exportable in Secure Enclave or StrongBox where available, with an explicit and user-visible software tier below Android 12 recorded at enrollment and shown to the operator. Signing and key agreement use separate keys; the thumbprint derives from the signing key. Device identity MUST NOT be derived from a Telegram id, phone number, email or any other low-entropy identifier.

2. **The manifest fetch MUST NOT count devices.** No `track_access`, no device-limit enforcement on the manifest path. And the config fetch MUST count by thumbprint, not by apparent source IP: `subscription.rs:203-262` with `get_active_ips` is what actually enforces the limit today, so leaving it IP-based means every ladder rung with a different egress burns a slot, and under fetch-through-tunnel the apparent IP is the exit node's, shared by every user of that node. R9 is retired only when both halves are done.

3. **The manifest locator is derived, and rotatable per subscription.** `loc = base32_crockford(HMAC-SHA256(secret, "csm1-loc" || 0x00 || subscription_uuid || u32be(gen))[0..15])`, where `gen` is a per-subscription counter column, not a panel-wide epoch, so revoking one leaked locator is one UPDATE rather than a fleet-wide event. A panel-wide epoch remains as an emergency lever.

4. **The subscription uuid stops being a Connect credential.** It is removed from the directive entirely (BC2); the client already knows its own uuid and assembles the config URL locally. Path fields in signed documents MUST be path-only, MUST begin with a single slash, MUST contain no scheme or authority, and are resolved only against the pinned enrollment origin or a host drawn from the signed mirror list. `rsbase`, `geo` and `doh` are restricted to hosts in that same list. Decoupling the uuid from the VLESS, Trojan, TUIC and Hysteria2 credential (`subscription_service.rs:191-205`) is a larger change against live users and is scheduled after cutover, not before; until it lands, the client MUST NOT present any control that claims to rotate access when it only rotates a link.

5. **Enrollment is an act an authenticated account can perform on itself.** A device key registers against a user id, and the account JWT is the enrollment authority for the second and subsequent devices. This is the bridge B lacked. Enrollment codes follow Headscale pre-auth-key semantics (single-use flag, ephemeral flag, roughly one hour default expiry, explicitly revocable), with `expires_at` NOT NULL and a server-side maximum, and a hard ceiling on `max_uses`.

6. **R12 is out of scope and stays open.** Fifteen minute unrevocable access tokens, no refresh reuse detection, no logout-all, one symmetric secret with no `kid`. CSM/1 signs configuration; it does not fix the session layer. Named in section 8.

### 5.6 Payments hand-off (brief 4.6)

1. **The protocol never carries commerce.** No prices, no bot handle, no purchase link, no "buy" call to action, in the enrollment payload or in any signed document. Payment state is signed data: `st: "expired"` with a reason code, rendered as a neutral status line. Operator text is inert under B3 and carries no URL the app will open, in any storefront. That is the Apple 3.1.3 answer and it does not depend on detecting the storefront, which A's own open question 7 admits it cannot do on Android.

2. **The user crosses the payment gap through the tunnel.** `onboarding_traffic_mb` is the designed escape hatch and the protocol supports it with the `onboarding` status: a new unpaid user can connect, and then reach whatever payment channel the operator runs, which is the only channel reliably available from inside Russia when Telegram is blocked.

3. **The out-of-band renewal contact is a licence obligation.** Collected at operator signup, because the protocol cannot carry it and the bot cannot be assumed reachable. Whether Bot API calls, `t.me` deep links and Stars complete from inside Russia is unverified and MUST be field-tested before any payment story is written down; it is a blocker on the payments track, not on the protocol.

4. **Apple 2.1(a) is a protocol feature, not a manual arrangement.** The issuance endpoint mints a permanent, multi-use, non-expiring demo code against a Webq Pro demo panel, handed to review with the deep link and the rendered QR. Note the correction in section 7: `max_uses: 0` can never redeem, because `store_service.rs:400` requires `used_count < max_uses`. The demo code must be `NULL` meaning unlimited, or a large finite value.

### 5.7 Privacy (brief 4.7)

1. **What may appear in a signed document, enumerated and enforced.** No Telegram id, username, phone, email, payment reference, referral code, family membership or ticket content, and no hash of any of them. Enforced by a panel unit test that asserts the CBOR key set of every emitted document is a subset of a hardcoded allowlist and that no encoded document contains the fixture user's identifiers. It fails the build.

2. **The per-device directive is sealed.** HPKE, RFC 9180 base mode, to the device key (BC2), so a mirror, CDN or onion front holds only a ciphertext and a thumbprint. The recipient key rotates on a schedule and its current value is published in the signed catalog, so a compelled or seized panel does not retroactively decrypt a long window of recorded traffic. A rekey message authenticated by the device signing key exists from v1; there is no path in which a device is stuck with a key it cannot replace.

3. **The catalog carries nothing personal.** Per-tier, byte-identical, nonce-free. Combined with the root-published tier hash from 5.2.4, that property is verifiable rather than asserted.

4. **`split.apps` never leaves the device.** Enforced by the client's own serializer, in both directions, not as an operator-configurable preference. An installed application list is the most identifying thing a VPN client could upload.

5. **Frequency is the privacy win and it is large.** Roughly 12 jittered fetches a day instead of roughly 720 on a fixed period, plus 256-byte-bucket padding drawn per request from a per-tenant range, plus destination diversity across an ASN-diverse mirror pool. The operator still learns the source IP at fetch time and at connect time, which is inherent to running a VPN, and the spec says so rather than claiming otherwise.

6. **Size correlation on the config fetch is mitigated, and where it is not, it is stated.** The config response is padded into coarse buckets at the panel (a trailing comment block is invisible to every YAML and JSON parser). Where a residual correlation remains, the spec states it instead of claiming bucketing the design does not have.

7. **Multi-tenancy obligations are contractual and the protocol minimizes what there is to disclose.** The enrolled operator is a third party under Apple 5.1.1(i) and 5.4. The licence agreement binds operators to the same data commitments the app declares; the in-app policy names the enrolled operator by hostname, which it can do because the client knows the operator identity by pinned key; and the "What this app sends" screen (B8) turns the disclosure into something the user can verify. Apple organization enrollment is a long-lead calendar dependency and starts before any code.

---

## 6. Non-negotiable invariants

The specification MUST encode all of these. A change to any of them is a `spec_version` bump and a re-review, not a patch.

**Canonicalization**

1. The signature covers the transmitted byte slice inside a fixed frame. No implementation re-serializes, re-orders, normalizes or parses before verifying. No verify-side re-encode-and-compare.
2. Framing is exact-length. Trailing bytes are rejected. `nsigs` cannot be inflated. Total length equals `7 + payload_len + 1 + 76 * nsigs`, exactly.
3. The strict CBOR decode profile and the strict Ed25519 profile from X1 are enforced during parse, in all three implementations, with a shared negative-fixture corpus as a merge gate in `cargo test`, `go test` and `flutter test`. Vectors are computed independently of the Rust signer.
4. Role authorization is read from the previously trusted document, never from the document under verification. There is no code path that returns a key without its role.

**Size and shape**

5. No response above 4 KB. No connection carrying more than the signed byte threshold, counted with the TLS handshake, with a packet ceiling alongside it. Both are signed catalog fields.
6. The catalog is chunked from v1. The panel refuses to sign an oversized payload rather than emitting one.
7. Refresh is jittered from a signed `ttl` in explicit seconds. Padding buckets are drawn per request from a per-tenant range. The legacy `Profile-Update-Interval` header keeps saying `"2"`.

**What the client refuses to do**

8. Refuse `http://` for any manifest, config, rule-set or geo fetch. The only non-TLS exception is `.onion`.
9. Refuse a document whose signing key is not authorized for that document type, whose version is at or below the stored high-water mark, whose framing is inexact, or whose decode violates the strict profile. Refuse a directive whose nonce does not match the one just sent.
10. Refuse to open any URL supplied by an operator. Operator text is inert, capped, URL-stripped at render, and never rendered on the same surface as the verification chrome.
11. Refuse to persist or echo any operator-supplied value that is not validated against a closed vocabulary.
12. Refuse to fetch a rule-set or geo file whose sha256 does not match the signed catalog.
13. Refuse to fall back to unverified legacy mode once a profile has pinned a root key. A missing capability field on an enrolled profile is a hard, non-dismissible error, not a downgrade. This closes the one-field downgrade attack that A's own compatibility section opened.
14. Refuse to display a connected state the engine cannot back.
15. Refuse to transmit `split.apps`, in either direction.
16. Never disconnect a user because a document expired. An expired document is still valid for connecting; it is refused only for accepting new settings or new status. This one is absolute.

**What the user must always be able to see and turn off**

17. Every compiled transport rung, on one screen, with a toggle, an order, and a live per-attempt history. Unavailable rungs render visible and disabled with the reason, never hidden.
18. The operator's identity: display name, root key fingerprint in groups of four, enrollment date, whether the pin was established out of band or in app, and whether it has ever changed.
19. The verification state of the documents currently in use: version, issued, expires, signer fingerprint, verification result, and the decoded fields.
20. The "What this app sends" screen, with a copy button.
21. The configuration age and its source, whenever the client is running on cached documents.
22. Any operator change to a setting the user set explicitly, and any narrowing of the user's security posture, as a Keep or Revert card.
23. Telemetry, if it exists at all, as an off-by-default setting whose contents are enumerated on screen.

---

## 7. Prerequisite panel work

Addressed to the panel team. This maps onto the brief's build order (section 6, items 1, 2, 3, 6, 9 and 12) and is sequenced so that everything in the first block lands against the live `exa_robot` tenant without changing a single byte any existing client sees.

### Block 1: additive, zero observable change. Ship first, independently of everything else.

**P1. Enrollment code issuance (brief item 1).** Three issuers writing the same row: an admin endpoint, a bot `/invite` command, and a `caramba-panel enroll issue` subcommand (C8). Extend `enrollment_codes` with `label`, `plan_id`, `ephemeral`, `revoked_at`. Two corrections you must make, both verified in source:
- **There is no `/api/v2/admin` router and no admin API auth surface.** `api/v2/mod.rs` contains only `bot_routes` and `app_routes`; the admin surface is a server-rendered HTMX UI nested at `$ADMIN_PATH` (`main.rs:851`, `main.rs:1602`) behind a Redis session cookie plus CSRF middleware that rejects state-changing methods lacking `HX-Request` or a same-origin `Origin`/`Referer`. Issuance therefore needs a real scoped admin API token with its own middleware and its own rate limit, mounted beside the bot router. This is a named deliverable, not a route.
- **`max_uses: 0` can never redeem.** Both the validity SELECT and the conditional increment use `used_count < max_uses` (`services/store_service.rs:400`). The Apple 2.1(a) demo code must be `max_uses NULL` meaning unlimited (schema plus predicate change) or a large finite value. Decide explicitly whether `plan_id` alters the current unconditional free-plan grant at redemption before this ships.

**P2. Panel identity and signing keys (brief item 3).** `caramba-panel csm keygen root` printing a BIP39 mnemonic once and refusing to write into the panel working directory; a `csm_keys` table holding public keys only; the online signing key under its own secret with its own rotation, never `SESSION_SECRET` (which `APP_JWT_SECRET` already falls back to, and which has no `kid` and no rotation path per R12). Reuse the ed25519 machinery already shipping in `license/activation.rs` and `caramba_shared::license`; the panel gains a signer, not a dependency.

**P3. The `csm` module and the three read routes.** Frame encode and sign, the payload builders, the locator HMAC with a per-subscription generation column, and `GET /sub/k1`, `/sub/c1/{cat_id}`, `/sub/m1/{loc}` on the root router beside the existing registration. Requirements that are easy to miss:
- The `m1` handler MUST NOT call `track_access` and MUST NOT enforce the device limit, and MUST have its own rate limit keyed by locator and by source IP, failing closed on Redis error for this route specifically. The existing `rate:sub:{uuid}` limiter lives inside `subscription_handler` and does not apply to a new route, and it fails open.
- Cache the expensive part. Filling the config hash requires several repository round trips plus full generation, and the panel config cache is 60 seconds while steady-state refresh is every two hours, so it is cold on essentially every directive fetch. Store the computed hash and selection blocks under the existing cache key and sign only the small envelope per request.
- The locator is HMAC output and is not invertible, so a locator index column plus its migration and backfill is part of this item.

**P4. `caramba-sub` passthrough (unscoped by every other design).** Verified: `apps/caramba-sub/src/main.rs` registers `/health`, `/sub/{uuid}`, `/app`, `/app/`, `/app/{*path}` and `/api/{*path}`, with no fallback and no wildcard beyond `/api`. Consequences:
- `/sub/k1` would match `/sub/{uuid}` and be proxied as a subscription fetch. `/sub/c1/{id}` and `/sub/m1/{loc}` 404. Add explicit routes.
- Its cache key is `sub:config:{uuid}:{client}:{relay}:{node}` and omits country and variant, while the panel's is `sub_config_v5:{uuid}:{client}:{node}:{variant}:{cc}:{relay}` (`subscription.rs:692`). Two subscribers in different countries share one entry, which produces config-hash mismatches through the sub path that succeed direct. Extend the key or bypass the cache for CSM-aware clients.
- It silently drops `variant`. Forward it before any directive can carry a non-empty variant selection, or the sub path deterministically returns the default variant and every signature fails.
- The `subscription_domain` 308 redirect lives inside `subscription_handler`, not in the router, so the new routes do NOT inherit it. Reimplement or extract it. `caramba-sub` also treats any upstream 3xx as a fatal 502.
- Body fidelity is fine and does not need work: `proxy_handler` streams `res.bytes_stream()` and copies headers verbatim, and reqwest is built without gzip or brotli in both crates, so no transparent decompression can desynchronize a copied `content-encoding`.
- The global `CompressionLayer` and the five `SetResponseHeaderLayer::overriding` layers on the panel's root router will otherwise compress and stamp constant headers on every signed response, which breaks the padding-bucket invariant and creates a cross-tenant header fingerprint. Layer the CSM routes separately and assert `Content-Encoding` is absent in a test.

**P5. Move branding under the signed surface (X2).** `GET /api/v2/app/branding` is public and unsigned in the `public` group in `api/v2/mod.rs`, and the client opens operator-supplied `support_url` and `bot_url` externally (`powered_by.dart:126`). Move the fields under signed data before anything else ships. Do not put seed material, keys or any bootstrap payload on a public endpoint.

**P6. Determinism and geo resolution for the config hash.** Two verified corrections to A's own plan:
- The randomness is not where A thought. `rand::rng().random_range(500..=1200)` at `subscription_generator.rs:885-910` sits inside `generate_v2ray_config` (810 to 1138), not `generate_clash_config` (1138 to 1548). So the Clash body is deterministic with respect to randomness and the config hash is achievable over it; it can never extend to the v2ray body without removing that padding or seeding it deterministically.
- The config body is a function of the requester's apparent country. `client_cc` comes from `x-country-code` or `cf-ipcountry` or a GeoIP lookup on the request IP (`subscription.rs:147-150`, `:666`), and it is in the cache key by design (`:692`), and the relay filter falls back to it. The ladder changes the apparent source IP by construction, so hash mismatch would be the normal case and A's stated behavior on mismatch is a silent fall back to cached, which means settings changes quietly stop taking effect. **Resolve geo server-side at directive-signing time and emit explicit `relay_country` and `node_id` so the config fetch never consults GeoIP.** Add `ORDER BY` to the relay query (`subscription_service.rs:1846-1850` has none) and a deterministic tiebreaker to node ordering (`node_repo.rs:959-967` orders by `sort_order` only). Gate this item on a cross-egress test: sign from an IP in one country, fetch from an IP in another, assert the hash matches.

### Block 2: behavior change behind flags. Ship after block 1 is stable on the live tenant.

**P7. Preferences as real state (brief item 6).** The new columns, the single write endpoint with body-bound proofs and nonces, and the removal of the write-on-GET side effects, with both query parameters retained as filters. Three sequencing requirements:
- **Migrate the mini app first.** `apps/caramba-app/src/exa/lib/subscription.ts` picks a relay from browser timezone, stores it in localStorage, and bakes `?relay_country=` into the copied URL, and the `UPDATE` at `subscription.rs:745-751` is what turns that client-side choice into server state (the comment at `:735` says so). Delete the write before the mini app is migrated and its picker becomes write-only to localStorage. Point it at the new write endpoint, or keep the write behind a flag until it is migrated.
- **There are two `node_id` writers.** A's plan removes only the auto-pin at `:617-634`. The explicit-node persist at `:636-644` also survives and must be reconciled, or the pure-read claim is half done.
- **Do not remove the auto-pin early (4.13).** Keep the auto-pin path for any request with no CSM device identity, and remove it only once Connect clients are the majority. Add the deterministic node tiebreaker from P6 before removing it, or a pasted-URL user's exit can silently change between fetches where the pin made it sticky.

**P8. The relay gap (brief item 2, R13).** Make `generate_clash_config` consume `relay_nodes` and emit mihomo `dialer-proxy` bindings plus an Auto-Relay group, mirroring the sing-box path. Emit additively (N exits plus M relays), NOT the sing-box path's cartesian product (N times M), because for a 40-node three-relay operator that is 43 proxies instead of 120, which is the difference between fitting under the freeze threshold and not. Gate behind a settings flag, default off for existing panels, on for new. With the flag off, output is byte-identical to today. The capability bit that unhides the relay picker (B1) is set by this flag, so the picker is dark until the generator is real. Fix the missing proxy-name uniquifier in the same pass (5.2.5).

**P9. The catalog as one node model with several renderers.** BC1 means the panel ships node data as well as rendered YAML, and the Go core renders for mihomo. Restructure so the catalog is the single model and Clash, sing-box and V2Ray are renderers of it, then hold Rust and Go to identical output for the same node set with a fixture. This is the two-renderer tax, and it is the item most likely to overrun.

### Block 3: cleanup (brief item 12)

Plan-scope `GET /relays` (it is not plan-scoped today, unlike `/servers`). Put `GET /api/v2/client/recommended` behind auth or fold it into the signed catalog. Extend rate limiting across the whole app router mirroring the bot router's two-layer design, and decide fail-open versus fail-closed deliberately rather than by accident. Emit both the legacy `Profile-Update-Interval` header and the explicit signed refresh value. Mirror ASN diversity enforcement at save time (D6).

### What the panel team can ignore

Everything in section 4. In particular: do not build derived rendezvous, do not build a public unauthenticated catalog endpoint, do not build a long-poll push channel, do not build a transfer capsule, and do not put bootstrap material on a public endpoint.

---

## 8. Open risks accepted

Each carries the trigger that forces a redesign rather than a patch.

**A1. The online signing key can invent nodes.** The directive references the catalog by hash, but a compromised online key can sign a different catalog. Full Uptane containment requires an offline signing ritual for every node addition, and a one-person operator will not perform one; buying it produces a protocol that is theoretically stronger and practically bypassed, with operators keeping the root key on the same host anyway. The offline root bounds key identity and therefore recovery. **Trigger:** a licensed operator population large enough that a compromised panel is a routine event rather than a tail risk, or the first real compromise, forces a mandatory offline catalog ceremony with a client-verifiable custody signal, which is a different trust model.

**A2. Online-key revocation takes up to 7 days against an adversary who can withhold.** The key document is the only carrier of the revocation list and its freshness rests on a 7 day expiry. An adversary who steals the online key on day one also captures the current key document and can replay it while suppressing refreshes. **Trigger:** if measurement shows key-document refresh failing for a material fraction of RU devices, add the TUF timestamp role (a roughly 60 byte root-signed document carrying the current key-document version and hash, signable in bulk by a cron on the root-holding machine), which cuts the window to 24 hours. The field numbers are reserved for it.

**A3. Whitelist mode is unsurvivable except by cached documents and a human carrying bytes.** Every other rung dies. The one demonstrated escape is tunneling through domestic infrastructure, which is a tunnel-transport problem rather than a manifest problem, and in whitelist mode the user cannot reach the VPN either. **Trigger:** whitelist mode becoming the default rather than an episodic measure makes the domestic carrier a product requirement, and it must then ship as a fully exercisable reviewed feature, not as a toggle whose parameters arrive later (4.3).

**A4. No embedded circumvention transport, which lands hardest on the users who need it most.** A Russian user with no working direct path, no working mirror, no tunnel already up and no proxy of their own has only the out-of-band rung. The bet is that ASN-diverse per-cohort mirrors plus fetch-through-tunnel plus bring-your-own-proxy plus a self-contained bootstrap blob covers most of that population, and that the brief's own evidence says an embedded Tor rung would probably not have covered the rest. **Trigger:** field measurement from a Russian vantage point showing the ladder failing above a set rate reopens the merged-module spike, with dnstt against the operator's own authoritative nameserver as the first candidate rather than Tor, precisely because it is the only mechanism in the material that plausibly survives a DoH-only network.

**A5. Apple 3.1.1, the license-key question, is unadjudicated.** No written rule and no public precedent. The mitigation is structural: the enroll link and QR are the primary flow, the pasted subscription URL is secondary, and the app is fully functional before enrollment. **Trigger:** a rejection citing 3.1.1 forces the enrollment flow to become an operator picker over a Webq-Pro-hosted directory, which is a different product and a different censorship posture.

**A6. RU App Store removal is the expected steady state, not a tail risk.** The exact peer set was pulled on 2026-03-28 with no court process. Removals are storefront-scoped, so installed copies keep working but stop updating. **Trigger:** removal makes the in-binary reserve mirror pool unreachable by app release, so the reserve set must live in the signed key document (7 day expiry, its own URL, root-signed) rather than only in the app bundle. Build it that way now; the trigger is when it becomes load-bearing.

**A7. The mirror pool is an enumeration target.** Anyone who enrolls gets every mirror hostname in their cohort. That is the same exposure Tor bridges have and the mitigation is the same: per-cohort subsets (D7), aggressive rotation, and a reserve pool held out of the published catalog. **Trigger:** evidence of cohort-wide mirror burns faster than the rotation cadence means the mirror set moves entirely into the sealed per-device document and out of the shared catalog.

**A8. Two config renderers until the panel refactor lands (P9).** Drift between the Rust generator serving Hiddify and v2rayNG and the Go renderer serving Connect. Mitigated by the verify-and-compare migration phase (C7) and by the identical-proxy-name fixture. **Trigger:** a drift class that the fixture cannot catch (for example, silent divergence driven by free-form `Inbound.settings` JSON) forces the panel to emit a per-exit mihomo outbound fragment inside the catalog, trading bytes for a single renderer.

**A9. R12 is untouched.** Fifteen minute unrevocable access tokens, no refresh reuse detection, no logout-all despite the index existing, one symmetric secret with no `kid` and no rotation, and `APP_JWT_SECRET` falling back to `SESSION_SECRET`. **Trigger:** the signing key must never be stored under `SESSION_SECRET`; if it is, a single symmetric leak becomes simultaneously a session-forgery event and a config-signing event with A2's window attached. That constraint is in section 7; the rest of R12 is a separate track.

**A10. The tunnel config is authenticated, not made more evasive.** CSM/1 does not change protocol selection, obfuscation, port choice, or the VLESS plus Reality plus vision problem on 443. A user whose exit protocol is blocked stays blocked. **Trigger:** this is the largest gap between "the config arrives" and "the user is connected", and it is where the derivation machinery would actually have paid off. Per-cohort transport assignment (which protocol, port, fingerprint, mux and flow each cohort uses) is the natural v2 item, and it belongs in the catalog schema's reserved space now.

**A11. Per-user equivocation on the catalog is undetectable in v1.** Rejecting a transparency log (4.10) has this cost. Partially bounded by publishing tier catalog hashes in the root-signed key document (5.2.4). **Trigger:** an operator caught equivocating, or a licensee population where the operator relationship is less trusted than it is today.

**A12. Nothing in the Connect track has been compiled, bound or signed (R18).** The whole plan rests on an integration that has never happened, against a live panel, bot, sub, node and mini app that serve real users. **Trigger:** the first integration milestone. Front-load it: the iOS Network Extension target, App Group and entitlements do not exist in the repo today and Apple organization enrollment has a multi-week lead time, so both start before protocol code, and the NE memory measurement with mihomo alone runs in week one rather than at the end.

---

## 9. Document plan

Next files in `apps/caramba-client/docs/protocol/`. Each is owned by this session unless noted.

**`02-SPEC.md`, the normative protocol specification.** The document model (key document, catalog, directive) and the state machine each moves through. The full field registry per document type with types, cardinality, caps and reserved ranges. The doc_type to role to threshold authorization table (5.1.3), written as a table, because it is the single rule three implementers will otherwise each invent differently. Freshness (version, expiry, nonce) and the first-run time anchor. The capability bitfield and its intersection rule. The settings vocabulary and the three-way patch semantics. The ladder: rung definitions, ordering rules, selection, timeouts, connection hygiene, and the explicit statement that a disabled rung is never tried. Enrollment and the bootstrap blob. Sealing and key rotation. Deprecation. Every MUST in section 6 restated in place. Normative language only; rationale lives here in `01-DECISION.md`.

**`03-WIRE.md`, the wire format.** The frame, byte by byte, with the exact-length rule and the signing pre-image. The strict CBOR decode profile and the strict Ed25519 profile, stated as conformance requirements rather than as advice. The armored text form and the multi-frame QR chunking format (C1). CBOR field tables with worked encodings and per-field byte counts. The size budget with the TLS handshake and packet counts included (D5), derived against real measurements rather than estimates, plus the chunking rule and the panel's refusal thresholds. Padding buckets. The catalog node entry sized against `StreamInfo` (BC1). URL and path constraints (5.5.4). Endpoint definitions with methods, auth, caching semantics and the `caramba-sub` routing requirements from P4.

**`04-THREAT-MODEL.md`.** Adversaries, enumerated: the RU network adversary with a trusted CA and inline DPI; the hostile or compromised operator; the compromised online key; the compromised or lost root key; the enrolled adversary who bought a subscription; the hostile mirror or CDN; the malicious co-tenant. For each: what they can do, what they cannot do and why, and the mechanism that bounds them. The full compromise ladder with blast radius and recovery path per key tier. The device-binding boundary and what it does not reach (the shared tunnel credential, until 5.5.4's second half lands). The privacy ledger: exactly what each party learns, restated from 5.7 in adversary terms. And every accepted risk from section 8 with its trigger, so the threat model and the risk register cannot drift apart.

**`05-TEST-VECTORS/`, a directory, not a file.** `vectors.json` plus raw `.bin` fixtures. Positive cases: each document type at minimum, typical and maximum size; single and dual signature; a full root rotation chain; a sealed directive; the armored and chunked forms; the bootstrap blob. Negative corpus, which is the part that matters: wrong-role signing pairs, threshold violations, version regression, expired, inexact framing, trailing bytes, inflated `nsigs`, duplicate CBOR map keys, non-minimal integers, indefinite lengths, unknown tags, floats, invalid UTF-8, small-order and non-canonical Ed25519 public keys, non-canonical S, HPKE wrong-recipient and tampered-AAD, oversized payload, and decompression bounds. Plus RFC 9180 key-schedule vectors and device-key-derivation vectors. Every entry carries an expected verdict. Harnesses in `cargo test`, `go test` and `flutter test` all load this directory and all three CI jobs fail on any disagreement. Vectors are computed independently, not emitted by the Rust signer alone.

**`06-MIGRATION.md`.** The four phases (C7) with entry and exit criteria per phase and the specific defect each phase exists to catch. The panel release sequence from section 7 with its flags, its byte-diff gates and its rollback for each step, including the signed kill switch that reverts a fielded client to the legacy path without an app-store release (a client on rung R0 with a valid cached document will otherwise stop asking whether a rollback is available). The mini-app relay-picker migration and its ordering constraint against the write-on-GET removal. The legacy compatibility matrix: what stays byte-identical, what changes behind a flag, and what an un-upgraded operator's client does, including the sticky-CSM rule that closes the downgrade attack (invariant 13). The device and profile migration for the roughly 20 existing users, including the server-pin mapping from Clash proxy name to stable node id and the visible fallback when it cannot be resolved. What is never removed: `/sub/{uuid}` and the four generators, forever.

**Owned elsewhere, referenced from here:** the panel team's own record of block 1 through 3 in section 7; the store-readiness artifacts (review notes enumerating the rungs, the pre-use disclosure screen, the Play VpnService declaration, the demo code and panel, organization enrollment, signed direct-APK distribution); and the field-measurement plan whose outputs feed A4, A10 and the size budget in `03-WIRE.md`.
