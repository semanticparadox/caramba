# CSM/1 Threat Model

Status: normative where it states requirements, analytical elsewhere. 2026-09-02. Companion to `01-DECISION.md` (rationale and the decision record), `02-SPEC.md` (protocol behavior) and `03-WIRE.md` (byte layout, verification order, error codes).

Scope: who attacks Caramba Connect, what each attacker can and cannot do, which mechanism bounds each one and at which verification step, what a compromise of each key tier costs and how the operator recovers, exactly what each party learns, and which risks are accepted rather than mitigated.

Audience: the same three implementers as `03-WIRE.md`, plus whoever has to decide, during an incident, what has actually been lost. Assume the reader has `03-WIRE.md` open and nothing else.

Key words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY carry their RFC 2119 meanings. Where this document states a MUST it is a requirement `02-SPEC.md` is expected to encode; section 11 lists those hand-offs so none is lost. Rationale is not repeated here; it lives in `01-DECISION.md` and is cited by section number.

Claims about current behavior are anchored to `file:line` in this repository and were read, not recalled. Where an input document disagrees with the code, the code is followed and the departure is recorded in section 10.

---

## 1. Method and scope

### 1.1 What this document decides

A threat model that lists attackers without saying where each one stops is decoration. Every subsection in section 2 answers three questions in the same order: what the adversary can do, what the adversary cannot do and why, and the named mechanism plus the verification step that bounds him. Where nothing bounds him, the entry says so and the item reappears in section 8 as a residual. Nothing is asserted to be mitigated unless a mechanism in `03-WIRE.md` or `02-SPEC.md` actually does the work.

Two rules govern the whole document.

> **Signed is not safe.** A signature proves a document came from a key. It proves nothing about the document's contents being in the user's interest. Every field an operator controls is an attack surface even when the signature verifies, and section 4 states the bounds a client MUST place on signed values for exactly that reason.

> **A bound that costs the operator nothing will not be enforced.** Where a containment property depends on an operator performing an offline ritual, this document says so and prices it, because `01-DECISION.md` A1 already concedes that a one-person operator will not perform one. A property that survives only under operator discipline is recorded as conditional, not as a guarantee.

### 1.2 Assets

Ordered by what an adversary would take first.

| # | Asset | Where it lives | Loss means |
|---|---|---|---|
| 1 | The tunnel credential | one value per subscription; `subscription_user_uuid` at `apps/caramba-panel/src/services/subscription_service.rs:191-205` | free use of the operator's fleet as the victim, and impersonation of the victim to the exit node |
| 2 | The user's traffic | the exit node, the routing rules, the DNS configuration | the product's entire purpose |
| 3 | The user's identity and location | source IP at fetch and at connect, account email or Telegram id, device thumbprint | in the RU market, a legal exposure and not only a privacy one (`00-DESIGN-BRIEF.md` R17) |
| 4 | Configuration integrity | the catalog and the directive | traffic leaves the device in cleartext while the UI shows connected |
| 5 | The operator's root private key | operator-held, offline, `caramba-panel csm keygen root` (`01-DECISION.md` 5.1.4) | permanent takeover of the tenant for every client that has pinned it |
| 6 | The operator's fleet list | the catalog, per tier | node IPs are what actually get blocked (`01-DECISION.md` 4.6) |
| 7 | The mirror pool | catalog `mir`, reserve pool `0x08` | the rescue channel, which is what the ladder exists for |
| 8 | Availability | every rung | in a blackout the product is the only channel to the operator |
| 9 | The account session | `APP_JWT_SECRET`, refresh tokens | enrollment of new devices as the victim (`01-DECISION.md` 5.5.5) |

### 1.3 Trust assumptions

Stated so that a reader can see which of them an incident has invalidated.

1. Ed25519 and SHA-256 are sound, and the strict profiles of `03-WIRE.md` sections 2 and 3 are implemented correctly in all three languages. The negative-fixture corpus in `05-TEST-VECTORS/` is the only evidence anyone has for this assumption; it is a merge gate precisely because the assumption is otherwise unverified.
2. HPKE base mode with `DHKEM(P-256, HKDF-SHA256)`, `HKDF-SHA256` and `ChaCha20Poly1305` is sound (`03-WIRE.md` 9.1).
3. The device's hardware keystore keeps a non-exportable P-256 key non-exportable on the hardware tier. On the software tier below Android 12 this assumption does not hold and the protocol says so out loud (section 5.4).
4. The operator's root private key is offline and the operator can perform a signing operation with it within seven days of needing to. This assumption is the weakest one in the list and section 3.3 prices its failure.
5. The user can obtain 20 characters of `link_pin` over a channel the network adversary does not control. Section 7.1 is the analysis of what happens when this fails, and the honest answer is that enrollment security equals the integrity of that channel and nothing else.
6. The device is not already compromised at the operating-system level. A device with a hostile OS reads the tunnel credential out of secure storage and no protocol fixes that.

### 1.4 Out of scope, named rather than omitted

- **The session layer.** Fifteen minute unrevocable access tokens (`apps/caramba-panel/src/api/v2/app_auth.rs:32`), 30 day refresh tokens with no reuse detection (`:33`), no logout-all, and one symmetric secret with no `kid` that falls back to `SESSION_SECRET` when `APP_JWT_SECRET` is unset (`:69-76`). CSM/1 signs configuration; it does not fix the session layer. `01-DECISION.md` A9, `00-DESIGN-BRIEF.md` R12.
- **Tunnel evasion.** CSM/1 authenticates the configuration; it does not make VLESS plus Reality plus vision on 443 survive a home ISP that kills it. `01-DECISION.md` A10.
- **The exit node.** A VPN operator sees the traffic. That is what a VPN is.
- **Traffic analysis by a global passive adversary.** Padding buckets and jitter raise the cost of shape correlation; they do not defeat an adversary who observes both ends.
- **Physical device seizure with an unlocked screen.**

---

## 2. Adversaries

Each adversary carries a short tag used in the matrix at 2.8 and in the risk register at section 9.

### 2.1 The RU network adversary (N)

**Who.** TSPU inline DPI plus the operator-compellable domestic resolver, plus a certificate authority the device already trusts: the Russian Trusted Root or Sub CA is bundled in 28.6 percent of RuStore apps by count and 81.4 percent by weighted download share, and since June 2026 seven major Russian banks serve state-issued certificates (`00-DESIGN-BRIEF.md` 2.2). Assume this adversary can read and rewrite any TLS session the client opens to a host whose certificate chains to that CA, can block by IP, ASN, SNI and apex, can freeze a TCP connection silently after roughly 25 packets or 15 to 20 KB on a foreign datacenter IP, can answer DNS, and can turn the whitelist on.

**What N can do.**

1. Terminate TLS to the panel origin, to any mirror, and to any DoH endpoint, and read and rewrite the plaintext. This includes the enrollment `POST /register` and `POST /login/email` bodies, which are protected by nothing but TLS (`apps/caramba-panel/src/api/v2/mod.rs:145-147`).
2. Read the legacy config body on `/sub/{uuid}`, which carries the tunnel credential in every outbound (`apps/caramba-panel/src/singbox/subscription_generator.rs:229`, `:332`, `:391`).
3. Replay a recorded CSM/1 frame at any time inside its expiry window.
4. Suppress a refresh indefinitely by dropping the connection, which is the withholding half of `01-DECISION.md` A2.
5. Observe the request line of every CSM/1 fetch: the locator is in the path of `/sub/m1/{loc}` and `/sub/r1/{loc}`, and the device thumbprint is in the `d=` query parameter (`03-WIRE.md` 13.2).
6. Block. This is the capability the ladder exists for and the one the protocol cannot remove.
7. Turn on whitelist mode, at which point only R0 and R6 survive (`01-DECISION.md` 5.3.7, A3).

**What N cannot do, and why.**

| Cannot | Bounded by | Step |
|---|---|---|
| Forge a key document, catalog, directive or chunk | Ed25519 over the frame pre-image, keys pinned by `link_pin`, not by any CA | V6, `03-WIRE.md` 1.3 |
| Substitute a key document at first trust when `link_pin` arrived out of band | `sha256(root_pk)[0..12]` equality, hard error, no continue-anyway affordance | `03-WIRE.md` 7.2 |
| Roll a client back to an older **directive** or key document | monotonic high-water mark per `(pid, doc_type, scope)`, equal accepted only when byte-identical | V9, `03-WIRE.md` 6.3 |
| Roll a client back to an older **catalog** | not V9, which is inert here: `cat_id` is derived from the catalog's own bytes, so every catalog is its own scope and meets an empty high-water mark. The bound is V14a, because a catalog is only entered at `verified` when a **directive** named its `chash`, plus that directive's own monotonicity under V9, plus V14b where `tiers` is published. | V14a, V14b, `03-WIRE.md` 6.3 |
| Replay a directive to a device that did not ask for it | 16-byte client nonce echoed inside the signed payload, plus `dtp` equality | V13 |
| Replay a document minted before the device enrolled | `time_floor`, monotonic, never decreasing | V11, `03-WIRE.md` 6.4 |
| Read the contents of a directive | HPKE seal to the device's P-256 agreement key | `03-WIRE.md` 9.4 step 6 |
| Substitute a rule-set or geo file at fetch time | sha256 per resource in the signed catalog, invariant 12 | `03-WIRE.md` 8.2 resource entry |
| Get the client to talk plain HTTP | scheme refusal, `.onion` the only exception, invariant 8 | `03-WIRE.md` 14.4 |
| Fingerprint the client by Go's own ClientHello | uTLS with explicit modern hello IDs, randomized per connection, constructed in **both** `NewCore` and `SetPanelURL` | `01-DECISION.md` D2 |
| Silently freeze the manifest fetch | byte and packet budget with the handshake charged, one catalog chunk per connection | `03-WIRE.md` 11.2, 11.5 |

**What bounds N and what does not.** The application-layer signature is the whole answer to the trusted-CA premise, and it is a complete answer for document integrity. It is not an answer for confidentiality of anything outside a sealed directive, and it is not an answer at all for the legacy config path. Two specifics:

- The `Content-Encoding` prohibition (`03-WIRE.md` 12.4) is load-bearing against N and it is currently violated by construction: the panel's root router applies `tower_http::compression::CompressionLayer::new()` to every response at `apps/caramba-panel/src/main.rs:1626`, and five `SetResponseHeaderLayer::overriding` layers at `:1630-1651` stamp a constant five-header set on everything. The constant stamp identifies any panel as a Caramba panel from one response, cross-tenant, which is `00-DESIGN-BRIEF.md` R16 made concrete.
- Verified as a live hazard on the ladder: `SetPanelURL` rebuilds the auth client, the subscription client and the subscription-info client with no `WithHTTPClient` (`libs/caramba-core/api/api.go:143-168`), and `NewPanelClient` defaults to a bare `&http.Client{Timeout: 30 * time.Second}` (`libs/caramba-core/auth/client.go:75-86`). A re-enrolled tenant therefore silently reverts to Go's default transport, losing uTLS, pinning and the ladder at once. `01-DECISION.md` D2 names this as the bug that ships and is found six months later; it is not hypothetical, it is the current code.

**Residual against N.** Sections 7.1 and 8. In short: enrollment credentials, the account password, and the entire legacy config body including the tunnel credential are protected by TLS alone against an adversary the brief documents as holding a trusted CA.

### 2.2 The hostile or compromised operator (O)

**Who.** The licensee running the panel, acting against a user, or anyone who has taken the panel over. Distinguished from 2.3 by holding the database and the origin as well as the online signing key.

**What O can do.**

1. See the source IP at every manifest fetch and every connect. Inherent to running a VPN, stated rather than denied (`01-DECISION.md` 5.7.5).
2. See all tunnel traffic at the exit.
3. Choose the exit node, the relay, the routing preset, the DNS servers, the enabled transports, and the refresh cadence, by signing a directive and a catalog.
4. Choose which domains route DIRECT rather than through the tunnel, by choosing the rule-set resources in the catalog. See below; this is the most damaging capability O retains.
5. Refuse service by `st` and `rc`, which is legitimate, and deny service by `exph` and `ttl`, which is not.
6. Equivocate between users on the catalog, undetectably in v1 (`01-DECISION.md` 4.10, A11), unless the tier hash is published and enforced.
7. Link a subscription to a Telegram account without any protocol step, because the Hysteria2 password is literally `format!("{}:{}", tg_id, user_uuid.replace("-", ""))` (`apps/caramba-panel/src/services/subscription_service.rs:1908`).
8. Push the user's IP address into Telegram. When a config fetch is refused on the device limit, the panel looks up `tg_id` and sends the blocked client IP to that chat (`apps/caramba-panel/src/subscription.rs:235-262`). That is a disclosure of user data to a third party, which Apple 5.4 forbids outright and 5.1.1(i) requires be disclosed and contractually bound.

**What O cannot do, and why.**

| Cannot | Bounded by |
|---|---|
| Get the client to open an operator-supplied URL | invariant 10; text is inert, URL-shaped substrings stripped at render, never on the verification surface (`03-WIRE.md` 14.6) |
| Persuade the client to persist or echo an opaque operator string | invariant 11 and closed vocabularies (`03-WIRE.md` section 5) |
| Learn the user's installed application list | invariant 15; `split.apps` is refused by the client's own serializer in both directions, and has no key in the `pol` table (`03-WIRE.md` 8.3) |
| Put a Telegram id, phone, email, payment reference or a hash of any of them into a signed document | `01-DECISION.md` 5.7.1, enforced by a panel unit test asserting the emitted CBOR key set is a subset of a hardcoded allowlist; it fails the build |
| Disconnect a user by letting a document expire | invariant 16, absolute (`03-WIRE.md` 6.5) |
| Narrow the user's security posture silently | invariant 22; the Keep or Revert card fires on any narrowing regardless of provenance (`01-DECISION.md` B2) |
| Show a connected state the engine cannot back | invariant 14; `engine/engine_stub.go` reports `Connected` with no tunnel, which is why this is an invariant and not an implementation note |
| Learn which transport rung carried a request | `01-DECISION.md` 5.4.6; the client MUST NOT report it, because it is a live map of which circumvention rungs still work, per device and per ASN, volunteered to a party who may be compelled |
| Execute code on the device | a subscription is configuration data, never code; `00-DESIGN-BRIEF.md` 2.1 and the `subimport` source read it cites |

**The capability O keeps, stated plainly.** The signed catalog names rule-set and geo resources by path plus sha256 (`03-WIRE.md` 8.2). The hash binds the bytes to whatever the signer chose. O is the signer. O can therefore publish a rule-set that routes a named set of domains DIRECT, the hash matches, invariant 12 is satisfied, and the traffic leaves the device in cleartext while the tunnel shows connected. `01-DECISION.md` C3 asserts that hashing turns a compromised key from "can re-target among existing nodes" into being unable to do this; that attribution is wrong, and section 10 Correction 1 records why. Hashing bounds a third party in the path. It does not bound the party that signs.

> Because nothing in the format bounds it, the client MUST bound it. A change to the set of rule-set providers, to a resource hash, or to a route entry's `rs` list MUST be treated as a narrowing of the user's security posture and MUST raise the Keep or Revert card of invariant 22, in the same way a DNS repoint does. **Encoded**: `02-SPEC.md` 7.7 now carries all three as rows of its closed narrowing list, and 7.7.1 states what Keep and Revert each do to the resource set.

### 2.3 The compromised online signing key (K-online)

**Who.** An attacker holding the online signing key, typically by holding the panel host. Assume he also holds the database and can therefore answer any request the panel answers. He does not hold the root private key.

**What K-online can do.**

1. Sign catalogs, catalog chunks and directives, which is roles `online` per `03-WIRE.md` 7.1.
2. Re-target a device among catalogs whose hashes the root has already published.
3. Set every operator-controlled dial in the directive and the catalog: `exph`, `ttl`, `jit`, `pb`, `thr`, `lad`, `st`, `rc`, `sel`, `pol`, `mir`, `doh`, `pin`.
4. Suppress refreshes and serve a stale but unexpired key document, which is the withholding half of `01-DECISION.md` A2.
5. Read the panel database, and therefore every subscription uuid, which is every tunnel credential (asset 1).

**What K-online cannot do, and the exact step that stops him.**

| Attempt | Stopped at | Code |
|---|---|---|
| Mint a key document naming his own key under role `root` | V1 resolves the required role as `root` from `doc_type` `0x01`; V3 reads `roles[1].ks` from the **previously trusted** document; V4 finds his `keyid_trunc` is not in it | `E_VERIFY_UNAUTHORIZED` |
| Add a second slot to fake a threshold | V4 again, and `03-WIRE.md` 1.4: an unauthorized slot rejects the whole frame, it is never skipped | `E_VERIFY_UNAUTHORIZED` |
| Skip to key document version N+5 to outrun a revocation | V10, the rotation rule: `ver != N+1` is a rotation failure, not a version failure | `E_VERIFY_ROTATION` |
| Sign a catalog he invented, for a tier whose hash the root published | V14b: `sha256(frame)` must equal the tier hash in the trusted key document | `E_VERIFY_CATHASH` |
| Roll a device back to a superseded directive | V9 | `E_VERIFY_VERSION` |
| Read a directive he did not mint | HPKE seal; he can mint new ones, he cannot open recorded ones | `E_SEAL_OPEN` |
| Reuse a directive across devices | `dtp` in the payload and in the HPKE `aad` (`03-WIRE.md` 9.2) | `E_VERIFY_DEVICE` |

**The condition on the strongest of those bounds.** V14b is what stops K-online inventing nodes, and it is conditional on the key document actually carrying a `tiers` entry for the tier in question. `tiers` is optional to **decode** (`03-WIRE.md` 8.1, key 13), so a verifier tolerates its absence rather than refusing every catalog on that tenant. If the operator never publishes tier hashes, V14b has nothing to compare against and K-online can mint an arbitrary catalog: new exits pointing at hosts he controls, a rule-set that routes chosen domains DIRECT, a mirror pool of his own, a `pin` list of his own SPKI hashes. Publishing tier hashes costs the operator a root signature every time the fleet changes, which is exactly the offline ceremony `01-DECISION.md` A1 predicts will not be performed.

> **Resolved since this document was first written.** The panel obligation is now unconditional and the decode tolerance is now explicitly a compatibility affordance rather than a licence: `02-SPEC.md` 4.3 requires a conforming panel to publish a `tiers` entry for every tier it serves and to re-sign the key document whenever a served tier's `chash` changes; `03-WIRE.md` 6.2 splits V14 into V14a, the unconditional `cat` equality, and V14b, the conditional tier-hash equality; and a client whose trusted key document carries no `tiers` entry for its directive's tier MUST render the reduced containment in the verification chrome required by invariant 19, using the exact string **`fleet not root-anchored`**, and MUST NOT present the fleet as verified. `02-SPEC.md` 8.8.2 carries the string. This remains the single highest-value operator action in the protocol, and `02-SPEC.md` 4.3 also states what publishing it costs: a fleet change now waits for a root signature before directives may move to the new catalog.

**What K-online keeps, even with `tiers` published.** Every dial in section 4. He can set `exph` to 0, `ttl` to its floor of 300 with `jit` 0, `pb` to `[0, 0]`, and `thr.conn_bytes` to a value that guarantees the connection is frozen by TSPU. None of these is bounded by any signature check, because a signature is exactly what he has. Section 4 is the answer.

### 2.4 The compromised or lost root key (K-root)

**Who.** Two different adversaries with the same name, and they need separating because their blast radii are opposites.

**Compromised root.** The attacker holds the root private key, and per `03-WIRE.md` 7.3 he holds the old key set, so he can mint a key document at exactly N+1 signed under both the old and the new root sets, meeting the threshold under both. He becomes the operator for every client that has pinned this `pid`. He installs his own online keys, publishes his own `tiers`, mints catalogs and directives, and every check in `03-WIRE.md` section 6 passes. There is no in-band recovery: the client's high-water mark has advanced past the operator's last legitimate version, and `01-DECISION.md` 5.1.3 names this as the exact scenario the role-authorization rule exists to prevent when only the online key is lost. When the root itself is lost, the rule has nothing left to enforce.

What he still cannot do:

- Decrypt directives already recorded. The HPKE recipient is the device, and its private key is non-exportable on the hardware tier. He can mint new sealed directives to any device whose recipient public key he holds, which as the panel he does.
- Recover a user's `split.apps` list, which never left the device.
- Make the client open a URL, persist an opaque string, or execute code. The store-facing invariants are client-side and survive a total key compromise. This is not a small point: it is why invariants 10, 11 and 15 are written as client refusals rather than as operator obligations.

**Lost root, not compromised.** No new key documents can ever be signed. Existing clients keep working from their trusted key document. The key document has a 7 day lifetime (`03-WIRE.md` 8.0), and what happens on day 8 is not currently specified: V12 applies to the document under verification, not to the anchor, and no step says whether an expired trusted key document is still a valid source of key sets.

> An expired trusted key document MUST remain usable as an authorization anchor for `roles` and `thr` lookups at V3. The alternative makes the 7 day expiry a fleet-wide kill switch that fires whenever the operator is unreachable for a week, which contradicts invariant 16 in spirit and would turn a lost key into an outage rather than a migration. The client MUST surface the anchor's age in the verification chrome (invariants 19 and 21) and MUST continue to enforce `rev` from that anchor. **Encoded**: `02-SPEC.md` 2.2 and 5.2, and `03-WIRE.md` 6.5 for a reader who has only that document.

Recovery from a lost root is out of band and per device. `pid` is `sha256(root_pk)[0..8]`, so a new root key is a new tenant identity: every device re-enrolls against a new `pid` with a new `link_pin`. At the current population of roughly twenty paying users that is a phone call each. At licensee scale it is not a plan. The only in-band lane is the optional Webq Pro countersignature, which is off by default and non-load-bearing by design (`01-DECISION.md` 4.11).

### 2.5 The enrolled adversary (E)

**Who.** Someone who bought a subscription. RKN, a competitor, or a researcher. He is authenticated, he is inside every authorization check, and his cost of entry is the subscription price.

**What E can do.**

1. Enumerate his tier's entire exit fleet. The catalog is per tier and byte-identical for every subscriber on that tier (`01-DECISION.md` 5.2.1). One purchase buys one tier's node list: hostnames, ports, protocols, Reality public keys and short ids.
2. Enumerate his cohort's mirror pool, plus the reserve pool at `/sub/r1/{loc}` since he holds a locator.
3. Repeat with more purchases to widen coverage.
4. Forge his own apparent source IP to the panel. `extract_client_ip` reads `cf-connecting-ip` then `x-forwarded-for` with no trusted-proxy check and no comparison against the peer address (`apps/caramba-panel/src/subscription.rs:76-87`). The same pattern appears in `apps/caramba-panel/src/api/v2/app_auth.rs:253-262` and `apps/caramba-panel/src/api/client.rs:404-414`.
5. Consequently: defeat the device limit, which counts distinct `last_ip` values in a 15 minute window (`apps/caramba-panel/src/services/subscription_service.rs:1469-1490`, consumed at `subscription.rs:203-262`); choose his own apparent country, since `client_cc` comes from the client-supplied `x-country-code` or `cf-ipcountry` headers before any GeoIP lookup (`subscription.rs:146-153`); and defeat the login rate limiter, which is keyed on the same forgeable value and additionally fails open on a Redis error (`app_auth.rs:271-288`).

**What E cannot do.**

| Cannot | Bounded by |
|---|---|
| Read another subscriber's directive | HPKE seal to a device thumbprint he does not hold |
| Mint or alter any signed document | he holds no role key |
| Enumerate a cohort he was not assigned to, without buying again | per-cohort mirror subsets (`01-DECISION.md` D7), reserve pool held out of the catalog (`03-WIRE.md` 8.6) |
| Enumerate the fleet anonymously | the catalog is authorized, not public; `01-DECISION.md` 4.6 rejected the public catalog and `03-WIRE.md` 13.2 admits chunk fetches only against `X-CSM-Loc` |
| Enumerate at zero marginal cost | locator-scoped rate limiting that fails **closed** on a Redis error (`03-WIRE.md` 13.3), deliberately opposite to every existing limiter in the panel |

**The arithmetic of cohorting, since it decides whether D7 is real.** Node subsets are expressible only per tier: the catalog carries a single `tier` field (`03-WIRE.md` 8.2, key 10), its hash is published in the key document's `tiers` map, and that map is capped at 16 pairs (`03-WIRE.md` 8.1, key 13). A node cohort therefore **is** a tier, and there are at most 16 of them. Under uniform assignment, the expected number of purchases needed to see all 16 is `16 * H(16)`, about 54. Mirror cohorts are independent of tiers because the reserve pool carries its own `coh` (`03-WIRE.md` 8.6, key 12), so mirror enumeration and node enumeration have different prices.

Cohorting only hides an individual when a cohort is large. A cohort of one is a beacon: an operator who assigns a distinct mirror set per subscriber has built the per-user equivocation channel that `01-DECISION.md` A11 accepts as undetectable, and pointed it at his own users.

> A cohort SHOULD contain at least 25 subscribers. Cohorting therefore begins to pay above roughly 200 subscribers with 8 cohorts, and below that the operator SHOULD run one cohort and rotate it, rather than manufacture a per-user fingerprint. The 25 figure is provisional; the measurement that replaces it is the observed mirror burn rate against the rotation cadence, which is the same measurement `01-DECISION.md` A7 names as its own trigger. **Encoded**: `02-SPEC.md` 8.1.1, together with the 16-cohort ceiling.

### 2.6 The hostile mirror or CDN (M)

**Who.** A host in the signed mirror pool, or a CDN in front of one, that is hostile, compelled, or simply logging. The design deliberately puts documents on infrastructure the operator does not control, so this adversary is created on purpose and must be priced.

**What M can do.**

1. Serve nothing, serve garbage, serve stale bytes. All three are handled: a parse failure means the rung returned nothing and the ladder advances (`03-WIRE.md` 6.1), and staleness is bounded by V9 and V11.
2. Learn the tuple `(locator, device thumbprint, source IP, timestamp)` on every directive fetch, because the locator is in the path and `d=` is in the query (`03-WIRE.md` 13.2), and `(locator, source IP, timestamp)` on every catalog chunk fetch, because admission is by `X-CSM-Loc` (`03-WIRE.md` 13.2). The locator is stable until `gen` is incremented, so this is a durable per-subscription identifier, and `dtp` is a durable per-device one.
3. Correlate a subscriber across IP changes, and therefore across the mobile and home-broadband networks the same person uses.

**What M cannot do.**

| Cannot | Bounded by |
|---|---|
| Read a directive | HPKE seal; the mirror holds a ciphertext and a thumbprint (`01-DECISION.md` BC2) |
| Learn the tunnel credential from a CSM document | the subscription uuid MUST NOT appear in any signed document (`03-WIRE.md` 14.5) |
| Alter a document | signature over the transmitted bytes, verified before use |
| Downgrade a client to unverified legacy mode by omitting a field | invariant 13: a missing capability field on a profile that has pinned a root key is a hard, non-dismissible error |
| Substitute a rule-set or geo file it hosts | sha256 in the signed catalog, invariant 12 |
| Serve a different catalog to different subscribers | content addressing plus the root-published tier hash, V14b, conditional on `tiers` being published (2.3) |

**Correction to the claim that a mirror learns only an IP and a timestamp.** `01-DECISION.md` 5.2.4 says exactly that of the catalog. It was true while the catalog was public. Making the catalog authorized (which was the right call, `01-DECISION.md` 4.6) means every catalog fetch now carries the locator, so a mirror learns a stable subscription identifier as well. Section 10 Correction 2. Sealing protects contents; it does not protect addressing.

**What M keeps, and it is the worst item in this document.** As long as the client still fetches `/sub/{uuid}` for its tunnel configuration, any host that observes that URL holds a bearer credential that is simultaneously a **write** primitive. See section 7.4.

### 2.7 The malicious co-tenant (T)

**Who.** Two distinct things share this name and both are real.

**T1: another operator's profile on the same device.** The client already supports more than one, keyed by panel URL (`apps/caramba-client/lib/features/enroll/enroll_controller.dart:312-318`).

What the protocol bounds: a directive signed by operator A is rejected by a profile pinned to operator B, because `pid` is checked byte for byte at V8 against the pinned value and the key sets are per `pid`. `01-DECISION.md` 5.1.1.

What the protocol does not bound, verified in the client:

- `TokenStore` is global and not tenant-scoped. Its keys are the fixed strings `caramba.access_token`, `caramba.refresh_token` and `caramba.user_id`, one value each (`apps/caramba-client/lib/data/token_store.dart:9-11`). There is exactly one session on the device at a time, and enrolling into a second operator overwrites the first operator's session.
- The `ApiClient` request interceptor attaches `Authorization: Bearer <access>` from that global store to any request that does not set `skipAuth`, regardless of which origin the client was constructed for (`apps/caramba-client/lib/data/api_client.dart:76-84`), and `enrollApiClientProvider` constructs exactly such a client against a panel URL taken from an enrollment link (`enroll_controller.dart:312-318`). The public enrollment calls do set `skipAuth`, so the current flow is not itself the bug; the shape is. Any protected call issued on a panel-scoped client while another operator's token is in the store sends that token to the other operator's origin.
- `ConnectionProfilesStore` keeps every profile in one JSON blob under one secure-storage key, `caramba.connection_profiles` (`apps/caramba-client/lib/data/connection_profiles_store.dart:14`), and each profile carries its panel URL, its access token and its subscription uuid (`enroll_controller.dart:264-285`). The subscription uuid is the tunnel credential. One blob therefore holds every enrolled operator's tunnel credential.

> Tenant isolation in CSM/1 is a property of **documents**, not of sessions or of storage. The token store and the profile store MUST be keyed by `pid`, and an `ApiClient` MUST refuse to attach a bearer token whose issuing `pid` does not match the origin it is about to contact. **Encoded**: `02-SPEC.md` 1.2, which adds a third store the original hand-off missed, the settings state with its provenance, its user-set marks, its cards, its write queue and its first-seen sunsets. `CoreConfig` is one JSON blob under one shared-preferences key today (`apps/caramba-client/lib/data/prefs_store.dart:22`, `lib/state/core_config_state.dart:21-56`), so a directive from tenant A that writes `pol[8]` or `pol[9]` changes the configuration tenant B's tunnel uses, which is the same defect one layer over from the token store.

**T2: a co-tenant of the protocol itself.** Every licensed operator ships the same client binary, the same frame magic, the same five-header response stamp from the panel's root router (`main.rs:1630-1651`), and the same padding grid. That uniformity is `00-DESIGN-BRIEF.md` R16: once a circumvention method is uniform, visible and widely reused, it gets targeted, and one apex-scoped or fingerprint-scoped rule then hits every tenant at once. The per-tenant diversity that answers it is entirely in signed data: `lad` for rung order, `pb` for the padding range, `mir` for the pool, `pin` for the pins (`01-DECISION.md` D3). None of it is in app logic, which is what keeps it store-legal (`01-DECISION.md` 4.3).

The residual, stated: a tenant that leaves `lad`, `pb` and `mir` at their defaults inherits the cross-tenant constant it was given the fields to avoid. The panel SHOULD refuse to sign a catalog whose `pb` equals `[0, 0]` and whose `mir` is empty at the same time, since that combination is the fully uniform configuration.

### 2.8 Adversary and asset matrix

`X` means the adversary reaches the asset today. `-` means a named mechanism stops him. `C` means conditional, with the condition named in the row.

| Asset | N | O | K-online | K-root | E | M | T1 |
|---|---|---|---|---|---|---|---|
| 1 Tunnel credential | X (legacy config body, TLS only) | X (holds the database) | X (holds the database) | X | own only | X (see 7.4) | X (one storage blob) |
| 2 User traffic | - (tunnel) | X (exit node) | C: rule-set repoint unless `tiers` published and the Keep card fires | X | - | - | - |
| 3 Identity and location | X (source IP, request line) | X | X | X | - | X (loc, dtp, IP) | - |
| 4 Configuration integrity | - (signature) | X (is the signer) | C: bounded by V14b when `tiers` is published | X | - | - | - |
| 5 Root private key | - | operator-held | - | is the asset | - | - | - |
| 6 Fleet list | X (already blocks nodes) | X | X | X | X (one tier per purchase) | - | - |
| 7 Mirror pool | C: burns what it sees | X | X | X | X (own cohort) | X (own identity) | - |
| 8 Availability | X (blocking is unanswerable) | X | X | X | - | X (one rung of several) | - |
| 9 Account session | X (TLS only) | X | X | X | own only | - | X (global token store) |

---

## 3. The compromise ladder

### 3.1 Key and secret inventory

| Tier | Secret | Held by | Signs or protects | Rotation mechanism |
|---|---|---|---|---|
| 0 | Device signing key, P-256 | the device, non-exportable on the hardware tier | write proofs (`X-CSM-Proof`), the HPKE rekey message | re-enrollment |
| 0 | Device agreement key, P-256 | the device, same store | receives the HPKE seal, generation named by `rkv` | device-signed rekey, from v1, no operator action (`03-WIRE.md` 9.5) |
| 1 | Locator HMAC secret | the panel | derives `loc` for every subscription | per-subscription `gen` column increment, plus a panel-wide epoch as an emergency lever (`01-DECISION.md` 5.5.3) |
| 2 | Online signing key, Ed25519 | the panel, its own secret, never `SESSION_SECRET` (`01-DECISION.md` P2) | catalogs, chunks, directives | key document at N+1 listing a new `online` key and the old one in `rev` |
| 3 | Root signing key, Ed25519 | the operator, offline, BIP39 mnemonic, printed once | key documents, bootstrap blobs, reserve pools | TUF rotation, exactly N+1, dual-signed (`03-WIRE.md` 7.3) |
| X | Subscription uuid | the panel database and every client that has ever fetched a config | the tunnel itself | none; see section 5.3 |
| X | `APP_JWT_SECRET` | the panel environment | every account session; falls back to `SESSION_SECRET` (`app_auth.rs:69-76`) | none; no `kid`, no rotation path |

Tiers 0 through 3 are the CSM/1 ladder. The two rows marked `X` are not part of it and are the reason section 5 exists.

### 3.2 Blast radius and recovery, per tier

**Tier 0, device keys.**
Blast radius: one device. The attacker can open that device's sealed directives from the moment he holds the agreement key, and can sign preference writes as that device. He cannot decrypt directives sealed to a generation he does not hold. He cannot move to a sibling device.
Detection: none automatic. A directive fetched by the attacker consumes a nonce the real device did not send, so the real device's next fetch simply fails V13 once and retries; that is not a reliable signal.
Recovery: the user revokes the device (`DELETE /api/v2/app/devices/{id}`, registered at `apps/caramba-panel/src/api/v2/mod.rs:165-169`, handled beside the device list at `apps/caramba-panel/src/api/v2/app_account.rs:90-255`, which is the same surface `02-SPEC.md` 9.6 requires CSM registrations to appear in) and re-enrolls. The next directive carries `st` 5 with `rc` 4002 or 4003 (`03-WIRE.md` section 5).
What recovery does not do: it does not rotate the tunnel credential, because the tunnel credential is per subscription and not per device (section 5.3). A device revoked in the panel keeps working as a tunnel client until the subscription uuid itself is rotated, which today rotates the credential for every other device at the same time.

**Tier 1, locator secret.**
Blast radius: a leaked single locator exposes one subscription's directive and reserve-pool URLs to whoever holds it, which is a request-rate and metadata exposure, not a content exposure, because the directive is sealed to a device thumbprint the holder does not have. A compromise of the HMAC secret itself exposes every subscription's locator, since `loc` is derived from `(secret, subscription_uuid, gen)`.
Detection: locator-scoped rate limiting keyed by locator and by source IP, failing closed (`03-WIRE.md` 13.3), makes bulk locator use visible as 429s in the panel rather than as free enumeration.
Recovery: one leaked locator is one `UPDATE` of that subscription's `gen`. A compromised secret is a panel-wide epoch bump. The per-subscription generation counter exists precisely so that the common case is not the fleet-wide case (`01-DECISION.md` 5.5.3).

**Tier 2, online signing key. This is the tier that will actually be compromised.**
Blast radius, with `tiers` published in the key document: the attacker re-targets devices among already-published catalogs, sets every dial in section 4, and denies service. He cannot invent a node, a mirror, a pin or a rule-set, because any catalog he mints fails V14.
Blast radius, with `tiers` absent: total control of configuration. New exits under his control, new mirrors, new pins, rule-sets that route chosen domains DIRECT. The tunnel still shows connected.
Detection: directive version regression is caught at V9, which is inert for catalogs (2.1); equivocation is not caught at all (`01-DECISION.md` A11); a minted catalog is caught at V14b only under the condition above.
Recovery: sign a key document at N+1 with the root key, listing the compromised `keyid_trunc` in `rev.kids` and a fresh online key in `keys` and `roles[2].ks`. Clients that see the `kid` in `rev` MUST reject that key and every document it signed, including documents already cached on disk (`03-WIRE.md` 8.1).
Propagation: normal case one refresh, bounded by `ttlk` at 300 to 86400 seconds and by the 300 second key-document cache (`03-WIRE.md` 13.2). Worst case seven days, the key-document lifetime, against an adversary who can withhold. That is `01-DECISION.md` A2 and its trigger is in section 9.

**Tier 3, root key.** Analysed in 2.4. Compromise is unrecoverable in band. Loss is recoverable only by re-enrolling every device against a new `pid`.

### 3.3 Propagation and revocation timing

| Event | Best case | Normal case | Worst case | Bound |
|---|---|---|---|---|
| Online key revoked | 300 s | one `ttlk` period, 300 to 86400 s | 604800 s | key document lifetime; A2 |
| Node revoked | immediate on the next document read | one directive `ttl`, default 7200 s | 604800 s | `rev.nodes` is honored against the **cached** catalog, so a seized node is dropped even offline (`03-WIRE.md` 8.1) |
| Device revoked, control plane | one directive `ttl` | 7200 s | `exph` | the offline grace window is an operator dial with its consequence printed beside it (`01-DECISION.md` C5) |
| Device revoked, tunnel | never | never | never | the tunnel credential is per subscription; section 5.3 |
| Root rotated | one key-document fetch | 300 s to `ttlk` | 604800 s | clients MUST NOT skip a version, so a device offline across two rotations walks them in one request via `?since=N` (`03-WIRE.md` 13.2) |

The 7 day worst case is what `01-DECISION.md` A2 reserves `doc_type` `0x07` for: a roughly 60 byte root-signed timestamp document carrying the current key-document version and hash, signable in bulk by a cron on the root-holding machine, which would cut the window to 24 hours. It is reserved in the registry (`03-WIRE.md` 1.2) and MUST NOT be emitted in v1.

### 3.4 What revocation costs the user

Revocation is not free and the cost is asymmetric in a way the operator must understand before pulling the lever.

Revoking an online key invalidates every document that key signed, including the cached catalog on disk. The client is then holding no valid catalog and cannot build a configuration until it fetches a new one signed by the replacement key. Invariant 16 protects an **expired** document, not a **revoked** one, and it should not: a revoked signer is a security event and stale configuration from a revoked signer is exactly what the mechanism exists to remove. The consequence, stated so nobody is surprised during an incident: an already-established tunnel keeps running because the engine already holds its configuration, but a client that reconnects after a revocation and cannot reach any rung has nothing to connect with. In a blackout, revoking the online key can take users offline more effectively than the adversary was managing.

> The client MUST render this state explicitly under invariant 21 (configuration age and source), naming it as **your provider replaced its signing key and this device has not yet received the new configuration**, and MUST offer the R6 out-of-band rung as the immediate remedy. **Encoded**: `02-SPEC.md` 10.4 and 8.8.2.
>
> Two things this document previously left ambiguous are now settled in `02-SPEC.md` and are worth restating here, because an incident is where they will be read. A revoked `online` key does **not** send the profile to the terminal `compromised` state: only a root pin mismatch or the revocation of every key in `roles[1]` does (`02-SPEC.md` 2.1 rule 5). And a running tunnel is **not** torn down by a revocation of the signing key; the engine already holds its configuration, and the only signed value that tears a tunnel down is `st = 5` (`02-SPEC.md` 4.6.1).

---

## 4. Bounds the client MUST place on signed data

Every field below is chosen by whoever holds the online signing key. A signature check does not evaluate it. These are the clamps that stand between a compromised signer and a client that harms itself on command.

| Field | Where | Hostile value | Harm | Required client bound |
|---|---|---|---|---|
| `thr.conn_bytes` | catalog, `03-WIRE.md` 8.2 | 65535 | the client pushes past the TSPU freeze point on one connection and hangs with no RST | MUST clamp to at most `CONN_BYTES_CEILING = 15360`; the signed value applies only when it is lower |
| `thr.conn_packets` | catalog | 255 | same, on the packet trigger | MUST clamp to at most `CONN_PACKETS_CEILING = 25` |
| `thr.resp_max` | catalog | 49152 | defeats invariant 5 and the chunking arithmetic of `03-WIRE.md` 11.3 | MUST reject a catalog whose `thr.resp_max` exceeds 4096; invariant 5 fixes the value, the field only allows lowering it |
| `exph` | directive, `03-WIRE.md` 8.3 | 0 | every device that fetches one poisoned directive stops offering to connect at all, and the value persists as the last accepted one | MUST NOT apply an `exph` below `EXPH_FLOOR = 86400` seconds unless the user set a shorter window explicitly in Settings |
| `ttl` | catalog and directive | 300 with `jit` 0 | 288 fetches a day on a fixed period, undoing the frequency win of `01-DECISION.md` 5.7.5 and manufacturing a flow-classifier feature | MUST enforce a floor of `TTL_FLOOR = 900` seconds on the client side, and MUST apply at least 10 percent jitter regardless of `jit` |
| `pb` | catalog | `[0, 0]` | a per-tenant constant response size, which is the cross-tenant constant D3 objects to | SHOULD warn in the diagnostics screen; the panel SHOULD refuse to sign `pb == [0,0]` with an empty `mir` |
| `lad.en` | catalog | `[0, 6]` only | the operator disables every network rung by default and the user never notices | the user's own toggles always win, and a rung the user enabled is never removed by signed data; `lad` sets defaults for a fresh profile only (`01-DECISION.md` 5.3.1) |
| `mir`, `doh`, `pin` | catalog | attacker-controlled hosts | the ladder walks into the attacker's infrastructure | already bounded: hosts must satisfy `03-WIRE.md` 14.1, paths resolve only against the pinned origin or a signed mirror, and `doh.h` must also appear in `mir` |
| `rs`, `geo` | catalog | a rule-set routing chosen domains DIRECT | cleartext egress with the tunnel showing connected | not bounded by any format rule; MUST raise the Keep or Revert card per 2.2 |
| `ann`, `sup`, `ui.t`, `nm`, `pn` | directive and catalog | a URL, a phishing string, an identifier | store exposure and user harm | already bounded: inert, capped, URL-stripped, never opened, never persisted (`03-WIRE.md` 14.6) |

**All five clamps are encoded in `02-SPEC.md` 8.6.1 and listed in its section 14.** Until they were, `02-SPEC.md` 8.6 carried the opposite MUST, "a client MUST NOT use a locally compiled value in preference to a signed one", and steps 8 of both walkthroughs in sections 7.2 and 7.3 depended on a mitigation no implementer would have built. The corrected form of that sentence is that a signed value binds when it is at or below the compiled ceiling, and the ceiling binds where it is above.

`CONN_BYTES_CEILING` at 15360 and `CONN_PACKETS_CEILING` at 25 are provisional and are the lower edge of the observed freeze trigger reported in `00-DESIGN-BRIEF.md` 2.2 (roughly 25 packets or 15 to 20 KB). They change on the same measurement that governs the four provisional constants in `03-WIRE.md` 17: the real TLS handshake byte and data-packet cost against the tenant's certificate chain, and the real freeze point, both from a Russian vantage point on mobile and home broadband.

`EXPH_FLOOR` at 86400 and `TTL_FLOOR` at 900 are policy, not measurement: they are the values at which a hostile directive cannot deny service faster than a user notices, and at which the fetch cadence stays inside the privacy claim of `01-DECISION.md` 5.7.5. **`TTL_FLOOR` is 900 rather than the 1800 this table first carried**, and the reason is operational rather than adversarial: `06-MIGRATION.md` 4.3 instructs an operator anticipating a risky cutover to lower `ttl` to 900 for the cutover window, which is what puts the kill-switch adoption bound at 1140 seconds, and a client-side floor of 1800 would make that acceleration silently ineffective. The kill switch is the only rollback that exists after PNR-2, so a floor that quietly halves its speed is a worse outcome than the 96 fetches a day the lower floor admits in the worst case. The `ttl` wire range stays `300..86400` (`03-WIRE.md` 8.2 key 19); a value below 900 is legal to encode and the client polls at 900.

The general rule, which is the one to remember if the table is ever out of date:

> A signed field that can only make the client's situation worse MUST be clamped at the client. A signed field that can only make it better MAY be honored unclamped. Where a field can do both, the client honors the safer of the signed value and its own ceiling.

---

## 5. The device-binding boundary

### 5.1 What `dtp` binds

`dtp` is `sha256(device_signing_SPKI_DER)[0..16]`, 128 bits (`03-WIRE.md` section 4). It binds exactly three things:

1. **Which device a directive can be opened by.** `dtp` is in the sealed outer payload and inside the HPKE `aad` (`03-WIRE.md` 9.2), so a directive resealed to another device fails `E_SEAL_OPEN`, and a directive relabelled for another device fails `E_VERIFY_DEVICE` at V13.
2. **Who may write preferences.** `X-CSM-Proof` is an ECDSA P-256 signature by the device signing key over `sha256("csm1-write" || 0x00 || method || 0x00 || path || 0x00 || sha256(body))` (`03-WIRE.md` 13.6). The proof covers the body, which is the defect `01-DECISION.md` B5 exists to avoid.
3. **What counts as a device, after P7.** The manifest path MUST NOT count devices at all (`03-WIRE.md` 13.3), and the config path must count by thumbprint rather than by apparent IP. `00-DESIGN-BRIEF.md` R9 is retired only when both halves land.

### 5.2 What it does not reach

**1. The tunnel credential.** Section 5.3 in full. This is the largest hole and it is scheduled, not fixed.

**2. The account session.** A device key is not a session credential. The account JWT is the enrollment authority for the second and subsequent devices (`01-DECISION.md` 5.5.5), so whoever holds the account holds device enrollment. The session layer is 15 minute unrevocable access tokens (`app_auth.rs:32`), 30 day refresh tokens with no reuse detection (`:33`), no logout-all despite the index existing, and one symmetric secret with no `kid` that falls back to `SESSION_SECRET` (`:69-76`). `01-DECISION.md` A9 accepts this; the constraint that must hold is that the CSM online signing key is never stored under `SESSION_SECRET`, because a single symmetric leak would then be simultaneously a session-forgery event and a config-signing event with A2's seven day window attached.

**3. Device counting on the legacy path, today.** The limit counts distinct `last_ip` values in a 15 minute window (`services/subscription_service.rs:1469-1490`) against an IP taken from `cf-connecting-ip` or `x-forwarded-for` with no trusted-proxy check (`subscription.rs:76-87`). It is a usability control, not a security control, and this document does not credit it as one. It also actively punishes the ladder: every rung with a different egress burns a slot, and under fetch-through-tunnel the apparent IP is the exit node's, shared by every user of that node.

**4. The hardware tier.** Section 5.4.

**5. Cross-device settings.** Exit, relay, routing preset, protocol and variant propagate across a user's devices by design, because that is the product goal (`01-DECISION.md` 5.4.4). A compromised device can therefore move a sibling's exit selection. What it cannot do silently is narrow the sibling's security posture: a cross-device write MUST NOT set `killSwitch`, `dns`, `split.mode` or the enabled transport set on a sibling without that sibling raising the Keep or Revert card unconditionally (invariant 22, `03-WIRE.md` 13.6).

**6. The app process boundary.** On iOS the Network Extension builds its own Go core with work directory `caramba/` (`apps/caramba-client/packages/caramba_vpn/darwin/Extension/PacketTunnelProvider.swift:138-166`) while the plugin builds a second one with work directory `caramba-tools/` (`.../Classes/CarambaVpnPlugin.swift:310-322`). Two work directories are two monotonic high-water-mark stores, which is a rollback hole and not defence in depth. `01-DECISION.md` X3 requires one fetcher, one verifier and one monotonic store, in the app process, with the extension handed a rendered configuration and a validity window. Until that lands, V9 is enforced twice against two different values, and an adversary who can steer which process fetches can pick the weaker one.

### 5.3 The shared tunnel credential, in full

This is one value with five jobs.

```
subscription_user_uuid(sub) = sub.vless_uuid, else sub.subscription_uuid
                              services/subscription_service.rs:191-205
```

| Consumer | Uses it as | Anchor |
|---|---|---|
| VLESS | the `uuid` field of the outbound | `singbox/subscription_generator.rs:229` |
| Trojan | the `password` field | `:332` |
| TUIC | the `uuid` field | `:391` |
| Hysteria2 | the second half of the password, as `"{tg_id}:{uuid_without_hyphens}"` | `services/subscription_service.rs:1908` |
| AmneziaWG | the seed of the derived private key | `services/subscription_service.rs:1909, 1918-1920` |

Five consequences follow, and every one of them is outside what a device key can fix.

1. **It is per subscription, not per device.** Every device on the subscription presents the same credential to the exit. The exit cannot tell them apart, so revoking a device in the panel does not revoke tunnel access.
2. **Anyone who has ever held a config body holds it.** That includes every legacy client the user pasted the URL into, `caramba-sub`, any CDN in front of it, and any adversary who terminated TLS on that fetch.
3. **Rotating it rotates it for everyone.** There is no per-device rotation, so the honest statement in `01-DECISION.md` 5.5.4 stands: until the decoupling lands, the client MUST NOT present any control that claims to rotate access when it only rotates a link.
4. **It carries the Telegram id.** The Hysteria2 password is `tg_id` concatenated with the uuid. `01-DECISION.md` 5.7.1 forbids a Telegram id in a signed document and the panel unit test enforces that, and both remain true; the identifier travels in the unsigned config body instead. Anyone holding a Hysteria2 config for a subscriber learns which Telegram account it belongs to, and `tg_id` is low entropy and directly resolvable to a person. This is section 10 Correction 3.
5. **CSM/1 keeps it out of signed documents and out of mirrors,** which is exactly what BC2 bought and it is worth having: the directive can ride a CDN, an onion front or a third-party mirror because the host holds only a ciphertext and a thumbprint. It does not remove the credential from the legacy path, which the same client still uses.

> Until the decoupling lands, the device-binding boundary is: **CSM/1 binds the control plane to a device; the data plane is bound to a subscription.** No sentence in any specification should imply otherwise.

### 5.4 The hardware tier boundary

The device keypair is P-256, generated non-exportable in Secure Enclave or StrongBox where available (`01-DECISION.md` 5.5.1). Below Android 12 the hardware path does not exist, because `PURPOSE_AGREE_KEY` is API 31, and the same constraint is what forces the HPKE KEM to be `DHKEM(P-256, HKDF-SHA256)` rather than the brief's X25519 (`03-WIRE.md` 9.1 and Correction 4 there).

On the software tier the private key is a file. An adversary with root on the device exports it, and from then on can open that device's sealed directives and sign its write proofs from anywhere. The protocol's answer is disclosure, not prevention: the tier is recorded in the enrollment, visible to the user, and visible to the operator (`01-DECISION.md` B4). That is the correct answer, and it must not be quietly dropped as a UI detail, because it is the only thing standing between a user and a false belief about what their device binding means.

---

## 6. Privacy ledger

### 6.1 What each party learns

| Party | Learns | From | Does not learn, and why |
|---|---|---|---|
| The enrolled operator's panel | source IP at each fetch and connect; account identity (email or Telegram id); device thumbprint and hardware tier; selected exit, relay, preset, protocol, variant; traffic counters; the highest directive version the device has accepted | inherent to running the service, plus `v=` (`03-WIRE.md` 13.2) | `split.apps`, invariant 15; which transport rung carried the request, `01-DECISION.md` 5.4.6; anything the closed vocabularies do not admit, invariant 11 |
| `caramba-sub` | the subscription uuid in the path; the real client IP, which it forwards as `X-Forwarded-For`; the User-Agent, which it forwards verbatim | `apps/caramba-sub/src/panel_client.rs:251-258` | nothing extra under CSM/1: it proxies frames it cannot read, and its config cache MUST NOT apply to CSM routes (`03-WIRE.md` 13.7 item 3) |
| A signed mirror or CDN | `(locator, device thumbprint, source IP, timestamp)` on a directive fetch; `(locator, source IP, timestamp)` on a chunk fetch; response sizes | locator in the path, `d=` in the query, `X-CSM-Loc` on chunks (`03-WIRE.md` 13.2) | directive contents, HPKE; the tunnel credential, `03-WIRE.md` 14.5; which tenant it is, only to the extent `pid` is inferable from the host it is serving |
| A DoH resolver named in the catalog | the hostnames the client resolves at bootstrap | R3 by construction | nothing else; and note `doh.h` must also be in `mir`, so it is operator-run or operator-fronted (`03-WIRE.md` 8.2), which is a deliberate refusal to hand the query stream to a domestic resolver (`01-DECISION.md` 4.5) |
| The exit node | all tunnel traffic; the tunnel credential, hence the subscription and, through the Hysteria2 password, the Telegram id | inherent, plus `subscription_service.rs:1908` | which device of the subscription is connected, because the credential is shared |
| The RU network observer | that a TLS connection was made to a host; SNI; sizes and timings; the full plaintext wherever a trusted CA is in play | 2.1 | the contents of any CSM/1 frame's signature-protected meaning is not secret, but a sealed directive's contents are; the padding grid coarsens sizes |
| The device's ISP resolver | the hostnames resolved outside R3 | bootstrap DNS; note `profile/profile.go:167-168` still hardcodes `https://1.1.1.1/dns-query`, `https://8.8.8.8/dns-query` and `tls://1.1.1.1:853`, all of which are blocked or being blocked in RU, and `01-DECISION.md` 5.3.8 moves them into the signed catalog | |
| Telegram | the blocked client IP, when a device-limit refusal fires | `apps/caramba-panel/src/subscription.rs:235-262` | this is a disclosure to a third party and is a licence and store obligation, not a protocol one |
| The app store | that the app exists, its declared data collection, and its review notes | store process | nothing at runtime |
| Webq Pro | nothing at runtime by default. The countersignature is optional, off by default, and non-load-bearing (`01-DECISION.md` 4.11) | | this is deliberate: a mandatory countersignature would make Webq Pro one compromise and one censorship target for every tenant at once |
| Another tenant on the same device | today, potentially the active session token and every profile's stored credentials (2.7) | `token_store.dart:9-11`, `connection_profiles_store.dart:14`, `api_client.dart:76-84` | signed documents, which are `pid`-scoped and rejected at V8 |

### 6.2 What no party learns

- The installed application list. It never leaves the device, in either direction, enforced by the client's own serializer rather than as a preference (invariant 15, `01-DECISION.md` 5.7.4).
- Which rungs of the ladder work, per device and per ASN. If rung telemetry ever ships it is opt-in, coarse-bucketed, aggregated over 24 hours, sent only over an established tunnel, and never on the same request that carries the device identity (`01-DECISION.md` 5.4.6).
- The contents of a directive, to anyone but the device it was sealed to.

### 6.3 Where the ledger is broken today

1. The Telegram id inside the Hysteria2 password (5.3, item 4).
2. The locator visible to every mirror (2.6, Correction 2).
3. The device-limit Telegram DM carrying the user's IP.
4. `caramba-sub` forwarding the real client IP to the panel with no padding and no jitter on the legacy path, which is `00-DESIGN-BRIEF.md` 4.7 verbatim and which CSM/1 improves only for the manifest path.

---

## 7. End-to-end attacks

Each walkthrough is a numbered sequence. The mechanism that stops the attacker is named at the step where it fires, with the verification step and the error code from `03-WIRE.md` sections 6.1 and 6.2. Where nothing stops him, the step says so.

### 7.1 A state CA plus inline DPI MITM on first enrollment

Adversary: N (2.1). Goal: become the operator for a new user, or failing that, take everything the user's first session exposes.

**Setup.** The user has installed the app. He has an enrollment artifact from somewhere. Three deliveries matter and they are not equivalent:

- (a) A `carambaconnect://enroll?panel=...&code=...&k=<link_pin>` link received over a channel N controls.
- (b) The same link received over a channel N does not control, or a printed bootstrap blob QR, or a dictated code with the pin folded in.
- (c) A pasted subscription URL, which is the secondary flow and carries no pin at all.

**Steps.**

1. The client normalizes the panel URL. `EnrollLink.normalizePanelUrl` accepts plain `http://` today (`apps/caramba-client/lib/data/models/enrollment.dart:60-71`, the scheme test at `:66-67` is `if (scheme != 'https' && scheme != 'http') return null;`). **Not stopped today.** After the change required by invariant 8 and `03-WIRE.md` 14.4, an `http://` panel URL is refused outright and N cannot downgrade the enrollment to cleartext.
2. The client fetches the key document from the panel origin. N terminates TLS with a certificate chaining to a root the device trusts, and serves a key document he minted with his own root key.
3. **Stopped, in case (b), at first trust.** `03-WIRE.md` 7.2: the first key document accepted for a `pid` MUST contain exactly one key under role `root` whose `sha256(pk)[0..12]` equals `link_pin`. N's key does not. The client refuses enrollment with a hard error and there is no continue-anyway affordance on any code-based path. Error: `E_VERIFY_NOANCHOR`.
4. **Not stopped, in case (a).** If the pin travelled the channel N controls, N rewrites the pin and the key document together and the equality holds. This is trust on first use and the specification says so plainly rather than dressing it up. The entire security of enrollment reduces to the integrity of the channel that carried 20 base32 characters. The manual-entry path therefore requires at least the first 40 bits, that is 8 characters, as a dictated field, because a phone call is a channel N does not control (`01-DECISION.md` 5.1.6, `03-WIRE.md` 7.2).
5. **Not stopped, in case (c).** A pasted subscription URL carries no pin, no `pid` and no root key. It is the legacy path and it gets legacy security, which is TLS. `01-DECISION.md` A5 keeps this flow secondary for store reasons; this document records that it is also the weakest one.
6. N falls back to replay. He recorded key document version 3 last month and has been suppressing version 4, which revoked a stolen online key. He serves version 3. **Partially stopped.** At first trust there is no high-water mark, so V9 cannot help, and `time_floor` is being established from this very document, so V11 cannot help. The only bound is V12: `now <= exp + 300`, and the key document lifetime is 604800 seconds. **A first-trust replay window of up to seven days exists and nothing narrows it in v1.** This is `01-DECISION.md` A2 seen from the enrollment side and it is the strongest argument for the reserved timestamp role, `doc_type` `0x07`.
7. N gives up on substitution and proxies the enrollment to the real panel, reading everything. He obtains the registration or login body, which contains the account password (`api/v2/mod.rs:145-147` places `/register` and `/login/email` in the public group with no application-layer protection), and the returned JWT pair. **Not stopped.** CSM/1 signs configuration; it does not protect the session layer (`01-DECISION.md` 5.5.6, A9).
8. With the JWT, N enrolls his own device against the victim's account, which is an act an authenticated account is designed to be able to perform on itself (`01-DECISION.md` 5.5.5). He now receives his own valid, sealed directives. **Working as designed, and it is the right design**: the alternative, B's device-key-only issuance, makes an ordinary reinstall a manual operator action per user in a market where the operator's support channel is blocked (`01-DECISION.md` 4.7).
9. N cannot open the victim's directives. Sealing is to the victim device's agreement key and `dtp` is inside the HPKE `aad` (`03-WIRE.md` 9.2). **Stopped**, `E_SEAL_OPEN`.
10. N reads the victim's legacy config fetch on `/sub/{uuid}` and extracts the tunnel credential from the first VLESS outbound. **Not stopped.** This is the single largest residual in the whole design and it is why section 5.3 exists.

**Score.** Steps 3 and 9 are hard stops. Step 6 is a seven day window nobody has closed. Steps 4, 5, 7, 8 and 10 are unstopped, and of those, 4 and 5 are inherent to trust on first use, 7 and 8 are the session layer that is explicitly out of scope, and 10 is scheduled work with a name (`01-DECISION.md` 5.5.4, second half).

### 7.2 A compromised online signing key attempting to escalate

Adversary: K-online (2.3). Goal: become root, and failing that, extract maximum harm from the role he has.

1. He mints a key document, `doc_type` `0x01`, `ver` = N+1, containing one key: his own, under role `root`, threshold 1. He signs it with the online key he holds. **Stopped at V4.** V1 resolves the required role as `root` from the `doc_type`, V3 reads `roles[1].ks` from the previously trusted key document, and V4 finds his `keyid_trunc` absent from it. `E_VERIFY_UNAUTHORIZED`. Note the ordering: this fires before V6, so the client never even evaluates his signature. `01-DECISION.md` 5.1.3 calls this the rule design A omitted, and it is the difference between a recoverable incident and a permanent takeover.
2. He tries to launder it by adding a second slot carrying a legitimate root `keyid_trunc` copied from the trusted document, with a garbage signature, hoping the verifier counts the one valid signature and skips the invalid slot. **Stopped at V4 and again at V6.** `03-WIRE.md` 1.4: a slot whose `keyid_trunc` is not in the authorized set rejects the whole frame, it is not skipped; and a frame carrying an unauthorized signature is a frame someone tried to launder. His own slot fails the set test.
3. He tries `nsigs = 3` with two copies of a legitimate root `keyid_trunc`, to reach a threshold of 2 with one stolen public key. **Stopped at P8**, before verification begins: slots MUST be in strictly ascending `keyid_trunc` order with no duplicates. `E_PARSE_SLOTORDER`.
4. He tries `nsigs = 4` with the frame truncated so only one slot is present, hoping a verifier reads past the buffer or counts declared rather than present slots. **Stopped at P7**, the exact-length rule: `total_len == 7 + payload_len + 1 + 76 * nsigs`, exactly. `E_PARSE_FRAMING`. This is graft C4 and it is checked before any signature work.
5. He jumps to `ver` = N+5, reasoning that a client which has been offline will not know what it missed. **Stopped at V10.** A key document must be exactly N+1 and a client MUST refuse to skip a version. `E_VERIFY_ROTATION`. The legitimate operator's answer to the same problem is `GET /sub/k1?since=N`, which returns the chain in one request (`03-WIRE.md` 13.2).
6. He abandons the root and works within `online`. He mints a catalog containing exits he controls, with his own `pin` entries so the client will accept his TLS. **Stopped at V14b, conditionally.** `sha256(frame)` must equal the tier hash published in the root-signed key document. If the operator publishes `tiers`, this fails, `E_VERIFY_CATHASH`, and this is the most valuable single check in the protocol. If the operator does not publish `tiers`, it succeeds and he owns the configuration. `02-SPEC.md` 4.3 now makes publishing it a panel MUST, which is the difference between this step being conditional in the format and conditional in practice. See 2.3.
7. He re-targets within published catalogs by signing a directive whose `sel.exit` names the node he prefers among the operator's real fleet. **Not stopped, and accepted:** this is `01-DECISION.md` A1 exactly, and the reason it is accepted is that full Uptane containment requires an offline signing ritual for every node addition that a one-person operator will not perform.
8. He sets `exph` to 0, `ttl` to 300 with `jit` 0, `pb` to `[0, 0]`, and `thr.conn_bytes` to 65535, denying service, manufacturing a beacon, flattening the padding, and steering the client into the TSPU freeze window. **Stopped only by section 4.** No signature check evaluates a value. If the clamps of section 4 are not implemented, all four succeed.
9. He publishes a rule-set whose sha256 matches a file that routes the user's banking and messaging domains DIRECT. **Stopped at V14b if `tiers` is published**, because the rule-set lives in the catalog. If not, it succeeds, and the client's own integrity check passes because the signer chose the hash. The Keep or Revert requirement in 2.2 is the remaining line of defence and it is a client-side one.
10. The operator notices and revokes. He signs key document N+1 offline with the root key, listing the compromised `kid` in `rev.kids`. Clients reject that key and every document it signed, including cached ones (`03-WIRE.md` 8.1). **Recovery works**, at a cost: section 3.4, and a propagation window of up to seven days against an adversary who withholds (`01-DECISION.md` A2).

**Score.** Steps 1 through 5 are all hard stops and they are hard for the same reason: authorization is read from the previously trusted document and never from the document under verification. Step 6 is the hinge of the entire compromise story and it depends on an operator action that `02-SPEC.md` 4.3 now makes mandatory rather than optional. Steps 7, 8 and 9 are where the real damage lives, and step 8's clamps now live in `02-SPEC.md` 8.6.1 rather than only here.

### 7.3 A hostile operator attempting to deanonymize and harm a user

Adversary: O (2.2). Goal: identify a specific subscriber, learn what he does, and coerce or expose him. Assume O is compelled rather than criminal, which is the realistic case in this market.

1. O wants to know which subscriber corresponds to a directive fetch. He already knows: the locator maps to a subscription in his own database, and he holds the account. **No mechanism is claimed here and none exists.** `01-DECISION.md` 5.7.5 states it: the operator learns the source IP at fetch time and at connect time, which is inherent to running a VPN.
2. O wants the user's installed application list, which is the most identifying thing a VPN client could upload. He adds a preference key for it and signs a directive requesting it. **Stopped at the client.** `split.apps` has no key in the `pol` table, MUST NOT be assigned one, and MUST NOT be transmitted in either direction (`03-WIRE.md` 8.3, invariant 15). The refusal is in the client's own serializer, not in a preference, so there is no configuration under which it turns on.
3. O sends an announce: "Verify your account at https://operator-support.example". **Stopped at render.** `ann` is capped at 80 bytes, is inert text, has URL-shaped substrings stripped, is never rendered on the same surface as the verification chrome, is never persisted and never echoed, and the client MUST refuse to open any operator-supplied URL (`03-WIRE.md` 14.6, invariants 10 and 11). Note where this closes an existing hole: `GET /api/v2/app/branding` is in the public group before `route_layer(require_app_jwt)` (`apps/caramba-panel/src/api/v2/mod.rs:145-157`), is unauthenticated and unsigned, returns operator-controlled `support_url` and `bot_url` (`apps/caramba-panel/src/api/v2/app_branding.rs:33-34`), and the client opens them with `LaunchMode.externalApplication` (`apps/caramba-client/lib/features/branding/powered_by.dart:122-127`). That is a pin bypass on the one field an attacker most wants, and X2 moves it first.
4. O repoints DNS to a resolver he logs. He signs a directive whose `pol` sets `dns.nameservers`. **Surfaced, not blocked.** Invariant 22: the Keep or Revert card fires on any narrowing of the user's security posture regardless of provenance, and a DNS repoint is named explicitly (`01-DECISION.md` B2). The user can revert. A user who taps Keep is not protected, and this document does not pretend otherwise.
5. O moves the user's banking domains to DIRECT via the routing preset, so that traffic leaves the device in cleartext while the tunnel shows connected. **Not blocked by anything in the wire format**, for the reason in 2.2: the hash binds the bytes to the signer's choice and O is the signer. The requirement in 2.2, that a change to rule-set providers or resource hashes raises the Keep or Revert card, is the only defence and it is a client-side one that `02-SPEC.md` must encode. This is the clearest breach of the constraint that a malicious operator must not harm the user beyond the VPN service he signed up for, and it is the reason C3 was grafted at all; C3 just does not do what its own sentence claims.
6. O equivocates: he serves this user a catalog nobody else sees, with a single exit under surveillance. **Detected only if `tiers` is published**, because that catalog's hash would have to be root-signed to pass V14. Otherwise undetectable in v1, which is `01-DECISION.md` A11, accepted after rejecting a transparency log in 4.10 on the grounds that the log is itself a censorship target.
7. O links the subscription to a person. He does not need the protocol: the Hysteria2 password is `"{tg_id}:{uuid}"` (`services/subscription_service.rs:1908`), and the device-block path sends the user's IP into that same Telegram account (`subscription.rs:235-262`). **Not stopped, and not a CSM/1 path.** The protocol's contribution is negative rather than positive here: it keeps identifiers out of signed documents (`01-DECISION.md` 5.7.1, enforced by a build-failing unit test) while the same identifiers travel in the unsigned config body.
8. O cuts the user off during a blackout by signing a directive with `st` = 5 and `exph` = 0. **Partially stopped.** Invariant 16 is absolute: an expired document never disconnects a user, never tears down a tunnel and never clears a cached configuration. But `exph` is a different lever, and section 4's `EXPH_FLOOR` is what stops it. Without that clamp, one directive ends the user's ability to connect at all.
9. O demands the client report which rungs worked, so he can tell the authorities which circumvention paths remain open in which ASN. **Stopped by design.** The client MUST NOT report which transport rung carried a request (`01-DECISION.md` 5.4.6). The only client-side state report is `v=`, the highest directive version accepted, whose upper bound the operator already knows.

**Score.** Steps 2, 3 and 9 are hard client-side refusals that survive even a root compromise, which is why they are invariants rather than server policy. Steps 4 and 8 are bounded by mechanisms that must be built. Steps 5, 6 and 7 are the residuals.

### 7.4 A hostile mirror steering the legacy config fetch

Adversary: M (2.6), or equally N (2.1), or `caramba-sub` itself. This attack has no CSM/1 defence at all and it is included because it is the one a reader would otherwise assume is covered.

1. The client, having verified its directive, still fetches its tunnel configuration from `/sub/{uuid}` because the uuid decoupling is scheduled after cutover (`01-DECISION.md` 5.5.4).
2. Any party in that path observes the URL. For N that is the trusted-CA MITM. For M and for `caramba-sub` it is the request line in the clear at the proxy, and `caramba-sub` additionally reconstructs it and forwards the client's real IP as `X-Forwarded-For` (`apps/caramba-sub/src/panel_client.rs:251-258`).
3. The URL is a bearer credential: `/sub/{uuid}` is registered on the panel's root router with no authentication other than the uuid itself (`apps/caramba-panel/src/main.rs:1584-1588`).
4. It is also a **write**. Three writes fire inside the GET handler:
   - `UPDATE subscriptions SET relay_country = $1 WHERE id = $2` whenever `?relay_country=` is present (`apps/caramba-panel/src/subscription.rs:744-751`),
   - the auto-pin, which persists the first node for a subscription with no explicit selection (`:616-633`),
   - the explicit node persist for any valid `?node_id=` (`:636-644`).
5. The adversary replays the URL with `?node_id=<the node he prefers>`. The victim's subscription is now pinned to that node server-side, for every subsequent fetch and for every client the victim uses. He can equally set `relay_country` to steer the relay, or to `none` to remove relaying entirely.
6. **Nothing in CSM/1 stops this.** The directive is signed and the client's own selection is authoritative in the Connect client, but the panel's server-side state has been changed by a third party, and every legacy client of the same subscription, plus the mini app, plus the Connect client's own next config fetch, reads that state.
7. It also defeats the geo determinism P6 exists to establish, because the adversary chooses `relay_country` and the panel's own `client_cc` comes from client-supplied headers (`subscription.rs:146-153`).

**What closes it:** P7, which removes the write-on-GET side effects while keeping both query parameters as pure filters, sequenced after the mini app's relay picker is migrated because the `UPDATE` is what turns that client-side choice into server state (`01-DECISION.md` P7). Until then this is a live, unauthenticated, third-party write against every subscription whose URL has ever crossed a hostile hop.

> This document records it as residual R-6 in section 8. It is not a CSM/1 defect; it is a defect CSM/1 does not reach, and the difference matters only to the people writing the specification, not to the user whose exit node was chosen for him.

---

## 8. Residual risks, named

Each is something no mechanism in this design stops. They are numbered so that a reader can cite them, and each carries the work item that would close it.

| # | Residual | Reached by | Closed by |
|---|---|---|---|
| R-1 | Enrollment security equals the integrity of the channel carrying `link_pin`. Trust on first use, stated plainly. | N | nothing in v1; the manual-entry 40-bit dictated pin is the mitigation, and the bootstrap blob on paper is the strong form |
| R-2 | A first-trust replay window of up to 604800 seconds, bounded only by the key document's own `exp` and only on a device whose clock is plausible. `02-SPEC.md` 5.4 now requires the client to run V12 against the device clock at enrollment when `BUILD_EPOCH <= clock <= BUILD_EPOCH + 10 years`, and to **refuse enrollment** rather than enrol blind when it is not; without that rule the window was unbounded, because `clock_trusted` is false at first trust and V12 was inert. | N | `doc_type` `0x07`, the reserved timestamp role (`01-DECISION.md` A2) |
| R-3 | Account password and session tokens are protected by TLS alone against an adversary documented as holding a trusted CA | N, O, K-online | the session-layer track; `01-DECISION.md` A9, `00-DESIGN-BRIEF.md` R12 |
| R-4 | The tunnel credential is per subscription, is shared across devices and legacy clients, and carries the Telegram id | everyone in the data path | `01-DECISION.md` 5.5.4, second half, scheduled after cutover |
| R-5 | A compromised online key with no published `tiers` owns the configuration, including routing to DIRECT | K-online | make `tiers` mandatory in practice (2.3), plus the Keep or Revert requirement in 2.2 |
| R-6 | `/sub/{uuid}` is an unauthenticated third-party **write** primitive | N, M, `caramba-sub` | P7 |
| R-7 | A mirror learns a stable per-subscription and per-device identifier from the request line | M | locator rotation via `gen`; nothing removes it entirely, since admission requires a locator |
| R-8 | Per-user equivocation on the catalog is undetectable without `tiers`, and unprovable even with it | O | `01-DECISION.md` A11; a transparency log was rejected in 4.10 |
| R-9 | Catalog chunk responses are a per-tier constant size, so an observer counting bytes learns the tier and roughly the fleet size | N, M | nothing in v1; `03-WIRE.md` 12.3 states it rather than claiming bucketing the design does not have |
| R-10 | Below Android 12 the device key is software-held and exportable by a rooted-device adversary | a local adversary | disclosure, not prevention (`01-DECISION.md` B4) |
| R-11 | The device limit is enforced against a forgeable header, and every rate limiter in the panel except the CSM ones fails open | E | thumbprint counting (`01-DECISION.md` 5.5.2), a trusted-proxy check on `extract_client_ip`, and the fail-closed CSM limiters (`03-WIRE.md` 13.3, which now names a concrete limit for every CSM route including `/sub/b1/{code}` and `/sub/k1`) |
| R-12 | Cross-tenant session and credential storage on the client is not `pid`-scoped | T1 | the storage hand-off in 2.7 |
| R-13 | Blocking is unanswerable. Whitelist mode leaves R0 and R6 only | N | `01-DECISION.md` A3, and the domestic-carrier trigger it names |

---

## 9. The accepted risk register, restated

Every item from `01-DECISION.md` section 8, with its trigger, so that the threat model and the risk register cannot drift apart. The adversary column ties each into section 2 and the "where in this document" column ties it to the analysis.

| Id | Risk | Adversary | Where analysed |
|---|---|---|---|
| A1 | The online signing key can invent nodes | K-online | 2.3, 3.2, 7.2 step 7 |
| A2 | Online-key revocation takes up to 7 days against an adversary who can withhold | K-online, N | 3.3, 7.1 step 6, R-2 |
| A3 | Whitelist mode is unsurvivable except by R0 and R6 | N | 2.1, R-13 |
| A4 | No embedded circumvention transport | N | 2.1, R-13 |
| A5 | Apple 3.1.1 is unadjudicated | store | 7.1 step 5 |
| A6 | RU App Store removal is the expected steady state | store | 2.6, and the reserve pool `0x08` |
| A7 | The mirror pool is an enumeration target | E | 2.5 |
| A8 | Two config renderers until the panel refactor lands | O (as drift, not as malice) | not a security boundary; a correctness one |
| A9 | R12, the session layer, is untouched | N, O | 1.4, 5.2 item 2, R-3 |
| A10 | The tunnel config is authenticated, not made more evasive | N | 1.4 |
| A11 | Per-user equivocation on the catalog is undetectable in v1 | O | 2.2, 7.3 step 6, R-8 |
| A12 | Nothing in the Connect track has been compiled, bound or signed | all | 1.3 assumption 1 |

**A1. The online signing key can invent nodes.** The directive references the catalog by hash, but a compromised online key can sign a different catalog. Full Uptane containment requires an offline signing ritual for every node addition, and a one-person operator will not perform one. The offline root bounds key identity and therefore recovery.
*Trigger:* a licensed operator population large enough that a compromised panel is a routine event rather than a tail risk, or the first real compromise, forces a mandatory offline catalog ceremony with a client-verifiable custody signal, which is a different trust model.
*This document adds:* the containment is stronger than A1 states when `tiers` is published and V14b is enforced, and weaker than A1 states when it is not, because the rule-set repoint of 7.3 step 5 is available on the same path. See 2.3 and Correction 1. `02-SPEC.md` 4.3 makes publishing `tiers` a panel MUST and prices what it costs; A1's trigger is unchanged.

**A2. Online-key revocation takes up to 7 days against an adversary who can withhold.** The key document is the only carrier of the revocation list and its freshness rests on a 7 day expiry. An adversary who steals the online key on day one also captures the current key document and can replay it while suppressing refreshes.
*Trigger:* if measurement shows key-document refresh failing for a material fraction of RU devices, add the TUF timestamp role, a roughly 60 byte root-signed document carrying the current key-document version and hash, signable in bulk by a cron on the root-holding machine, which cuts the window to 24 hours. The field numbers are reserved for it (`doc_type` `0x07`, `03-WIRE.md` 1.2).

**A3. Whitelist mode is unsurvivable except by cached documents and a human carrying bytes.** Every other rung dies. The one demonstrated escape is tunneling through domestic infrastructure, which is a tunnel-transport problem rather than a manifest problem, and in whitelist mode the user cannot reach the VPN either.
*Trigger:* whitelist mode becoming the default rather than an episodic measure makes the domestic carrier a product requirement, and it must then ship as a fully exercisable reviewed feature, not as a toggle whose parameters arrive later (`01-DECISION.md` 4.3).

**A4. No embedded circumvention transport, which lands hardest on the users who need it most.** A Russian user with no working direct path, no working mirror, no tunnel already up and no proxy of their own has only the out-of-band rung.
*Trigger:* field measurement from a Russian vantage point showing the ladder failing above a set rate reopens the merged-module spike, with dnstt against the operator's own authoritative nameserver as the first candidate rather than Tor, precisely because it is the only mechanism in the material that plausibly survives a DoH-only network.

**A5. Apple 3.1.1, the license-key question, is unadjudicated.** No written rule and no public precedent. The mitigation is structural: the enroll link and QR are the primary flow, the pasted subscription URL is secondary, and the app is fully functional before enrollment.
*Trigger:* a rejection citing 3.1.1 forces the enrollment flow to become an operator picker over a Webq-Pro-hosted directory, which is a different product and a different censorship posture.

**A6. RU App Store removal is the expected steady state, not a tail risk.** The exact peer set was pulled on 2026-03-28 with no court process. Removals are storefront-scoped, so installed copies keep working but stop updating.
*Trigger:* removal makes the in-binary reserve mirror pool unreachable by app release, so the reserve set must live in signed data with a 7 day expiry, its own URL and a root signature, rather than only in the app bundle. Built that way now as `doc_type` `0x08` at `/sub/r1/{loc}` (`03-WIRE.md` 8.6 and its Correction 6); the trigger is when it becomes load-bearing.

**A7. The mirror pool is an enumeration target.** Anyone who enrolls gets every mirror hostname in their cohort. That is the same exposure Tor bridges have and the mitigation is the same: per-cohort subsets, aggressive rotation, and a reserve pool held out of the published catalog.
*Trigger:* evidence of cohort-wide mirror burns faster than the rotation cadence means the mirror set moves entirely into the sealed per-device document and out of the shared catalog.
*This document adds:* the cohort arithmetic in 2.5, including the 16-cohort ceiling that follows from the `tiers` map cap and the provisional minimum cohort size of 25 subscribers.

**A8. Two config renderers until the panel refactor lands (P9).** Drift between the Rust generator serving Hiddify and v2rayNG and the Go renderer serving Connect. Mitigated by the verify-and-compare migration phase and by the identical-proxy-name fixture.
*Trigger:* a drift class the fixture cannot catch, for example silent divergence driven by free-form `Inbound.settings` JSON, forces the panel to emit a per-exit mihomo outbound fragment inside the catalog, trading bytes for a single renderer.
*Security note:* drift is a correctness risk, not an authorization one, with one exception. Two renderers that disagree on the proxy name break `Server.ID == Server.Name == the Clash proxy name`, and a client that cannot resolve a server pin falls back to a node the user did not choose. The Clash generator still has no proxy-name uniquifier, unlike the sing-box path's `unique_tag` at `singbox/subscription_generator.rs:1566`, which is why `id` is mandatory in the node entry and `pn` is not a key (`03-WIRE.md` 8.2.1).

**A9. R12 is untouched.** Fifteen minute unrevocable access tokens, no refresh reuse detection, no logout-all despite the index existing, one symmetric secret with no `kid` and no rotation, and `APP_JWT_SECRET` falling back to `SESSION_SECRET`.
*Trigger:* the signing key must never be stored under `SESSION_SECRET`; if it is, a single symmetric leak becomes simultaneously a session-forgery event and a config-signing event with A2's window attached.
*Verified:* `apps/caramba-panel/src/api/v2/app_auth.rs:32-33` and `:69-76`. All five sub-claims hold as written.

**A10. The tunnel config is authenticated, not made more evasive.** CSM/1 does not change protocol selection, obfuscation, port choice, or the VLESS plus Reality plus vision problem on 443. A user whose exit protocol is blocked stays blocked.
*Trigger:* per-cohort transport assignment, which protocol, port, fingerprint, mux and flow each cohort uses, is the natural v2 item and belongs in the catalog schema's reserved space now.

**A11. Per-user equivocation on the catalog is undetectable in v1.** Rejecting a transparency log has this cost. Partially bounded by publishing tier catalog hashes in the root-signed key document.
*Trigger:* an operator caught equivocating, or a licensee population where the operator relationship is less trusted than it is today.
*This document adds:* "partially bounded" is conditional on `tiers` being present. With `tiers` absent there is no bound at all, and the client cannot currently tell the user which case it is in, which is why 2.3 requires the verification chrome to say so.

**A12. Nothing in the Connect track has been compiled, bound or signed.** The whole plan rests on an integration that has never happened, against a live panel, bot, sub, node and mini app that serve real users.
*Trigger:* the first integration milestone. Front-load it: the iOS Network Extension target, App Group and entitlements do not exist in the repo today and Apple organization enrollment has a multi-week lead time, so both start before protocol code, and the NE memory measurement with mihomo alone runs in week one rather than at the end.
*Security consequence:* assumption 1 of section 1.3 is unverified until the three-language negative corpus runs green, and every "stopped at V*n*" claim in section 7 is a claim about a specification, not about a binary.

---

## 10. Corrections to the inputs

Each item is a place where this document departs from `01-DECISION.md`, `00-DESIGN-BRIEF.md` or `03-WIRE.md`. The code is followed where they disagree with it.

**Correction 1: resource hashing bounds a third party, not the signer.** `01-DECISION.md` C3 argues that carrying a sha256 per rule-set turns a compromised online key from "can re-target among existing nodes" into being unable to "move named domains from proxy to DIRECT so traffic leaves the device in cleartext with the tunnel showing connected". That attribution does not hold. The catalog carries both the resource path and its hash (`03-WIRE.md` 8.2), and the party that signs the catalog chooses both. Hashing removes the substitution capability from any host in the fetch path, which is a real and worthwhile gain against N and M, and it removes nothing at all from K-online or O. The property C3 wants exists only via V14, that is only when the tier hash is published in the root-signed key document, and `tiers` is an optional field. Consequences are in 2.2, 2.3, 7.2 step 9 and 7.3 step 5, and the compensating client-side requirement is stated in 2.2.

**Correction 2: an authorized catalog means the mirror learns the locator.** `01-DECISION.md` 5.2.4 says of the catalog that "a mirror still learns only an IP and a timestamp". That was true of the public catalog it replaced. Chunk fetches are admitted by `X-CSM-Loc` and directive fetches carry the locator in the path and `dtp` in the query (`03-WIRE.md` 13.2), so every mirror learns a stable per-subscription identifier and, on the directive path, a stable per-device one. Rejecting the anonymous public catalog in 4.6 was correct; it cost exactly this, and the ledger in section 6 states it rather than repeating the original claim.

**Correction 3: the Telegram id is in the tunnel credential, not only out of the signed documents.** `01-DECISION.md` 5.7.1 forbids a Telegram id, or a hash of one, in any signed document, and mandates a panel unit test that fails the build. Both remain correct and both are encoded. What neither says is that the identifier is already in the data plane: `get_user_keys` builds the Hysteria2 password as `format!("{}:{}", tg_id, user_uuid.replace("-", ""))` at `apps/caramba-panel/src/services/subscription_service.rs:1894-1916`, specifically `:1908`. Any party holding a Hysteria2 configuration for a subscriber learns which Telegram account it belongs to. This does not change any CSM/1 field; it changes what the privacy claim may say, and it adds a second reason for the decoupling in `01-DECISION.md` 5.5.4.

**Correction 4: the device limit is not a security control today, and the ladder is not what breaks it.** `01-DECISION.md` B4 and 5.5.2 correctly identify that device-limit enforcement counts by apparent source IP at `subscription.rs:203-262` through `get_active_ips`, and that every ladder rung with a different egress burns a slot. The stronger fact is that the IP itself is client-supplied: `extract_client_ip` reads `cf-connecting-ip` then `x-forwarded-for` with no trusted-proxy check and no comparison against the peer address (`apps/caramba-panel/src/subscription.rs:76-87`), and the same pattern governs the login rate limiter (`api/v2/app_auth.rs:253-262`) and the client API (`api/client.rs:404-414`). An adversary who can reach the panel origin directly therefore mints an unlimited supply of distinct "devices", chooses his own apparent country because `client_cc` also comes from request headers (`subscription.rs:146-153`), and defeats the login limiter, which additionally fails open on a Redis error (`app_auth.rs:283-287`). Thumbprint counting fixes the counting; it does not by itself make the header trustworthy, and any code that keeps using `extract_client_ip` for a security decision inherits the flaw.

**Correction 5, resolved since it was written: `hpk` and `hpkv` in the catalog cannot be the sealing recipient key.** `03-WIRE.md` 8.2 defines catalog keys 25 and 26 as "current HPKE recipient public key, P-256 uncompressed" and "HPKE recipient key generation", while 9.3 and 9.4 make the recipient the device (`dtp` names it, and step 5 looks up "the private key for generation `rkv`" on the device). The catalog is per tier and byte-identical for every subscriber on that tier (`01-DECISION.md` 5.2.1, 5.7.3), so a recipient key carried there is one key shared by every subscriber on the tier. An implementer who reads `hpk` as the sealing recipient produces a system in which **any subscriber on a tier can decrypt any other subscriber's directive**, which destroys the entire point of BC2. The reading this document adopts: the sealing recipient is always the device's own agreement key, registered at enrollment and rotated by the device-signed rekey message of `03-WIRE.md` 9.5, and `rkv` names that device key's generation. `hpk` and `hpkv` MUST NOT be used as a sealing recipient by any implementation, ever.

> **The interim rule this correction used to end with, "a conforming client MUST ignore catalog keys 25 and 26", is withdrawn.** `02-SPEC.md` 10.2 resolves the fields as the **panel's** own HPKE recipient key, with a single v1 use in the client-to-panel direction, its own `info` string `"CSM1-seal-w1"` and its own `aad` type byte `0xFF`, gated on the presence of `hpk` rather than on capability bit 1, with its storage, its starting generation and its rotation cadence stated there. `03-WIRE.md` 8.2 keys 25 and 26 and 9.5 name the fields accordingly. A live MUST-ignore here against a MAY-use in the specification would have guaranteed divergence, which is the one thing a threat model must not manufacture.

**Correction 6: reusing the licensing Ed25519 code would import a forbidden call.** `01-DECISION.md` P2 says to "reuse the ed25519 machinery already shipping in `license/activation.rs` and `caramba_shared::license`". That code path ends in `verifying_key.verify(&msg, &signature)` at `libs/caramba-shared/src/license.rs:224`, the permissive variant. `03-WIRE.md` 2.3 requires `VerifyingKey::verify_strict` and states that `VerifyingKey::verify` MUST NOT be used. Reuse means reuse the dependency and the key-handling patterns, not the call site. A copy-paste of the existing verification helper would silently produce a Rust verifier that accepts signatures the Go and Dart verifiers reject, which is the split-brain X1 exists to prevent, and it would pass any positive-only test suite.

**Correction 7: an expired trust anchor needs a stated rule.** `03-WIRE.md` V12 checks the expiry of the document under verification. Nothing states whether an expired but previously trusted key document remains a valid source of `roles` and `thr` at V3. Both answers are defensible and the difference is a fleet-wide outage: if an expired anchor is unusable, a seven day operator absence takes every device off configuration updates and, in the strict reading, off verification entirely. This document requires the anchor to remain usable, with the age surfaced, per 2.4.

**Correction 8: `01-DECISION.md` X3's line references are correct, with drift.** The two-core hazard is real and verified: `CarambaNewClient` is constructed in the Network Extension at `apps/caramba-client/packages/caramba_vpn/darwin/Extension/PacketTunnelProvider.swift:147` with work directory `caramba/` at `:139-140` and `configure(_:subscriptionID:accessToken:)` at `:166`, and a second client with work directory `caramba-tools/` is built at `.../Classes/CarambaVpnPlugin.swift:310-322`. Recorded here only so that a reader chasing X3 finds the same code.

---

## 11. Hand-offs

Requirements this document states that another document must encode. Each is a MUST or SHOULD from the sections above, gathered so none is lost between files.

**To `02-SPEC.md`.** Every item below is a MUST or SHOULD stated in the sections above. The **Encoded in** column is the conformance check: a hand-off with no entry there is a requirement that did not land, and this table exists because six of these eight previously had none.

| # | Requirement | Source | Encoded in |
|---|---|---|---|
| 1 | Clamp the signed dials of section 4: `CONN_BYTES_CEILING` 15360, `CONN_PACKETS_CEILING` 25, `thr.resp_max` rejected above 4096, `EXPH_FLOOR` 86400, `TTL_FLOOR` 900 with at least 10 percent jitter regardless of `jit`. The first two are provisional against the RU freeze measurement named in `03-WIRE.md` 17; the last two are policy. | section 4 | `02-SPEC.md` 8.6.1, listed in its section 14 |
| 2 | Treat a change to the rule-set provider set, to a resource hash, or to a route entry's `rs` list as a narrowing of security posture that raises the Keep or Revert card of invariant 22. This required editing a closed list, not adding prose. | 2.2, 7.3 step 5 | `02-SPEC.md` 7.7 (three new rows) and 7.7.1 |
| 3 | Require the panel to publish a `tiers` entry per served tier, and require the client to render `fleet not root-anchored` in the verification chrome when the trusted key document has no `tiers` entry for the directive's tier. | 2.3 | `02-SPEC.md` 4.3 and 8.8.2; `03-WIRE.md` 6.2 V14a/V14b |
| 4 | State that an expired trusted key document remains a valid authorization anchor, with its age surfaced under invariants 19 and 21, and with `rev` still enforced from it. | 2.4, Correction 7 | `02-SPEC.md` 2.2 and 5.2; `03-WIRE.md` 6.5 |
| 5 | Define the user-facing state for "the provider replaced its signing key and this device has not yet received the new configuration", with the R6 rung offered as the immediate remedy. | 3.4 | `02-SPEC.md` 10.4 and 8.8.2 |
| 6 | Key the client's token store and profile store by `pid`, and forbid an `ApiClient` from attaching a bearer token whose issuing `pid` does not match the origin it is contacting. | 2.7 | `02-SPEC.md` 1.2, extended to the settings state |
| 7 | Resolve the meaning of catalog keys 25 and 26. | Correction 5 | `02-SPEC.md` 10.2; the interim MUST-ignore is withdrawn |
| 8 | Carry the cohort guidance: minimum cohort size 25 subscribers, provisional, and the 16-cohort ceiling that follows from the `tiers` map cap. | 2.5 | `02-SPEC.md` 8.1.1 |

Two further requirements this document states in passing, added to the table because they are the same class of thing:

| # | Requirement | Source | Encoded in |
|---|---|---|---|
| 9 | A verified `st = 5` is the only signed value that tears down a running tunnel; a revoked signing key does not. | 3.4, 2.1 | `02-SPEC.md` 4.6.1 and 2.1 rule 5 |
| 10 | Every error code maps to a profile transition and a user-visible string class, so the shared code vocabulary of `03-WIRE.md` 6.6 does not stop at the wire. | 3.4, 6.3 | `02-SPEC.md` 8.8.1 |

**To `05-TEST-VECTORS/`:** every "stopped at" claim in section 7 names a step and an error code, and each MUST have a negative fixture: role confusion at V4 (an online key signing a `0x01`), a laundered slot at V4, duplicate slots at P8, inflated `nsigs` at P7, a version skip at V10, a wrong-tier catalog hash at V14b, a wrong-recipient seal at `E_SEAL_RECIPIENT`, and a tampered `aad` at `E_SEAL_OPEN`. All three implementations MUST return the same code for the same fixture, because two implementations failing for different reasons is how a real divergence hides (`03-WIRE.md` 6.6).

**To `06-MIGRATION.md`:** residual R-6 (the `/sub/{uuid}` write primitive) is closed by P7, and P7 is gated on the mini-app relay-picker migration. That ordering constraint is a security deadline, not only a compatibility one, and the migration document owns it.

**To the panel team:** Corrections 4 and 6, plus the `Content-Encoding` and constant-header hazards of 2.1, are panel-side and are already scoped in `01-DECISION.md` P3, P4 and P7. The `Content-Encoding` half is now scoped concretely: `03-WIRE.md` 13.1 states how the CSM router must be constructed so the compression and header layers do not wrap it, and 13.4 requires the header-set assertion to run with `Accept-Encoding: gzip, br` on the request so a still-attached layer fails the test rather than passing it. The one item not yet scoped anywhere is a trusted-proxy check on `extract_client_ip`, without which thumbprint counting sits on top of a forgeable geo and rate-limiting key.

---

## Changelog

One review pass, 2026-09-02, by three reviewers reading the whole set for cross-document consistency, panel implementability and client implementability. What it changed in this document:

**Blocking**

- Section 4's clamps are now encoded in `02-SPEC.md` 8.6.1 instead of existing only here against the opposite MUST there. `TTL_FLOOR` is lowered from 1800 to 900, because 1800 would have made the kill-switch acceleration of `06-MIGRATION.md` 4.3 silently ineffective and the kill switch is the only rollback that exists after PNR-2.
- Correction 5's closing "a conforming client MUST ignore catalog keys 25 and 26" is withdrawn and replaced by a pointer to `02-SPEC.md` 10.2, which now resolves the fields. A live MUST-ignore here against a MAY-use there was a guaranteed divergence.
- Section 2.3's `tiers` requirement is marked resolved, and 2.1's rollback row is corrected: V9 is inert for catalogs by construction, and the real bound is V14a plus the directive's own monotonicity.

**Serious**

- Section 11 is rewritten as a conformance table with an **Encoded in** column, because six of the eight hand-offs had landed nowhere. Two further requirements the document states in passing are added to it.
- Section 2.2, 2.4, 2.5, 2.7 and 3.4's requirements each carry an **Encoded** pointer now, so a dropped hand-off is visible from the paragraph that states it rather than only from a list at the end.
- Residual R-2's bound is corrected: it was stated as 604800 seconds, which was only true if V12 ran, and V12 is inert at first trust. `02-SPEC.md` 5.4's clock-plausibility rule is what makes the stated bound true, and the row now says so.

**Minor**

- R-11 cites `01-DECISION.md` 5.5.2 rather than a section 5.5.2 of this document, which does not exist, and notes that `03-WIRE.md` 13.3 now carries a concrete limit for every CSM route.
- 3.2's tier 0 recovery and `02-SPEC.md` 9.6 are cross-referenced so a reader can see that `apps/caramba-panel/src/api/v2/mod.rs:165-169` and `app_account.rs:90-255` are the route registration and the handler for one surface, not two.
- The 7.2 score note points at where step 8's clamps now live.
