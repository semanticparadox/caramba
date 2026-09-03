# CSM/1 Protocol Specification

Status: normative, 2026-09-02. Companion to `01-DECISION.md` (rationale, not repeated here), `03-WIRE.md` (byte layout, not repeated here) and `04-THREAT-MODEL.md`.

Scope: what a CSM/1 document means, what state each one moves through, what a client and a panel must do with each field, and the closed vocabularies three implementations must share. Everything below the field level is `03-WIRE.md`: frame layout, the exact-length rule, the signing pre-image, the strict CBOR and Ed25519 profiles, CBOR encodings, size budget, padding, endpoint shapes and URL constraints. This document cites those by section number and does not restate them.

Audience: a Rust signer and verifier in `apps/caramba-panel`, a Go verifier and renderer in `libs/caramba-core`, a Dart verifier and UI in `apps/caramba-client`. Assume the reader has this document and `03-WIRE.md` and nothing else.

Key words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY carry their RFC 2119 meanings.

---

## 0. Conventions

**Division of ownership.** Where this document and `03-WIRE.md` both describe a field, `03-WIRE.md` owns its type, its byte encoding and its size cap; this document owns its meaning, its cardinality in context, its closed value vocabulary, who may write it, and what a client does with it. Where the two conflict, section 13 records the conflict explicitly and states which one binds. Nothing is left to be inferred from silence.

**Correction notes.** Where this document departs from `01-DECISION.md`, `00-DESIGN-BRIEF.md` or `03-WIRE.md` because the code says otherwise or the rule as written is not implementable, the departure is marked **Correction** and carries its evidence with a `file:line` pointer into this repository. Section 13 collects them.

**Invariant references.** `INV-n` refers to invariant `n` of `01-DECISION.md` section 6. Every one of the 23 is restated in place in this document; section 12 is the index that proves it.

**Roles in prose.** "The panel" is the operator's `caramba-panel` deployment, which is the signer. "The client" is Caramba Connect: the Flutter app plus the Go core in one process, which is the verifier. "The core" is `libs/caramba-core` specifically. "The operator" is the human or organization running the panel. "The user" is the person holding the device.

---

## 1. The document model

### 1.1 The documents

CSM/1 defines five signed documents and one sealed envelope. Three of them carry the protocol; the other three exist so the first three can be bootstrapped, delivered and escaped with.

| Doc | `doc_type` | Signed by | Scope | Lifetime | Nonce | Content-addressed | Sealed |
|---|---|---|---|---|---|---|---|
| Key document | `0x01` | root | tenant | 7 days | no | no | no |
| Catalog | `0x02` | online | plan tier | 30 days | no | yes | no |
| Directive | `0x03` | online | one device | 1 hour | yes | no | in transit, as `0x06` |
| Catalog chunk | `0x04` | online | plan tier | 30 days | no | carries a slice | no |
| Bootstrap blob | `0x05` | root | tenant | 30 days | no | no | no |
| Sealed directive | `0x06` | online | one device | 1 hour | inherits | no | yes |
| Reserve pool | `0x08` | root | one locator | 7 days | no | no | no |

The split is the Uptane image and director split reduced to its minimum useful form (`01-DECISION.md` 5.2.1). The catalog says what exists; the directive says which of it this device gets and in what state its subscription is. The key document says who may say either.

> The three-document split is load-bearing and MUST NOT be collapsed. A panel MUST NOT put node connection material in a directive, and MUST NOT put anything per-device in a catalog. A catalog MUST be byte-identical for every subscriber on one plan tier: two subscribers on tier `t` who fetch the same `cat_id` MUST receive the same bytes. This is what makes the catalog cacheable, mirrorable and verifiable against the root-published tier hash, and it is what makes the "no tracking beacon" claim checkable rather than asserted (`01-DECISION.md` 5.2.4, 5.7.3).

### 1.2 Tenant, profile, device, subscription

**Tenant.** A tenant is a key, addressed by an origin. `pid = sha256(root_ed25519_public_key)[0..8]`. The protocol MUST NOT introduce a `tenant_id` column, a `panel_id` DTO field or a tenant claim in any JWT. The base URL is the address; the pinned root key is the identity (`01-DECISION.md` 5.1.1). Verified as compatible with the code: there is no tenant concept anywhere in the panel, and the client already holds multiple panels at once through `enrollApiClientProvider`, a `Provider.autoDispose.family<ApiClient, String>` keyed by panel URL (`apps/caramba-client/lib/features/enroll/enroll_controller.dart:312-319`).

> A document signed by tenant A MUST be rejected by a profile pinned to tenant B, even when both profiles are held on one device. The check is `payload.pid` byte-equals the pinned `pid`, at verification step V8 of `03-WIRE.md` 6.2.

**Profile.** A profile is the client's per-tenant state: the pinned `pid`, the pinned `link_pin`, the currently trusted key document, the current catalog, the current directive, the locator, the device key pair generation, the high-water marks, the time floor, the ladder order and enabled set, and the local settings state with provenance. One profile per enrolled tenant. A profile is the unit that enters and leaves the state machine in section 2.1.

> **Every store that holds profile state MUST be keyed by `pid`.** Tenant isolation in CSM/1 is a property of documents, not of sessions or of storage, and the client's storage is global today. Verified: `TokenStore` holds three fixed keys with one value each, `caramba.access_token`, `caramba.refresh_token` and `caramba.user_id` (`apps/caramba-client/lib/data/token_store.dart:9-11`), so enrolling a second operator overwrites the first operator's session; `ConnectionProfilesStore` keeps every profile in one blob under one key (`lib/data/connection_profiles_store.dart:14-15`), so one blob holds every enrolled operator's tunnel credential; and `CoreConfig` is one JSON blob under one shared-preferences key, `caramba.core_config` (`lib/data/prefs_store.dart:22`, `lib/state/core_config_state.dart:21-56`), so a directive from tenant A that writes `pol[8]` or `pol[9]` changes the configuration tenant B's tunnel uses.
>
> The migration is additive and is owned by `06-MIGRATION.md` 7.1. Three stores become `pid`-keyed: the token store, the profile store, and the settings state with its provenance, its user-set marks, its outstanding Keep or Revert cards, its write queue and its first-seen deprecation sunsets. The existing single blob migrates into the `pid` of the profile that owns it, and a blob with no owning `pid` migrates into the legacy `rawSub` bucket.
>
> An `ApiClient` MUST refuse to attach a bearer token whose issuing `pid` does not match the origin it is about to contact. The current interceptor attaches the global token to any request that does not set `skipAuth`, regardless of the origin the client was constructed for (`lib/data/api_client.dart:76-84`), while `enrollApiClientProvider` constructs exactly such a client against a panel URL taken from an enrollment link (`lib/features/enroll/enroll_controller.dart:312-319`). `04-THREAT-MODEL.md` 2.7 and residual R-12 are what this closes.

**Device.** A device is a P-256 signing key thumbprint, `dtp = sha256(device_signing_SPKI_DER)[0..16]` (`03-WIRE.md` section 4). Device identity MUST NOT be derived from a Telegram id, a phone number, an email address or any other low-entropy identifier (`01-DECISION.md` 5.5.1). A device holds two keys, a signing key and an agreement key, and they MUST be distinct (section 10.2).

**Subscription.** A subscription is the panel's existing billing object. CSM/1 does not change it and does not carry its uuid. The client already knows its own subscription uuid and assembles the legacy config URL locally.

> The subscription uuid MUST NOT appear in any signed CSM/1 document, in any field, in any encoding. INV from `01-DECISION.md` BC2 and 5.5.4. The reason is one function: `subscription_user_uuid` returns `sub.vless_uuid` falling back to `sub.subscription_uuid` (`apps/caramba-panel/src/services/subscription_service.rs:191-205`), and that value is simultaneously the VLESS uuid, the Trojan password, the TUIC uuid and part of the Hysteria2 password. A directive is deliberately exposed to mirrors; putting the uuid in one publishes the tunnel credential to every host in the mirror pool.

**Locator.** `loc` is the derived, rotatable address of a device's directive and reserve pool, `base32_crockford(HMAC-SHA256(secret, "csm1-loc" || 0x00 || subscription_uuid || u32be(gen))[0..15])`, 24 characters (`03-WIRE.md` section 4). `gen` is a per-subscription column, not a panel-wide epoch, so revoking one leaked locator is one `UPDATE` (`01-DECISION.md` 5.5.3). `secret` is `CSM_LOC_SECRET`, a dedicated 32-byte panel secret that `03-WIRE.md` section 4 forbids sharing with `SESSION_SECRET` or `APP_JWT_SECRET`; rotating it is equivalent to bumping `gen` for every subscription at once and is the panel-wide emergency lever.

### 1.3 What each document is allowed to say

The allowlist is normative and is enforced by a panel unit test that fails the build (`01-DECISION.md` 5.7.1).

| Document | May carry | MUST NOT carry |
|---|---|---|
| Key document | keys, roles, thresholds, revocation, tier catalog hashes, deprecations, its own refresh interval | anything per-device, anything per-subscription, any node connection material, any locator |
| Catalog | node connection material, relays, routes, mirrors, DoH endpoints, resource hashes, thresholds, ladder defaults, pins, the panel HPKE key, capabilities | anything per-device, anything per-subscription, any locator, any nonce, any timestamp other than `iat` and `exp` |
| Directive | nonce, device thumbprint, status, reason code, catalog binding, tier, capability echo, selection, settings echo, inert text, refresh cadence, grace window, locator, traffic counters | node connection material, the subscription uuid, any credential, any operator URL |
| Bootstrap blob | origin, enrollment code, root public key, mirrors, DoH endpoints, operator display name | prices, a bot handle, a purchase link, any "buy" call to action, any key material other than the root public key |
| Reserve pool | mirrors, DoH endpoints, a cohort identifier | anything else |

> No signed document may carry a Telegram id, a username, a phone number, an email address, a payment reference, a referral code, a family membership, ticket content, or a hash of any of them (`01-DECISION.md` 5.7.1). The panel test asserts two things: that the CBOR key set of every emitted document is a subset of a hardcoded allowlist, and that no encoded document contains the fixture user's identifiers as a byte substring.

---

## 2. State machines

Each document type moves through a small, explicit state machine. The machines are normative: an implementation MUST be able to name the state it is in for each document, and the "What this app sends" screen (INV-20) and the verification chrome (INV-19) render from these states.

### 2.1 Profile

```
                     enroll link, QR, blob, or manual code
   unenrolled  ────────────────────────────────────────────►  pinning
                                                                 │
                              link_pin captured, pid not yet known│
                                                                 ▼
                          key document fetched and pin matched   anchored
                                                                 │
                              device keys generated, enroll POST │
                                                                 ▼
        ┌──────────────────────────────────────────────────  enrolled
        │                                                        │
        │ directive verified, catalog verified                   │
        ▼                                                        │
     trusted  ◄──────────────── refresh succeeded ───────────────┘
        │  │
        │  └── refresh failed, cached documents still unexpired ──►  trusted_stale
        │                                                                │
        │  ◄──────────────── refresh succeeded ──────────────────────────┘
        │                                                                │
        │                        every cached document expired           │
        │                                                                ▼
        │                                                          grace
        │                                                                │
        │                          exph elapsed since last verified      │
        │                                                                ▼
        │                                                          grace_exhausted
        │
        ├── every kid in roles[1].ks appears in rev, or root pin mismatch ──► compromised (terminal)
        │
        └── a kid in roles[2].ks appears in rev ──► trusted_stale, or grace; NOT compromised
```

Rules that are not visible in the diagram:

1. **`unenrolled` and `pinning` are the only states in which a root key may be pinned.** Once a profile leaves `pinning` the pinned `link_pin` is immutable for the life of the profile. Changing it is deleting the profile and enrolling again, and the UI MUST present it that way.
2. **`trusted_stale` is not an error.** It is the normal state of a device on a blocked network and the client MUST render it as "running on cached configuration, N hours old" (INV-21), never as a failure.
3. **`grace` still connects.** An expired document is refused for accepting new instructions and new status; it is never a reason to disconnect (INV-16, section 5.2).
4. **`grace_exhausted` stops offering to connect.** This is the only state in which the client refuses to bring a tunnel up on operator-configured data, and it is reached only after `exph` seconds have elapsed since the last successful verification. `exph` is an operator dial and the client MUST render its consequence beside it: a longer window buys blackout tolerance and buys the same amount of un-revocable service (`01-DECISION.md` C5).
5. **`compromised` is terminal and non-dismissible, and only two things reach it.** A root pin mismatch, and the revocation of every key in `roles[1]`. In that state the client MUST discard every cached document for the profile, MUST NOT connect using them, and MUST NOT offer a "continue anyway" affordance. Recovery is re-enrollment out of band.

   > **Revocation of an `online` key MUST NOT reach `compromised`.** Publishing the retired `kid` in `rev` is the ordinary hygiene of section 10.4, and routing it to a terminal state would make the operator's normal response to a suspected leak an out-of-band re-enrollment for every device on a fleet whose support channel is assumed blocked. A `kid` in `roles[2].ks` appearing in `rev` invalidates every document that key signed, including cached ones on disk (section 10.6), and moves the profile to `trusted_stale`, or to `grace` when it now holds no unexpired verified document at all. The client MUST immediately attempt a refresh, MUST render the state described in section 10.4, and **MUST NOT tear down a running tunnel**: the engine already holds its configuration, and disconnecting a user because their provider rotated a key is the failure `04-THREAT-MODEL.md` 3.4 prices.
6. **There is no downgrade edge.** A profile that has reached `anchored` or beyond MUST NOT fall back to unverified legacy behavior for any reason, including a missing `cap` field, a 404 on a CSM route, or an operator downgrading their panel (INV-13). A missing `cap` field on an enrolled profile is a hard, non-dismissible error, not a downgrade. The one exception is the signed kill switch defined in `06-MIGRATION.md`, which is itself a signed document and therefore not a downgrade.

### 2.2 Key document

```
   absent ──► pin_only ──► trusted(N) ──► trusted(N+1) ──► ...
                              │   ▲
                              │   └── still the anchor after exp; see below
                              │
                              ├── a kid in roles[2].ks appears in rev ──► trusted(N), documents
                              │      signed by that kid discarded; profile falls to trusted_stale
                              │
                              └── every kid in roles[1].ks appears in rev ──► compromised
```

- `pin_only`: the client holds `link_pin` and nothing else. The only document it will accept is a `0x01` at any version whose single `root` key hashes to the pin, or a `0x05` bootstrap blob whose `rk` hashes to the pin (`03-WIRE.md` 7.2).
- `trusted(N) → trusted(N+1)`: only through the rotation rule of section 10.5. Versions MUST NOT be skipped.
- There is no path from `trusted(N)` back to `pin_only`.
- A key document is the only document whose acceptance may change the trusted key set, and it is the only document whose own key set participates in its own verification, and then only under the rotation rule.
- **There is no `expired` state for the trusted key document, and that is deliberate.** `exp` on a key document governs when the client should have refetched it, not when it stops being the authorization anchor.

> An expired trusted key document MUST remain a valid authorization anchor. The client MUST keep reading `roles`, `thr` and `rev` from it at verification step V3 after its `exp` has passed, MUST keep enforcing `rev` from it, and MUST surface the anchor's age in the verification chrome (INV-19, INV-21). The alternative turns the 7-day lifetime into a fleet-wide kill switch that fires whenever the operator is unreachable for a week, which is an outage rather than a security property. `03-WIRE.md` 6.5 carries the same rule and `04-THREAT-MODEL.md` 2.4 and Correction 7 are the analysis.

### 2.3 Catalog

```
   none ──► referenced ──► fetching ──► reassembled ──► verified ──► active
                              ▲             │                          │
                              └── retry ────┘                          │
                                                                       │
     directive names a different cat, or a node id appears in rev ─────┘
                                                                       ▼
                                                       superseded  /  pruned
```

- `referenced`: a verified directive named `cat` (a 32-byte `chash`) and `cn` (a chunk count). The client now knows the identity and the length of a catalog it does not have.
- `fetching`: chunks `0 .. cn-1` are being retrieved. Every catalog is chunked, including a one-chunk catalog (`03-WIRE.md` 8.4, 11.4). Each chunk is independently signed and independently verified before reassembly.
- `reassembled`: `sha256(joined)[0..10] == cid` and `len(joined) == tl`.
- `verified`: the reassembled bytes verify as a `0x02` frame from parse step P1 through verification step V14b. `sha256(frame)` equals the `cat` the directive named, unconditionally (V14a), and equals the tier hash published in the trusted key document when that document carries a `tiers` entry for the directive's `tier` (V14b, section 4.3).
- `active`: the client is building configuration from it.
- `pruned`: a node id present in the trusted key document's `rev.nodes` has been removed from the in-memory node set. Pruning MUST be applied against the cached catalog even when the client is offline and has no network at all, so a seized node is dropped without a refresh (`01-DECISION.md` 5.2.9).

> A catalog MUST NOT be entered at `verified` from an unsolicited fetch. The `chash` must come from a verified directive first, or from the tier hash in the trusted key document. A catalog that no trusted document names is not trusted, however well it verifies.

> A client MUST check each chunk's `cid` against `cat[0..10]` of the directive that named the catalog **before** accepting that chunk, not only after reassembly. `03-WIRE.md` 8.4 checks `cid` at reassembly, which is correct but late: without the per-chunk check a client can spend up to 64 fetches, and up to 64 TCP connections under the one-chunk-per-connection rule of `03-WIRE.md` 11.5, collecting chunks of a catalog it was never going to accept. A chunk whose `cid` does not match is `E_PARSE_FIELD` and the rung returned nothing.

### 2.4 Directive

```
   none ──► requested(nonce) ──► sealed_received ──► opened ──► verified ──► current
                   │                                                            │
                   │ ttl elapsed, or a settings write accepted                   │
                   ▼                                                            │
              requested(nonce')  ◄──────────────────────────────────────────────┘
                                                                                │
                                        exp + 300 < now, clock trusted          │
                                                                                ▼
                                                                             expired
```

- `requested(nonce)`: the client generated a fresh 16-byte random nonce and sent it as `?n=`. Exactly one nonce is outstanding per profile per request. A nonce is valid for 300 seconds; after that the client MUST discard it and generate a new one rather than accept a late reply.
- `sealed_received`: a `0x06` frame arrived and its outer signature verified. The seal has not been opened.
- `opened`: HPKE `Open` succeeded and the plaintext is a byte string that begins `43 53 4d 31 03`.
- `verified`: the inner `0x03` frame passed every check including the nonce echo (V13) and the version rule (V9).
- `current`: the client is acting on it.
- `expired`: still valid for connecting, refused for new instructions and new status (INV-16).

> A directive MUST NOT be accepted whose `nonce` does not byte-equal the nonce this device just sent. INV-9. This is the freshness mechanism that survives a wrong clock, which is the normal case after a factory reset and common where DNS blocking prevents NTP (`01-DECISION.md` 5.2.3).

### 2.5 What never changes state

A verification failure never advances a high-water mark, never advances the time floor, never replaces a cached document and never changes the profile state. It is recorded in the per-rung attempt history and surfaced in the verification chrome (INV-17, INV-19), and the ladder MAY continue to the next rung, because a hostile mirror is exactly the case the ladder exists for. A parse failure is weaker still: it means the bytes are not a CSM/1 frame at all, and the correct handling is to treat the rung as having returned nothing (`03-WIRE.md` 6.1).

---

## 3. Authorization: doc_type to role to threshold

This is the single rule three implementers would otherwise each invent differently (`01-DECISION.md` 5.1.3). It is written as a table here and in `03-WIRE.md` 7.1, and the two MUST agree; if they ever diverge, `03-WIRE.md` 7.1 binds, because the verifier reads it.

| `doc_type` | Document | Required role | Threshold read from | Key set read from |
|---|---|---|---|---|
| `0x01` | key document | `root` | `roles[1].thr` of the **previously trusted** key document | `roles[1].ks` of the previously trusted key document, **and** `roles[1].ks` of the document under verification (rotation only, section 10.5) |
| `0x02` | catalog | `online` | `roles[2].thr` of the trusted key document | `roles[2].ks` of the trusted key document |
| `0x03` | directive | `online` | `roles[2].thr` of the trusted key document | `roles[2].ks` of the trusted key document |
| `0x04` | catalog chunk | `online` | `roles[2].thr` of the trusted key document | `roles[2].ks` of the trusted key document |
| `0x05` | bootstrap blob | `root` | 1 | the single key whose `sha256(pk)[0..12]` equals `link_pin` |
| `0x06` | sealed directive | `online` for the outer frame; the inner `0x03` is verified again in full under its own row | `roles[2].thr` | `roles[2].ks` |
| `0x08` | reserve pool | `root` | `roles[1].thr` of the trusted key document | `roles[1].ks` of the trusted key document |

> A verifier MUST resolve the required role from the `doc_type` byte in the frame, and MUST read the key set and the threshold for that role from the previously trusted key document. It MUST NOT read them from the document under verification, except under the rotation rule, which requires both. INV-4.
>
> There MUST be no code path, no API endpoint and no serialization anywhere in the panel, the core or the client that returns or stores a public key without its role. INV-4.

Three consequences an implementer must encode:

1. **Role lives only in `roles`.** A key entry in `keys` carries no role field. A key document in which some entry of `keys` is not referenced by any `ks` MUST be rejected at parse step P11, and every `kid` in every `ks` MUST appear in `keys`. One authority for role means the two cannot disagree.
2. **An unauthorized signature rejects the whole frame.** A slot whose `keyid_trunc` is not in the authorized key set is not skipped and does not merely fail to count toward the threshold. The frame is rejected (`03-WIRE.md` 1.4). A frame carrying an unauthorized signature is a frame someone tried to launder.
3. **Revocation is checked before signature arithmetic.** Verification step V5 precedes V6. A revoked key never gets its signature verified, so a client cannot be steered into accepting a document whose signer it has already been told to reject.

The attack this table exists to stop: an adversary holding the online signing key mints a key document at version N+1 containing only their own key under role `root`, the client's high-water mark advances, and the operator can never recover. That voids the design's headline compromise-recovery property.

---

## 4. Field registry

Types, byte encodings and caps are in `03-WIRE.md` section 8. This section is the semantic registry: cardinality in context, who writes each field, what the client does with it, and the reserved ranges.

### 4.1 Reserved ranges and the extension rule

`03-WIRE.md` 3.3 defines the key ranges. Restated here because it governs every table below:

- Keys **1..63** are **critical**. An unrecognized key in this range MUST cause a parse failure.
- Keys **64..1023** are **non-critical**. An unrecognized key in this range MUST be ignored, and MUST still satisfy the ordering, minimality and size rules.
- Key 0 and keys **1024 and above** MUST be rejected.

> Any future field whose misinterpretation could weaken a security property MUST be allocated in the critical range, so an old client refuses rather than silently ignores. Any future field that is purely additive MUST be allocated in the non-critical range. A field MUST NOT be moved between ranges; moving one is a `spec_version` bump.

**Value vocabularies are not key ranges.** An unrecognized *value* in a field whose type is one of the enumerations of `03-WIRE.md` section 5 is a parse failure. An unrecognized *value* in a field that is charset-constrained but not enumerated (a node `id`, a route `id`, a deprecation `s`, an `rc` code) MUST NOT be a parse failure; the client validates the charset and length, then ignores or generically renders what it does not know. The two rules are different and conflating them is how a panel that adds a routing preset breaks every fielded client at once. Every field below states which of the two applies.

**Reserved and not to be used in v1:**

| Location | Key | Reserved for |
|---|---|---|
| Common envelope | 6, 7, 8 | future envelope fields; MUST NOT appear in v1 |
| Key document | 14 | nothing; permanently unused, see Correction 6 of `03-WIRE.md` section 16 |
| `doc_type` | `0x07` | the TUF timestamp role (`01-DECISION.md` A2); not emitted in v1 |
| `role` enum | 3 | `timestamp`; MUST NOT appear in v1 |
| `cap` bits | 12..31 | future capabilities; a signer MUST emit zero, a v1 verifier MUST ignore |
| Catalog | 27..63 | per-cohort transport assignment (`01-DECISION.md` A10) |

### 4.2 Common envelope, every document

| Key | Name | Cardinality | Written by | Semantics |
|---|---|---|---|---|
| 1 | `v` | exactly 1 | panel | Specification version. MUST equal `1`. A verifier MUST reject any other value at parse step P10, before any key lookup, because a future version may redefine every key below. |
| 2 | `pid` | exactly 1 | panel | Tenant identity. Checked byte-equal against the pinned `pid` at V8. |
| 3 | `ver` | exactly 1 | panel | Monotonic document version, scoped per section 5.1. What generates it is per `doc_type` and is stated in section 4.9, because a version nobody can generate monotonically is a check nobody passes. |
| 4 | `iat` | exactly 1 | panel | Issued at, Unix seconds UTC. |
| 5 | `exp` | exactly 1 | panel | Expires at, Unix seconds UTC. The panel MUST NOT sign a document whose `exp - iat` exceeds the lifetime for its `doc_type` in section 5.2; it MAY sign a shorter one. |
| 9 | `pd` | 0 or 1 | panel | Padding. All bytes MUST be `0x00`; a non-zero byte is a parse failure, so the field cannot be a covert channel from a hostile signer. Ignored on decode. |

### 4.3 Key document, `0x01`

| Key | Name | Cardinality | Written by | Semantics and client behavior |
|---|---|---|---|---|
| 10 | `keys` | 1..16 entries | operator, offline | Every public key this tenant uses. Each entry `{kid, alg, pk}`. `kid` MUST equal `sha256(pk)[0..12]` and a verifier MUST recompute it. Every `pk` MUST pass all three clauses of `03-WIRE.md` 2.1 at parse step P12; this is the only point at which key material enters the trusted set. Every entry MUST be referenced by at least one role. |
| 11 | `roles` | 1..3 pairs | operator, offline | Role to `{ks, thr}`. Role `1` (`root`) MUST be present in every key document. Role `2` (`online`) MUST be present in any key document a tenant serving catalogs publishes. Role `3` MUST NOT appear in v1. `thr` is in `1..16` and MUST NOT exceed `len(ks)`. |
| 12 | `rev` | 0 or 1 | operator, offline | `{kids, nodes}`. `kids` is up to 64 revoked `bstr(12)` key ids; `nodes` is up to 256 revoked node id strings. Semantics in section 10.6. |
| 13 | `tiers` | 0..16 pairs | operator, offline | Tier id to the `chash` of that tier's catalog. Publishing it in a root-signed document is what stops a compromised online key minting a per-user catalog, which is the tracking-beacon channel content addressing would otherwise hide (`01-DECISION.md` 5.2.4). See the two rules below. |
| 15 | `dep` | 0..16 entries | operator, offline | Deprecations, section 11. |
| 16 | `ttlk` | 0 or 1 | operator, offline | Seconds until the client should refetch this document, `300..86400`. Default when absent: 21600 (6 hours). Jittered by the same rule as `ttl` (section 5.6). |

**`tiers`: optional to decode, mandatory to emit.**

> A conforming panel MUST publish a `tiers` entry for every tier it serves, and MUST re-sign the key document whenever any served tier's catalog `chash` changes. This is the single highest-value operator action in the protocol: without it a compromised online signing key can invent an entire fleet, including exits it controls, `pin` entries for its own certificates, and a rule-set that routes chosen domains DIRECT while the tunnel shows connected (`04-THREAT-MODEL.md` 2.3, 7.2 step 6, 7.3 step 5).
>
> A client MUST check `sha256(catalog frame)` against `tiers[tier]` at verification step V14b when the entry is present, and MUST refuse the catalog on mismatch. When the trusted key document carries no `tiers` entry for the directive's `tier`, the client MUST NOT refuse the catalog on that ground; it MUST record the reduced containment in the verification chrome using the exact string **`fleet not root-anchored`** (section 8.8.2), and MUST NOT present the fleet as verified anywhere in the UI.

The field is typed optional in `03-WIRE.md` 8.1 so that a verifier tolerates a panel that has not yet published tier hashes rather than refusing every catalog on that tenant. The tolerance is a compatibility affordance, not a licence: the panel obligation above is unconditional, and `06-MIGRATION.md` phase 2 entry is where it is checked.

**What publishing `tiers` costs, stated so it is not discovered during an incident.** Because V14b compares against a root signature, a panel MUST NOT point a directive at a catalog whose `chash` is not in the current key document's `tiers` for that tier. A fleet change therefore runs: content digest changes, the panel signs and persists the new catalog (`03-WIRE.md` 1.5), a root-signed key document naming the new `chash` is imported, and only then do directives move. Until then the panel keeps serving the previous catalog. An operator SHOULD run a scheduled root signing at least weekly so a fleet change lands within one cycle.

### 4.4 Catalog, `0x02`

| Key | Name | Cardinality | Written by | Semantics and client behavior |
|---|---|---|---|---|
| 10 | `tier` | exactly 1 | panel | Plan tier this catalog serves. MUST equal the directive's `tier` for the client to accept the pairing. Derived from the panel's `plans.id` by a rule the panel states once and never changes; the panel MUST refuse to sign for a plan whose derived tier exceeds 65535, and MUST refuse to serve CSM routes for a tenant with more than 16 tiers, because `tiers` is capped at 16 pairs and a tier with no published hash has no V14b. |
| 11 | `ex` | 1..512 entries | panel | Exit nodes, section 4.5. A catalog with an empty `ex` MUST be rejected: a tier with no exits is a signing bug, not a state. |
| 12 | `re` | 0..64 entries | panel | Relay nodes, same entry shape. A node listed in `re` MUST NOT also appear in `ex`. |
| 13 | `ro` | 0..32 entries | panel | Routing presets offered to this tier, section 7.3. |
| 14 | `cap` | exactly 1 | panel | Operator capability bitfield, section 6. |
| 15 | `mir` | 0..32 entries | panel | Signed mirror pool for this cohort. The panel MUST reject a pool with fewer than three distinct `asn` values and two distinct `cc` values at save time, resolving both server-side rather than trusting an operator-typed label (`01-DECISION.md` D6). |
| 16 | `doh` | 0..8 entries | panel | Bootstrap DoH endpoints. Each `doh.h` MUST also appear as a `mir.h`. The consequence is stated rather than hidden: the operator must run or front its own DoH resolver, and a third-party public resolver cannot be named in a CSM/1 catalog. |
| 17 | `rs` | 0..32 entries | panel | Rule-set providers with a sha256 per file. |
| 18 | `geo` | 0..8 entries | panel | Geo databases with a sha256 per file. |
| 19 | `ttl` | exactly 1 | panel | Directive refresh cadence in explicit seconds, `300..86400`. Section 5.6. |
| 20 | `jit` | exactly 1 | panel | Refresh jitter as a percent of `ttl`, `0..50`. |
| 21 | `thr` | exactly 1 | panel | Signed size thresholds `{conn_bytes, conn_packets, resp_max}`. Section 8.6. |
| 22 | `pb` | exactly 1 | panel | Per-tenant padding bucket range. |
| 23 | `lad` | 0 or 1 | panel | Ladder defaults `{ord, en}`. Section 8.3. |
| 24 | `pin` | 0..32 entries | panel | TLS SPKI pins for manifest hosts. |
| 25 | `hpk` | 0 or 1 | panel | The **panel's** HPKE recipient public key. This is not the device's key and MUST NOT be used as the recipient of a `0x06`. Section 10.2 and Correction 7. |
| 26 | `hpkv` | 0 or 1 | panel | Generation of `hpk`. MUST be present when `hpk` is present and MUST be absent when it is absent. |

> A client MUST refuse to load a rule-set or geo file whose sha256 does not match the `h` of its `rs` or `geo` entry. INV-12. This is the only integrity anyone provides for the data that decides which packets enter the tunnel: verified as absent today, `libs/caramba-core/routing/routing.go:186-192` emits providers as `{type, behavior, format, url, interval}` with no hash, no signature and no pinning, and `component/geodata/init.go:71-88` downloads direct from GitHub.

> **A signed hash and a mutable mirror cannot both be right, and today the panel's mirror is mutable.** `sync_rulesets` re-downloads from upstream and rewrites `RULESETS_DIR` on a twelve-hour loop (`apps/caramba-panel/src/handlers/rulesets.rs:176`, spawned at `apps/caramba-panel/src/main.rs:838-846` with `tokio::time::sleep(Duration::from_secs(12 * 60 * 60))` at `:844`), and upstream lists change constantly. A catalog is signed for 30 days. Within twelve hours of any signature the served bytes stop matching `h`, and every CSM client then refuses to load routing rules while the tunnel is up.
>
> The resolution is content addressing, and it is a panel deliverable: rule-set and geo files MUST be served from an immutable, content-addressed store at `/rulesets/{name}/{sha256}` and `/geo/{name}/{sha256}`, so a sync writes a **new** object rather than overwriting one, and a signed hash never goes stale. `rs.u` and `geo.u` name the content-addressed path. Until that store exists, a panel MUST NOT emit `rs` or `geo` entries for any file under an automatic sync, and MUST leave capability bit 6 clear, which makes the client fall back to mihomo's built-in geosite and geoip data (section 6.3). `06-MIGRATION.md` 3.2 carries it as a P3 deliverable and 3.3 P9 carries the renderer half.

#### 4.4.1 Catalog membership: which inbounds become entries

The catalog and the legacy Clash body must describe the same proxy set, or the byte-diff gate of `06-MIGRATION.md` 3.5 and the P9 identical-proxy-name fixture have no defined input. The Clash generator filters materially and the catalog MUST filter identically:

| Inbound | In the Clash body | In the catalog |
|---|---|---|
| disabled | skipped (`apps/caramba-panel/src/singbox/subscription_generator.rs:1157`) | omitted |
| `amneziawg`, with `amneziawg_client_enabled()` false | skipped (`:1173-1177`) | omitted |
| `amneziawg`, with the setting true | emitted as a wireguard outbound | emitted, `pr = 8` |
| `xhttp` or `splithttp` | skipped outright, because mihomo does not support the transport (`:1180-1188`) | **omitted** |
| everything else enabled and in the plan's node group | emitted | emitted |

> A panel MUST NOT emit a node entry with `nw = 5` (`xhttp`) in a catalog served to a mihomo renderer, because no renderer on the client can dial it, and a control offering a node the engine cannot use is the same lie as a capability bit set over an absent feature. The `nw = 5` value stays in the enumeration of `03-WIRE.md` section 5 because the panel's own model uses it and a future renderer may; a v1 client that receives one MUST omit that entry from its rendered configuration and MUST record the omission on the diagnostics screen rather than failing the catalog.
>
> The AmneziaWG gate is expressed as omission, not as a capability bit. A capability bit would tell the client the operator has a feature it cannot see the nodes for; omission tells it the truth.

### 4.5 Node entry

Field types, caps and the measured sizes are `03-WIRE.md` 8.2.1. The semantic rules:

| Field | Cardinality | Rule |
|---|---|---|
| `id` | exactly 1 | The stable machine identity, `n<node_id>i<inbound_id>`, charset `[0-9A-Za-z_-]`, 1..24. It is the key for selection, pinning, probing and autotune. `id` MUST NOT equal the string `default`, which is the reset sentinel of section 7.5. |
| `pn` | exactly 1 | The verbatim mihomo proxy name, byte-identical to what the Rust generator emits (`apps/caramba-panel/src/singbox/subscription_generator.rs:1165`). It preserves `Server.ID == Server.Name == the Clash proxy name` and the flag-emoji country decoding (`libs/caramba-core/subscription/subscription.go:31-48`). |
| `cc` | exactly 1 | ISO 3166-1 alpha-2, uppercase. First-class, alongside `pn`, so a display-name change stops breaking server pinning, the prober and autotune at once (`01-DECISION.md` 5.2.5). |
| `h`, `p`, `pr`, `nw`, `se` | exactly 1 each | The connection tuple. Mandatory on every entry. |
| everything else | 0 or 1 | Protocol-conditional. A renderer MUST omit an absent optional field entirely rather than emit an empty value. `fl = 0` in particular means the renderer MUST omit the `flow` key, not emit an empty string; the panel comment at `subscription_generator.rs:230-232` records that an empty `flow` breaks Happ, and the Go renderer must reproduce the omission. |
| `rl` | 0 or 1 | The `id` of the relay entry in `re` this exit chains through. MUST name an entry that exists in `re` of the same catalog. |

> `pn` is not a key and MUST NOT be used as one. `generate_clash_config` has no proxy-name uniquifier, unlike the sing-box path's `unique_tag` closure at `subscription_generator.rs:1566`, so two inbounds of the same protocol shape on the same country's node produce the same `pn` today. `01-DECISION.md` 5.2.5 requires the uniquifier to be fixed in the same pass and the Go renderer to reuse the identical algorithm, with a fixture asserting Rust and Go emit identical proxy names for the same node set. Until that lands, `id` is the only safe key and this specification makes it mandatory for that reason.

### 4.6 Directive, `0x03`

| Key | Name | Cardinality | Written by | Semantics and client behavior |
|---|---|---|---|---|
| 10 | `nonce` | exactly 1 | panel, echoing client | Checked byte-equal against the outstanding nonce at V13. |
| 11 | `dtp` | exactly 1 | panel | Checked byte-equal against this device's thumbprint at V13. |
| 12 | `st` | exactly 1 | panel | Status, closed enumeration of `03-WIRE.md` section 5. Section 4.6.1. |
| 13 | `rc` | 0 or 1 | panel | Machine reason code. Range-structured, not enumerated: an unrecognized value MUST render as the generic text for its `st` and MUST NOT be a parse failure. `rc` is never rendered verbatim to the user. |
| 14 | `cat` | exactly 1 | panel | The `chash` of the catalog this directive is bound to. |
| 15 | `cn` | exactly 1 | panel | The chunk count that catalog is served in, `1..64`. |
| 16 | `tier` | exactly 1 | panel | MUST equal the bound catalog's `tier`. |
| 17 | `cap` | exactly 1 | panel | Capability echo, section 6.5. |
| 18 | `sel` | 0 or 1 | panel, from user and operator writes | Authoritative resolved selection, section 7.4. **Mandatory whenever the panel flag `csm_geo_pinned` is on**, with `rcc` and `nid` mandatory inside it (`03-WIRE.md` 8.3): a panel that omits it is otherwise conformant and silently reintroduces the GeoIP dependence `01-DECISION.md` P6 exists to remove. |
| 19 | `pol` | 0 or 1 | panel, from user and operator writes | Authoritative settings state with provenance, section 7. |
| 20 | `ann` | 0 or 1 | operator | Announce text, inert, render-only, never persisted, never echoed. |
| 21 | `sup` | 0 or 1 | operator | Support contact text, inert, render-only. This is the signed replacement for the `support_url` field that `GET /api/v2/app/branding` serves unauthenticated today and that the client opens with `LaunchMode.externalApplication` (`apps/caramba-client/lib/features/branding/powered_by.dart:126`). It MUST NOT be opened. |
| 22 | `ui` | 0..4 entries | operator | UI hints as `{kind, inert text}`. |
| 23 | `ttl` | exactly 1 | panel | Refresh cadence for the directive, overriding the catalog's `ttl` when they differ. |
| 24 | `exph` | 0 or 1 | operator | Offline grace window in seconds, `0..2592000`. Default when absent: 604800 (7 days). The client clamps it: section 8.6, `EXPH_FLOOR`. |
| 25 | `loc` | exactly 1 | panel | The current locator. When it differs from the locator the client used for this request, the client MUST persist the new one and use it for the next request. This is how a rotation reaches a device that still holds the old locator. |
| 26 | `traf` | 0 or 1 | panel | Signed traffic counters `{up, dn, tot, exp}`, the signed replacement for the `Subscription-Userinfo` header (`apps/caramba-panel/src/subscription.rs:832-835`), which keeps being emitted unchanged on `/sub/{uuid}` for Hiddify, v2rayNG and sing-box. |

#### 4.6.1 Status and what the client does with it

`st` decides what the client offers, and it is the only thing that does. A refusal reaches the client as a signed `st` inside a 200 response, never as a status line, because signed fields survive caching and mirrors in a way a status line does not (`01-DECISION.md` 5.2.8). This replaces the bare 403 text body at `apps/caramba-panel/src/subscription.rs:183` and `:263`.

| `st` | Name | May connect | Client behavior |
|---|---|---|---|
| 1 | `pending_approval` | no | Neutral status line, no call to action. The Free-instance manual approval state. |
| 2 | `onboarding` | yes | Connects normally. This is the designed payment gap crossing: a new unpaid user connects on the onboarding grant and reaches whatever payment channel the operator runs, which is the only channel reliably available from inside Russia when Telegram is blocked (`01-DECISION.md` 5.6.2). |
| 3 | `active` | yes | Normal operation. |
| 4 | `expired` | no | Neutral status line. No price, no bot handle, no purchase link, no "buy" call to action, in any storefront (`01-DECISION.md` 5.6.1). |
| 5 | `revoked` | no | Neutral status line. The client MUST stop offering to connect immediately, and MUST NOT fall back to a cached `active` directive to keep connecting: `revoked` is the one status that overrides the cached-document rule, because otherwise revocation is unenforceable for the length of the grace window. |
| 6 | `suspended` | no | As `expired`. |
| 7 | `quota_exceeded` | no | As `expired`, with the `traf` counters rendered. |
| 8 | `device_limit` | no | As `expired`, with a pointer to the device list the account already exposes at `GET /api/v2/app/devices`. |

> `st = 5` (`revoked`) is the only status that survives into the cache. When a directive with `st = 5` has been verified, the client MUST persist that fact against the profile and MUST NOT connect on any earlier cached directive, even one that is unexpired and says `active`. Every other status is superseded normally by the next verified directive.
>
> `st = 5` is also the only signed value that MUST tear down a **running** tunnel, and it MUST do so at the moment it is verified. INV-16 governs expiry and is absolute for expiry; revocation is not expiry. A revoked subscription that keeps carrying traffic until the user happens to disconnect makes revocation unenforceable for the length of a session, which is the failure the persisted flag above exists to prevent, and leaving the tunnel up would only move the same problem from the cache to the connection. The client MUST name the reason in the disconnect notice and MUST NOT offer a reconnect affordance while the flag is set. The authenticity failures of `06-MIGRATION.md` 6.5 are the opposite case and MUST NOT disconnect.

#### 4.6.2 Mapping the panel's own subscription state onto `st` and `rc`

`st` is a closed critical enumeration whose unknown values are a parse failure (`03-WIRE.md` section 5), so every state the panel can be in has to have a value here. The live `subscriptions.status` vocabulary is four strings, verified by inspection of every literal in `apps/caramba-panel/src` and `libs/caramba-db/migrations`: `pending`, `active`, `throttled` and `expired`. `throttled` is the free plan's temporary daily block and it is a first-class state, not an error: `apps/caramba-panel/src/api/v2/app.rs:73-79` counts a throttled subscription as the user's current plan, and `apps/caramba-panel/src/api/client.rs:833` treats it the same way.

| Panel condition | `st` | `rc` | May connect |
|---|---|---|---|
| `subscriptions.status = 'pending'` | 1 `pending_approval` | 1001 | no |
| `subscriptions.status = 'active'`, quota fine | 3 `active` | 0 | yes |
| `subscriptions.status = 'active'`, onboarding grant in force | 2 `onboarding` | 0 | yes |
| `subscriptions.status = 'throttled'` | 7 `quota_exceeded` | **3003** daily allowance exhausted | no |
| `subscriptions.status = 'expired'` | 4 `expired` | 2001 | no |
| `ensure_subscription_within_quota` returns false | 7 `quota_exceeded` | 3001 | no |
| onboarding grant exhausted | 7 `quota_exceeded` | 3002 | no |
| device limit reached on the config path | 8 `device_limit` | 4001 | no |
| device revoked by the user, or by the operator | 5 `revoked` | 4002, 4003 | no |
| account closed or suspended by the operator | 5 `revoked`, 6 `suspended` | 1003, 1002 | no |

`rc = 3003` is added to the `rc` registry of `03-WIRE.md` section 5 by this mapping. It is in the non-normative extension space of the quota range, so an older client renders the generic `quota_exceeded` text for it and does not fail, which is exactly what that range is for.

> Every row above replaces a bare 403 with a signed 200. `apps/caramba-panel/src/subscription.rs:182` currently rejects any status that is not `active`, which sends `throttled` and `pending` users a text body they cannot act on. On the CSM routes the refusal is the signed `st` and `rc` above; `/sub/{uuid}` keeps its text bodies unchanged forever (`06-MIGRATION.md` 6.1).

### 4.7 What generates `ver`, per document type

`ver` is checked at V9 against a high-water mark scoped per section 5.1, and the panel has to be able to produce a value that is monotonic in that scope, from more than one process, without a race. The generator is not the same for every type and no earlier draft said what any of them was.

| `doc_type` | Scope of the high-water mark | Generated by |
|---|---|---|
| `0x01` key document | empty, per `pid` | a counter in `csm_root_docs`; the import step of `06-MIGRATION.md` 3.7 refuses anything that is not exactly `current + 1`, which is also the rotation rule of section 10.5 |
| `0x02`, `0x04` catalog and chunk | `cat_id`, so V9 is inert (section 5.1) | a per-tier content-change counter, incremented exactly when the tier's content digest changes (`03-WIRE.md` 1.5, 6.3). It orders two catalogs for one tier in the chrome and for the operator; it is not a freshness check. |
| `0x03`, `0x06` directive | the **locator** | a per-locator counter column, incremented under the same transaction that signs, with a unique index on `(locator, ver)` |
| `0x05` bootstrap blob | empty, per `pid` | the batch number of the offline signing run that produced it (`06-MIGRATION.md` 3.7) |
| `0x08` reserve pool | the locator | a per-cohort counter, since one signed frame serves every locator in a cohort |

> **The directive counter is per locator and a locator is per subscription, while directives are per device.** Several devices on one subscription therefore draw from one sequence, which is correct and is the point: the high-water mark's scope is the locator, so device A accepting version 41 and device B then accepting version 40 would be a rollback on B and must be refused. The panel MUST allocate the next `ver` for a locator atomically, under the same transaction that records the signature, and MUST NOT derive it from a clock. Two devices refreshing in the same second get two consecutive versions, and each device's own high-water mark advances past the other's without either rejecting anything, because both are above what that device last held.
>
> The consequence an operator sees: a subscription with many devices burns versions quickly. At the default `ttl` a ten-device subscription reaches roughly 43800 versions a year, which is four orders of magnitude below the `< 2^32` cap of `03-WIRE.md` 8.0.

### 4.8 Catalog chunk, `0x04`; bootstrap blob, `0x05`; reserve pool, `0x08`

Chunk semantics are entirely in `03-WIRE.md` 8.4 and need no additional protocol rules: the chunk carries a slice of a complete catalog frame, each chunk is independently signed, and reassembly is checked against `cid` and `tl` before the reassembled bytes are verified again in full.

Bootstrap blob semantics are in section 9.7. Reserve pool semantics are in section 8.1 under rung R2.

### 4.9 Sealed directive, `0x06`

Outer fields and the HPKE suite are `03-WIRE.md` section 9. The protocol rules that are not byte-level:

1. The outer frame's signature is verified before any asymmetric seal work. A mirror serving garbage is caught by the cheap check first.
2. A `dtp` mismatch is a correctness failure, not a security event, and MUST NOT be reported as tampering. The seal would fail anyway; the check exists so the failure is legible.
3. The inner frame is verified in full, from parse step P1 through V14b. The outer verification shortcuts nothing. In particular the nonce check and the version rule run against the inner frame.
4. A client that holds no private key for the outer `rkv` MUST treat this as `E_SEAL_RECIPIENT` and MUST trigger the rekey path of section 10.3 rather than failing permanently.

---

## 5. Freshness

Three independent mechanisms, and they are not interchangeable (`01-DECISION.md` 5.2.3). A wrong clock defeats expiry. A hostile mirror replaying a captured document defeats nothing but is stopped by the version rule for catalogs and key documents, and by the nonce for directives. An adversary who can withhold refreshes defeats the version rule and is stopped only by expiry, and only up to the grace window.

### 5.1 Version

The high-water mark is stored per `(pid, doc_type, scope)`, where `scope` is the locator for `0x03` and `0x08`, the `cat_id` for `0x02` and `0x04`, and empty for `0x01` and `0x05`.

> - `ver < hwm` MUST be rejected.
> - `ver == hwm` MUST be rejected unless the frame is byte-identical to the stored frame for that tuple, in which case it is accepted as the same document and no state changes.
> - `ver > hwm` is accepted, and `hwm` is advanced to `ver` only after every remaining verification step has passed.
>
> The high-water mark is persisted and monotonic. INV-9, as corrected in `03-WIRE.md` 6.3.

> **V9 is inert for `0x02` and `0x04` and the specification says so rather than implying a protection it does not have.** `cat_id` is derived from the catalog's own bytes, so two distinct catalogs never share a scope and an older catalog is always evaluated against an `hwm` of 0. The anti-rollback bound for a catalog is verification step V14a plus the monotonicity of the **directive** that named it, because a catalog is only ever entered at `verified` when a verified directive named its `chash` (section 2.3). Scoping the catalog high-water mark by `tier` instead would make V9 fire and is rejected: it would permanently refuse a legitimate operator revert to a previously published, still-valid catalog, which the persisted-frame rule of `03-WIRE.md` 1.5 makes a normal operation. `04-THREAT-MODEL.md` 2.1 carries the corrected claim.

> **The high-water mark MUST live in exactly one store, in the app process.** On iOS `PacketTunnelProvider.swift:147,166` builds a Go core inside the Network Extension while `CarambaVpnPlugin.swift:313-318` builds a second one with a separate work directory. Two work directories are two high-water marks, which is a rollback hole and not defence in depth (`01-DECISION.md` X3). There is one fetcher, one verifier and one monotonic store, they live in the app process, and the extension receives a rendered configuration plus a validity window. The precedent exists in the same file: `rawMode` already has the app fetch and hand bytes to the extension.

### 5.2 Expiry

Lifetimes, and the maximum a panel may sign:

| `doc_type` | `LIFETIME_MAX` seconds |
|---|---|
| `0x01` key document | 604800 |
| `0x02` catalog | 2592000 |
| `0x03` directive | 3600 |
| `0x04` catalog chunk | 2592000 |
| `0x05` bootstrap blob | 2592000 |
| `0x06` sealed directive | 3600 |
| `0x08` reserve pool | 604800 |

Skew tolerance is 300 seconds in both directions.

> An expired document MUST NOT disconnect a user, MUST NOT tear down a tunnel, and MUST NOT clear a cached configuration. Expiry means the document is refused for accepting **new instructions and new status**. A cached, expired, previously verified document remains valid for connecting. INV-16, and this one is absolute.

The one bound on that absoluteness is `exph`, the operator's offline grace window, which is a separate dial and a separate state (`grace_exhausted`, section 2.1), not a consequence of expiry.

### 5.3 Nonce

The client generates 16 bytes from a cryptographically secure random source per directive request, sends them as `?n=` in base32 Crockford, and checks the echo byte for byte. A nonce MUST NOT be reused across requests, MUST NOT be derived from the device thumbprint, the clock or a counter, and MUST be discarded 300 seconds after it was sent.

Exactly one nonce is outstanding per profile. A client that issues a second directive request before the first completes MUST abandon the first nonce; a reply carrying an abandoned nonce MUST be rejected at V13.

### 5.4 The first-run time anchor

On first run there is no trusted clock. A factory-reset device on a network where DNS blocking prevents NTP is the normal case, not the exception.

> The client MUST establish `time_floor` at enrollment as the highest `iat` of any document successfully verified during enrollment. `time_floor` MUST be persisted per profile, MUST be monotonic, and MUST NEVER decrease.
>
> After enrollment, `time_floor` advances on, and only on, acceptance of a `0x03` directive: `time_floor = max(time_floor, directive.iat)`.
>
> Verification step V11 tests, for every document type:
>
> ```
> iat + LIFETIME_MAX[doc_type] + 300 >= time_floor
> ```
>
> A document that fails this test had already expired at the moment the profile last heard from the panel, and MUST be rejected.

**Correction 1 to `03-WIRE.md` 6.4 and `01-DECISION.md` 5.2.3.** Both say `time_floor` is the greater of the enrollment-time server `Date` header and the highest observed `iat`. The `Date` header is unsigned and attacker-controlled: a hostile mirror that returns `Date: Sat, 01 Jan 2101 00:00:00 GMT` sets a floor that no legitimately signed document can ever clear, and the floor never decreases, so the profile is permanently bricked by one response from one rung. The floor is therefore derived from signed `iat` values only. The `Date` header retains a narrower job, in section 5.5.

**Correction 2 to `03-WIRE.md` 6.2 step V11.** V11 as written tests `iat >= time_floor`. That rejects every cached document older than the newest one seen, so a valid 20-day-old catalog is refused the moment a fresh directive advances the floor. The test above is the corrected form: the document must not have been expired at the floor, which is what the floor was for. `03-WIRE.md` V11 should be amended to match; until it is, this rule binds, and the `05-TEST-VECTORS/` corpus carries a positive fixture (a 20-day-old catalog under a fresh floor) that distinguishes the two.

> **First trust, where neither the floor nor the high-water mark exists.** At enrollment `clock_trusted` is false, so the skew clause of V11 and all of V12 are inert and an adversary in the fetch path could serve an arbitrarily old key document that still matches `link_pin`, resurrecting a revoked online key with a stale `rev` list, and then a catalog signed by it.
>
> A client MUST compile in `BUILD_EPOCH`, the Unix second at which the running build was produced. At enrollment, and only at enrollment, the client MUST classify the device clock as **plausible** when `BUILD_EPOCH <= device_clock <= BUILD_EPOCH + 315360000` (ten years). When the clock is plausible the client MUST run V11's `iat <= now + 300` clause and V12 against it for the enrollment key document, catalog and directive, exactly as if `clock_trusted` were true. When it is not plausible the client MUST refuse to complete enrollment, MUST say that the device clock is wrong and name that as the reason, MUST NOT enrol blind, and MUST NOT set the device clock itself.

This bounds first-trust replay at the key document's own 604800-second lifetime on a device whose clock is roughly right, and refuses enrollment rather than accepting an unbounded replay on a device whose clock is not. `04-THREAT-MODEL.md` residual R-2 carries what remains, and `03-WIRE.md` 6.4 states the same rule for a reader who has only that document.

### 5.5 The clock-trusted predicate

A client maintains one boolean per profile, `clock_trusted`, and one monotonic-uptime offset.

- `clock_trusted` is false until a `0x03` directive has been verified. At that point the client sets `clock_estimate = directive.iat`, records the device's monotonic uptime alongside it, and sets `clock_trusted` true.
- While `clock_trusted` is false, verification steps `iat <= now + 300` and `now <= exp + 300` MUST be skipped. Freshness rests on the nonce for the directive and on the version rule plus `time_floor` for everything else. This is what `01-DECISION.md` 5.2.3 means when it says the nonce is the only mechanism that survives a wrong clock.
- The server `Date` header MAY be used to seed a display-only clock estimate when the device clock is implausible. It MUST be clamped to `[time_floor, time_floor + 2592000]`, and it MUST NOT influence `time_floor`, any verification step, or any expiry decision.
- `clock_trusted` reverts to false if the device's wall clock moves backwards by more than 300 seconds relative to the monotonic offset, which is the factory-reset and manual-clock-change signal.

### 5.6 Refresh cadence

> Refresh is jittered polling with conditional GETs. The client MUST NOT hold a long-poll connection and MUST NOT emit a fixed-interval keepalive. INV-7 and `01-DECISION.md` 4.8.

- The directive refresh interval is drawn per cycle as `ttl * (1 + u)` where `u` is uniform in `[-jit/100, +jit/100]`. Defaults `ttl = 7200`, `jit = 20` give roughly 12 fetches a day at unpredictable moments instead of the roughly 720 a fixed two-minute period produces.
- The key document refresh interval is drawn the same way from `ttlk`, defaulting to 21600 seconds.
- The catalog is fetched only when a verified directive names a `cat` the client does not hold. Chunks are content-addressed and cacheable for a day.
- **The mirror set refreshes on its own faster cadence, independent of `ttl`**, so the rescue channel is not slowed by the privacy win (`01-DECISION.md` 5.3.6). That cadence is `min(ttl, 3600)` seconds with the same jitter, and it fetches the reserve pool at `/sub/r1/{loc}` rather than the whole catalog.
- A refresh MUST be deferred, not skipped, while the device is on a metered connection and the app is backgrounded; the interval is multiplied by 4 in that case and the deferral is visible on the diagnostics screen.
- The legacy `Profile-Update-Interval` header keeps emitting the bare string `"2"` on `/sub/{uuid}` forever, for Hiddify and sing-box, which read it as hours, and for the Go core, which parses it as minutes (`libs/caramba-core/subscription/subscription.go:183-187`). Caramba Connect reads `ttl` and ignores the header entirely. This settles the ambiguity without either legacy consumer changing. INV-7.

---

## 6. Capabilities

### 6.1 The bitfield

`cap` is 4 bytes read as a 32-bit big-endian bitfield. Bit assignments are in `03-WIRE.md` 5.1 and are not repeated. The catalog carries the operator's bitfield; the directive echoes it.

### 6.2 The intersection rule

```
effective = operator_cap AND client_cap
```

`client_cap` is compiled into the running client and is not configurable, not fetched, and not stored. A bit the client does not implement is zero in `client_cap`, so an operator advertising a capability an old client lacks yields zero, which is the safe direction. A bit the client implements but the operator does not advertise yields zero as well.

> A control gated on a capability bit MUST be hidden when the effective bit is zero. It MUST NOT be rendered as an enabled control that silently does nothing. `01-DECISION.md` B1.

The exception is the transport ladder, where INV-17 requires the opposite: an unavailable rung renders visible and disabled with its reason, never hidden. The distinction is deliberate. A rung is a promise the app makes about itself and the user is entitled to audit the whole list; a capability is a fact about the operator's deployment and a control for a thing the operator does not have is a lie about the operator.

### 6.3 What each bit gates

| Bit | Effective 1 | Effective 0 |
|---|---|---|
| 0 per-node material | The client builds its own mihomo configuration from the catalog. Changing exit, relay or preset is local and immediate, with no network. | The client falls back to the legacy config fetch at `/sub/{uuid}` with `?node_id=` and `?relay_country=`. Changing a selection requires a round trip and shows a spinner. |
| 1 sealed directives | Directives arrive as `0x06`. | **There is no bare-directive path in v1.** Every directive-bearing response in `03-WIRE.md` 13.2 is one `0x06` frame, so a v1 panel MUST set bit 1 and MUST NOT clear it (`06-MIGRATION.md` 4.7). Bit 1 clear therefore means only this: the client MUST NOT fetch a directive from any rung other than R1 (section 10.1 rule 1), and MUST record the condition in the verification chrome. No refusal code is needed and none is assigned. |
| 2 relay chaining | The relay picker is shown and writable. | The relay picker is not rendered at all. This is what stops it being a placebo while `generate_clash_config` still ignores `_relay_nodes` (`apps/caramba-panel/src/singbox/subscription_generator.rs:1138-1143`). |
| 3 settings write | Settings changes queue a signed write and sync across the user's devices. | Settings are local to this device. The UI states that, once, on the settings screen. Section 7.9. |
| 4 signed mirror pool | Rung R2 is available. | Rung R2 renders visible and disabled with reason `not_offered_by_operator`. |
| 5 DoH endpoints | Rung R3 is available. | Rung R3 renders visible and disabled with reason `not_offered_by_operator`. |
| 6 resource hashes | Rule-set and geo fetches are integrity-checked and permitted. | The client MUST NOT fetch any rule-set or geo file from a catalog-named URL, and falls back to mihomo's built-in geosite and geoip data. Refusing to fetch is the safe direction: INV-12 forbids loading a resource whose hash does not match the catalog, and a catalog with no hashes cannot satisfy it. |
| 7 deprecation channel | Deprecation notices render in Settings. | No deprecation surface. |
| 8 onboarding grant | `st = onboarding` is expected and rendered as a normal connectable state. | `st = onboarding` still connects; the bit only tells the UI whether to explain the grant. |
| 9 device enrollment | The second-device flow of section 9.6 is offered. | The account holds one device; adding another is an operator action. |
| 10 variant forwarding | `sel.variant` is honored end to end and the client appends `&variant=` to the legacy config URL. Requires a core change: `subscription.FetchOptions` carries only `NodeID` and `RelayCountry` today (`libs/caramba-core/subscription/subscription.go:114-121`). | The client MUST send no `variant` on the legacy config URL and MUST NOT expose a variant control. Section 7.3. |
| 11 port hopping | Node entries may carry `hop` and the renderer emits port ranges. | The renderer MUST ignore `hop` on every node entry. |

### 6.4 A missing capability field

> A missing `cap` on a profile that has already pinned a root key is a hard, non-dismissible error, not a downgrade to "assume everything". INV-13.

`cap` is mandatory in both the catalog and the directive, so its absence is already a parse failure at step P11. The rule is stated again here because the failure mode it closes is a one-field downgrade attack: strip `cap`, and a client that treated absence as "unknown, so allow" would re-enable every control the operator had turned off.

### 6.5 Catalog and directive disagreeing

> **The `cap` carried in the freshest verified and unexpired directive is the operator capability.** The catalog's `cap` is used only while the client holds no verified, unexpired directive, which is the first-run case and the deep-offline case. The intersection rule of section 6.2 then applies to whichever copy won.
>
> The exception is the four bits that assert the presence of catalog **content** rather than an operator policy: bit 0 (per-node material), bit 4 (mirror pool), bit 5 (DoH endpoints) and bit 6 (resource hashes). A bit in that set that is 1 in the directive but whose backing array is absent or empty in the bound catalog MUST be treated as 0. That is a statement of fact about the bytes the client holds, not a policy override, and it cannot grant a capability.
>
> The client MUST record a catalog-versus-directive disagreement in the verification chrome, so an operator can see that a device is running on an old catalog.

**Correction 11 to an earlier form of this section, which read `catalog.cap AND directive.cap`.** The AND rule is safe in one direction and broken in the other. Clearing a bit works under both rules, so the kill switch of `06-MIGRATION.md` section 4 fires either way. **Restoring** one does not: under AND, a cached catalog signed while `csm_cap_mask` was `FFFFFFFE` carries bit 0 = 0 for the whole of its 30-day life, so `06-MIGRATION.md` 4.6 ("restoring the mask returns clients to catalog rendering within the same bound as 4.3") and its phase 3 entry criterion 4, which requires observing the kill switch fire and then restoring it, are both false under it. Nothing is lost by taking the fresher copy: both documents are signed by the same `online` role key, so an adversary holding that key gains nothing he did not already have, and the content-presence carve-out above preserves the honesty the AND rule was reaching for. `03-WIRE.md` 5.1 now carries the rule for a reader who has only that document.

---

## 7. Settings

This is the section the product depends on. The user must be able to change server, relay, routing preset, protocol and the other service settings from the app, on any device, with the change taking effect immediately and syncing when the network allows.

### 7.1 Two families, one round trip

Settings live in two places in the directive and they are not the same thing.

- **`pol`** is the authoritative settings state with per-field provenance. It is the `CorePolicy` vocabulary and it is what the client applies to the core through `SetPolicyJSON`.
- **`sel`** is the panel's resolved selection: what the legacy config fetch at `/sub/{uuid}` will produce for this device. It exists because `01-DECISION.md` P6 requires geo to be resolved server-side at directive-signing time so the config fetch never consults GeoIP.

> Vocabulary is the `CorePolicy` string set (`apps/caramba-client/packages/caramba_vpn/lib/src/core_policy.dart:41-176`), never `CoreConfig` indices. `CoreConfig` stores selections as indices into client-side option lists (`apps/caramba-client/lib/state/core_config_state.dart:25-48`), which is version-fragile as a sync unit: adding one option to a picker silently reinterprets every stored value. `01-DECISION.md` 5.4.1.

> `corePolicyFrom` (`apps/caramba-client/lib/state/core_policy_mapping.dart:45-58`) is the single translation point from indices to strings, and **its inverse MUST be added in the same file** so a fetched selection repopulates the pickers. Without the inverse, settings sync is write-only, and a second device shows stale UI over correct behavior, which reads as a bug forever. `01-DECISION.md` 5.4.1.

### 7.2 The settings table

This is the closed registry. A setting not in this table does not sync, in either direction.

| Setting | User-facing control | Wire location | Scope | Operator may write | Card on operator change |
|---|---|---|---|---|---|
| exit server | server picker | `sel.exit` (+ `sel.nid`) | subscription | yes | only if user-set |
| relay | relay picker, gated on cap bit 2 | `pol[3]` (country) + `sel.relay` (node id) + `sel.rcc` (resolved) | subscription | yes | only if user-set |
| routing preset | routing picker | `pol[2]` and `sel.preset` | subscription | yes | only if user-set |
| protocol | protocol picker | `pol[1]` and `sel.proto` | subscription | yes | only if user-set |
| connection variant | none in v1 | `sel.variant` | subscription | yes | never rendered |
| network stack | advanced | `pol[4]` | device | **no** | n/a |
| MTU | advanced | `pol[5]` | device | yes | only if user-set |
| IPv6 | advanced | `pol[6]` | device | **no** | n/a |
| fake-IP | advanced | `pol[7]` | device | **no** | n/a |
| kill switch | security | `pol[8]` | device | yes | **always** |
| DNS nameservers | security | `pol[9]` | device | yes | **always** |
| DNS fallback | security | `pol[10]` | device | yes | **always** |
| split mode | split tunneling | `pol[11]` | device | yes | **always** |
| split apps | split tunneling | **never transmitted** | device | no | n/a |
| enabled transport rungs | transports screen | **never transmitted** | device | no | **always**, on any shortening |

Reading the table:

- **Scope `subscription`** means the value propagates across the user's devices, because that is the product goal (`01-DECISION.md` 5.4.4). **Scope `device`** means it does not, because app lists and network stacks are platform-specific.
- **"Operator may write: no"** means the panel MUST reject the key in an operator-initiated write, and a client that receives that key with `src = operator` MUST ignore the value and keep its local one, recording the event in the diagnostics screen. These three are device-local performance knobs with no operator-side meaning.
- **"Card: always"** means the Keep or Revert card fires unconditionally on any narrowing, regardless of provenance and regardless of whether the user ever touched the setting (section 7.7).

> `split.apps` MUST NOT be transmitted, in either direction, ever. It has no key in `pol`, it is not assignable one, and the client's own serializer enforces the omission in both directions rather than leaving it to an operator-configurable preference. INV-15. An installed application list is the most identifying thing a VPN client could upload.

> The enabled transport rung set MUST NOT be reported to the operator, per request or in aggregate. It is a live map of which circumvention rungs still work, per device and per ASN, volunteered to a party who may be compelled. `01-DECISION.md` 5.4.6. The card still fires locally when a sibling-device write would shorten it, because such a write is refused at the client, not accepted and mirrored.

### 7.3 Value vocabularies, in full

Every vocabulary below is closed. A value outside it in a `want` request MUST be rejected by the panel with 400. A value outside it in a signed `pol` or `sel` MUST cause the client to ignore that one key and record the event, not to reject the whole directive: a panel that gains a tenth routing preset must not brick every fielded client, and the alternative is exactly the brittleness that `03-WIRE.md` section 5's enumeration rule is careful to confine to genuinely enumerated fields.

**`pol[1]` protocol**, the `CorePolicy.protocol` string set. Authoritative source `libs/caramba-core/profile/profile.go:566-573`.

| Value | Meaning |
|---|---|
| `auto` | the core selects by url-test. The wire value is the literal `auto`; the core canonicalizes it to the empty string (`profile.go:583-585`). |
| `AmneziaWG` | rendered as a wireguard outbound |
| `VLESS-Reality` | |
| `VLESS` | |
| `Hysteria2` | |
| `TUIC` | |
| `Shadowsocks` | |

The Dart picker exposes `''` for auto and maps it to `auto` on the wire (`core_policy_mapping.dart:62-68`). The wire value is `auto`; the empty string MUST NOT be sent.

> The `CorePolicy` doc comment at `apps/caramba-client/packages/caramba_vpn/lib/src/core_policy.dart:95` lists the protocol vocabulary as `auto | AmneziaWG | VLESS-Reality | Hysteria2 | TUIC | Shadowsocks` and omits `VLESS`, which the core does accept (`protocolClashType`, `libs/caramba-core/profile/profile.go:566-573`). The table above is the vocabulary; the Dart doc comment is incomplete and MUST NOT be copied as the source of truth.

**`pol[2]` preset**, the routing preset id. Authoritative source `libs/caramba-core/routing/presets.go`, nine ids plus the empty string.

| Value | Line |
|---|---|
| `""` | no preset, base rules only, no country logic (`policy_json.go:83-86`) |
| `ru-smart` | `presets.go:111` |
| `ru-full` | `presets.go:138` |
| `telegram-only` | `presets.go:148` |
| `ir-smart` | `presets.go:159` |
| `by-smart` | `presets.go:178` |
| `cn-smart` | `presets.go:196` |
| `streaming` | `presets.go:206` |
| `adblock` | `presets.go:219` |
| `global` | `presets.go:226` |

> The UI identifier `full` MUST NOT appear on the wire. `kRoutingPresetWire` maps the Dart UI id `full` to the core id `ru-full` (`apps/caramba-client/lib/state/core_policy_mapping.dart:20-22`), and the wire carries the core id only. A panel that emits `full` is emitting a value no core accepts.

> A catalog route entry `ro[].id` MUST be drawn from this same vocabulary. The catalog's `ro` list is how an operator restricts which presets a tier offers, not how it invents new ones; a preset is code in the core, and code that a signed document could name into existence would be a dormant feature activated by remote data (`01-DECISION.md` 4.3). A route entry whose `id` the client does not implement MUST render visible and disabled with reason `app_version_unsupported`, and MUST NOT be selectable.

**`pol[3]` relay**, the relay country. **Three states, not two:**

| Value | Meaning |
|---|---|
| two uppercase ASCII letters | the user chose that country explicitly |
| `--` | the user chose **no relay** explicitly |
| `""` (empty) | the user has expressed no choice; the operator resolves |

The empty string is "unset", not "off". This matters because the live default is the opposite of off: with no explicit relay the panel falls back to the persisted `subscriptions.relay_country` and then to `client_cc`, and includes same-country relay chains (`apps/caramba-panel/src/subscription.rs:744-757`, the fallback arm at `:756` is `_ => client_cc.clone()`). Collapsing empty onto `--` would remove relay chaining from every subscriber who has never touched the picker, which on the live tenant is nearly all of them, because no panel endpoint has ever written `subscriptions.relay_country` except the GET side effect at `:745-751`.

> When `pol[3]` is the empty string the panel MUST still resolve and emit a concrete `sel.rcc`, and MUST mark `pol[3]` with `src = 3` (default). The predicate in section 7.4 is written against that.

**Two required core changes, named here because neither exists.** `normalizeRelay` accepts only two ASCII letters or empty (`libs/caramba-core/api/policy_json.go:193-207`), so `--` cannot pass through `CorePolicy.relay` today, and `CoreConfig` collapses "Off" and "Auto" onto the same empty string (`apps/caramba-client/lib/state/core_policy_mapping.dart:79-85`). The core needs an explicit-none representation that reaches the URL as `relay_country=none`, and `subscription.FetchOptions` needs to be able to emit it: today `if opts.RelayCountry != ""` simply omits the parameter (`libs/caramba-core/subscription/subscription.go:131-143`), which is the "unset" case and not the "none" case. Until both land, a client MUST treat `--` as unset on the legacy URL and MUST record the degradation on the diagnostics screen rather than silently sending nothing.

**`pol[4]` stack**: `gvisor`, `system`, `mixed`. `CanonicalStack` (`profile.go:597-609`). The Dart picker's `auto` maps to omitting the key, not to a wire value (`core_policy_mapping.dart:87-92`).

**`pol[5]` mtu**: `0` meaning "core default", or `576..9000`. Validated at `policy_json.go:118-127`. The Dart picker offers `1280`, `1420`, `1500` and an auto that omits the key.

**`pol[6]` ipv6**, **`pol[7]` fakeIp**, **`pol[8]` killSwitch**: booleans. Note `fakeIp` is not a boolean in the core's own state; it selects `DNS.EnhancedMode` between `fake-ip` and `redir-host` (`policy_json.go:135-141`). The wire type is the boolean, and the mapping is the core's.

**`pol[9]` dns.nameservers**, **`pol[10]` dns.fallback**: arrays of 0..8 resolver URLs, each 1..128 bytes. A resolver URL MUST use scheme `https` (DoH) or `tls` (DoT); `http://` and bare-IP plain DNS MUST be rejected (INV-8 applies to every fetch the client makes, and DNS is a fetch). The Dart presets today are `https://1.1.1.1/dns-query` with `tls://1.1.1.1:853`, the Google pair, and the AdGuard pair (`core_policy_mapping.dart:25-38`).

> **The Dart client is the enforcement point for this rule and the Go core is not.** `cleanList` only trims and drops empties (`libs/caramba-core/api/policy_json.go:211-220`), and the core's own hardcoded defaults are a DoH pair plus a DoT entry (`libs/caramba-core/profile/profile.go:167-168`), so the core will accept whatever it is handed. A resolver URL that fails the scheme test MUST be dropped by the client before `SetPolicyJSON` is called, and the drop MUST be recorded on the diagnostics screen.

**`pol[11]` split.mode**: `off`, `bypass`, `allow`. Validated at `policy_json.go:169-181`.

**`sel.variant`**: a uint index into the panel's fixed variant list, in source order, with `0` meaning none.

| Value | Panel id | Line |
|---|---|---|
| 0 | none, no variant applied | `subscription_service.rs:2040-2043` |
| 1 | `vless-reality-direct` | `connection_variants.rs:19` |
| 2 | `vless-httpupgrade-direct` | `:27` |
| 3 | `vless-httpupgrade-relay` | `:35` |
| 4 | `vless-ws-relay` | `:43` |
| 5 | `grpc-auto` | `:51` |
| 6 | `grpc-direct` | `:59` |
| 7 | `grpc-relay` | `:67` |
| 8 | `hysteria2-direct` | `:75` |
| 9 | `hysteria2-relay` | `:83` |

**Correction 3 to `03-WIRE.md` 8.3.** `03-WIRE.md` types `sel.variant` as `uint, < 2^8`, "protocol variant", with no vocabulary. The panel's variant is a string id from a fixed nine-entry list and it is matched by string equality at `apps/caramba-panel/src/singbox/connection_variants.rs:104-110`. The wire type is kept; the enumeration above is the mapping, and it is normative in both directions. A panel MUST map the uint to the string before calling `apply_connection_variant`, and MUST NOT accept the string on the wire.

**And the honest note about variant.** `apply_connection_variant` is called only from `generate_singbox` (`apps/caramba-panel/src/services/subscription_service.rs:2040-2041`). The Clash generator never sees it, and the Go core fetches `?client=clash` exclusively (`libs/caramba-core/subscription/subscription.go:132`). Therefore `variant` has no effect on anything Caramba Connect receives today, whether it renders locally from the catalog or fetches the legacy config. The client MUST NOT expose a variant control in v1.

> **Carrying `sel.variant` onto the legacy URL is conditional on capability bit 10 and on nothing else.** With bit 10 clear, which is every deployment until `caramba-sub` forwards the parameter (`03-WIRE.md` 13.7 point 4), the client MUST NOT append `&variant=`, and `06-MIGRATION.md` 4.5 point 2 says the same. With bit 10 set the client appends it, which requires the `FetchOptions` field named above. An earlier form of this paragraph said the client carries the variant through unconditionally "so a user who set it from the mini app is not silently reset"; that sentence was dead on arrival, because the panel MUST NOT set bit 10 before the forwarding lands, and it is the sentence an implementer reading section 7 in isolation would have acted on. It is withdrawn.

**`sel.rcc`**: the resolved relay country, exactly 2 characters, and always a **concrete resolution**. Either an uppercase ISO 3166-1 alpha-2 code, or the sentinel `--` meaning "no relay". It is never empty and never absent when `csm_geo_pinned` is on. The panel resolves it from the subscription's stored preference first and from a geo lookup only at enrollment or at an explicit user write; it MUST NOT resolve it from the apparent source IP of a manifest fetch, because the ladder changes that IP by construction (`03-WIRE.md` 8.3).

**Correction 4 to `03-WIRE.md` 8.3, now applied there.** `03-WIRE.md` 8.3 originally specified the literal `"NO"` as the no-relay sentinel, on the stated grounds that it "is not a valid ISO country in this position". `NO` is Norway. An operator with a Norwegian relay could not express it, and worse, a Norwegian relay selection would silently disable relaying. The sentinel is `--`, which is two characters, satisfies the exactly-2 cap, and is not an alpha-2 code under any registry. The renderer maps `--` to the URL literal `none`, which is what `apps/caramba-panel/src/subscription.rs:754` accepts (`Some("none") | Some("NONE")`). `03-WIRE.md` 8.3 and `06-MIGRATION.md` 4.5 and 7.6 have since been amended to `--`, so no document in the set still carries the retracted form.

**`sel.exit`** and **`sel.relay`**: node entry `id` values, `[0-9A-Za-z_-]{1,24}`, naming entries in the bound catalog's `ex` and `re` respectively. Neither may be the literal `default`.

**`sel.nid`**: the numeric `node_id` the legacy config fetch takes as `?node_id=`. Supplied by the panel.

> A client MUST NOT derive `sel.nid` by parsing `sel.exit`. The `id` format `n<node_id>i<inbound_id>` is documentation of how the panel builds the string, not a parsing contract, and an operator that changes it must not break every fielded client's URL construction.

### 7.4 `sel` and `pol` MUST agree

`sel` and `pol` overlap on three settings and are disjoint on the rest. The overlap is deliberate: `pol` is what the client applies to the core, `sel` is what the client puts on the legacy URL. They MUST be consistent, and the consistency is checkable, so it is checked.

**The three self-contained predicates, checked at parse.** A client MUST reject a directive, as `E_PARSE_FIELD`, when any of the following holds. Each is decidable from the incoming bytes alone, which is what `03-WIRE.md` 6.1 requires of that code.

| Predicate | |
|---|---|
| `sel.preset` and `pol[2]` are both present and differ | preset must be one value |
| `sel.proto` is present and does not equal `PROTO_WIRE[pol[1]]` | see the mapping below |
| `sel.rcc` is present and disagrees with `pol[3]` under the table below | relay country must be one value |

`sel.rcc` against `pol[3]`, given the three states of section 7.3:

| `pol[3]` | required `sel.rcc` |
|---|---|
| a 2-letter country code | that code, uppercased |
| `--` | `--` |
| `""` (unset) | any legal value, including `--`; the operator resolved it, and `pol[3].src` MUST be `3` |

**The two catalog-dependent predicates, checked after the catalog verifies, not at parse.**

> These MUST NOT be evaluated at parse and MUST NOT return `E_PARSE_FIELD`. The client learns which catalog is bound only from this directive's `cat` (section 4.6 key 14), and section 2.3 forbids entering a catalog at `verified` before a directive names it, so on first run and after any `cat` change there is no bound catalog at the moment the directive is parsed. A predicate that cannot be evaluated is not a rejection criterion.

Both are evaluated once the bound catalog has reached `verified`, and neither rejects the directive:

| Predicate | Outcome |
|---|---|
| `sel.exit` names no entry in the bound catalog's `ex` | The selection is unresolvable. The client falls back to the operator default for the exit and raises an **informational notice**, not a Keep or Revert card. This is the section 7.9 row "Operator has the capability but not this value". |
| `sel.relay` names an entry in `re` whose `cc` is not `sel.rcc`, or names no entry at all | The relay selection is unresolvable. The client falls back to the operator default for the relay and raises the same informational notice. |

Both outcomes MUST be recorded on the diagnostics screen with the offending value rendered as inert text, and both MUST be re-evaluated at the next catalog change. Neither may silently choose a different node: section 7.9's last row and `06-MIGRATION.md` 7.5 both forbid a silent substitution, because the user pinned a server for a reason the client does not know.

`PROTO_WIRE`, from the `CorePolicy` protocol string to the `pr` enumeration of `03-WIRE.md` section 5:

| `pol[1]` | `sel.proto` |
|---|---|
| `auto` | 0 |
| `VLESS-Reality` | 1 |
| `VLESS` | 1 |
| `Hysteria2` | 4 |
| `TUIC` | 5 |
| `Shadowsocks` | 6 |
| `AmneziaWG` | 8 |

> The mapping is authoritative in one direction only. `VLESS` and `VLESS-Reality` both map to `1`, so a client MUST NOT reconstruct `pol[1]` from `sel.proto`. `pol` is the source of truth for what the user chose; `sel` is a derived projection of it.

`sel.relay` is a **node id** and `pol[3]` is a **country code**. They are different fields with different types and different vocabularies, and an implementer who treats them as the same field will produce a directive that fails the consistency check above. This is stated because the field names invite the mistake.

### 7.5 Three-way patch semantics

One writer, one round trip. `want` in the request, authoritative signed `sel` and `pol` out (`01-DECISION.md` B6, 5.4.2). Request shape, headers and the proof are `03-WIRE.md` 13.6.

> - A key **absent** from `want` means unchanged.
> - A key whose value is the text string `"default"` means **reset to the operator default**.
> - Any other value means **set**.

> The reset sentinel is the text string `"default"` for **every** key, whatever that key's normal value type. A decoder MUST accept a CBOR text string in any `want` value position for this purpose alone, including where the key's type is a boolean, an unsigned integer or an array. There is no `["default"]` form and no null form.

CBOR null is forbidden by the strict decode profile (`03-WIRE.md` 3.1 rule C7), and the sentinel exists because of a concrete ABI constraint that `01-DECISION.md` B6 names: `CorePolicy.toJson` omits nulls (`core_policy.dart:158-171`) and Go's `policyPatch` uses pointers with "absent means do not change" (`libs/caramba-core/api/policy_json.go:18-36`), so explicit null is not representable in either today. The sentinel preserves both contracts unchanged.

> No value in any settings vocabulary may be the string `default`. Section 7.3 forbids it for node ids, relay ids and route ids; no `CorePolicy` value is `default`; and a panel MUST reject a node id, relay id or route id equal to `default` at catalog-signing time. Without this rule the sentinel is ambiguous exactly where it is most dangerous.

Concurrency: `If-Match` carries the `ver` of the directive the client is amending. On a stale `If-Match` the response is 409 carrying the current signed directive, which is a full frame, not a diff (`03-WIRE.md` 13.5). The client MUST merge its outstanding `want` against the returned state and retry once; on a second 409 it MUST surface the conflict to the user rather than loop.

### 7.6 Provenance and precedence

Every entry of `pol` is a two-element array `[value, src]` where `src` is `1` user, `2` operator, `3` default (`03-WIRE.md` section 5). The client additionally keeps its own record, per key, of whether the user has ever set that key explicitly on this device.

Panel-side precedence, when computing `pol[k].value`:

```
most recent accepted user write  >  operator setting  >  tenant default
```

and `src` records which of the three won.

Client-side effective value, per key:

```
if the client holds an unanswered Keep or Revert card for k:
    the client's pre-existing local value
else:
    pol[k].value
```

That is the whole rule. There is no third store and no merge: the panel decides precedence, the client obeys, and the card is the only mechanism by which the client withholds obedience.

### 7.7 Keep or Revert

> When a directive delivers, for key `k`, a value that differs from the client's current effective value, and either
>
> - `pol[k].src == 2` (operator) while the client's record says the user set `k` explicitly on this device, **or**
> - the change narrows the user's security posture, regardless of provenance,
>
> the client MUST NOT apply the change. It MUST retain its current value and raise a Keep or Revert card. INV-22.

**Narrowing of security posture** is a closed list. It is closed, and it is longer than the settings table, because two of its rows arrive in the catalog rather than in `pol`:

| Change | Arrives in | |
|---|---|---|
| `killSwitch` true to false | `pol[8]` | |
| `split.mode` from `bypass` or `allow` to `off` | `pol[11]` | |
| `dns.nameservers` or `dns.fallback` changed at all, in any direction | `pol[9]`, `pol[10]` | a repointed resolver is a repointed resolver |
| the enabled transport rung set becoming a proper subset of its current value | local only | arrives only as a local sibling-device write attempt, which is refused |
| the set of rule-set providers changes: an `rs` or `geo` entry added, removed, or its `n` changed | catalog `rs`, `geo` | section 7.7.1 |
| a resource hash `h` changes for an entry whose `n` is unchanged | catalog `rs`, `geo` | section 7.7.1 |
| a route entry's `rs` list changes | catalog `ro[].rs` | section 7.7.1 |

#### 7.7.1 Why a resource change is a narrowing

The catalog names each rule-set and geo file by path plus sha256, and INV-12 makes the client refuse anything whose hash does not match. That bounds every host in the fetch path. It does not bound the party that **signs**, because the signer chose both the path and the hash. A hostile or compromised operator can therefore publish a rule-set that routes a named set of domains DIRECT, the hash matches, INV-12 is satisfied, and the traffic leaves the device in cleartext while the tunnel shows connected.

`01-DECISION.md` C3 attributes to hashing a property hashing does not have; `04-THREAT-MODEL.md` Correction 1 records why, and 7.3 step 5 calls this the clearest breach of the constraint that a malicious operator must not harm the user beyond the VPN service they signed up for. Nothing in the wire format bounds it.

> Because nothing in the format bounds it, the client MUST. A change to the set of `rs` or `geo` entries, to any `h` within them, or to any route entry's `rs` list MUST raise the Keep or Revert card of INV-22, unconditionally and regardless of provenance, exactly as a DNS repoint does. **Keep** applies the new resources; **Revert** keeps the client on the previously verified resource set, and where the client no longer holds those bytes it falls back to mihomo's built-in geosite and geoip data rather than loading the new ones. The card MUST name the provider names that changed and MUST NOT render any operator-supplied description text on that surface (INV-10).

The card is the only defence that exists here, and it is a client-side one. It is also the reason this list is longer than the `pol` table: a narrowing does not have to arrive as a setting.

Card behavior:

- A card MUST persist until the user answers it. It MUST NOT auto-expire, MUST NOT be dismissed by navigation, and MUST NOT be answered by a timeout.
- **Keep** retains the local value, marks the key user-set, and queues a write re-asserting it.
- **Revert** applies the operator value and clears the user-set mark for that key.
- At most 3 cards may be outstanding. A fourth coalesces into the oldest, which becomes a multi-key card listing every affected setting. Cards MUST NOT be dropped.
- A card MUST name the setting, the old value, the new value, and the provenance, in the user's language. It MUST NOT render any operator-supplied text on the same surface (INV-10).

> A cross-device write MUST NOT be able to set `killSwitch`, `dns`, `split.mode` or the enabled transport set on a sibling device without that device raising the card unconditionally. `01-DECISION.md` 5.4.4. This is the cheapest real difference between a control plane and a remote control.

### 7.8 Local first, then queued

> A setting change is **accepted** locally and immediately, and the signed write queues and drains over whatever ladder rung is available, with a visible "not yet synced to your provider" state. No setting change ever blocks on the network. `01-DECISION.md` 5.4.3.

This is possible because the client holds the catalog and builds its own configuration (capability bit 0). With bit 0 clear the client must fetch, and the UI must show the fetch; that is the honest degradation and section 6.3 states it.

> "Accepted immediately" is not "in force immediately", and section 7.11 is the difference. The core applies policy at the **next** `Up`, so a change to any `pol` key that the running tunnel depends on does not take effect until a reconnect. An earlier form of this section said a setting change "applies locally and immediately", which is true of the client's own state and false of the tunnel, and that is the sentence a product breaks on.

Write queue rules:

- At most 32 queued writes. Writes coalesce per key: a second change to the same key replaces the first rather than appending.
- A queued write is dropped after 7 days undelivered, and the user is told once that the change is local only.
- The queue is persisted and survives a restart.
- A write carries a fresh nonce and a proof over `sha256(request body)` (`03-WIRE.md` 13.6). A queued write whose nonce is older than 300 seconds MUST be re-signed with a fresh nonce before sending, not sent stale.
- The signed echo in the response MUST cover every field the write can set, not a subset. `01-DECISION.md` B5 names the failure this exists to avoid: B's own proof covered method, path, nonce and session but not the body, and three of the four fields a state-CA MITM would rewrite were absent from the echo.

### 7.9 When the panel does not support a setting

Four distinct cases, four distinct behaviors. This is the part a product breaks on.

| Case | Detection | Client behavior |
|---|---|---|
| Operator has no such capability | effective capability bit is 0 | The control is hidden (section 6.2). Nothing queues. |
| Operator has the capability but not this value | the value is missing from the catalog: no such `ro` entry, no relay in that country, no node with that id | The value renders visible and disabled with reason `not_offered_by_operator`. A selection that becomes unavailable when a catalog changes falls back to the operator default for that key, and the client raises an informational notice, not a card. |
| Panel does not implement settings sync at all | capability bit 3 is 0, or `PUT /api/v2/app/preferences` returns 404 or 405 | The setting is local to this device. The settings screen states this once, plainly. Any queued writes are dropped. The client MUST NOT retry a 404 or 405 on a schedule. |
| Client does not implement a value the panel sent | the value passes charset and length but is outside this build's vocabulary | The client ignores that one key, keeps its local value, records the event on the diagnostics screen, and renders the setting as `app_version_unsupported`. It MUST NOT reject the directive and MUST NOT persist the unknown value. INV-11: the client persists and echoes only values it can validate against a closed vocabulary. |

The last row is the one that matters for a licensed protocol whose operators upgrade on their own schedule, and it is why section 7.3 makes the settings vocabularies non-fatal on unknown values while `03-WIRE.md` section 5's true enumerations stay fatal.

### 7.10 What never crosses the boundary

Restated in one place because it is enforced in three:

1. `split.apps`, in either direction (INV-15).
2. The enabled transport rung set, and which rung carried any request (`01-DECISION.md` 5.4.6).
3. Any operator-supplied opaque string that is not validated against a closed vocabulary. Announce text, support text and UI hint text are render-only: never persisted, never echoed, never used as a key (INV-11, `01-DECISION.md` C6).
4. Any client-side state report beyond the version high-water mark. The client reports `v`, the highest directive version it has accepted, and nothing else. That is one integer whose upper bound the operator already knows (`01-DECISION.md` 5.4.6).

### 7.11 Applying `pol` to the core, and what a change does to a running tunnel

Two things are true of the code this section drives and neither is visible from the wire format. `SetPolicyJSON` is atomic, all or nothing, with the whole patch rejected on one bad value ("применение атомарно", `libs/caramba-core/api/policy_json.go:50-53`), and it takes effect at the next `Up`, not on the call ("Политика применяется при СЛЕДУЮЩЕМ Up ... приложение обязано переподключиться само", `:55-56`). The app deliberately never reconnects on its own ("Приложение НЕ переподключается само: рвать работающий туннель без спроса недопустимо", `apps/caramba-client/lib/features/settings/reconnect_banner.dart:4-5`).

**How `pol` reaches the core.**

> The client MUST NOT pass `pol` to `SetPolicyJSON`. It merges `pol` into its own local settings state, applies the provenance and card rules of sections 7.6 and 7.7, and then rebuilds and sends the **whole** `CorePolicy` from that merged state.

Four consequences follow, each from a verified defect in the current path:

1. **Pre-validate before the call.** Every value MUST be checked against the vocabularies of section 7.3 before `SetPolicyJSON` is invoked. Because the call is atomic, one unknown preset id would otherwise reject `protocol`, `killSwitch`, DNS and `stack` together, which is exactly what section 7.9's last row forbids: the client is supposed to ignore the one key it does not understand and keep the rest.
2. **Never swallow the failure.** `apps/caramba-client/lib/state/vpn_state.dart:101-113` currently catches the error and sets `appliedPolicyJson = null`, so the tunnel comes up on the previous policy with no user-visible signal. A rejected policy MUST surface as a diagnostics entry and MUST block the connect action until it is resolved, because a silently ignored policy is a silently wrong kill switch.
3. **Re-attach the split app list on every write.** `CorePolicySplit.toJson` always emits `apps` (`packages/caramba_vpn/lib/src/core_policy.dart:59-64`) and `policy_json.go:158-184` rebuilds `SplitTunnel` from the patch, so sending an operator-delivered `split.mode` without the local app list erases the user's split-tunnel selection, which is the one list section 7.10 protects most strongly. The client MUST carry its own `split.apps` into every `CorePolicy` it builds, and it MUST NOT transmit that list anywhere (INV-15).
4. **`mtu` and `stack` cannot be reset remotely, and the specification says so rather than pretending.** `policy_json.go:126` applies `mtu` only `if mtu > 0` and `:113` applies `stack` only `if stack != ""`, so the `"default"` sentinel of section 7.5 has no effect for those two keys against the current core ABI. Until the core gains explicit reset values, a `"default"` on `pol[5]` or `pol[4]` MUST be treated by the client as "restore this client's own compiled default and send that value", not as "send zero and hope". The core change is listed in section 12.2.

**What a change does to a running tunnel.**

| Change | While disconnected | While connected |
|---|---|---|
| any `pol` key, user-initiated | applied to the next `Up` | the reconnect banner is raised, naming the setting; the tunnel is not torn down |
| any `pol` key, operator-initiated, no card | applied to the next `Up` | the reconnect banner is raised, naming the setting **and** the provider as its source |
| any `pol` key that raises a Keep or Revert card | nothing applies until the card is answered | nothing applies until the card is answered; **Keep** raises no banner because nothing changed, **Revert** raises the banner |
| `sel.exit` or `sel.relay` change | applied to the next `Up` | the reconnect banner is raised |
| a narrowing under section 7.7 or 7.7.1 | card first, then as above | card first, then as above |
| `st = 5` (`revoked`) | connect is refused | the tunnel is torn down immediately (section 4.6.1) |

> The client MUST NOT tear down a running tunnel for any row above except the last. It MUST raise the existing reconnect banner instead, and the banner MUST say which setting changed and whether the user or the provider changed it. Reconnecting without asking is forbidden for the reason the code comment already gives; reconnecting silently on an operator's instruction would additionally make a remote party able to interrupt a user's session at will.

---

## 8. The ladder

> The ladder is a source list: ordered, individually toggleable, geography-independent, fully enumerated on one screen. It progresses automatically on failure, and the review notes say so accurately. `01-DECISION.md` 5.3.1, D1.

> **A rung the user has switched off is never tried. Ever. Including on failure, including when every enabled rung has failed, including on a cold start with no cached documents.** There is no emergency override, no "last resort" escalation past a disabled rung, and no operator field that can re-enable one. This is absolute.

### 8.1 The rungs

| Rung | Name | What it does |
|---|---|---|
| R0 | cached signed documents | Reads the last-good verified documents from disk. Always enabled, never disableable, always first in every legal order. Costs no network. |
| R1 | direct HTTPS to the enrolled origin | One request to the pinned enrollment origin, uTLS ClientHello, SPKI-pinned. |
| R2 | signed mirrors | The `mir` pool from the catalog, plus the reserve pool from `/sub/r1/{loc}` when one has been fetched. Mirrors are drawn per cohort from a pool larger than any one cohort sees (`01-DECISION.md` D7), weighted by `w`, and the panel enforces at least three distinct ASNs and two distinct countries at save time. The reserve pool is a separate root-signed document at a locator-scoped URL precisely so that pulling it costs an adversary an enrolled subscription and a live locator rather than one anonymous request (`03-WIRE.md` Correction 6). Cohort sizing is section 8.1.1. |
| R3 | DoH-resolved address with explicit SNI | Resolves a mirror hostname through a `doh` entry, then connects to the returned or listed literal address, sends `h` as SNI, and validates the certificate against `h` plus its SPKI pins. Certificate validation is not disabled and no IP-SAN certificate is needed, because the name being validated is the hostname in SNI. This is not the bare-IP mirror that `01-DECISION.md` 4.2 rejects. |
| R4 | through the app's own tunnel | Sends the request through the running mihomo instance. Availability is platform-conditional; section 8.2. |
| R5 | user-entered SOCKS5 or HTTP proxy | A proxy the user typed in. Used for manifest and configuration fetch only, **never for tunnel traffic**. The client MUST state that on the screen where the proxy is entered. |
| R6 | out of band | QR, file or paste, in the armored form of `03-WIRE.md` section 10. Always on, never disableable, never automatic: it requires a user action by construction. |

> Nothing is conditional on geography and nothing is dormant. Every compiled rung is enumerated on one screen, including rungs unavailable on the current device or not offered by the current operator, which render visible and disabled with the reason, never hidden. INV-17, `01-DECISION.md` 5.3.2.

Reason vocabulary for a disabled or unavailable rung, closed:

| Reason | Meaning |
|---|---|
| `user_disabled` | the user turned it off |
| `not_offered_by_operator` | the capability bit or the catalog data is absent |
| `platform_unsupported` | this build on this OS cannot do it |
| `not_configured` | R5 with no proxy entered, R3 with no DoH entry |
| `app_version_unsupported` | the operator offers it and this build does not implement it |

#### 8.1.1 Cohort sizing

Cohorting only hides an individual when a cohort is large. A cohort of one is a beacon: an operator who assigns a distinct mirror set per subscriber has built the per-user equivocation channel `01-DECISION.md` A11 accepts as undetectable and pointed it at his own users.

> A cohort SHOULD contain at least **25 subscribers**. Below roughly 200 subscribers an operator SHOULD run **one** cohort and rotate it, rather than manufacture a per-user fingerprint. The 25 figure is **provisional**; the measurement that replaces it is the observed mirror burn rate against the rotation cadence, which is the same measurement `01-DECISION.md` A7 names as its own trigger.

Node cohorts are a different quantity from mirror cohorts and have a hard ceiling. A node subset is expressible only per tier, because a catalog carries a single `tier` field and its hash lives in the key document's `tiers` map, which is capped at 16 pairs (`03-WIRE.md` 8.1, key 13). **A node cohort therefore is a tier, and there are at most 16 of them.** Under uniform assignment the expected number of purchases needed to see all 16 is `16 * H(16)`, about 54. Mirror cohorts are independent of tiers, because the reserve pool carries its own `coh` (`03-WIRE.md` 8.6, key 12), so mirror enumeration and node enumeration have different prices. `04-THREAT-MODEL.md` 2.5 is the arithmetic.

### 8.2 R4 and what it actually does on each platform

> The specification says what fetch-through-tunnel does per platform rather than claiming it uniformly. `01-DECISION.md` X4.

Verified: `apps/caramba-client/packages/caramba_vpn/android/src/main/kotlin/com/caramba/caramba_vpn/CarambaVpnService.kt:253-258` calls `builder.addDisallowedApplication(packageName)`, excluding the app from its own tunnel by design, with the comment stating why. And a local mihomo listener exists only in proxy mode: the mixed inbound is constructed only when `EffectiveMode() == ModeProxy` (`libs/caramba-core/profile/profile.go:181-182, 210, 240, 262`), and the default mode is `ModeTun` (`profile.go:151`).

The consequence, stated rather than papered over: on Android, in the normal TUN mode, R4 has no path. The app is outside the tunnel and there is no loopback listener to proxy through.

> The core MUST expose a loopback mixed inbound on `127.0.0.1` in **both** tunnel modes, bound to localhost only, for the CSM fetcher's use, and the Go `HTTPDoer` for rung R4 MUST dial through it. Until that ships, R4 MUST render visible and disabled on Android with reason `platform_unsupported`, and the diagnostics screen MUST NOT report R4 success on iOS and R4 failure on Android for the same tenant on the same network without saying why.

On iOS the app is not excluded from its own tunnel, so R4 works through the system routing table once the tunnel is up. On desktop it works in proxy mode and, with the loopback listener above, in TUN mode.

R4 is unavailable whenever the tunnel is down, which is the cold-start case. It is therefore never first in a useful order, and the default `lad.ord` reflects that.

### 8.3 Order and enablement

- The default order and the default enabled set come from the signed catalog's `lad.ord` and `lad.en` (`01-DECISION.md` D3). Per-tenant defaults are how diversity lives in signed data rather than in app logic.
- `lad.ord` is a permutation of a subset of `{0..6}` with no duplicates. Rung 0 and rung 6 MUST appear in `lad.en`; a catalog that omits either MUST be rejected.
- Rung 0 MUST be first in the effective order regardless of what `lad.ord` says. Reading a verified document off disk before opening a socket is not a policy choice.
- The user may reorder and toggle. The user's order and enabled set are persisted per profile and **override the catalog defaults permanently once the user has touched them**. A later catalog MUST NOT silently restore the operator default over a user choice; that is an operator change to a user-set field and it goes through the Keep or Revert card of section 7.7, with the shortening case firing unconditionally.
- The default order, when a catalog carries no `lad`: `[0, 1, 2, 3, 4, 5, 6]`. The default enabled set: `[0, 1, 2, 3, 6]`. R4 and R5 default off because R4 needs a tunnel and R5 needs user input.

### 8.4 Selection and attempts

One fetch cycle walks the effective order once. Per rung, per cycle:

| Rung | Attempts per cycle | Selection within the rung |
|---|---|---|
| R0 | 1 | the stored last-good document |
| R1 | 1 | the pinned origin |
| R2 | up to 3 | distinct hosts, drawn without replacement, weighted by `w`, never two from the same `asn` in one cycle |
| R3 | up to 2 | distinct `doh` entries, then distinct mirror hosts under each |
| R4 | 1 | through the tunnel |
| R5 | 1 | the configured proxy |
| R6 | 0 automatic | user-initiated only |

A cycle stops at the first rung that returns a document that verifies. A cycle that reaches the end of the enabled order without one enters backoff (section 8.7) and the profile stays on cached documents.

> A verification failure does not, by itself, stop the ladder: the client MAY continue to the next rung, because a hostile mirror is exactly the case the ladder exists for. It MUST NOT, however, treat a verification failure as equivalent to an empty response when deciding what to show the user. `03-WIRE.md` 6.2.

### 8.5 Timeouts and the cycle budget

| Constant | Value |
|---|---|
| `TCP_CONNECT_TIMEOUT` | 5 s |
| `TLS_HANDSHAKE_TIMEOUT` | 5 s |
| `ATTEMPT_TIMEOUT_R1_R2_R3` | 12 s |
| `ATTEMPT_TIMEOUT_R4_R5` | 20 s |
| `CYCLE_BUDGET` | 90 s |

A cycle that exceeds `CYCLE_BUDGET` MUST abandon the remaining rungs and enter backoff; it MUST NOT extend. These replace the current unbounded behavior: `fetchSubscriptionBody` uses a bare Dio with 15 s and 20 s timeouts, `followRedirects: true`, no size cap and no retry (`apps/caramba-client/lib/data/subscription_fetch.dart:22-49`), and the Go clients use `&http.Client{Timeout: 30s}` with no transport and exactly one retry, on HTTP 401 only (`libs/caramba-core/auth/client.go:75-86, 305-350`).

Every response body MUST be read through a size-limited reader capped at `thr.resp_max`. `component/resource/vehicle.go:87-183` in vendored mihomo is the working reference for the caching and limiting parts of the same loop.

### 8.6 Connection hygiene

The rule is `03-WIRE.md` 11.2 and is not restated. Two protocol-level consequences:

> No response above 4 KB. No connection carrying more than the signed byte threshold, counted with the TLS handshake included, with a packet ceiling alongside it. Both are signed catalog fields, not compiled constants. INV-5.

`thr.conn_bytes`, `thr.conn_packets` and `thr.resp_max` are read from the trusted catalog. Before a catalog has ever been verified, the client uses the defaults 8192, 22 and 4096. **A signed value binds when it is at or below the client's compiled ceiling; where it is above, the ceiling binds.** This is what lets the correction from the missing field measurement ship as data rather than as an app release, without also letting whoever holds the online signing key steer the client into harm.

#### 8.6.1 Clamps on signed values

Every field below is chosen by whoever holds the online signing key, and a signature check does not evaluate it. These are the clamps that stand between a compromised signer and a client that harms itself on command. `04-THREAT-MODEL.md` section 4 is the analysis; this is the requirement.

| Field | Hostile value | Harm | Client bound |
|---|---|---|---|
| `thr.conn_bytes` | 65535 | the client pushes past the TSPU freeze point on one connection and hangs with no RST | MUST clamp to at most `CONN_BYTES_CEILING = 15360`; a signed value binds only when it is lower |
| `thr.conn_packets` | 255 | the same, on the packet trigger | MUST clamp to at most `CONN_PACKETS_CEILING = 25` |
| `thr.resp_max` | 49152 | defeats INV-5 and the chunking arithmetic of `03-WIRE.md` 11.3 | MUST **reject the catalog** when `thr.resp_max` exceeds 4096. INV-5 fixes the value; the field may only lower it. |
| `exph` | 0 | one poisoned directive stops the device offering to connect at all, and the value persists as the last accepted one | MUST NOT apply an `exph` below `EXPH_FLOOR = 86400` seconds, unless the user set a shorter window explicitly in Settings |
| `ttl` | 300 with `jit` 0 | 288 fetches a day on a fixed period, undoing the frequency win of `01-DECISION.md` 5.7.5 and manufacturing a flow-classifier feature | MUST enforce a floor of `TTL_FLOOR = 900` seconds, and MUST apply at least **10 percent** jitter regardless of `jit` |
| `pb` | `[0, 0]` | a per-tenant constant response size | SHOULD warn on the diagnostics screen; the panel SHOULD refuse to sign `pb == [0,0]` together with an empty `mir` |
| `lad.en` | `[0, 6]` only | the operator disables every network rung by default and the user never notices | the user's own toggles always win; a rung the user enabled is never removed by signed data, and `lad` sets defaults for a fresh profile only (section 8.3) |
| `rs`, `geo`, `ro[].rs` | a rule-set routing chosen domains DIRECT | cleartext egress with the tunnel showing connected | not bounded by any format rule; MUST raise the Keep or Revert card of section 7.7.1 |

> The general rule, which is the one to remember if the table is ever out of date: a signed field that can only make the client's situation worse MUST be clamped at the client; a signed field that can only make it better MAY be honored unclamped; where a field can do both, the client honors **the safer of the signed value and its own ceiling**.

`CONN_BYTES_CEILING` and `CONN_PACKETS_CEILING` are **provisional** and are the lower edge of the observed freeze trigger in `00-DESIGN-BRIEF.md` 2.2, roughly 25 packets or 15 to 20 KB. They change on the same measurement that governs the four provisional constants in `03-WIRE.md` 17. `EXPH_FLOOR` and `TTL_FLOOR` are policy, not measurement.

**`TTL_FLOOR` is 900, not 1800, and the reason is the kill switch.** `06-MIGRATION.md` 4.3 tells an operator anticipating a risky cutover to lower `ttl` to 900 for the cutover window, which is what puts the kill-switch adoption bound at 1140 seconds. A client-side floor of 1800 would make that acceleration silently ineffective, so the only rollback that exists after PNR-2 would be slower than the document that owns it says it is. 900 is the value at which the documented acceleration works, and it costs at most 96 fetches a day against the 288 that `ttl = 300` with no jitter would produce. The `ttl` field keeps its wire range of `300..86400` (`03-WIRE.md` 8.2 key 19); a signed value below 900 is legal to encode, and the client polls at 900.

> The catalog is chunked from v1, and the panel refuses to sign an oversized payload rather than emitting one. INV-6.

### 8.7 Backoff

Between failed cycles: 30 s, doubling, capped at 3600 s, with ±20 percent jitter, reset to 30 s on any successful cycle. Backoff is per profile, not per rung. A user-initiated refresh bypasses backoff exactly once and does not reset the counter.

### 8.8 What the user sees

> Every compiled transport rung, on one screen, with a toggle, an order, and a live per-attempt history. INV-17.

The attempt history retains the last 200 entries per profile, each carrying rung, host or an opaque mirror label, start time, outcome, and the error code from `03-WIRE.md` 6.6 where there was one. It is local, it is never uploaded, and it is the raw material for the "What this app sends" screen.

Also required, and each is an invariant:

- The operator's identity: display name, root key fingerprint in groups of four, enrollment date, whether the pin was established out of band or in app, and whether it has ever changed (INV-18).
- The verification state of the documents in use: version, issued, expires, signer fingerprint, verification result, decoded fields (INV-19).
- The "What this app sends" screen, rendering the decoded fields of the last request, with a copy button (INV-20).
- The configuration age and its source whenever the client is running on cached documents (INV-21).
- Telemetry, if it exists at all, off by default, with its contents enumerated on screen (INV-23).

#### 8.8.1 What each error code means to the user

`03-WIRE.md` 6.6 requires all three implementations to return the same code for the same fixture. That settles the wire and settles nothing above it: three UI teams reading the same corpus would otherwise render three different things for one fixture, which is the same class of divergence one layer up. This table is the mapping, and it is normative.

The **string class** is what the user is told, not a literal string. Four classes, and the distinction between them is the only thing the user can act on:

- **`transport`**: something in the path returned bytes that are not ours. Rendered as a rung failure in the attempt history, never as a security claim.
- **`stale`**: we hold a document we cannot replace right now. Rendered as configuration age and source (INV-21).
- **`authenticity`**: a document arrived that does not verify against this operator's keys. Rendered in the verification chrome (INV-19), named as such, never as "network error".
- **`fatal`**: the profile cannot continue on the data it holds.

| Code | Profile transition | Rung reason | String class |
|---|---|---|---|
| `E_PARSE_SHORT`, `E_PARSE_MAGIC`, `E_PARSE_LEN`, `E_PARSE_NSIGS`, `E_PARSE_FRAMING`, `E_PARSE_CBOR` | none | attempt recorded, ladder advances | `transport` |
| `E_PARSE_DOCTYPE`, `E_PARSE_SLOTORDER`, `E_PARSE_ENVELOPE`, `E_PARSE_FIELD` | none | attempt recorded, ladder advances | `transport`, and the diagnostics entry names the field |
| `E_VERIFY_NOANCHOR` | on a first run, none: this is the pre-enrollment state, not a failure | attempt recorded | `transport` before enrollment; `authenticity` after |
| `E_VERIFY_ROLE` | none | attempt recorded | `authenticity` |
| `E_VERIFY_UNAUTHORIZED`, `E_VERIFY_THRESHOLD`, `E_VERIFY_SIG` | none on a non-pinned origin; on the **pinned** origin after the whole enabled ladder has been walked, `trusted_stale` | attempt recorded | `authenticity` |
| `E_VERIFY_REVOKED` on an incoming document | none | attempt recorded | `authenticity` |
| `E_VERIFY_REVOKED` on a **cached** document at load time | `trusted_stale`, or `grace` if nothing verified remains; never `compromised` for a role 2 key | n/a | `stale`, with the section 10.4 wording |
| `E_VERIFY_PID` | none; this is another tenant's document and the ladder advances | attempt recorded | `transport` |
| `E_VERIFY_VERSION`, `E_VERIFY_ROTATION` | none | attempt recorded | `authenticity` |
| `E_VERIFY_IAT`, `E_VERIFY_EXPIRED` | none | attempt recorded | `stale` |
| `E_VERIFY_NONCE`, `E_VERIFY_DEVICE` | none; retry once with a fresh nonce, then treat as `authenticity` | attempt recorded | `transport` on the first occurrence, `authenticity` on the second |
| `E_VERIFY_CATHASH` at V14a | none; the catalog is discarded and refetched once | attempt recorded | `transport` on the first occurrence, `authenticity` on the second |
| `E_VERIFY_CATHASH` at V14b | none; the catalog is **refused** and the client keeps the previous one | attempt recorded | `authenticity`, and the chrome says the fleet does not match what the provider's offline key published |
| `E_SEAL_RECIPIENT` | none; triggers the rekey path of section 10.3 | attempt recorded | `transport` |
| `E_SEAL_SUITE` | none | attempt recorded | `authenticity` |
| `E_SEAL_OPEN` | none | attempt recorded | `authenticity`; this is the one `03-WIRE.md` 9.4 calls a security event and it MUST be surfaced, not swallowed |
| root pin mismatch at first trust | enrollment refused, no profile created | n/a | `fatal` |
| every `kid` in `roles[1].ks` revoked | `compromised` (terminal) | n/a | `fatal` |

> No code in this table produces `compromised` on its own. `compromised` is reached only by the two rows at the bottom, and section 2.1 rule 5 is why.

#### 8.8.2 The chrome strings the threat model requires

Three states have to be nameable in the UI, because a mechanism the user cannot see is a mechanism the user cannot act on. The exact strings are a localization matter; the exact **conditions** are not.

| Condition | Rendered as |
|---|---|
| the trusted key document carries no `tiers` entry for the directive's `tier` | **`fleet not root-anchored`**: the provider's server list is not covered by their offline key, so a compromise of their online key would not be caught here (section 4.3, `04-THREAT-MODEL.md` 2.3) |
| a cached document was discarded because its signing `kid` is now in `rev`, and no replacement has arrived | **your provider replaced its signing key and this device has not yet received the new configuration**, with the R6 out-of-band rung offered as the immediate remedy (section 10.4, `04-THREAT-MODEL.md` 3.4) |
| the trusted key document is past its `exp` and still the anchor | the anchor's age, beside the operator identity, under INV-19 and INV-21 (section 2.2) |

#### 8.8.3 Where the client's own protocol state lives

Rolling back any of the values below defeats a stated mechanism, so each one needs a named home and a stated integrity requirement. The natural default on this client is `shared_preferences`, which is a plist on iOS and an XML file on Android, editable by a rooted-device or backup-restore adversary.

| State | Store | Integrity |
|---|---|---|
| high-water marks, per `(pid, doc_type, scope)` | secure storage, `caramba.csm_hwm`, one map, one store, in the app process | best effort; the real bounds against a local adversary are the time floor and the nonce, and `04-THREAT-MODEL.md` owns that boundary |
| `time_floor`, per profile | secure storage, on the profile | as above |
| `clock_trusted` and the monotonic offset | in memory only; MUST be recomputed at each launch | n/a; a persisted `clock_trusted` would survive a clock change, which is what it exists to detect |
| the persisted `st = 5` revoked flag | secure storage, on the profile | this one is load-bearing: a client that loses it reconnects on a revoked subscription |
| `csmPinned`, `csmPid`, `csmLinkPin` | secure storage, on the profile | load-bearing; clearing them is deleting the profile (`06-MIGRATION.md` 6.5) |
| first-seen deprecation sunsets | secure storage, per profile, per surface | load-bearing; without it the 180-day promise of section 11.2 is worth nothing |
| verified document frames for rung R0 | the app support directory, `csm/<pid>/`, verbatim as received | none needed: a tampered cached frame fails verification at load, which is why the cache stores frames and not parsed state |
| the write queue, the cards, the user-set marks | secure storage, per profile | best effort |

> When any store above reads back empty or inconsistent, the client MUST treat the profile as needing a full re-verification from the network rather than assuming the safe value, MUST NOT reset a high-water mark or a time floor to zero and continue, and MUST record the event on the diagnostics screen. A store that has silently reset is indistinguishable from a rollback, and the client should say so rather than quietly proceeding.

### 8.9 Where the ladder lives

> The ladder is implemented once, in Go, behind `HTTPDoer`, wired at `auth.WithHTTPClient` and `subscription.WithHTTPClient`, and constructed in **both** `api.NewCore` and `api.SetPanelURL`. `01-DECISION.md` 5.3.3, D2.

Verified as unmet at both sites today: `api.NewCore` builds `auth.NewPanelClient(cfg.PanelBaseURL, auth.WithStore(store))` and `subscription.NewClient(subBase)` with no `WithHTTPClient` (`libs/caramba-core/api/api.go:119, 124`), and `SetPanelURL` rebuilds all three clients the same way (`api.go:162-165`). So the option pair exists and neither call site uses it. Forgetting `SetPanelURL` in particular silently reverts a re-enrolled tenant to Go's own ClientHello, which is the bug that ships and is found six months later.

> The Dart control plane MUST NOT open its own sockets to an operator. `ApiClient` and `fetchSubscriptionBody` route through the core, as JSON strings, following the `SetPolicyJSON` pattern. Otherwise enrollment, login, token refresh and preferences all bypass the ladder, the locator cannot be re-read after a token expiry, and the app degrades to R0 permanently while the core happily climbs a ladder for a configuration it can no longer be told to change. `01-DECISION.md` 5.3.3.

**Correction 17 to `01-DECISION.md` 5.3.3, which says this rides "the existing primitive FFI boundary".** There is no such boundary. The FFI surface is exactly `create`, `configure`, `importSubscription`, `setTunnelMode`, `setTunFd`, `setPolicy`, `probe`, `up`, `down`, `status`, `traffic` and `free` (`apps/caramba-client/packages/caramba_vpn/lib/src/ffi/caramba_core_bindings.dart:57-83`, `:205-271`). There is no request symbol, no frame-verify symbol and no manifest-fetch symbol, and the same additions are needed independently in five bridges. The new surface is enumerated in section 12.2 as ABI v3, because describing it as already present is how it fails to get built.

Ship `transport_mihomo.go` and `transport_default.go` twins per the `engine_mihomo.go` and `engine_stub.go` discipline, or `go build ./...` breaks for every consumer without the `mihomo` build tag.

> uTLS, SPKI pinning and padding are mandatory on the control plane. uTLS hello IDs are selected from mihomo's already-vendored roster (`component/tls/utls.go`), randomized per connection rather than per epoch, with explicit modern hello IDs rather than the vendored `Auto` aliases, which resolve to a Chrome build that no longer ships. `01-DECISION.md` D2, D3, D4, 5.3.5.

### 8.10 Bootstrap de-blocking is part of the ladder

Every fetch the client makes travels the same `HTTPDoer` and carries a sha256 from the signed catalog (`01-DECISION.md` 5.3.8):

- `{BASE}` substitution in rule-provider URLs generalizes from the single panel base URL (`libs/caramba-core/routing/presets.go:49`) to the ordered mirror pool.
- `CompiledProviders` emits a `proxy:` key, which mihomo accepts and which `libs/caramba-core/routing/routing.go:186-192` does not emit today.
- `SetGeoIpUrl`, `SetGeoSiteUrl`, `SetMmdbUrl` and `SetASNUrl` are called at catalog mirrors, because geo databases download direct from GitHub today (`component/geodata/init.go:71-88`).
- Bootstrap DoH moves off the hardcoded 1.1.1.1 and 8.8.8.8 into the catalog's `doh` list (`libs/caramba-core/profile/profile.go:149-175`).
- The `gstatic.com` connectivity probe is replaced by a host from the signed pool (`profile.go:668-675`).

> `http://` MUST be refused for any manifest, configuration, rule-set or geo fetch. The only non-TLS exception is `.onion`, because onion addresses are self-authenticating. INV-8. `EnrollLink.normalizePanelUrl` accepts plain `http://` today, verified at `apps/caramba-client/lib/data/models/enrollment.dart:67` (`if (scheme != 'https' && scheme != 'http') return null;`), as does `ImportLink.fromUrl` at `:111`. Both MUST stop.
>
> One redirect hop MAY be followed, and only when the target host equals the tenant's configured `subscription_domain`, because `apps/caramba-panel/src/subscription.rs:113-137` issues that redirect unconditionally today. The profile URL is then normalized to the target so the hop disappears. `followRedirects` is otherwise false, with per-hop scheme and origin validation and a body size cap.

### 8.11 Whitelist mode

> Whitelist mode is not solved and this specification says so. Only R0 and R6 survive it: cached documents, and a human carrying bytes. `01-DECISION.md` 5.3.7, A3.

The `transport.Carrier` interface may exist in the Go core. No carrier ships in v1 and no toggle for one appears in the UI, because a visible, empty, disabled toggle that becomes functional when a signed catalog populates it is a dormant feature activated by remote data (`01-DECISION.md` 4.3). The distinction that survives review is between data that reconfigures an existing, reviewable, exercisable code path and data that activates a code path that was inert at review.

---

## 9. Enrollment and the bootstrap blob

### 9.1 What enrollment establishes

Enrollment is the one moment at which trust is created rather than checked. It establishes, in order: the pinned root key, the tenant identity, the device key pair, the locator, the time floor, and the first trusted key document, catalog and directive.

> Pinning is trust on first use and the specification says so plainly. `01-DECISION.md` 5.1.6.

### 9.2 The enrollment code

> ```
> code = link_pin[0..8] || secret
> ```
>
> where `link_pin` is the 20-character `base32_crockford(sha256(root_public_key)[0..12])` and `secret` is 12 characters of `base32_crockford` over 60 bits from a cryptographically secure random source. The code is 20 characters. It is rendered in five hyphen-separated groups of four, and hyphens are cosmetic and MUST be ignored on parse (`03-WIRE.md` 4.1).

Example shape: `49Q8-M87P-KQZ3-WFDG-ZTJX`, where the first eight characters `49Q8M87P` are the pin prefix, 40 bits, and the remaining twelve, `KQZ3WFDGZTJX`, are the secret. This is the code `05-TEST-VECTORS/` `pos-b1-min` carries. An earlier form of this example reused `49Q8M87PK6WP9QXG3T30`, which is the published fixture `link_pin` in full, so its "secret" was the public pin and derivable by anyone holding the root public key. A worked example whose secret is derivable teaches the wrong thing twice.

- The pin is folded into the code so there is one string to dictate over a phone call, which is the enrollment path that survives Telegram being blocked (`01-DECISION.md` 5.1.6, `00-DESIGN-BRIEF.md` R1).
- The manual-entry path carries at least the first 40 bits of `link_pin` as a required field. This form carries exactly 40.
- The panel MUST verify the pin prefix against its own root key **before** consulting `enrollment_codes`, so a code for another tenant fails without a database round trip and without a timing signal.
- **Legacy codes bypass the prefix check, and this rule is a MUST.** The live table is `code TEXT NOT NULL UNIQUE` with no format constraint (`libs/caramba-db/migrations/20260623000000_enrollment_codes.sql:13-27`), and every row in it was issued before any root key existed, so none of them begins with `link_pin[0..8]`. A panel that applies the prefix check unconditionally rejects every outstanding invite on the live tenant without a database round trip, which is exactly the behavior the rule above asks for and exactly the wrong outcome. The prefix check applies only to codes carrying the `kind` column that section 9.3 adds; a row with `kind IS NULL` is legacy and goes straight to the table. `06-MIGRATION.md` section 7 owns the backfill.
- The client MUST check the pin prefix against the `link_pin` it holds, when it holds one from a QR or a blob, and MUST check the full pin against the first key document always.

> On pin mismatch the client MUST refuse enrollment with a hard error. There MUST NOT be a "continue anyway" affordance on any code-based enrollment path. `01-DECISION.md` 5.1.6.

**Correction 5.** The bootstrap blob fixture in `03-WIRE.md` 8.5 carries `code = "K7QW-3M2P-9XRT"`, twelve characters, and the fixture's `link_pin` is `49Q8M87PK6WP9QXG3T30`. The code does not begin with the pin prefix, so the fixture does not satisfy the folding requirement of `01-DECISION.md` 5.1.6. `03-WIRE.md` presents it as a grouping example rather than as a format, and no byte-level claim depends on it, but the `05-TEST-VECTORS/` corpus MUST regenerate the bootstrap blob with a conforming 20-character code, and the `code` cap of 32 bytes in `03-WIRE.md` 8.5 accommodates the 24 characters the hyphenated form occupies.

### 9.3 Issuance

Three issuers write the same row (`01-DECISION.md` C8, P1):

1. A scoped admin API token endpoint, mounted beside the bot router with its own middleware and its own rate limit. This is a named deliverable, not a route: there is no `/api/v2/admin` router and no admin API auth surface today, only a server-rendered HTMX UI behind a Redis session cookie and CSRF middleware (`apps/caramba-panel/src/main.rs:851, 1602`).
2. A bot `/invite` command.
3. A `caramba-panel enroll issue` subcommand. This is the path that works when Telegram is blocked, which is the market. It MUST be a `caramba-panel` subcommand and not a new binary, because no `caramba-cli` crate exists.

Code semantics follow Headscale pre-auth keys (`01-DECISION.md` 5.5.5): a single-use flag, an ephemeral flag, a default expiry of 3600 seconds, and explicit revocability.

| Constant | Value |
|---|---|
| `CODE_TTL_DEFAULT` | 3600 s |
| `CODE_TTL_MAX` | 604800 s |
| `CODE_MAX_USES_CEILING` | 100, for user codes |

**Correction 6, and the schema change it forces.** `01-DECISION.md` 5.5.5 requires `expires_at` NOT NULL with a server-side maximum. `01-DECISION.md` 5.6.4 requires the Apple 2.1(a) demo code to be permanent, multi-use and non-expiring. Both cannot hold on one nullable column. The current schema is `expires_at TIMESTAMPTZ NULL` with the comment "NULL = never expires" and `max_uses INTEGER NOT NULL DEFAULT 1` (`libs/caramba-db/migrations/20260623000000_enrollment_codes.sql:22-25`), and the validity predicate is `(expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP) AND used_count < max_uses` (`apps/caramba-panel/src/services/store_service.rs:398-402`).

Resolution: add a `kind` column with values `user` and `demo`.

- `kind = 'user'`: `expires_at` NOT NULL, `expires_at <= created_at + CODE_TTL_MAX`, `max_uses` NOT NULL and at most `CODE_MAX_USES_CEILING`.
- `kind = 'demo'`: `expires_at` MAY be NULL and `max_uses` MAY be NULL meaning unlimited. The predicate becomes `(expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP) AND (max_uses IS NULL OR used_count < max_uses)`, and the conditional increment at the same site needs the same guard. Note that `max_uses = 0` can never redeem under either predicate, which is the correction `01-DECISION.md` P1 already records; the demo code MUST be `NULL`, not `0`.
- A `demo` code MUST only be issued against a Webq Pro demo panel and MUST NOT be issuable on a licensed operator's panel.

### 9.4 Device keys

Two keys, generated on device, both P-256, both non-exportable where the hardware allows:

| Key | Curve | Purpose | Used for |
|---|---|---|---|
| signing | P-256 ECDSA | `PURPOSE_SIGN` / `kSecAttrCanSign` | `dtp`, the write proof, the rekey message |
| agreement | P-256 ECDH | `PURPOSE_AGREE_KEY` / `kSecAttrCanDerive` | HPKE recipient for sealed directives |

- `dtp = sha256(device_signing_SPKI_DER)[0..16]`. The full DER `SubjectPublicKeyInfo`, including the `AlgorithmIdentifier`, is the one encoding all three platforms already agree on (`03-WIRE.md` section 4).
- The keys MUST be distinct. Signing and key agreement use separate keys (`01-DECISION.md` 5.5.1).
- **The hardware tier is explicit and user-visible.** Secure Enclave on Apple platforms; StrongBox or TEE on Android 12 and above; a software tier below Android 12, because `PURPOSE_AGREE_KEY` is API 31 and the hardware claim does not hold beneath it. The tier is recorded at enrollment, shown to the user on the operator identity screen, and reported to the operator (`01-DECISION.md` B4).
- The signing key is the device identity and MUST NOT be rotated in v1. Replacing it is a re-enrollment and the UI presents it that way. The agreement key rotates freely; section 10.3.

### 9.5 The flow

1. The user arrives with a code, a QR, a deep link or a bootstrap blob. The client extracts `origin`, `code` and, where present, `link_pin` and a mirror set.
2. The client normalizes the origin to `https://host[:port]`. `http://` is refused.
3. The client fetches `GET /sub/k1` over the ladder. R1 first if the origin is reachable, otherwise R2 or R3 using the blob's mirror set, which is why the escape kit must not live behind the host being escaped (`01-DECISION.md` C2).
4. The key document is verified against `link_pin` under the first-trust rule of `03-WIRE.md` 7.2: exactly one key under role `root` whose `sha256(pk)[0..12]` matches. Mismatch is a hard error.
5. `pid` is computed and pinned. `time_floor` is set from this document's `iat`.
6. The client generates the two device keys and records the hardware tier.
7. `POST /api/v2/app/csm/enroll/code` for a first device, carrying the code, or `POST /api/v2/app/csm/enroll/device` for a second and later device, carrying no code and authenticated by the account JWT. Either way the body carries the device signing SPKI, the device agreement public key, the hardware tier, the agreement key generation and a fresh nonce, under `X-CSM-Proof` by the signing key being registered. The field table, the ordering of the panel's checks and the status codes are `03-WIRE.md` 13.8. The body is sealed to the panel's `hpk` when one is known; on a first enrollment it is not known, so the body travels in the clear under TLS, and section 10.2 states what that costs.
8. The panel redeems the code in one transaction, records the device, allocates the locator, and returns a sealed directive.
9. The client verifies the directive, learns `cat` and `cn`, fetches and verifies the catalog chunks, and reaches `trusted`.
10. `time_floor` is advanced to the directive's `iat`.

> Enrollment is an act an authenticated account can perform on itself. `01-DECISION.md` 5.5.5. This is the bridge that a device-key-only design lacks: without it an ordinary reinstall is a manual operator action per user, in a market where the operator's support channel is blocked (`01-DECISION.md` 4.7).

### 9.6 Second and later devices

The account JWT is the enrollment authority for the second and subsequent devices, gated on capability bit 9. No enrollment code is needed. The account already exposes its device list at `GET /api/v2/app/devices` with rename and delete (`apps/caramba-panel/src/api/v2/app_account.rs:90-255`; the delete route is registered at `apps/caramba-panel/src/api/v2/mod.rs:165-169`), and a CSM device registration MUST appear in that list so the two device models stop being unreconciled.

> **That MUST is a schema and query change, and it is a named deliverable rather than an assertion.** The list reads `subscription_device_leases` and filters `sdl.last_ip <> '0.0.0.0'`, plus `NOT IN` the node and frontend-server IP sets (`app_account.rs:94-118`). A device that has enrolled but never fetched a config has no lease and no IP and does not appear; a device that fetched through the tunnel has a node IP and is filtered out **by design**. The deliverable is a `dtp` column on `subscription_device_leases`, or a `csm_devices` table joined into the same view, plus the revised query that surfaces a lease-less CSM device with its hardware tier and its enrollment time. `06-MIGRATION.md` 3.2 P3 carries it, and it is an ordering constraint on `csm_device_count_mode` moving to `union`, because the union rule counts thumbprints the list cannot currently show.

> The manifest fetch MUST NOT count devices. No `track_access`, no device-limit enforcement on `/sub/m1/{loc}` or `/sub/r1/{loc}`. And the configuration fetch MUST count by thumbprint, not by apparent source IP. INV from `01-DECISION.md` 5.5.2.
>
> Today the limit is enforced on the configuration fetch by apparent IP, through `get_active_ips` (`apps/caramba-panel/src/subscription.rs:203-262`). Every ladder rung with a different egress burns a slot, and under fetch-through-tunnel the apparent IP is the exit node's, shared by every user of that node. `00-DESIGN-BRIEF.md` R9 is retired only when both halves are done.

### 9.7 The bootstrap blob

A root-signed `0x05` frame carrying the enrollment origin, the code, the root public key, a mirror set, a DoH list and an inert operator display name (`03-WIRE.md` 8.5). At 374 bytes it is one QR code and it is dictatable in the degenerate case where the user has only the code and the pin.

Rules:

- The blob MUST be producible and distributable without touching the operator's origin. Printing it, mailing it and photographing it are all in scope; it is the escape kit.
- The blob MUST NOT carry prices, a bot handle, a purchase link or any "buy" call to action (`01-DECISION.md` 5.6.1).
- `sha256(rk)[0..12]` MUST equal the `link_pin` the user was given out of band, and MUST equal the hash of the key the first key document presents under role `root`.
- A blob whose `rk` does not match the dictated pin MUST be rejected with a hard error and no "continue anyway" path.
- The armored frame stream (`03-WIRE.md` 10.1) MAY carry a blob plus a key document plus a catalog as one offline snapshot, and MAY carry a full root rotation chain, so the out-of-band rung survives a rotation.

### 9.8 Deep links and manual entry

The existing scheme is preserved and extended additively. `carambaconnect://enroll?panel=<url>&code=<code>` gains `&k=<link_pin>`. Verified as safe to extend: `EnrollLink.tryParse` reads only `panel` and `code` from `uri.queryParameters` and ignores everything else (`apps/caramba-client/lib/data/models/enrollment.dart:31-41`), so an old client sees an unchanged link and a new client sees a pin.

`carambaconnect://import?url=<encoded>` is unchanged and remains the legacy path. A profile created through `import` is a `rawSub` profile with no pinned key and no CSM state; it MUST NOT be silently upgraded to a CSM profile, because there is nothing to pin it to.

Manual entry accepts the 20-character code with or without hyphens, case-insensitively, and rejects any character outside the Crockford alphabet after the `I`/`L`/`O` folding of `03-WIRE.md` 4.1.

### 9.9 Enrollment failure modes

| Failure | Client behavior |
|---|---|
| pin mismatch on the key document | hard error, profile discarded, no retry offered against the same origin |
| code rejected (404) | plain "this code is not valid", no distinction between expired, used and unknown, because the panel returns an empty body for all three |
| every rung fails before a key document arrives | offer the out-of-band rung and nothing else; do not create a profile |
| key document arrives but the directive does not | keep the anchored profile and the pinned key, retry the directive on the normal backoff; the pin is the expensive part and it is already done |
| the origin serves a valid key document for a different `pid` than a previously enrolled profile on this device | both profiles coexist; this is normal multi-tenancy, not an error |

---

## 10. Sealing and key rotation

### 10.1 Sealing

The per-device directive is HPKE-sealed so a mirror, a CDN or an onion front holds only a ciphertext and a thumbprint (`01-DECISION.md` BC2, 5.7.2). The suite, the `info` and `aad` strings, the field layout and the order of operations are `03-WIRE.md` section 9.

Two protocol-level rules:

1. Sealing is what makes a third-party mirror acceptable. A client MUST NOT fetch a directive from a rung other than R1 when capability bit 1 is clear, because an unsealed directive on a mirror hands that mirror the device thumbprint, the status, the selection and the traffic counters in the clear.
2. The plaintext is a complete `0x03` frame, magic and signature slots included. A recipient that recovers a plaintext not beginning `43 53 4d 31 03` MUST treat it as `E_SEAL_OPEN` and MUST NOT attempt a partial parse.

### 10.2 The two HPKE keys, disambiguated

**Correction 7 to `03-WIRE.md` 8.2 and 9.3, and to `01-DECISION.md` 5.7.2.** There are two HPKE keys in CSM/1 and the documents name both "the recipient key", which is not survivable across three implementations.

| Key | Where published | Who holds the private half | Generation counter | Used for |
|---|---|---|---|---|
| device agreement key | registered at enrollment, never published | the device | sealed directive `rkv` | the panel seals a directive **to the device** |
| panel HPKE key | catalog `hpk` | the panel | catalog `hpkv` | the client seals a request body **to the panel** |

The reasoning that forces the split: a catalog is byte-identical for every subscriber on a tier, so a per-device key cannot live in it, and `01-DECISION.md` 5.7.2 justifies rotating the published key by saying "a compelled or seized panel does not retroactively decrypt a long window of recorded traffic", which is only true of a key whose private half the panel holds. `03-WIRE.md` 9.4 step 5 is correspondingly unambiguous that `rkv` names a key the **device** holds.

Panel HPKE key usage in v1 is exactly one thing: the `PUT /api/v2/app/preferences` request body and both enrollment bodies MAY be sealed to `hpk`, with the same suite as `03-WIRE.md` 9.1 and with its own domain separation:

```
info = "CSM1-seal-w1"                                   12 ASCII bytes, no NUL
aad  = "CSM1" || 0xFF || pid(8) || dtp(16) || u32be(0)  33 bytes
```

> The `info` string and the second `aad` byte MUST differ from the device-directed seal of `03-WIRE.md` 9.2, which uses `"CSM1-seal-v1"` and `0x06`. Reusing both and relying on the key alone for separation works, but it is fragile and it is the kind of thing that survives one refactor and not two. `0xFF` is chosen because `03-WIRE.md` 1.2 puts `0xF0..0xFF` in the private-experimentation range that MUST NOT be emitted and MUST be rejected as a `doc_type`, so it can never collide with a real frame's type byte; `0x00` was the earlier value and is worse, because `03-WIRE.md` 1.2 registers `0x00` as `invalid`.

**Gating, storage and rotation.**

> Client-to-panel sealing is gated on the **presence of `hpk`** in the trusted catalog and on nothing else. It is not gated on capability bit 1, which means "sealed directives available" and is a statement about the opposite direction; an operator who has device sealing but no panel HPKE key, or the reverse, must be able to express that, and `06-MIGRATION.md` 4.7 separately forbids clearing bit 1 at all. A panel that does not offer client-to-panel sealing MUST omit both `hpk` and `hpkv`; a client that holds a catalog without `hpk` MUST send write and enrollment bodies in the clear under TLS.
>
> The private half of `hpk` lives with the online signing key secret, never in the `settings` table and never under `SESSION_SECRET`. `hpkv` starts at **1** and increments by one on each rotation. The panel MUST accept a body sealed to the previous generation for one `ttlk` period after a rotation, so a client holding a stale catalog is not locked out of writing, and MUST refuse anything older than that.
>
> Rotation cadence is monthly, aligned with the catalog lifetime so a rotation costs one catalog re-sign that was going to happen anyway. `01-DECISION.md` 5.7.2 justifies the rotation by saying a compelled or seized panel does not retroactively decrypt a long window of recorded traffic, and that is only true of a key whose private half the panel holds, which is this one.

It buys confidentiality of the user's routing and DNS preferences against a state-CA MITM, which is a real adversary under the 28.6 percent RuStore trusted-root figure in `00-DESIGN-BRIEF.md` 2.2, and it buys nothing against the panel itself, which sees the plaintext by construction.

### 10.3 Device agreement key rotation

> A device MUST be able to replace its own agreement key without operator action. The rekey message is authenticated by the device **signing** key, which is a separate key, and it exists from v1. There is no state in which a device is stuck with a key it cannot replace. `03-WIRE.md` 9.5.

The rekey path is `PUT /api/v2/app/preferences` with a `want` map containing no settings keys and a non-critical key 64 carrying `{new agreement public key, new generation}`, signed by the device signing key under the same `X-CSM-Proof` rule. Key 64 is in the non-critical range so a panel that does not implement rekey ignores it rather than rejecting the whole write, and the client detects non-implementation by the absence of the new generation in the next directive's `rkv`.

Trigger conditions the client MUST handle:

- `E_SEAL_RECIPIENT` because the device holds no private key for the offered `rkv`: rekey immediately, then re-request.
- Keystore invalidation, which happens on Android when the user changes their device credential and the key was bound to it: detect on first use, rekey, and record the event.
- A device restore onto new hardware: the non-exportable key is gone by construction and this is a re-enrollment, not a rekey.

### 10.4 Online key rotation

The online key changes by publishing a key document at version N+1 whose `roles[2].ks` contains the new `kid`. Root signatures are required as for any key document.

> A panel MUST publish a key document containing a new online key, and MUST wait at least `max(ttlk, 3600)` seconds, before signing any catalog or directive with it. A client that has not yet refreshed its key document will reject every document signed by a key it has not been told about, and the rejection is indistinguishable from an attack.

The recommended overlap is `2 * ttlk`. During overlap both keys are in `ks` and the panel MAY sign with either. Removing the old key from `ks` is a further key document at N+2; adding it to `rev` is what actually revokes it and is a separate decision with a separate consequence.

**What revocation costs, and what the user is told.** A `kid` in `rev.kids` invalidates every document that key signed, including the cached catalog on disk (section 10.6). The client then holds no valid catalog and cannot build a configuration until it fetches one signed by the replacement key. INV-16 protects an **expired** document, not a **revoked** one, and it should not: stale configuration from a revoked signer is exactly what the mechanism exists to remove.

> A running tunnel keeps running, because the engine already holds its configuration, and the client MUST NOT tear it down (section 2.1 rule 5). A client that reconnects after a revocation and can reach no rung has nothing to connect with, and it MUST say so rather than showing a generic failure.
>
> The client MUST render this state under INV-21, naming it as **your provider replaced its signing key and this device has not yet received the new configuration**, and MUST offer the R6 out-of-band rung as the immediate remedy. Section 8.8.2 carries the condition.

In a blackout, revoking the online key can take users offline more effectively than the adversary was managing. That is not a reason to avoid revoking; it is a reason for the operator to know it before pulling the lever, and `04-THREAT-MODEL.md` 3.4 prices it.

### 10.5 Root rotation

> A key document with `ver = N+1`, where `N` is the version of the currently trusted key document, MUST be verified twice over the same pre-image: once against `roles[1].ks` and `roles[1].thr` of the currently trusted document, and once against `roles[1].ks` and `roles[1].thr` of the document under verification. Both MUST pass. A client MUST refuse to skip a version: `ver != N+1` is `E_VERIFY_ROTATION`. `01-DECISION.md` 5.1.7, `03-WIRE.md` 7.3.

Clients walk intermediates in one request, `GET /sub/k1?since=N`, which returns versions `N+1` through at most `N+8` as a frame stream. An out-of-band bundle MAY carry the full chain in the armored form, so the offline rung survives rotation.

Root key custody is operator-held (`01-DECISION.md` 5.1.4): generated by `caramba-panel csm keygen root`, printed once as a BIP39-style mnemonic so paper backup is realistic, derived deterministically from it, with the fingerprint printed alongside so a restore can be verified. The tool MUST refuse to write the private half into the panel working directory and SHOULD refuse to run when it detects it is on the panel host.

The online signing key lives under its own secret with its own rotation, and MUST NOT be stored under `SESSION_SECRET`, which `APP_JWT_SECRET` already falls back to and which has no `kid` and no rotation path. A single symmetric leak must not be simultaneously a session-forgery event and a configuration-signing event (`01-DECISION.md` A9).

### 10.6 Revocation

> A client that sees a `kid` in `rev.kids` MUST reject that key and every document it signed, **including documents already cached on disk**, immediately. A node id in `rev.nodes` MUST be honored against the cached catalog, so a seized node is dropped even while the client is running offline. INV from `01-DECISION.md` 5.2.9.

- Rejection of a cached document on revocation is not conditional on being online. The check runs at load time from disk.
- Revoking the last key of a role leaves the tenant unable to sign that document type. A panel MUST refuse to publish a key document in which any present role's `ks` is entirely revoked.
- Revoking a root key that is still in `roles[1].ks` is a contradiction and MUST be rejected at parse.

> **These are emission-side checks and they belong to a tool, not to a handler.** The `caramba-panel csm build` and `caramba-panel csm import` subcommands of `06-MIGRATION.md` 3.7 MUST both refuse a key document that violates any of the following, and MUST name the violated rule rather than failing generically: an entry of `keys` referenced by no `ks` (section 4.3); a `kid` in an `ks` that is absent from `keys`; a `kid` present in both `roles[1].ks` and `rev.kids`; a role whose `ks` is entirely revoked; a `thr` above `len(ks)`; a `pk` that fails any clause of `03-WIRE.md` 2.1; a `ver` that is not exactly one above the newest imported document; a total frame length above `DOC_FRAME_MAX`; and a `tiers` map that omits any tier the panel currently serves. A client rejects all but the last two at parse, which is too late to help the operator who signed it and has already destroyed their own fleet's trust anchor.
- Propagation is bounded by the key document's 7 day expiry in the worst case and by one refresh in the normal case. An adversary who steals the online key on day one also captures the current key document and can replay it while suppressing refreshes; that is accepted risk A2 and its trigger is the `0x07` timestamp role, whose field numbers are already reserved.

### 10.7 Loss

Root key loss is unrecoverable in band. The optional Webq Pro countersignature is the opt-in recovery lane and defaults off; when it is absent or fails, nothing breaks. A mandatory countersignature is rejected because it would make Webq Pro a single compromise and censorship target for every tenant at once (`01-DECISION.md` 4.11).

---

## 11. Deprecation

> Nothing is ever withdrawn without an announcement that predates it and survives caching and mirrors. `01-DECISION.md` B7.

Deprecations are `dep` entries in the **key document**, root-signed, each `{s, sun}` where `s` is a surface identifier and `sun` is a sunset in Unix seconds. `01-DECISION.md` B7 places them in the catalog; `03-WIRE.md` 8.1 key 15 places them in the key document. The key document binds, and it is the better home: a promise about withdrawal should be root-signed rather than signable by a compromised online key, and the key document has the shortest refresh interval of the three.

### 11.1 The surface vocabulary

```
surface = <class> ":" <instance>
```

Total length at most 48 bytes, ASCII, `class` from the closed set below, `instance` matching `[a-z0-9._-]{1,40}`.

| Class | `instance` | Example |
|---|---|---|
| `endpoint` | a route short name | `endpoint:sub.r1` |
| `rung` | `r0`..`r6` | `rung:r5` |
| `cap` | a decimal bit number | `cap:10` |
| `setting` | a settings-table key name | `setting:fakeip` |
| `preset` | a routing preset id | `preset:cn-smart` |
| `proto` | a `pr` enumeration name | `proto:vmess` |
| `tier` | a decimal tier id | `tier:2` |
| `spec` | `v1` | `spec:v1` |

> An unrecognized `class` MUST be ignored and rendered as a generic deprecation notice naming its sunset date. It MUST NOT be a parse failure. A deprecation is an announcement, and refusing to parse an announcement about a thing you do not know is exactly backwards. This is the non-enumerated case of section 4.1, not the enumerated case of `03-WIRE.md` section 5.

### 11.2 Rules

- `sun` MUST be at least `iat + 15552000`, that is 180 days. A `dep` entry violating this MUST be rejected at parse (`E_PARSE_FIELD`), which puts the minimum notice period in the format rather than in an operator's discretion.
- The client persists, per surface, the **first** sunset it ever saw. The effective sunset is `max(first_seen, current)`. A later key document MAY extend a sunset; it MUST NOT bring one forward, and an attempt to do so is rendered at the first-seen date. Without this rule the 180 day promise is worth nothing, because a second document could set the sunset to tomorrow.
- The client surfaces deprecations in Settings whenever any exists, and raises a persistent, non-blocking notice for any surface within 30 days of its effective sunset.
- A deprecation MUST NOT change behavior. It is an announcement. The surface keeps working until the operator removes it, and the client keeps using it.
- Gated on capability bit 7. An operator with no deprecation channel has no `dep` entries and the client renders no surface.

### 11.3 What may never be deprecated

A `dep` entry naming any of the following MUST be rejected at parse. These are the frozen compatibility surface (`00-DESIGN-BRIEF.md` section 3) and the protocol's own escape hatches.

| Surface | Why |
|---|---|
| `endpoint:sub.uuid` | `/sub/{uuid}` and the four generators are never removed. `06-MIGRATION.md`. |
| `rung:r0` | cached documents are the last line and are never disableable |
| `rung:r6` | the out-of-band rung is always on and never disableable |
| `setting:killswitch`, `setting:dns.nameservers`, `setting:dns.fallback`, `setting:split.mode` | the user's security posture is not an operator-withdrawable feature |
| `cap:1` | withdrawing sealing would silently move directives back into the clear on mirrors |

---

## 12. Invariant conformance index

Every MUST from `01-DECISION.md` section 6 is restated in place above. This table is the proof, and it is the checklist a reviewer walks.

| INV | Statement, in brief | Restated in |
|---|---|---|
| 1 | signature covers the transmitted bytes; no re-serialization, no verify-side re-encode | `03-WIRE.md` 1.3; referenced in section 0 |
| 2 | framing is exact-length; trailing bytes rejected; `nsigs` cannot be inflated | `03-WIRE.md` 1.1; consequence used in section 4.2 (`pd` is inside the payload) |
| 3 | strict CBOR and strict Ed25519 profiles enforced in all three implementations, with a shared negative corpus as a merge gate | `03-WIRE.md` 2, 3; section 4.1 governs the key ranges |
| 4 | role authorization read from the previously trusted document; no path returns a key without its role | section 3 |
| 5 | no response above 4 KB; connection byte and packet ceilings; both signed catalog fields, clamped at the client | sections 8.6, 8.6.1 |
| 6 | catalog chunked from v1; panel refuses oversized rather than emitting | section 8.6 |
| 7 | jittered refresh from a signed `ttl`; per-request padding buckets; legacy header keeps saying `"2"` | section 5.6 |
| 8 | refuse `http://`; only `.onion` excepted | section 8.10 |
| 9 | refuse unauthorized role, version regression, inexact framing, strict-profile violation, wrong nonce | sections 3, 5.1, 5.3 |
| 10 | refuse to open any operator URL; operator text inert, capped, URL-stripped, off the verification surface | sections 4.6 (`sup`), 7.7 (card), 7.10 |
| 11 | refuse to persist or echo any operator value outside a closed vocabulary | sections 7.3, 7.9 last row, 7.10 |
| 12 | refuse a rule-set or geo file whose sha256 does not match the catalog | section 4.4 |
| 13 | refuse to fall back to unverified legacy once a root key is pinned; missing capability is a hard error | sections 2.1 rule 6, 6.4 |
| 14 | refuse to display a connected state the engine cannot back | section 12.1 below |
| 15 | refuse to transmit `split.apps`, in either direction | sections 7.2, 7.10 |
| 16 | never disconnect because a document expired | sections 2.1 rule 3, 5.2 |
| 17 | every rung on one screen, with a toggle, an order and a per-attempt history; unavailable rungs visible and disabled with a reason | sections 8.1, 8.8 |
| 18 | operator identity visible: name, fingerprint, enrollment date, pin origin, whether it changed | section 8.8 |
| 19 | verification state of documents in use visible | section 8.8 |
| 20 | "What this app sends" screen with a copy button | section 8.8 |
| 21 | configuration age and source visible whenever running on cached documents | sections 2.1 rule 2, 8.8 |
| 22 | any operator change to a user-set setting, and any narrowing of posture, as a Keep or Revert card | section 7.7 |
| 23 | telemetry, if it exists, off by default with contents enumerated on screen | section 8.8 |

### 12.1 INV-14, stated in place

> The client MUST NOT display a connected state that the engine cannot back. INV-14.

Verified as a live hazard: `libs/caramba-core/engine/engine_stub.go:40` sets `e.state = StateConnected` inside `Start` with no tunnel, and `Status` reports it at `:58`, so a build without the `mihomo` tag reports connected while traffic leaks. The stub exists deliberately, so the CLI and the upper layers build without CGO, and it is not the defect; reporting its state to the user as a tunnel would be.

Three requirements follow:

1. The engine MUST expose a capability signal distinguishing a real tunnel from a stub, and the FFI and gomobile boundaries MUST carry it as a primitive.
2. The client MUST treat a stub engine as effective capability zero for every tunnel-dependent control, exactly as it treats a cleared operator capability bit, and MUST render the connect control disabled with reason `platform_unsupported`.
3. The client MUST NOT show `connected` on the strength of an engine state alone. It requires the engine state **and** a successful connectivity check through the tunnel, and until both hold the state is `connecting`.

### 12.2 The client-to-core integration surface

This section exists because every other section of this specification describes what a verified catalog **means** and none of them says how it becomes a running tunnel. Three primitives exist today and each contradicts a rule stated above.

**What is there now, verified.**

- `Core.Up(ctx, serverID)` has two branches (`libs/caramba-core/api/api.go:644-712`). The panel branch requires `c.auth.IsAuthenticated()` and fetches `/sub/{uuid}` itself at `:687`, which discards everything the client verified. The raw branch uses `c.importedConfig` and sets `pinProxy = strings.TrimSpace(serverID)` at `:659`.
- `AssembleMihomoConfigPinned(rawYAML, policy, pinProxy)` (`libs/caramba-core/profile/profile.go:232-257`) pins by **mihomo proxy name**, matched by string equality inside the `CARAMBA` selector (`applyPin`, the comparison at `:549`). Section 4.5 makes `id` mandatory and states that `pn` is not a key and MUST NOT be used as one, precisely because `generate_clash_config` has no uniquifier.
- On the Dart side the only route into the raw branch is `connectRaw({raw, format, label, serverId})` (`packages/caramba_vpn/lib/src/contract.dart:255-268`), which re-parses the YAML through `importSubscription` and replaces the real `Server` model with a synthetic one.

**What CSM/1 requires.**

> A CSM profile remains `ConnectionProfileType.panelAccount`. It MUST NOT be routed through `connectRaw`, because that path re-parses a config the client just built and loses the `id` to `pn` correspondence the catalog carries.
>
> The core gains one entry point, `Core.UpRendered(ctx, renderedYAML []byte, nodeID string)`, and the Dart contract gains one method, `Future<void> connectRendered({required String config, required String nodeId, required S server})`. The rendered configuration is what the client built from the verified catalog; `nodeId` is a catalog node entry `id`, never a proxy name. `VpnStatus.server` carries the real `Server` for the selected `id`, so the server picker, the prober and autotune keep working against one identity.
>
> The core gains an id-keyed pin, `AssembleMihomoConfigPinnedByID(rawYAML, policy, nodeID)`, and the Go renderer emits a `csm-id` annotation per proxy so the pin can resolve without string-matching a display name. Where a client is on the legacy render path (capability bit 0 clear, or phases 1 and 2), it translates its stored catalog `id` into a `pn` through the cached catalog and uses the existing name-keyed pin, with the visible fallback of `06-MIGRATION.md` 7.5 when the translation is ambiguous.

**ABI v3, enumerated, because it is not "existing".**

Section 8.9 requires the Dart control plane to route through the core, and section 9.4 puts the device signing key in Secure Enclave or StrongBox, which Dart cannot reach either. Three groups of symbols are new. Each takes and returns a JSON string, following the `SetPolicyJSON` pattern, and each needs a matching method-channel name in all five bridges: Android `CarambaVpnPlugin.kt`, Apple `CarambaVpnPlugin.swift`, Windows `caramba_vpn_plugin.cpp`, Linux `caramba_vpn_plugin.cc`, and the gomobile surface.

| Group | Symbol | Argument | Result |
|---|---|---|---|
| HTTP through the ladder | `CarambaLadderRequest` | `{"method","path","origin","headers":{},"body_b64","timeout_ms","rungs":[..]}` | `{"status","headers":{},"body_b64","rung","error"}` |
| | `CarambaUpRendered` | `{"config_b64","node_id"}` | `{"ok","config_path","engine"}` |
| Device keys | `CarambaDeviceKeygen` | `{"purpose":"sign"\|"agree","require_hardware":true}` | `{"spki_b64","tier":1\|2\|3,"error"}` |
| | `CarambaDeviceSign` | `{"message_b64"}` | `{"sig_b64"}`, 64 bytes `r \|\| s`, low `s`, per `03-WIRE.md` 13.6 |
| | `CarambaDeviceAgree` | `{"peer_pub_b64","kdf_info_b64"}` | `{"shared_b64"}` |
| Engine capability | `CarambaEngineCapability` | none | `{"real_tunnel":true\|false}`, section 12.1 requirement 1 |

The device-key group is **not** implementable in Go and MUST live in each platform bridge: Secure Enclave and StrongBox are reached through `SecKeyCreateRandomKey` and `KeyGenParameterSpec`, and a Go implementation would by definition put the key in a file, which is the software tier. The core calls out to the bridge for `CarambaDeviceSign` and `CarambaDeviceAgree`; on Windows, Linux and any build with no hardware store, the bridge returns tier 3 and holds the key in the platform credential store.

> On iOS the Network Extension needs **none** of these. Under `01-DECISION.md` X3 the extension holds no fetcher, no verifier and no monotonic store; it receives a rendered configuration plus a validity window. `CarambaUpRendered` is what the app process calls, and the extension's own `CarambaNewClient` construction is reduced to starting an engine on bytes it was handed.

`00-DESIGN-BRIEF.md` freezes ABI v2. ABI v3 is additive: every new symbol is looked up with the same missing-symbol handling `CarambaCoreMissingSymbol` already provides, so a client running against an older dylib degrades to "CSM unavailable on this build" rather than crashing, exactly as `setPolicy` and `probe` degrade today.

**Core changes that are not new symbols.** Gathered here because each is named in passing elsewhere and each will otherwise be discovered during integration:

| Change | Where it is required |
|---|---|
| `AssembleMihomoConfigPinnedByID`, and a `csm-id` annotation per rendered proxy | this section |
| a loopback mixed inbound on `127.0.0.1` in **both** tunnel modes, and an `HTTPDoer` for rung R4 that dials through it | section 8.2 |
| `HTTPDoer` actually wired at `auth.WithHTTPClient` and `subscription.WithHTTPClient`, in **both** `api.NewCore` and `api.SetPanelURL` | section 8.9 |
| `transport_mihomo.go` and `transport_default.go` twins, per the `engine_mihomo.go` and `engine_stub.go` discipline | section 8.9 |
| explicit reset values for `mtu` and `stack` in `policyPatch`, so the `"default"` sentinel of section 7.5 means something for those two keys | section 7.11 item 4 |
| an explicit-none relay representation reaching the URL as `relay_country=none`, and a `FetchOptions` field that can emit it | section 7.3 |
| a `FetchOptions.Variant` field, needed only once capability bit 10 is set | section 7.3 |
| `io.LimitReader` at `thr.resp_max` on `FetchProfile`, replacing the unbounded `io.ReadAll` | section 8.5, `06-MIGRATION.md` Correction 9 |
| the inverse of `corePolicyFrom` in `core_policy_mapping.dart`, so a fetched selection repopulates the pickers | section 7.1 |

---

## 13. Corrections to the inputs

Each item departs from `01-DECISION.md`, `00-DESIGN-BRIEF.md` or `03-WIRE.md`, and each carries its evidence. The code is followed where a document disagrees with it, and the arithmetic is followed where a document disagrees with itself.

**Correction 1: `time_floor` MUST NOT incorporate the `Date` header.** `03-WIRE.md` 6.4 and `01-DECISION.md` 5.2.3 both define the floor as the greater of the enrollment-time server `Date` header and the highest observed `iat`. `Date` is unsigned and attacker-controlled, and the floor never decreases, so a single hostile mirror returning a `Date` far in the future permanently bricks the profile: no legitimately signed document can ever clear the floor again. The floor is derived from signed `iat` values only (section 5.4). The `Date` header retains a clamped, display-only role (section 5.5).

**Correction 2: verification step V11 needs the lifetime term.** `03-WIRE.md` 6.2 V11 tests `iat >= time_floor`. Once the floor advances to a fresh directive's `iat`, every legitimately cached document older than that directive fails, including a 20-day-old catalog with 10 days of life left. The corrected test is `iat + LIFETIME_MAX[doc_type] + 300 >= time_floor`, which rejects exactly what the floor was for, a document that had already expired when the profile last heard from the panel, and nothing else. Section 5.4. `03-WIRE.md` V11 should be amended; a positive fixture in `05-TEST-VECTORS/` distinguishes the two forms.

**Correction 3: `sel.variant` needs a vocabulary, and it is an index into a nine-entry string list.** `03-WIRE.md` 8.3 types it `uint, < 2^8` with no enumeration. The panel matches variants by string equality against a fixed list (`apps/caramba-panel/src/singbox/connection_variants.rs:19-83, 104-110`). The wire type is kept and section 7.3 supplies the mapping, with `0` meaning none. Separately, and more importantly for the product: `apply_connection_variant` runs only inside `generate_singbox` (`apps/caramba-panel/src/services/subscription_service.rs:2040-2041`) while the Go core fetches `?client=clash` exclusively (`libs/caramba-core/subscription/subscription.go:132`), so `variant` changes nothing the Connect client receives, and this specification forbids exposing a control for it in v1.

**Correction 4: the no-relay sentinel is `--`, not `"NO"`.** `03-WIRE.md` 8.3 originally specified the literal `"NO"` for `sel.rcc` on the stated grounds that it "is not a valid ISO country in this position". `NO` is Norway. An operator with a Norwegian relay could not express it, and a user selecting one would silently get no relay at all. `--` is two characters, fits the exactly-2 cap, and is not an alpha-2 code. The renderer maps it to the URL literal `none`, which `apps/caramba-panel/src/subscription.rs:754` accepts as `Some("none") | Some("NONE")`. **Applied**: `03-WIRE.md` 8.3 and `06-MIGRATION.md` 4.5 and 7.6 now carry `--`, and `05-TEST-VECTORS/` `pos-m1-norelay` already did. Section 7.3 additionally distinguishes the sentinel from the *unset* empty string, which the two-state reading conflated with it.

**Correction 5: the bootstrap blob fixture's code does not fold in the pin.** `03-WIRE.md` 8.5 carries `code = "K7QW-3M2P-9XRT"` against a fixture `link_pin` of `49Q8M87PK6WP9QXG3T30`. `01-DECISION.md` 5.1.6 requires the pin folded into the code and at least its first 40 bits carried on the manual-entry path. Section 9.2 specifies the format as 8 pin characters plus 12 secret characters, and the `05-TEST-VECTORS/` bootstrap blob must be regenerated to match. No byte-level claim in `03-WIRE.md` depends on the code's content, and the 32-byte `code` cap accommodates the hyphenated 24-character form.

**Correction 6: `expires_at NOT NULL` and the non-expiring demo code cannot both hold on one column.** `01-DECISION.md` 5.5.5 requires `expires_at` NOT NULL with a server-side maximum; 5.6.4 requires a permanent, multi-use, non-expiring demo code for Apple 2.1(a). The live schema has `expires_at TIMESTAMPTZ NULL` documented as "NULL = never expires" and `max_uses INTEGER NOT NULL DEFAULT 1` (`libs/caramba-db/migrations/20260623000000_enrollment_codes.sql:22-25`), with the predicate at `apps/caramba-panel/src/services/store_service.rs:398-402`. Section 9.3 resolves it with a `kind` column: `user` codes get NOT NULL expiry and a `max_uses` ceiling, `demo` codes get nullable both, and the predicate gains a `max_uses IS NULL` branch alongside the `expires_at IS NULL` branch it already has.

**Correction 7: there are two HPKE keys and the inputs name both "the recipient key".** `03-WIRE.md` 8.2 calls catalog `hpk` the "current HPKE recipient public key" while `03-WIRE.md` 9.3 and 9.4 make `rkv` a generation of a key the **device** holds, and `01-DECISION.md` 5.7.2 justifies rotation by a property that only holds for a key the panel holds. A per-device key cannot live in a per-tier byte-identical catalog. Section 10.2 splits them: the device agreement key (device-held, generation `rkv`, seals panel to device) and the panel HPKE key (panel-held, catalog `hpk`/`hpkv`, seals client to panel), and gives the second one its single v1 use so the field is not left meaningless.

**Correction 8: the routing preset wire vocabulary is the core's nine ids, and `full` is not one of them.** `00-DESIGN-BRIEF.md` 4.4 and `01-DECISION.md` 5.4.1 both say the vocabulary is the `CorePolicy` string set without enumerating it. The `CorePolicy` doc comment lists nine ids (`apps/caramba-client/packages/caramba_vpn/lib/src/core_policy.dart:98-99`) which match `libs/caramba-core/routing/presets.go` exactly, but the Dart UI uses its own id `full` and translates it through `kRoutingPresetWire` (`apps/caramba-client/lib/state/core_policy_mapping.dart:20-22, 70-76`). A panel or a Go renderer that emitted `full` would produce a value `routing.PresetByID` rejects at `libs/caramba-core/api/policy_json.go:88-91`. Section 7.3 states the wire vocabulary explicitly and forbids `full`.

**Correction 9: `01-DECISION.md` 5.4.4's cross-device propagation list and the panel's write surface do not currently overlap.** 5.4.4 says exit, relay, routing preset, protocol and variant propagate across a user's devices. The panel has no endpoint that writes any of them: relay is persisted as a side effect of a GET (`apps/caramba-panel/src/subscription.rs:745-751`), node pinning is persisted as another side effect (`:617-633` auto-pin and `:636-644` explicit), and routing preset, protocol and variant have no server-side representation at all. Section 7 specifies the target state; the sequencing, including the mini-app relay-picker migration that must precede removing the write-on-GET, is `01-DECISION.md` P7 and `06-MIGRATION.md`, and this document does not restate it.

**Correction 10: `01-DECISION.md` B7 puts deprecations in the catalog and `03-WIRE.md` puts them in the key document.** `03-WIRE.md` 8.1 key 15 binds. Section 11 states the reason: a promise about withdrawal should be root-signed rather than signable by a compromised online key, and the key document refreshes fastest of the three.

**Correction 11: the effective capability is the freshest verified directive's, not the AND of the catalog and the directive.** Stated in place at section 6.5 with its evidence: clearing a bit works under both rules, restoring one does not, and `06-MIGRATION.md` 4.6 and its phase 3 entry criterion 4 are false under the AND rule. `03-WIRE.md` 5.1 now carries the rule for a reader who has only that document.

**Correction 12: revocation of an `online` key MUST NOT reach `compromised`.** `01-DECISION.md` 5.2.9 requires a client that sees a `kid` in `rev` to reject that key and every document it signed. An earlier form of section 2.1 routed any revoked trusted `kid` to the terminal `compromised` state, which made the operator's ordinary response to a suspected online-key leak, described as routine hygiene in section 10.4, a fleet-wide out-of-band re-enrollment on a market whose support channel is assumed blocked. Sections 2.1 and 2.2 split it: role 2 revocation invalidates documents and drops the profile to `trusted_stale` or `grace`; only a root pin mismatch, or the revocation of every key in `roles[1]`, is terminal.

**Correction 13: `tiers` is optional to decode and mandatory to emit, and V14 is two checks.** `03-WIRE.md` 8.1 types `tiers` optional and its step V14 read as an unconditional conjunction, so a verifier written from it rejected every catalog on a tenant that publishes no tier hashes. `04-THREAT-MODEL.md` 2.3 depends throughout on the conditional reading and calls publishing `tiers` the single highest-value operator action. Section 4.3 resolves it: the panel obligation is unconditional, the decode tolerance stays, V14b is conditional, and absence is rendered as `fleet not root-anchored`. `03-WIRE.md` 6.2 is amended to match.

**Correction 14: there is no bare-directive delivery path, so the bit-1-clear column described one that does not exist.** Every directive-bearing row of `03-WIRE.md` 13.2 returns one `0x06` frame, `03-WIRE.md` 8.3 says the `0x03` is never transmitted bare, and `06-MIGRATION.md` 4.7 forbids clearing bit 1 at all. The earlier text additionally assigned `E_VERIFY_ROLE` to the refusal, which is the role-resolution failure at V3 and not an envelope-type refusal. Section 6.3 now says bit 1 clear means only that a directive MUST NOT be fetched from a rung other than R1, which is what section 10.1 rule 1 already said, and no code is assigned.

**Correction 15: `pol[3]` has three states and the earlier two-state reading would have disabled relaying fleet-wide at cutover.** The empty string is "unset, the operator resolves", not "off". The live default with no explicit relay falls back to `client_cc` and includes same-country chains (`apps/caramba-panel/src/subscription.rs:744-757`), and no panel endpoint has ever written `subscriptions.relay_country` except the GET side effect at `:745-751`, so almost every live subscription is in that state. Section 7.3 gives the three states, section 7.4 gives the predicate against them, and the two required core changes are named there.

**Correction 16: two of the five `sel`-versus-`pol` predicates cannot be evaluated at parse time.** `sel.exit` and `sel.relay` are checked against the bound catalog, and the client learns which catalog is bound only from the directive being parsed, while section 2.3 forbids entering a catalog at `verified` before a directive names it. `E_PARSE_FIELD` is also a code `03-WIRE.md` 6.1 defines as decidable with no stored state. Section 7.4 moves both out of parse into a post-catalog check whose outcome is the section 7.9 fallback plus an informational notice.

**Correction 17: the FFI boundary the ladder is supposed to ride does not exist.** Recorded in place at section 8.9 with the symbol list, and answered by the ABI v3 enumeration in section 12.2.

**Correction 18: `04-THREAT-MODEL.md` section 4's clamps are requirements of this document and were absent from it.** Section 8.6.1 encodes all five, section 14 lists them, and `TTL_FLOOR` is 900 rather than 1800 so that the kill-switch acceleration `06-MIGRATION.md` 4.3 documents is not silently ineffective. The five clamps are what `04-THREAT-MODEL.md` 7.2 step 8 and 7.3 step 8 mean by "stopped only by section 4", so their absence made both end-to-end walkthroughs depend on a mechanism no implementer would have built.

**Verified and relied upon rather than restated:** the `HTTPDoer` seam exists on both Go clients (`libs/caramba-core/auth/client.go:38-40, 57-60`; `libs/caramba-core/subscription/subscription.go:92-96`) and neither `NewCore` nor `SetPanelURL` uses it (`libs/caramba-core/api/api.go:119, 124, 162-165`); the mixed inbound exists only in `ModeProxy` (`libs/caramba-core/profile/profile.go:181-182, 210`) and the default is `ModeTun` (`:151`); the Android service excludes the app from its own tunnel by design (`CarambaVpnService.kt:253-258`); the engine stub reports `StateConnected` with no tunnel (`libs/caramba-core/engine/engine_stub.go:40, 58`); `EnrollLink.tryParse` ignores unknown query parameters, so `k=` is safe to add (`apps/caramba-client/lib/data/models/enrollment.dart:31-41`); and `policyPatch` uses pointers with "absent means do not change" and silently ignores unknown keys (`libs/caramba-core/api/policy_json.go:18-36`), which is what the `"default"` sentinel preserves.

---

## 14. Constants owned by this document

Every number this specification introduces. Byte-level and size-budget constants are in `03-WIRE.md` 17 and are not duplicated.

| Constant | Value | Section |
|---|---|---|
| `LIFETIME_MAX[0x01]` | 604800 s | 5.2 |
| `LIFETIME_MAX[0x02]`, `[0x04]`, `[0x05]` | 2592000 s | 5.2 |
| `LIFETIME_MAX[0x03]`, `[0x06]` | 3600 s | 5.2 |
| `LIFETIME_MAX[0x08]` | 604800 s | 5.2 |
| `CONN_BYTES_CEILING` | 15360, **provisional** | 8.6.1 |
| `CONN_PACKETS_CEILING` | 25, **provisional** | 8.6.1 |
| `thr.resp_max` maximum a client accepts | 4096 | 8.6.1 |
| `EXPH_FLOOR` | 86400 s | 8.6.1 |
| `TTL_FLOOR` | 900 s | 8.6.1 |
| minimum applied jitter, regardless of `jit` | 10 percent | 8.6.1 |
| `BUILD_EPOCH` plausibility window at enrollment | `[BUILD_EPOCH, BUILD_EPOCH + 315360000]` | 5.4 |
| minimum cohort size | 25 subscribers, **provisional** | 8.1.1 |
| cohort ceiling for nodes | 16, from the `tiers` map cap | 8.1.1 |
| nonce lifetime | 300 s | 5.3, 7.8 |
| default `ttlk` when absent | 21600 s | 4.3, 5.6 |
| default `exph` when absent | 604800 s | 4.6 |
| clock-backwards threshold that clears `clock_trusted` | 300 s | 5.5 |
| `Date` clamp window | `[time_floor, time_floor + 2592000]` | 5.5 |
| mirror-set refresh cadence | `min(ttl, 3600)` s | 5.6 |
| metered-background refresh multiplier | 4 | 5.6 |
| maximum outstanding Keep or Revert cards | 3 | 7.7 |
| write queue depth | 32 | 7.8 |
| queued write drop age | 604800 s | 7.8 |
| default `lad.ord` when absent | `[0,1,2,3,4,5,6]` | 8.3 |
| default `lad.en` when absent | `[0,1,2,3,6]` | 8.3 |
| R2 attempts per cycle | 3 | 8.4 |
| R3 attempts per cycle | 2 | 8.4 |
| `TCP_CONNECT_TIMEOUT` | 5 s | 8.5 |
| `TLS_HANDSHAKE_TIMEOUT` | 5 s | 8.5 |
| `ATTEMPT_TIMEOUT_R1_R2_R3` | 12 s | 8.5 |
| `ATTEMPT_TIMEOUT_R4_R5` | 20 s | 8.5 |
| `CYCLE_BUDGET` | 90 s | 8.5 |
| backoff base, cap, jitter | 30 s, 3600 s, ±20 percent | 8.7 |
| attempt history retention | 200 entries | 8.8 |
| enrollment code length | 20 characters, 8 pin plus 12 secret | 9.2 |
| `CODE_TTL_DEFAULT` | 3600 s | 9.3 |
| `CODE_TTL_MAX` | 604800 s | 9.3 |
| `CODE_MAX_USES_CEILING` | 100 | 9.3 |
| online key overlap before first use | `max(ttlk, 3600)` s, recommended `2 * ttlk` | 10.4 |
| `?since=` walk, versions per response | 8 | 10.5 |
| minimum deprecation notice | 15552000 s (180 days) | 11.2 |
| deprecation notice surfacing threshold | 2592000 s (30 days) | 11.2 |
| `surface` maximum length | 48 bytes | 11.1 |

Three of these are provisional and they are marked. `CONN_BYTES_CEILING` and `CONN_PACKETS_CEILING` fall out of the same missing measurement as the four provisional constants of `03-WIRE.md` 11.1 and 17: the real TLS handshake byte and data-packet cost against a tenant's actual certificate chain, and the real connection freeze point, both taken from a Russian vantage point on mobile and home broadband. The minimum cohort size falls out of a different one, the observed mirror burn rate against the rotation cadence, which is `01-DECISION.md` A7's own trigger. Everything else in the table is policy or arithmetic and does not move on a measurement.

---

## Changelog

One review pass, 2026-09-02, by three reviewers reading the whole set for cross-document consistency, panel implementability and client implementability. What it changed in this document:

**Blocking**

- Section 8.6.1 adds the five clamps on signed threshold fields that `04-THREAT-MODEL.md` section 4 makes a MUST and this document previously contradicted with the opposite MUST. `TTL_FLOOR` is 900, not 1800, so that `06-MIGRATION.md` 4.3's kill-switch acceleration still works.
- Section 6.5 replaces the `catalog.cap AND directive.cap` rule with directive-authoritative plus a content-presence carve-out, because the AND rule makes restoring a killed capability impossible for a catalog lifetime.
- Section 4.3 resolves `tiers`: optional to decode, mandatory to emit, V14b conditional, absence rendered as `fleet not root-anchored`.
- Section 2.1 rule 5 and section 2.2 stop routing `online`-key revocation to the terminal `compromised` state.
- Section 6.3 deletes the bare-directive path for capability bit 1, which no endpoint provides, and the `E_VERIFY_ROLE` misuse with it.
- Section 12.2 is new: the client-to-core integration surface and the ABI v3 symbol list, neither of which existed anywhere in the set while section 8.9 described them as already present.
- Section 7.3 gives `pol[3]` three states, so that cutover does not silently disable relay chaining for every subscriber who has never touched the picker, and fixes the no-relay sentinel at `--`.
- Section 5.4 adds the `BUILD_EPOCH` clock-plausibility rule, which is what bounds first-trust replay while `clock_trusted` is false.

**Serious**

- Section 7.4 moves the two catalog-dependent consistency predicates out of the parse step, where they cannot be evaluated, into a post-catalog check with a fallback and a notice.
- Section 7.7.1 is new: a change to the rule-set provider set, to a resource hash, or to a route entry's `rs` list is a narrowing of security posture and raises the Keep or Revert card. This required editing the closed list in 7.7, which is where `04-THREAT-MODEL.md` hand-off 2 had to land.
- Section 7.11 is new: how `pol` reaches the core, why it MUST be merged rather than passed through, and what a change does to a running tunnel. Section 7.8's "applies locally and immediately" is corrected.
- Section 8.8.1 maps every error code to a profile transition, a rung reason and a user-visible string class, so three UI teams do not render three different things for one corpus fixture.
- Section 8.8.2 and 8.8.3 add the chrome strings and the client-side state storage table that the threat model's hand-offs 3, 4 and 5 require.
- Section 1.2 requires the token, profile and settings stores to be `pid`-keyed, which is hand-off 6 and residual R-12.
- Section 8.1.1 carries the cohort guidance of hand-off 8.
- Section 2.2 and 5.2 state that an expired trusted key document remains a valid authorization anchor, which is hand-off 4 and was load-bearing for availability.
- Section 4.4.1 states the catalog membership rule, section 4.4 the content-addressed rule-set store, and section 4.4 the `tier` derivation and its ceilings.
- Section 4.6.2 maps the panel's four live subscription statuses onto `st` and `rc`, including `throttled`, which had no representable value.
- Section 9.6 turns the device-list MUST into a named schema and query deliverable.
- Section 10.2 gives the panel HPKE key its own `info` and `aad`, gates it on `hpk` presence rather than on capability bit 1, and states its storage, its starting generation and its rotation cadence.
- Section 9.2 adds the legacy-code bypass, without which every outstanding invite on the live tenant stops redeeming.

**Minor**

- Section 9.2's worked enrollment code no longer uses the published `link_pin` as its "secret".
- Section 2.3 requires a chunk's `cid` to be checked before the chunk is accepted, not only at reassembly.
- Section 4.6.1 answers the one case left open: a verified `st = 5` tears down a running tunnel, and it is the only signed value that may.
- Section 7.3 notes that the Dart `CorePolicy` doc comment omits `VLESS`, and that the Dart client is the enforcement point for the DNS scheme rule because the Go core validates nothing.
- Section 7.3 makes carrying `sel.variant` conditional on capability bit 10, withdrawing a sentence that was dead on arrival.

Eight new corrections, 11 through 18, are recorded in section 13.
