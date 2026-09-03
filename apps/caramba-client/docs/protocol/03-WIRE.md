# CSM/1 Wire Format

Status: normative, 2026-09-02. Companion to `01-DECISION.md` (rationale) and `02-SPEC.md` (protocol behavior). This document is written first because every other document in the set references it.

Scope: the byte layout of every CSM/1 artifact, the decode and signature profiles a conforming implementation MUST enforce, the size budget the format is designed against, and the HTTP surface that carries it.

Audience: three implementers who must agree byte for byte. A Rust signer and verifier in `apps/caramba-panel`, a Go verifier in `libs/caramba-core`, a Dart verifier in `apps/caramba-client`. Assume the reader has only this document.

Key words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY carry their RFC 2119 meanings. Rationale is not repeated here; it lives in `01-DECISION.md` and is cited by section number.

---

## 0. Conventions

**Endianness.** Every multi-byte integer that this document defines outside CBOR is big-endian (network byte order). This covers `payload_len` in the frame, the `u32be(gen)` term in the locator derivation, and the `u32be(ver)` term in the HPKE additional data. CBOR is big-endian by definition of RFC 8949 and needs no separate statement.

**Byte ranges.** `x[0..n]` means the first `n` bytes of `x`, leftmost first. `sha256(k)[0..8]` is therefore the leftmost 8 bytes of the 32-byte digest, not a truncation of a numeric value.

**Hex.** All hex dumps in this document are real. They were produced by an independent generator, not by the panel signer, and their Ed25519 signatures verify against the published seeds. See section 15 for the reproduction recipe. This satisfies the requirement in `01-DECISION.md` X1 that vectors are computed independently.

**Notation for CBOR.** `uint` is CBOR major type 0, `bstr` is major type 2, `tstr` is major type 3, `array` is major type 4, `map` is major type 5, `bool` is the simple values `0xf4` and `0xf5`. Map keys in CSM/1 are always `uint`.

**Correction notes.** Where this document departs from `01-DECISION.md` or `00-DESIGN-BRIEF.md` because the code says otherwise or the arithmetic does not hold, the departure is marked **Correction** and carries its evidence. Section 16 collects them.

---

## 1. The frame

Every signed CSM/1 artifact is exactly one frame. A frame is a flat byte string with no nesting and no length prefix outside itself.

```
offset  size            field
------  --------------  ---------------------------------------------------
0       4               magic          = 0x43 0x53 0x4D 0x31  ("CSM1")
4       1               doc_type       (section 1.2)
5       2               payload_len    u16 big-endian, 1..49152 inclusive
7       payload_len     payload        (strict CBOR, section 3)
7+L     1               nsigs          1..4 inclusive
8+L     76 * nsigs      signatures     nsigs * { keyid_trunc(12) || sig(64) }
```

where `L` is `payload_len`.

### 1.1 The exact-length rule

> A verifier MUST require the total frame length to satisfy, exactly:
>
> ```
> total_len == 7 + payload_len + 1 + 76 * nsigs
> ```
>
> A verifier MUST reject any byte string longer or shorter than that value. Trailing bytes are a rejection, never an ignored suffix. `nsigs` MUST NOT be inflated beyond the number of signature slots the length accounts for, and the length check is what enforces it.

This is `01-DECISION.md` invariant 2 and graft C4. It is checked before any signature work, and it is a parse failure, not a verification failure (section 6).

Worked check against the minimal directive in section 8.3: `payload_len = 144`, `nsigs = 1`, so `total_len = 7 + 144 + 1 + 76 = 228`. The published frame is 228 bytes. Appending a single `0x00` yields 229 and MUST be rejected without attempting signature verification.

### 1.2 Document type registry

| `doc_type` | Name | Short name | Signing role | Sealed | Section |
|---|---|---|---|---|---|
| `0x00` | invalid | | | | MUST be rejected |
| `0x01` | key document | `k1` | root | no | 8.1 |
| `0x02` | catalog | `c1` | online | no | 8.2 |
| `0x03` | directive | `m1` | online | no | 8.3 |
| `0x04` | catalog chunk | `c1c` | online | no | 8.4 |
| `0x05` | bootstrap blob | `b1` | root | no | 8.5 |
| `0x06` | sealed directive | `m1s` | online | yes, carries an `m1` | 9 |
| `0x07` | reserved (timestamp role, `01-DECISION.md` A2) | | timestamp | | not emitted in v1 |
| `0x08` | reserve pool | `r1` | root | no | 8.6 |
| `0x09`..`0xEF` | reserved | | | | MUST be rejected |
| `0xF0`..`0xFF` | private experimentation | | | | MUST NOT be emitted; MUST be rejected |

An unknown or reserved `doc_type` is a **parse** failure. It carries no security meaning because the verifier cannot determine which role should have signed it, and guessing is exactly the confusion the domain separator exists to prevent.

### 1.3 The signing pre-image

> The signed byte string is the first `7 + payload_len` bytes of the frame, as transmitted:
>
> ```
> pre = magic || doc_type || u16be(payload_len) || payload
> ```
>
> A signer MUST sign these bytes. A verifier MUST verify over these bytes as received. No implementation may re-serialize, re-order, normalize, canonicalize, pretty-print or parse-and-re-encode the payload before verifying, and no implementation may perform a verify-side re-encode-and-compare step.

This is `01-DECISION.md` 5.1.5 and invariant 1. The magic and the `doc_type` are inside the pre-image, so a catalog signature can never be replayed as a directive signature and a v1 signature can never be replayed under a future magic.

Concretely, for the minimal directive of section 8.3 the pre-image is:

```
43 53 4d 31 03 00 90  ||  <144 payload bytes>
```

151 bytes total. `nsigs` and the signature slots are NOT in the pre-image. That is deliberate and is safe only because of the exact-length rule: `nsigs` cannot be raised without changing `total_len`, and `total_len` is checked before verification.

### 1.4 Signature slots

Each slot is 76 bytes:

```
offset  size  field
0       12    keyid_trunc = sha256(ed25519_public_key)[0..12]
12      64    sig         = Ed25519 signature over `pre`, RFC 8032 pure Ed25519
```

`keyid_trunc` is 96 bits. It is a lookup hint, never an authorization. Authorization comes from the role table in section 7.

- Slots MUST be ordered by ascending `keyid_trunc`, compared as unsigned bytes. A signer MUST emit them sorted; a verifier MUST reject an unsorted slot list. This makes a frame with a given signer set byte-unique, which is what allows a catalog to be content-addressed.
- Two slots MUST NOT carry the same `keyid_trunc`. A verifier MUST reject a duplicate rather than counting it twice toward the threshold.
- A slot whose `keyid_trunc` is not in the authorized key set for the required role MUST cause rejection of the whole frame. It is not skipped. A frame carrying an unauthorized signature is a frame someone tried to launder.

`nsigs` is capped at 4. Single-operator reality is threshold 1, and root rotation (section 7.3) is the only case that needs 2. The cap exists so a hostile frame cannot force 255 Ed25519 verifications.

### 1.5 Deterministic signatures are required

> A signer MUST use pure Ed25519 as specified in RFC 8032 section 5.1.6, with the deterministic nonce derived from the private key and the message. A signer MUST NOT use Ed25519ph, Ed25519ctx, or any randomized-nonce variant.

Consequence, and the reason this is normative rather than incidental: catalogs are content-addressed by `sha256(frame)` and their per-tier hash is published in the root-signed key document (`01-DECISION.md` 5.2.4). Determinism fixes the signature for an identical message, and an identical message is what the panel must arrange.

> **Determinism is not sufficient on its own, and the panel MUST NOT rely on it alone.** `iat` and `exp` are mandatory in every payload (section 8.0), so re-signing at a different wall-clock time produces different bytes, a different `chash` and a different `cat_id` even when the fleet has not changed. A panel MUST therefore **persist the signed catalog frame and its chunk frames**, keyed by `(tier, content_digest)`, where `content_digest` is a digest over the tier's node, relay, route, mirror, DoH, resource, pin and threshold model and over nothing else: not over the clock, not over the requester, not over a row id. It MUST re-sign a tier only when that content digest changes, it MUST set `iat` to the time the content digest changed rather than to the time of the request, and `exp` follows from `iat`. Serving a tier is then a lookup, never a signing operation.

The storage is a named panel deliverable (`06-MIGRATION.md` 3.2, P3): a `csm_catalogs` table holding `(tier, content_digest, ver, iat, chash, frame, chunk frames)`. Without it the published tier hash is invalidated by every restart, `06-MIGRATION.md` 2.2 exit criterion 4 is unpassable, and V14b fails fleet-wide on a schedule.

---

## 2. Strict Ed25519 profile (conformance requirement)

This is a conformance requirement, not advice. `01-DECISION.md` X1 and invariant 3. All three implementations MUST enforce every clause, and the shared negative-fixture corpus in `05-TEST-VECTORS/` is a merge gate in `cargo test`, `go test` and `flutter test`.

### 2.1 Public key ingest

A 32-byte Ed25519 public key `A` is accepted only if all of the following hold. They run at two places and MUST run at both: at parse step **P12** on every `pk` inside a key document, which is where key material enters the trusted set, and at verification step **V6** on the public key of every signature slot. They MUST be re-run on every use rather than cached as a bit.

1. **Canonical encoding.** Let `y` be the little-endian integer formed from `A` with bit 255 (the sign bit of `x`) masked to zero. `y` MUST be strictly less than `p = 2^255 - 19`. Encodings with `y >= p` MUST be rejected.
2. **On the curve.** `A` MUST decompress to a point on Edwards25519. A decompression failure is a rejection.
3. **Not of small order.** Let `A` be the decompressed point. `[8]A` MUST NOT be the identity element. Implement this as three point doublings followed by an identity test; do not implement it as a hardcoded blacklist of encodings, because the blacklist form is easy to transcribe incorrectly and the predicate is exact.

Clause 3 rejects the 8 points of order dividing 8, including the identity itself and the two order-2 and order-4 encodings, under every canonical and non-canonical spelling that survives clause 1.

### 2.2 Signature verification

Given `sig = R || S` (32 bytes each) and pre-image `pre`:

1. `S` MUST be canonical: interpreted as a little-endian integer, `S < L` where `L = 2^252 + 27742317777372353535851937790883648493`. A non-canonical `S` is a rejection, not a normalization.
2. `R` MUST decode as a valid curve point under the clause 1 and clause 2 rules of section 2.1. `R` of small order is permitted at this stage only because clause 3 of 2.1 already removed small-order `A`; implementations MAY additionally reject small-order `R` and the reference vectors expect no difference either way.
3. Verification MUST use the **cofactorless** equation of RFC 8032 section 5.1.7:

   ```
   [S]B == R + [SHA-512(R || A || pre) mod L]A
   ```

   comparing the compressed encodings. A cofactored check (`[8][S]B == [8]R + [8][h]A`) MUST NOT be used, because it accepts signatures the other two implementations would reject and that divergence is a split-brain between what the UI shows and what the tunnel dials.

### 2.3 Per-language notes

These are notes, not permissions to deviate. The predicates above are authoritative.

- **Rust.** `ed25519-dalek` v2 is already a workspace dependency (`libs/caramba-shared/Cargo.toml:16`, feature-gated behind `license`) and the panel already performs Ed25519 verification for licensing (`libs/caramba-shared/src/license.rs:27,205`). Use `VerifyingKey::verify_strict`, which implements 2.1 clause 3 and 2.2 clause 1. `VerifyingKey::verify` MUST NOT be used.
- **Go.** `crypto/ed25519.Verify` performs 2.2 clause 1 and clause 3 but does NOT perform 2.1 clause 3. The small-order test MUST be added explicitly at key ingest using `filippo.io/edwards25519`, which is already in the module graph through mihomo.
- **Dart.** `apps/caramba-client/pubspec.yaml` declares no cryptography package today (verified: a grep for `crypto`, `pointycastle`, `cryptography`, `cbor` and `base32` in that file returns nothing). Adding one is a prerequisite, per `00-DESIGN-BRIEF.md` build item 4. Whichever package is chosen, all three clauses of 2.1 and all three of 2.2 MUST be verified against the negative corpus before the dependency is accepted, because most Dart Ed25519 implementations perform none of them.

---

## 3. Strict CBOR decode profile (conformance requirement)

`01-DECISION.md` X1 and invariant 3. Canonicity in CSM/1 is enforced as a **local predicate over the incoming bytes**, never as a re-encode-and-compare. This section is that predicate, in full.

A decoder MUST reject the payload, as a parse failure, on any of the following.

### 3.1 Structural rules

| # | Rule |
|---|---|
| C1 | The payload MUST be exactly one top-level CBOR data item, and that item MUST be a map (major type 5). |
| C2 | The item MUST consume exactly `payload_len` bytes. One trailing byte inside the payload is a rejection. |
| C3 | Definite lengths only. Indefinite-length maps, arrays, byte strings and text strings (`0x5f`, `0x7f`, `0x9f`, `0xbf`) MUST be rejected. |
| C4 | Shortest-form heads. An integer or length argument MUST use the shortest additional-information encoding that can express it: values 0..23 inline, 24..255 with `0x18`, 256..65535 with `0x19`, 65536..2^32-1 with `0x1a`, above that `0x1b`. A non-minimal head is a rejection. |
| C5 | Tags (major type 6) MUST be rejected, in any position, including tag 2 and 3 bignums. |
| C6 | Floats (`0xf9`, `0xfa`, `0xfb`) MUST be rejected. |
| C7 | The only simple values permitted are `0xf4` (false) and `0xf5` (true). `0xf6` (null), `0xf7` (undefined) and every other simple value MUST be rejected. |
| C8 | Negative integers (major type 1) MUST be rejected in v1, in any position. No v1 field uses one. |
| C9 | Map keys MUST be unsigned integers (major type 0). A text-string or byte-string key is a rejection. |
| C10 | Map keys MUST appear in strictly ascending numeric order. Equal keys, and therefore duplicates, are a rejection by the same test. |
| C11 | Text strings MUST be well-formed UTF-8. A decoder MUST validate; it MUST NOT substitute replacement characters. |
| C12 | Nesting depth MUST NOT exceed 6. The top-level map is depth 1. |

Rule C10 subsumes duplicate-key detection into an order check, which is one comparison per key and cannot be forgotten independently. Implementers MUST NOT satisfy C10 by sorting after decode.

### 3.2 Size limits

| Limit | Value | Applies to |
|---|---|---|
| `MAX_DEPTH` | 6 | nesting |
| `MAX_MAP_PAIRS` | 64 | any single map |
| `MAX_ARRAY_ITEMS` | 512 | any single array |
| `MAX_TSTR_BYTES` | 256 | any text string |
| `MAX_BSTR_BYTES` | 3072 | any byte string |
| `MAX_UINT` | 2^53 - 1 | any unsigned integer value |

`MAX_UINT` is stated so that a Dart or JavaScript-hosted decoder never silently loses precision. No v1 field approaches it; the largest is a Unix timestamp.

`MAX_BSTR_BYTES` is sized to admit a full catalog chunk payload (2816 bytes, section 11.3) and the padding field, and nothing larger.

### 3.3 Unknown keys: critical and non-critical ranges

> - Map keys **1..63** are the **critical** range. A key in this range that the decoder does not recognize for the map it appears in MUST cause a parse failure.
> - Map keys **64..1023** are the **non-critical** range. An unrecognized key in this range MUST be ignored, and MUST still satisfy C4, C9, C10 and the size limits.
> - Map keys **1024 and above** MUST be rejected. Key 0 MUST be rejected.

This is the extension mechanism. Any future field whose misinterpretation could weaken a security property goes in the critical range, where an old client refuses rather than ignores. Any future field that is purely additive goes in the non-critical range. A key in the critical range costs 1 encoded byte up to key 23 and 2 bytes from 24; the field tables in section 8 put every hot field below 24 for that reason.

### 3.4 Worked encodings of the primitives

These are the exact byte sequences an implementer should expect to see, drawn from the fixtures in section 8.

| Value | Bytes | Note |
|---|---|---|
| key `1` | `01` | uint 1, inline |
| key `10` | `0a` | uint 10, inline |
| key `24` | `18 18` | uint 24, one-byte argument; note the 2-byte cost |
| `1` | `01` | |
| `443` | `19 01 bb` | uint, two-byte argument, minimal |
| `1788307200` | `1a 6a 97 67 00` | uint, four-byte argument, minimal |
| `true` | `f5` | |
| `false` | `f4` | |
| `"DE"` | `62 44 45` | tstr, length 2 |
| `"n17i3"` | `65 6e 31 37 69 33` | tstr, length 5 |
| 8-byte bstr | `48` + 8 bytes | bstr, length 8 inline |
| 32-byte bstr | `58 20` + 32 bytes | bstr, length 32, one-byte argument |
| map of 2 pairs | `a2` + pairs | |
| map of 14 pairs | `ae` + pairs | |
| array of 1 item | `81` + item | |

Non-minimal counterexamples that MUST be rejected under C4: `18 01` for the value 1, `19 01 bb` is correct for 443 but `1a 00 00 01 bb` is not, `58 08` for an 8-byte string is not (use `48`).

---

## 4. Derived identifiers

Every identifier in CSM/1 is derived. None is allocated by a database sequence, because a sequence is a tenant-correlating value and because a derived identifier can be recomputed by a verifier that holds only the inputs.

| Name | Derivation | Wire form | Length |
|---|---|---|---|
| `pid` | `sha256(root_ed25519_public_key)[0..8]` | `bstr(8)` | 8 bytes |
| `keyid_trunc` | `sha256(ed25519_public_key)[0..12]` | 12 raw bytes in the signature slot; `bstr(12)` in the key document | 12 bytes |
| `link_pin` | `base32_crockford(sha256(root_public_key)[0..12])` | text, 20 characters | 96 bits |
| `loc` | `base32_crockford(HMAC-SHA256(secret, "csm1-loc" \|\| 0x00 \|\| subscription_uuid \|\| u32be(gen))[0..15])` | text, 24 characters | 120 bits |
| `dtp` | `sha256(device_signing_SPKI_DER)[0..16]` | `bstr(16)` | 128 bits |
| `chash` | `sha256(complete catalog frame)` | `bstr(32)` | 32 bytes |
| `cat_id` | `base32_crockford(chash[0..10])` | text, 16 characters | 80 bits |
| `bid` | `base32_crockford(sha256(armored byte stream)[0..5])` | text, 8 characters | 40 bits |

Notes that decide interoperability:

- `subscription_uuid` in the `loc` derivation is the ASCII text of the UUID exactly as the panel stores it, lowercase with hyphens, 36 bytes, not the 16 raw bytes. The `0x00` separator is a single literal NUL byte and `"csm1-loc"` is 8 ASCII bytes with no terminator.
- `gen` is a per-subscription generation counter, a database column, not a panel-wide epoch (`01-DECISION.md` 5.5.3). Rotating one leaked locator is one `UPDATE`.
- `device_signing_SPKI_DER` is the full DER `SubjectPublicKeyInfo` of the device's P-256 signing key, including the `AlgorithmIdentifier`, as produced by `SecKeyCopyExternalRepresentation` wrapped into SPKI on iOS and by `PublicKey.getEncoded()` on Android. Hashing the raw 65-byte point instead would produce a different thumbprint on every platform that spells the wrapper differently; the SPKI is the one encoding all three platforms already agree on.
- `chash` covers the complete frame, signature slots included. A re-signature is therefore a different catalog with a different `cat_id`. Section 1.5 is what makes that stable rather than churning: the panel persists the signed frame and re-signs only on a content change, so `cat_id` is stable for as long as the tier's content is.
- The HMAC key in the `loc` derivation is a dedicated 32-byte secret, `CSM_LOC_SECRET`, generated by `caramba-panel csm keygen` alongside the online signing key secret and stored with it. It MUST NOT be `SESSION_SECRET`, MUST NOT be `APP_JWT_SECRET`, and MUST NOT be stored in the panel `settings` table. Rotating it changes every locator on the tenant at once, so it is the panel-wide emergency lever of `01-DECISION.md` 5.5.3 and it requires the same locator-index backfill as the per-subscription `gen` column.

### 4.1 base32 Crockford

Alphabet, index 0 through 31:

```
0123456789ABCDEFGHJKMNPQRSTVWXYZ
```

`I`, `L`, `O` and `U` are absent by construction.

- Encoding takes the input bytes as a bit stream, most significant bit first, and emits one character per 5 bits. If the final group has fewer than 5 bits it is right-padded with zero bits. No `=` padding characters are appended, ever.
- Encoded length is `ceil(len_bytes * 8 / 5)` characters. 10 bytes give exactly 16 characters, 15 bytes give exactly 24, 12 bytes give 20 with 4 pad bits, 16 bytes give 26 with 2 pad bits.
- A decoder MUST accept lowercase, MUST map `I`, `i`, `L` and `l` to the value 1, MUST map `O` and `o` to the value 0, and MUST ignore `-` anywhere in the string. A decoder MUST reject any other character.
- A decoder MUST reject a string whose trailing pad bits are non-zero, so that each byte string has exactly one accepted spelling.

The alphabet is chosen for two properties at once: it is dictatable over a phone call, which is the enrollment path that survives Telegram being blocked (`01-DECISION.md` 5.1.6, `00-DESIGN-BRIEF.md` R1), and every character of it is inside the QR alphanumeric mode character set, which is what makes section 10 efficient.

---

## 5. Enumerations

All enumerations are closed vocabularies. A value outside the listed set MUST cause a parse failure when it appears in a critical-range field. This is `01-DECISION.md` C6 and invariant 11: the client persists and echoes only values it can validate against a closed vocabulary.

**`role`** (key document `roles` map keys)

| Value | Role |
|---|---|
| 1 | `root` |
| 2 | `online` |
| 3 | `timestamp` (reserved, `01-DECISION.md` A2; MUST NOT appear in v1) |

**`alg`** (key entry)

| Value | Algorithm |
|---|---|
| 1 | Ed25519, RFC 8032 pure |

**`st`** (directive status; `01-DECISION.md` 5.2.8)

| Value | Status |
|---|---|
| 1 | `pending_approval` |
| 2 | `onboarding` |
| 3 | `active` |
| 4 | `expired` |
| 5 | `revoked` |
| 6 | `suspended` |
| 7 | `quota_exceeded` |
| 8 | `device_limit` |

**`rc`** (machine reason code; 0 means no reason supplied)

| Range | Meaning | Defined values |
|---|---|---|
| 0 | none | `0` |
| 1000..1099 | account | `1001` awaiting operator approval, `1002` account suspended by operator, `1003` account closed |
| 2000..2099 | payment | `2001` term ended, `2002` payment failed, `2003` trial ended |
| 3000..3099 | quota | `3001` traffic quota exhausted, `3002` onboarding grant exhausted, `3003` daily allowance exhausted, resets at the next period (`02-SPEC.md` 4.6.2, the panel's `throttled` state) |
| 4000..4099 | device | `4001` device limit reached, `4002` device revoked by user, `4003` device revoked by operator |
| 5000..5099 | operator action | `5001` plan withdrawn, `5002` node fleet unavailable |

An unrecognized `rc` MUST be rendered as the generic text for its `st` and MUST NOT be a parse failure; the range is non-normative extension space. `rc` is a machine code and is never rendered verbatim to the user.

**`pr`** (node protocol). Values follow the panel's own protocol match arms (`apps/caramba-panel/src/singbox/subscription_generator.rs:224-437`).

| Value | Protocol |
|---|---|
| 1 | `vless` |
| 2 | `vmess` |
| 3 | `trojan` |
| 4 | `hysteria2` |
| 5 | `tuic` |
| 6 | `shadowsocks` |
| 7 | `naive` |
| 8 | `wireguard` (the panel's `amneziawg` inbound, rendered as a wireguard outbound) |

**`nw`** (transport network; `StreamInfo.network`, `subscription_generator.rs:92`)

| Value | Network |
|---|---|
| 1 | `tcp` |
| 2 | `ws` |
| 3 | `grpc` |
| 4 | `httpupgrade` |
| 5 | `xhttp` (equivalently `splithttp`) |
| 6 | `quic` |

**`se`** (security; `StreamInfo.security`)

| Value | Security |
|---|---|
| 0 | `none` |
| 1 | `tls` |
| 2 | `reality` |

**`fp`** (uTLS client fingerprint; the mihomo `client-fingerprint` vocabulary, panel default `chrome` at `subscription_generator.rs:584`)

| Value | Fingerprint |
|---|---|
| 1 | `chrome` |
| 2 | `firefox` |
| 3 | `safari` |
| 4 | `ios` |
| 5 | `android` |
| 6 | `edge` |
| 7 | `360` |
| 8 | `qq` |
| 9 | `random` |
| 10 | `randomized` |

**`fl`** (VLESS flow)

| Value | Flow |
|---|---|
| 0 | absent; the renderer MUST omit the key entirely, not emit an empty string |
| 1 | `xtls-rprx-vision` |

The `0` case is normative because the panel comment at `subscription_generator.rs:230-232` records that emitting an empty `flow` breaks Happ, and the Go renderer must reproduce that omission.

**`alp`** (ALPN entries)

| Value | ALPN |
|---|---|
| 1 | `h2` |
| 2 | `http/1.1` |
| 3 | `h3` |

**`cg`** (TUIC congestion control; panel default `bbr` at `subscription_generator.rs:393-394`)

| Value | Control |
|---|---|
| 1 | `bbr` |
| 2 | `cubic` |
| 3 | `new_reno` |

**`ssm`** (Shadowsocks method)

| Value | Method |
|---|---|
| 1 | `2022-blake3-aes-128-gcm` |
| 2 | `2022-blake3-aes-256-gcm` |
| 3 | `2022-blake3-chacha20-poly1305` |
| 4 | `aes-128-gcm` |
| 5 | `aes-256-gcm` |
| 6 | `chacha20-ietf-poly1305` |

**`rung`** (ladder rung identifiers, used in the catalog `lad` field; `01-DECISION.md` 5.3.1)

| Value | Rung |
|---|---|
| 0 | R0 cached signed documents |
| 1 | R1 direct HTTPS to the enrolled origin |
| 2 | R2 signed mirrors |
| 3 | R3 DoH-resolved address with explicit SNI |
| 4 | R4 through the app's own tunnel |
| 5 | R5 user-entered SOCKS5 or HTTP proxy |
| 6 | R6 out of band |

**`src`** (per-field setting provenance; `01-DECISION.md` B2)

| Value | Provenance |
|---|---|
| 1 | `user` |
| 2 | `operator` |
| 3 | `default` |

**`ui.k`** (UI hint kind; `01-DECISION.md` B3, inert text only)

| Value | Kind |
|---|---|
| 1 | `notice` |
| 2 | `warning` |
| 3 | `maintenance` |
| 4 | `renewal_due` |
| 5 | `capability_unavailable` |

### 5.1 Capability bitfield

`cap` is `bstr(4)`, interpreted as a 32-bit big-endian bitfield. It appears in the catalog (operator capabilities) and is echoed in the directive. `01-DECISION.md` B1.

| Bit | Mask | Capability |
|---|---|---|
| 0 | `0x00000001` | per-node connection material present in the catalog (BC1) |
| 1 | `0x00000002` | sealed directives available (HPKE, doc_type `0x06`) |
| 2 | `0x00000004` | relay chaining is real: set only when the Clash generator emits relay chains (`01-DECISION.md` P8) |
| 3 | `0x00000008` | settings sync write endpoint available |
| 4 | `0x00000010` | signed mirror pool present |
| 5 | `0x00000020` | DoH endpoints present in the catalog |
| 6 | `0x00000040` | rule-set and geo integrity hashes present (C3) |
| 7 | `0x00000080` | deprecation channel present (B7) |
| 8 | `0x00000100` | onboarding traffic grant available |
| 9 | `0x00000200` | device key enrollment available |
| 10 | `0x00000400` | `variant` is forwarded end to end through `caramba-sub` (`01-DECISION.md` P4) |
| 11 | `0x00000800` | port hopping supported by the fleet |
| 12..31 | | reserved; a signer MUST emit zero, a v1 verifier MUST ignore |

The intersection rule: `effective = operator_cap AND client_cap`, where `client_cap` is the compiled-in bitfield of the running client. A control gated on a bit MUST be hidden when the effective bit is zero, and MUST NOT be rendered as an enabled control that silently does nothing. Bit 2 is the concrete case: the relay picker stays dark until the generator is real, which is what stops it being a placebo (`00-DESIGN-BRIEF.md` R13).

Per invariant 13, a missing `cap` field on a profile that has already pinned a root key is a hard, non-dismissible error, not a downgrade to "assume everything".

**Which copy wins when the catalog and the directive disagree.** `cap` appears in two documents with different lifetimes, the catalog at 30 days and the directive at 1 hour, and during a capability change they disagree for as long as the cached catalog lives.

> The `cap` carried in the freshest **verified and unexpired** directive is the operator capability. The catalog's `cap` is used only while the client holds no verified, unexpired directive, which is the first-run case and the deep-offline case. The intersection rule then applies to whichever copy won: `effective = resolved_operator_cap AND client_cap`.
>
> The exception is the four bits that assert the presence of catalog content rather than an operator policy: bit 0 (per-node material), bit 4 (mirror pool), bit 5 (DoH endpoints) and bit 6 (resource hashes). A bit in that set that is 1 in the directive but whose backing array is absent or empty in the bound catalog MUST be treated as 0, because the data the bit promises is not there. This is a statement of fact, not a policy override, and it cannot grant a capability.
>
> A client MUST record a catalog-versus-directive disagreement in the verification chrome, so an operator can see that a device is running on an old catalog.

Both documents are signed by the same `online` role key, so taking the fresher one grants an adversary holding that key nothing he did not already have, and it is what makes the kill switch of `06-MIGRATION.md` section 4 propagate in one directive interval rather than in one catalog lifetime.

---

## 6. Verification order, and the parse/verify boundary

An implementation MUST perform these steps in this order, and MUST NOT proceed past a failure.

The split is load-bearing. **Parse failures** are decided entirely from the incoming bytes with no key material and no stored state. **Verification failures** require the trusted key document, the pinned tenant identity, the stored high-water mark, the clock or the outstanding nonce. Everything computable without secrets happens first, so a hostile frame cannot steer the verifier into key material before it is well-formed.

### 6.1 Parse steps

| Step | Check | Code on failure |
|---|---|---|
| P1 | `total_len >= 8` | `E_PARSE_SHORT` |
| P2 | `bytes[0..4] == "CSM1"` | `E_PARSE_MAGIC` |
| P3 | `doc_type` is a defined, non-reserved value (section 1.2) | `E_PARSE_DOCTYPE` |
| P4 | `payload_len = be16(bytes[5..7])`, and `1 <= payload_len <= 49152` | `E_PARSE_LEN` |
| P5 | `total_len >= 8 + payload_len` | `E_PARSE_SHORT` |
| P6 | `nsigs = bytes[7 + payload_len]`, and `1 <= nsigs <= 4` | `E_PARSE_NSIGS` |
| P7 | `total_len == 7 + payload_len + 1 + 76 * nsigs`, exactly | `E_PARSE_FRAMING` |
| P8 | signature slots are in strictly ascending `keyid_trunc` order, no duplicates | `E_PARSE_SLOTORDER` |
| P9 | payload decodes under the strict CBOR profile of section 3 | `E_PARSE_CBOR` |
| P10 | common envelope keys 1..5 all present, correctly typed, and `v == 1` | `E_PARSE_ENVELOPE` |
| P11 | every critical-range key is known for this `doc_type`; every mandatory field for this `doc_type` is present; every field satisfies the type, cap and cross-field rule stated for it in section 8 | `E_PARSE_FIELD` |
| P12 | `doc_type == 0x01` only: every `pk` in `keys` passes all three clauses of section 2.1, and every `kid` equals `sha256(pk)[0..12]` | `E_PARSE_FIELD` |

A parse failure means the bytes are not a CSM/1 frame. The correct handling is to discard them and treat the transport rung as having returned nothing, which advances the ladder. A parse failure MUST NOT be surfaced to the user as a tampering claim, because the overwhelmingly likely cause is a captive portal, a mirror serving an error page, or a truncated response.

Step P12 exists because section 2.1 governs a key entering the trusted set and no other step reaches the `pk` bytes inside a key document's `keys` array: V6 validates the public keys of the slots that signed the frame in front of it, not the key material the frame is trying to install. Without P12 a key document could carry a small-order or non-canonical `pk`, be accepted, and poison every later verification. It is a parse step, not a verification step, because it is decidable entirely from the incoming bytes with no key material and no stored state, which is the boundary this section draws. `05-TEST-VECTORS/` records the same gap as correction `cor-3` and assigns the same code.

### 6.2 Verification steps

| Step | Check | Code on failure |
|---|---|---|
| V1 | resolve the required role from `doc_type` using the table in section 7.1 | cannot fail; see below |
| V2 | load the trusted key document for the pinned `pid`. For `doc_type` `0x01` and `0x05` at first trust, the anchor is `link_pin` instead (section 7.2) | `E_VERIFY_NOANCHOR` |
| V3 | read the authorized key set and the threshold for the required role **from the previously trusted document**, never from the document under verification | `E_VERIFY_ROLE` |
| V4 | every slot's `keyid_trunc` is in that authorized key set | `E_VERIFY_UNAUTHORIZED` |
| V5 | no slot's `keyid_trunc` appears in the trusted document's `rev` list | `E_VERIFY_REVOKED` |
| V6 | each slot's public key passes section 2.1; each signature passes section 2.2 over the pre-image | `E_VERIFY_SIG` |
| V7 | the count of distinct valid signers is at least the threshold | `E_VERIFY_THRESHOLD` |
| V8 | payload `pid` byte-equals the pinned `pid` | `E_VERIFY_PID` |
| V9 | version rule (section 6.3) | `E_VERIFY_VERSION` |
| V10 | `doc_type == 0x01` only: rotation rule (section 7.3) | `E_VERIFY_ROTATION` |
| V11 | `iat <= now + 300` when `clock_trusted`, **and** `iat + LIFETIME_MAX[doc_type] + 300 >= time_floor` (section 6.4) | `E_VERIFY_IAT` |
| V12 | `now <= exp + 300` when `clock_trusted`; see section 6.5 for what expiry means | `E_VERIFY_EXPIRED` |
| V13 | `doc_type == 0x03` only: `nonce` byte-equals the nonce this device just sent, and `dtp` byte-equals this device's thumbprint | `E_VERIFY_NONCE`, `E_VERIFY_DEVICE` |
| V14a | `doc_type == 0x02` only: `sha256(frame)` equals the `cat` the trusted directive named | `E_VERIFY_CATHASH` |
| V14b | `doc_type == 0x02` only, **and only when the trusted key document carries a `tiers` entry for the directive's `tier`**: `sha256(frame)` equals that entry | `E_VERIFY_CATHASH` |

V1 carries no error code because it cannot fail. Parse step P3 has already rejected every undefined, reserved and private `doc_type`, and section 7.1 has a row for every value that survives. `E_VERIFY_ROLE` is reachable only at V3, on a tenant whose trusted key document has no `roles` entry for the required role, which is the `root_only` shape in `05-TEST-VECTORS/`.

V14 is split because `tiers` is optional in the format (section 8.1, key 13). V14a is unconditional and is what binds a catalog to the directive that named it. V14b is what stops a compromised online key inventing a fleet, and it can only run when the operator has published the tier hash.

> A conforming panel MUST publish a `tiers` entry for every tier it serves (section 8.1). A client whose trusted key document carries no `tiers` entry for its directive's `tier` MUST NOT reject the catalog on that ground; it MUST record the reduced containment in the verification chrome as **fleet not root-anchored** and MUST NOT present the fleet as verified. `04-THREAT-MODEL.md` 2.3 is why: without V14b there is no bound at all on what a compromised online key may put in a catalog, and the client is the only party that can tell the user which case it is in.

A verification failure is a security event. It MUST be recorded in the per-rung attempt history that invariant 17 requires the user to be able to see, it MUST be surfaced in the verification chrome that invariant 19 requires, and it MUST NOT be silently swallowed. During the shadow phase of the migration (`01-DECISION.md` C7 phase 1) it is logged and never fatal; from the verify-and-compare phase onward it is fatal for that document.

A verification failure does not, by itself, stop the ladder. The client MAY continue to the next rung, because a hostile mirror is exactly the case the ladder exists for. It MUST NOT, however, treat a verification failure as equivalent to an empty response when deciding what to show the user.

### 6.3 The version rule, stated exactly

`01-DECISION.md` invariant 9 says a document at or below the stored high-water mark is refused. Taken literally that makes re-reading a cached document impossible, since a re-fetch of an unchanged catalog carries the same `ver`. The exact rule is:

> Let `hwm` be the stored high-water mark for the tuple `(pid, doc_type, scope)`, where `scope` is the locator for `doc_type` `0x03` and `0x08`, the `cat_id` for `0x02` and `0x04`, and empty for `0x01` and `0x05`.
>
> - `ver < hwm` MUST be rejected.
> - `ver == hwm` MUST be rejected **unless** the frame is byte-identical to the stored frame for that tuple, in which case it is accepted as the same document and no state changes.
> - `ver > hwm` is accepted, and `hwm` is advanced to `ver` only after every remaining step in section 6.2 has passed.

> **V9 is inert for `doc_type` `0x02` and `0x04`, by construction, and this is deliberate.** `cat_id` is `base32_crockford(chash[0..10])`, a function of the catalog's own bytes (section 4), so two distinct catalogs never share a scope and an older catalog is always evaluated against an `hwm` of 0. The anti-rollback bound for a catalog is not V9: it is V14a, because a catalog is only ever entered at `verified` when a **directive** named its `chash`, and that directive is itself monotonic under V9 in the locator scope. Scoping the catalog high-water mark by `tier` instead would make V9 fire, and is rejected: it would permanently refuse a legitimate operator revert to a previously published, still-valid catalog, which is an operation the persisted-frame rule of section 1.5 makes normal. `04-THREAT-MODEL.md` 2.1 states the bound in the corrected form.
>
> The catalog's own `ver` is therefore ordering information rather than a freshness check. A panel MUST derive it from a per-tier content-change counter, incremented exactly when the content digest of section 1.5 changes, so that the verification chrome and the operator can order two catalogs for one tier. It MUST NOT be a wall clock and MUST NOT be a row id.

The high-water mark is persisted and is monotonic. It MUST live in exactly one store. `01-DECISION.md` X3 records why: on iOS `PacketTunnelProvider.swift:147,166` builds a Go core inside the Network Extension while `CarambaVpnPlugin.swift:313-318` builds a second one with a separate work directory, and two work directories are two high-water marks, which is a rollback hole rather than defence in depth.

### 6.4 The time floor

On first run there is no trusted clock. A factory-reset device with DNS blocking preventing NTP is the normal case, not the exception.

> The client MUST establish `time_floor` at enrollment as the highest `iat` of any document successfully verified during enrollment. `time_floor` MUST be persisted per profile, MUST be monotonic, and MUST NEVER decrease. After enrollment it advances on, and only on, acceptance of a `0x03` directive: `time_floor = max(time_floor, directive.iat)`.
>
> Verification step V11 tests, for every document type:
>
> ```
> iat + LIFETIME_MAX[doc_type] + 300 >= time_floor
> ```
>
> A document that fails this test had already expired at the moment the profile last heard from the panel, and MUST be rejected.

`01-DECISION.md` 5.2.3. The nonce is the mechanism that survives a wrong clock entirely; the floor is what stops an adversary replaying a document that was already dead when the profile last heard from the panel.

**Correction A to `01-DECISION.md` 5.2.3, and to an earlier draft of this section.** Both defined the floor as the greater of the enrollment-time server `Date` header and the highest observed `iat`. The `Date` header is unsigned and attacker-controlled, and the floor never decreases, so one hostile mirror returning `Date: Sat, 01 Jan 2101 00:00:00 GMT` sets a floor no legitimately signed document can ever clear and the profile is permanently bricked. The floor is derived from signed `iat` values only. The `Date` header keeps a clamped, display-only role (`02-SPEC.md` 5.5).

**Correction B, and it is why V11 carries the lifetime term.** The earlier form of V11 tested `iat >= time_floor`. Once the floor advances to a fresh directive's `iat`, every legitimately cached document older than that directive fails, including a 20-day-old catalog with 10 days of life left. The corrected test rejects exactly what the floor exists for and nothing else. `05-TEST-VECTORS/` carries `pos-c1-stale-but-live`, a 20-day-old catalog under a fresh floor, whose expected verdict is **accept**; an implementation of the literal earlier form fails that fixture, which is the intended signal.

**First trust, where neither the floor nor the high-water mark exists yet.** At enrollment `clock_trusted` is false (`02-SPEC.md` 5.5), so V11's skew clause and V12 are both inert and an adversary could serve an arbitrarily old key document that still matches `link_pin`, resurrecting a revoked online key.

> A client MUST compile in `BUILD_EPOCH`, the Unix second at which the running build was produced. At enrollment, and only at enrollment, the client MUST classify the device clock as **plausible** when `BUILD_EPOCH <= device_clock <= BUILD_EPOCH + 315360000` (ten years). When the clock is plausible the client MUST run V11's `iat <= now + 300` clause and V12 against it for the enrollment key document, catalog and directive. When it is not plausible the client MUST refuse to complete enrollment and MUST say that the device clock is wrong, naming it as the reason; it MUST NOT enrol blind and it MUST NOT set the device clock itself.

This bounds the first-trust replay window at the key document's own 604800-second lifetime on a device whose clock is roughly right, and refuses enrollment rather than accepting an unbounded replay on a device whose clock is not. `04-THREAT-MODEL.md` residual R-2 carries what remains.

### 6.5 What expiry does and does not do

> An expired document MUST NOT disconnect a user, MUST NOT tear down a tunnel, and MUST NOT clear a cached configuration. Expiry means the document is refused for accepting **new instructions and new status**. A cached, expired, previously verified document remains valid for connecting.

This is invariant 16 and it is absolute. Skew tolerance is 300 seconds in both directions at V11 and V12.

> **An expired trusted key document remains a valid authorization anchor.** V12 applies to the document under verification, never to the anchor read at V3. A client MUST continue to read `roles`, `thr` and `rev` from its trusted key document after that document's `exp` has passed, MUST continue to enforce `rev` from it, and MUST surface the anchor's age in the verification chrome. The alternative makes the 7-day lifetime a fleet-wide kill switch that fires whenever the operator is unreachable for a week. `04-THREAT-MODEL.md` 2.4 and Correction 7.

The offline grace window (`01-DECISION.md` C5) is a separate, operator-set dial carried as `exph` in the directive. It bounds how long the client will keep operating on cached documents before it stops offering to connect at all, and the client MUST render its consequence beside it: a longer window buys blackout tolerance and buys the same amount of un-revocable service.

### 6.6 Error code registry

Parse: `E_PARSE_SHORT`, `E_PARSE_MAGIC`, `E_PARSE_DOCTYPE`, `E_PARSE_LEN`, `E_PARSE_NSIGS`, `E_PARSE_FRAMING`, `E_PARSE_SLOTORDER`, `E_PARSE_CBOR`, `E_PARSE_ENVELOPE`, `E_PARSE_FIELD`.

Verify: `E_VERIFY_ROLE`, `E_VERIFY_NOANCHOR`, `E_VERIFY_UNAUTHORIZED`, `E_VERIFY_REVOKED`, `E_VERIFY_SIG`, `E_VERIFY_THRESHOLD`, `E_VERIFY_PID`, `E_VERIFY_VERSION`, `E_VERIFY_ROTATION`, `E_VERIFY_IAT`, `E_VERIFY_EXPIRED`, `E_VERIFY_NONCE`, `E_VERIFY_DEVICE`, `E_VERIFY_CATHASH`.

Seal (section 9): `E_SEAL_RECIPIENT`, `E_SEAL_SUITE`, `E_SEAL_OPEN`.

**Armored form (section 10):** every failure of the armored reader maps to `E_PARSE_FRAMING`. There is no `E_ARMOR_*` family and one MUST NOT be invented. The six conditions section 10.3 and section 4.1 make rejections, a `bid` or `n` disagreement, a missing ordinal, a duplicate ordinal with different data, a non-final chunk that is not exactly 620 bytes, a non-Crockford character, and non-zero trailing pad bits, all mean the same thing to a caller: the artifact in front of it is not a frame stream. Distinguishing them would give three implementations six more opportunities to disagree for no operational gain, and the reader SHOULD carry the specific condition in its log message rather than in its code. `05-TEST-VECTORS/` assigns `E_PARSE_FRAMING` to all six armor negatives on this rule.

These identifiers are the shared vocabulary of the negative-fixture corpus in `05-TEST-VECTORS/`. All three implementations MUST return the same code for the same fixture; agreeing that a fixture fails is not sufficient, because two implementations failing for different reasons is how a real divergence hides.

---

## 7. Authorization: doc_type to role to threshold

This is the table `01-DECISION.md` 5.1.3 calls out as the single rule three implementers would otherwise each invent differently.

### 7.1 The table

| `doc_type` | Document | Required role | Threshold source | Key set source |
|---|---|---|---|---|
| `0x01` | key document | `root` | `roles[1].thr` of the **previously trusted** key document | `roles[1].ks` of the previously trusted key document, plus `roles[1].ks` of the document under verification (rotation, section 7.3) |
| `0x02` | catalog | `online` | `roles[2].thr` of the trusted key document | `roles[2].ks` of the trusted key document |
| `0x03` | directive | `online` | `roles[2].thr` of the trusted key document | `roles[2].ks` of the trusted key document |
| `0x04` | catalog chunk | `online` | `roles[2].thr` of the trusted key document | `roles[2].ks` of the trusted key document |
| `0x05` | bootstrap blob | `root` | 1 | the single key whose `sha256[0..12]` matches `link_pin` (section 7.2) |
| `0x06` | sealed directive | `online` for the outer frame; the inner `0x03` frame is verified again in full under its own row | `roles[2].thr` | `roles[2].ks` |
| `0x08` | reserve pool | `root` | `roles[1].thr` of the trusted key document | `roles[1].ks` of the trusted key document |

> A verifier MUST resolve the required role from the `doc_type` in the frame, and MUST read the key set and the threshold for that role from the previously trusted key document. It MUST NOT read them from the document being verified, with the single exception of the rotation rule in section 7.3, which requires **both**.
>
> There MUST be no code path, and no API endpoint, that returns a public key without its role.

Without this rule an attacker holding the online signing key mints a key document at version N+1 containing only their own key under role `root`, the client's high-water mark advances, and the operator can never recover. That voids the design's headline compromise-recovery property.

The corollary, enforced at P11: a key document in which some entry of `keys` is not referenced by any role MUST be rejected. Role lives only in `roles`; the key entries carry no role field. One authority, no possibility of disagreement.

### 7.2 First trust

Trust on first use, and the specification says so plainly.

At enrollment the client holds `link_pin`, 96 bits of `base32_crockford(sha256(root_public_key)[0..12])`, carried as `k=` on the enrollment deep link and inside the bootstrap blob. The first key document accepted for a `pid` MUST contain exactly one key under role `root` whose `sha256(pk)[0..12]` matches `link_pin`. On mismatch the client MUST refuse enrollment with a hard error. There MUST NOT be a "continue anyway" affordance on any code-based enrollment path.

The manual-entry path, which is the one that survives Telegram being blocked, MUST carry at least the first 40 bits, that is the first 8 base32 characters, of `link_pin` as a required dictated field. `01-DECISION.md` 5.1.6 folds the pin into the enrollment code so there is one string to dictate, and `02-SPEC.md` 9.2 gives the format: 20 characters, `link_pin[0..8]` followed by 12 characters of secret, rendered in five hyphen-separated groups of four. Against this document's fixture `link_pin` of `49Q8M87PK6WP9QXG3T30`, a conforming code is `49Q8-M87P-KQZ3-WFDG-ZTJX`, which is the code the `05-TEST-VECTORS/` blob `pos-b1-min` carries. A client MUST ignore hyphens when parsing it (section 4.1). The code printed inside the section 8.5 hex dump, `K7QW-3M2P-9XRT`, predates that rule and does not satisfy it; see the note under that fixture.

### 7.3 Root rotation

`01-DECISION.md` 5.1.7, following TUF.

> A key document with `doc_type` `0x01` and `ver = N+1`, where `N` is the version of the currently trusted key document, MUST be verified twice over the same pre-image:
>
> 1. against `roles[1].ks` and `roles[1].thr` of the **currently trusted** document, and
> 2. against `roles[1].ks` and `roles[1].thr` of the **document under verification**.
>
> Both MUST pass. A client MUST refuse to skip a version: `ver != N+1` is `E_VERIFY_ROTATION`, not `E_VERIFY_VERSION`.

A rotation frame therefore carries at least two signature slots when the key sets differ, sorted by `keyid_trunc` per section 1.4, and `nsigs` accounts for both.

Clients MUST be able to walk intermediate versions in one request, `GET /sub/k1?since=N` (section 13.2), and an out-of-band bundle MAY carry the full intermediate chain in the multi-frame armored form of section 10, so the offline rung survives rotation.

---

## 8. Payloads: field tables and worked encodings

### 8.0 The common envelope

Every payload, of every `doc_type`, begins with these keys. They are in the critical range and keys 1 through 5 are mandatory everywhere.

| Key | Name | Type | Mandatory | Cap | Meaning |
|---|---|---|---|---|---|
| 1 | `v` | uint | yes | must equal `1` | CSM specification version |
| 2 | `pid` | bstr | yes | exactly 8 | tenant identity, `sha256(root_pk)[0..8]` |
| 3 | `ver` | uint | yes | < 2^32 | monotonic document version |
| 4 | `iat` | uint | yes | < 2^53 | issued at, Unix seconds UTC |
| 5 | `exp` | uint | yes | < 2^53 | expires at, Unix seconds UTC |
| 6 | | | | | reserved, critical: MUST NOT appear in v1 |
| 7 | | | | | reserved, critical: MUST NOT appear in v1 |
| 8 | | | | | reserved, critical: MUST NOT appear in v1 |
| 9 | `pd` | bstr | no | <= 3072 | padding, all bytes MUST be `0x00`, ignored on decode (section 12) |

Doc-specific fields begin at key 10.

Expiry values, normative per `01-DECISION.md` 5.2.1: key document `exp - iat = 604800` (7 days), catalog `2592000` (30 days), directive `3600` (1 hour), bootstrap blob `2592000` (30 days), reserve pool `604800` (7 days). A panel MUST NOT sign a document whose lifetime exceeds these values; it MAY sign a shorter one.

Envelope cost, measured from the fixtures: 1 byte for the map head plus `01 01` (2) plus `02 48` + 8 (10) plus `03` + 1 (2) plus `04 1a` + 4 (6) plus `05 1a` + 4 (6) = **27 bytes** for a document with `ver < 24`. It is 28 bytes for `ver` in 24..255, 29 bytes for 256..65535, and 31 bytes above that, because only the `ver` term grows.

### 8.0.1 Per-document emission bounds

The field caps in the tables below bound a **decoder**. They do not bound a **signer**, and for the three document types that have no chunking path the caps admit a document that cannot be delivered: a key document at every cap measures 5118 bytes against a `thr.resp_max` of 4096 (`05-TEST-VECTORS/` `document_sizes.key_document`, correction `cor-4`), and `mir` at 32 entries does the same to a bootstrap blob and a reserve pool.

> A panel MUST NOT sign a `0x01`, `0x05` or `0x08` frame whose **total frame length**, padding included, exceeds `DOC_FRAME_MAX = 4096` bytes. It MUST refuse and log, naming the tenant and the field that overflowed, rather than emitting a document no client can read. `0x02` and `0x04` are exempt because every catalog is chunked (section 11.4) and a chunk frame is bounded separately by `CHUNK_RESP_MAX`.

`DOC_FRAME_MAX` equals `RESP_MAX`, because for these three types the frame **is** the response body and `02-SPEC.md` 8.5 requires every body to be read through a limiter set at `thr.resp_max`. Nothing above that limit is deliverable, so nothing above it may be signed.

The field that overflows first in practice is the key document's revocation list: `rev.nodes` at its cap of 256 entries is roughly 2.6 KB on its own. A panel that needs a longer revocation list than `DOC_FRAME_MAX` admits MUST drop revoked node ids that no longer appear in any tier's catalog, since a node id that names nothing has nothing to revoke, before it drops anything else. Extending the `0x04` chunk mechanism to `0x01` is the v2 fix and is reserved, not specified here.

### 8.1 Key document, `doc_type = 0x01`

Root-signed. Carries the key set, the role thresholds, the revocation list and the per-tier catalog hashes. This is the trust anchor for everything else.

| Key | Name | Type | Mandatory | Cap | Meaning |
|---|---|---|---|---|---|
| 10 | `keys` | array of key entries | yes | 1..16 | every public key this tenant uses |
| 11 | `roles` | map, key = `role` enum | yes | 1..3 pairs | role to `{ks, thr}` |
| 12 | `rev` | map | no | | revocation, see below |
| 13 | `tiers` | map, key = tier id uint | no, but see below | <= 16 pairs, each key in 1..1023 | tier id to `bstr(32)` catalog `chash` |
| 14 | | | | | reserved, critical (see Correction 6, section 16) |
| 15 | `dep` | array of deprecation entries | no | <= 16 | `01-DECISION.md` B7 |
| 16 | `ttlk` | uint | no | 300..86400 | seconds until the client should refetch this document |

**Key entry** (map):

| Key | Name | Type | Mandatory | Cap |
|---|---|---|---|---|
| 1 | `kid` | bstr | yes | exactly 12 |
| 2 | `alg` | uint | yes | `alg` enum, `1` |
| 3 | `pk` | bstr | yes | exactly 32 for `alg == 1` |

`kid` MUST equal `sha256(pk)[0..12]`. A verifier MUST recompute it and MUST reject a mismatch (`E_PARSE_FIELD`). Carrying it is a size cost of 14 bytes per key and buys a lookup that does not require hashing every candidate; carrying it unchecked would buy an attacker a free alias. Every `pk` is additionally validated against all three clauses of section 2.1 at parse step P12, because this is the only place key material enters the trusted set.

**`tiers` is optional to decode and mandatory to emit.** The field is typed optional so that a verifier written to this document tolerates a panel that does not publish it, and so that V14b degrades to a chrome statement rather than to a fleet-wide rejection. That tolerance is not a licence: a conforming panel MUST publish a `tiers` entry for every tier it serves, and MUST re-sign the key document whenever any tier's catalog `chash` changes. `04-THREAT-MODEL.md` 2.3 calls this the single highest-value operator action in the protocol, and section 6.2 states what a client does when it is absent.

**The consequence for fleet changes, stated rather than discovered later.** Because V14b compares against the root-signed `tiers` entry, a panel MUST NOT point a directive at a catalog whose `chash` is not in the current key document's `tiers` while `tiers` is published for that tier. The sequence for a fleet change is therefore: content digest changes, the panel signs and persists the new catalog (section 1.5), a root-signed key document naming the new `chash` is imported, and only then do directives move to the new `cat`. Until the key document arrives the panel keeps serving the previous catalog and the previous `cat`. That is a real operational cost, it is the offline ceremony `01-DECISION.md` A1 predicts an operator will avoid, and the mitigation is cadence rather than exemption: an operator SHOULD run a scheduled root signing at least weekly so a fleet change lands within one cycle, and MAY sign on demand.

**`tier` identity.** `tier` is a `uint` in the range **1..1023**, derived from the panel's `plans.id` by a rule the panel states once and never changes. A panel MUST refuse to sign a catalog for a plan whose derived `tier` falls outside that range, and MUST refuse to serve CSM routes for a tenant with more than 16 tiers, because `tiers` is capped at 16 pairs and a tier without a published hash is a tier without V14b. Both refusals are startup or save-time checks, not request-time ones.

> **Correction: the upper bound is 1023, not 65535, and the lower bound is 1.** An earlier revision of this table typed `tier` as `uint < 2^16` while also making the tier id a CBOR **map key** in `tiers`. Section 3.3 rejects map key 0 and every key at or above 1024, so a conforming panel deriving a tier id of 1024 or above signed a key document that no conforming verifier could decode: the tenant went dark at P9 with `E_PARSE_CBOR`, an error naming CBOR rather than tiers. Tier 0 was unrepresentable for the same reason. The range is therefore stated once, here, and it binds the `tier` field of the catalog (section 8.2 key 10) and of the directive (section 8.3 key 16) as well as the `tiers` keys, so that every tier a panel can name is a tier a root can anchor. Sixteen tiers is the cohort ceiling, so 1023 is not a constraint an operator can reach.

**Role entry** (value in `roles`):

| Key | Name | Type | Mandatory | Cap |
|---|---|---|---|---|
| 1 | `ks` | array of bstr(12) | yes | 1..16 |
| 2 | `thr` | uint | yes | 1..16, and `thr <= len(ks)` |

Every `kid` in every `ks` MUST appear in `keys`, and every entry of `keys` MUST appear in at least one `ks`. Violations are `E_PARSE_FIELD`.

**Revocation** `rev` (map). `01-DECISION.md` 5.2.9.

| Key | Name | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `kids` | array of bstr(12) | <= 64 | revoked key ids |
| 2 | `nodes` | array of tstr | <= 256 | revoked node ids (section 8.2.1 charset) |

> A client that sees a `kid` in `rev` MUST reject that key and every document it signed, **including documents already cached on disk**, immediately. A node id in `rev` MUST be honored against the cached catalog, so a seized node is dropped even while the client is running offline with no network at all.

**Deprecation entry** `dep`:

| Key | Name | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `s` | tstr | <= 48 | surface identifier, closed vocabulary maintained in `02-SPEC.md` |
| 2 | `sun` | uint | | sunset, Unix seconds; MUST be at least `iat + 15552000` (180 days) |

**Worked encoding, minimal key document.** One root key, one online key, threshold 1 each, no revocation, no tiers. Root-signed, `nsigs = 1`.

```
payload_len = 173     nsigs = 1     total = 257     7 + 173 + 1 + 76 = 257

0000  43 53 4d 31 01 00 ad a7 01 01 02 48 22 6e 8a 20
0010  f6 99 b9 64 03 01 04 1a 6a 97 67 00 05 1a 6a a0
0020  a1 80 0a 82 a3 01 4c 22 6e 8a 20 f6 99 b9 64 df
0030  b0 1e 86 02 01 03 58 20 8b 16 0c 71 c6 10 08 32
0040  1c ae 0d 0d c9 a9 80 b6 e5 9c b2 6d 0f 4d 1f a8
0050  df c0 30 c3 67 5e 2b 7c a3 01 4c 21 e3 e2 cc 0a
0060  3b a7 77 e6 9c e1 4c 02 01 03 58 20 75 f3 50 b3
0070  eb 21 34 4a 96 de 19 5d 82 07 9e 45 f0 a5 6f ec
0080  dc 73 6c 16 b6 1d 56 61 9a fd 56 53 0b a2 01 a2
0090  01 81 4c 22 6e 8a 20 f6 99 b9 64 df b0 1e 86 02
00a0  01 02 a2 01 81 4c 21 e3 e2 cc 0a 3b a7 77 e6 9c
00b0  e1 4c 02 01 01 22 6e 8a 20 f6 99 b9 64 df b0 1e
00c0  86 a1 61 3d 53 85 a1 e5 52 15 37 17 7a 83 a2 fd
00d0  09 0a e2 56 bc e5 13 f2 a9 a6 b9 88 09 00 f7 b6
00e0  4e 37 d3 eb 72 69 bf 80 fc d2 49 c5 72 48 11 9a
00f0  13 a2 7e 03 b1 6e f4 7e 63 59 cb 99 20 4f d8 7a
0100  0e

sha256(frame) = 671eaaaf6729274419faefe0cb44430126d6421e2f0f628bc4f1fab376bdad35
```

Field walk, byte by byte:

| Offset | Bytes | Meaning |
|---|---|---|
| 0 | `43 53 4d 31` | magic `CSM1` |
| 4 | `01` | doc_type, key document |
| 5 | `00 ad` | payload_len = 173 |
| 7 | `a7` | CBOR map, 7 pairs |
| 8 | `01 01` | `v` = 1 |
| 10 | `02 48 22 6e 8a 20 f6 99 b9 64` | `pid` = `226e8a20f699b964` |
| 20 | `03 01` | `ver` = 1 |
| 22 | `04 1a 6a 97 67 00` | `iat` = 1788307200 (2026-09-02T00:00:00Z) |
| 28 | `05 1a 6a a0 a1 80` | `exp` = 1788912000 (+7 days) |
| 34 | `0a 82` | `keys`, array of 2 |
| 36 | `a3 01 4c ...` | key entry 1: 3 pairs, `kid` bstr(12) `226e8a20f699b964dfb01e86` |
| 51 | `02 01` | `alg` = 1 |
| 53 | `03 58 20 8b16...2b7c` | `pk`, the root public key |
| 88 | `a3 01 4c 21e3...e14c` | key entry 2: online `kid` `21e3e2cc0a3ba777e69ce14c` |
| 105 | `02 01 03 58 20 75f3...5653` | `alg` = 1, online public key |
| 141 | `0b a2` | `roles`, map of 2 |
| 143 | `01 a2 01 81 4c 226e...1e86 02 01` | role 1 (`root`): `ks` = [root kid], `thr` = 1 |
| 164 | `02 a2 01 81 4c 21e3...e14c 02 01` | role 2 (`online`): `ks` = [online kid], `thr` = 1 |
| 180 | `01` | `nsigs` = 1 |
| 181 | `22 6e 8a 20 f6 99 b9 64 df b0 1e 86` | slot `keyid_trunc`, the root key |
| 193 | `a1 61 3d ... d8 7a 0e` | 64-byte Ed25519 signature over bytes 0..180 |

Note offset 180: the pre-image is bytes `0x0000` through `0x00b3` inclusive, that is 7 + 173 = 180 bytes. The signature covers the magic, the type byte, the length field and the payload, and stops there.

### 8.2 Catalog, `doc_type = 0x02`

Online-signed, content-addressed, nonce-free, byte-identical for every subscriber on a plan tier. Therefore cacheable and mirrorable. Authorized, not public (`01-DECISION.md` 5.2.4 and 4.6).

| Key | Name | Type | Mandatory | Cap | Meaning |
|---|---|---|---|---|---|
| 10 | `tier` | uint | yes | 1..1023 | plan tier this catalog serves |
| 11 | `ex` | array of node entries | yes | 1..512 | exit nodes |
| 12 | `re` | array of node entries | no | 0..64 | relay nodes |
| 13 | `ro` | array of route entries | no | 0..32 | routing presets |
| 14 | `cap` | bstr | yes | exactly 4 | operator capability bitfield (section 5.1) |
| 15 | `mir` | array of mirror entries | no | 0..32 | signed mirror pool, per cohort |
| 16 | `doh` | array of DoH entries | no | 0..8 | bootstrap DoH endpoints |
| 17 | `rs` | array of resource entries | no | 0..32 | rule-set providers with sha256 |
| 18 | `geo` | array of resource entries | no | 0..8 | geo databases with sha256 |
| 19 | `ttl` | uint | yes | 300..86400 | refresh cadence in **explicit seconds** |
| 20 | `jit` | uint | yes | 0..50 | refresh jitter, percent of `ttl` |
| 21 | `thr` | map | yes | | signed size thresholds, see below |
| 22 | `pb` | array[2] of uint | yes | each 0..15, `pb[0] <= pb[1]` | per-tenant padding bucket range (section 12) |
| 23 | `lad` | map | no | | ladder defaults, see below |
| 24 | `pin` | array of pin entries | no | 0..32 | TLS SPKI pins for manifest hosts |
| 25 | `hpk` | bstr | no | exactly 65 | **panel** HPKE recipient key, P-256 uncompressed; for client-to-panel sealing only, never the recipient of a `0x06` |
| 26 | `hpkv` | uint | no | < 2^16 | generation of `hpk`; starts at 1; MUST be present exactly when `hpk` is present |

`ttl` in explicit seconds is what settles the hours-versus-minutes ambiguity without either legacy consumer changing: the panel keeps emitting the bare string `"2"` in `Profile-Update-Interval` for Hiddify and sing-box (`apps/caramba-panel/src/subscription.rs:840`), which the Go core parses as minutes (`libs/caramba-core/subscription/subscription.go:183-187`) and Hiddify reads as hours, while Caramba Connect reads `ttl` and ignores the header entirely.

**`thr`** (signed size thresholds; `01-DECISION.md` invariant 5, D5):

| Key | Name | Type | Default | Meaning |
|---|---|---|---|---|
| 1 | `conn_bytes` | uint | 8192 | maximum accounted bytes on one TCP connection, **handshake included** |
| 2 | `conn_packets` | uint | 22 | maximum accounted data-bearing packets on one TCP connection |
| 3 | `resp_max` | uint | 4096 | maximum response body bytes |

All three are signed catalog fields, not compiled constants. Section 11 derives the defaults and names the measurement that changes them. A client does not honor them unclamped: `02-SPEC.md` 8.6 and section 14 there carry the compiled ceilings that bound a hostile signer, and the rule is that the client honors the safer of the signed value and its own ceiling.

**Array ordering, and what it is and is not.**

> The panel MUST emit every array in this document under a **total order that is a pure function of the content model**, applied identically in every panel process and across restarts. Signing the same tier twice, from two processes, MUST produce identical bytes. Content addressing rests entirely on this: an unordered `SELECT` makes `chash` a function of Postgres row order, and the tier hash published in the key document then stops matching within one restart.
>
> The RECOMMENDED order is ascending by the entry's primary identifier compared as raw bytes: `id` for `ex`, `re` and `ro`; `h` for `mir`, `doh` and `pin`; `n` for `rs` and `geo`; `kid` for `keys`; ascending numeric for `alp`. A panel that uses a different total order MUST document it and MUST cover it by the determinism test.
>
> A verifier MUST NOT reject a document on array order. Order is a signer obligation enforced by `06-MIGRATION.md` 2.2 exit criterion 4, not a decode rule, and the `05-TEST-VECTORS/` catalogs deliberately carry `ro` and `rs` in operator order rather than in the recommended order so that a verifier which secretly depends on ordering fails the corpus.

The panel-side consequence is named in `06-MIGRATION.md` 3.2 P6: `get_all_active_relay_infos` has no `ORDER BY`, `get_nodes_for_plan` orders by `sort_order` alone with no tiebreaker, and the inbound query `fetch_inbounds_for_nodes` (`apps/caramba-panel/src/services/subscription_service.rs:1877-1883`) has no `ORDER BY` either. All three are deliverables of that step.

**`lad`** (ladder defaults; `01-DECISION.md` D3, 5.3.1):

| Key | Name | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `ord` | array of `rung` uint | 1..7, no duplicates | default rung order for this tenant |
| 2 | `en` | array of `rung` uint | subset of `ord` | default enabled set |

Rung 0 and rung 6 MUST appear in `en`. R6 is never disableable. The user may reorder and toggle, and a rung the user has switched off is never tried, ever, including on failure.

**Mirror entry:**

| Key | Name | Type | Mandatory | Cap |
|---|---|---|---|---|
| 1 | `h` | tstr | yes | <= 64, hostname (section 14.1) |
| 2 | `sni` | tstr | yes | <= 64, explicit per-mirror SNI |
| 3 | `pin` | array of bstr(32) | yes | 1..4, SPKI sha256 |
| 4 | `asn` | uint | yes | < 2^32 |
| 5 | `cc` | tstr | yes | exactly 2, uppercase ISO 3166-1 alpha-2 |
| 6 | `w` | uint | no | 1..100, selection weight, default 10 |
| 7 | `ip` | array of tstr | no | 0..4, literal addresses for the R3 rung |

`asn` and `cc` are resolved server-side at save time and the panel MUST reject a pool with fewer than three distinct ASNs and two distinct countries (`01-DECISION.md` D6). Carrying them in the signed catalog lets the client verify the diversity claim rather than trust it.

**DoH entry:**

| Key | Name | Type | Mandatory | Cap |
|---|---|---|---|---|
| 1 | `h` | tstr | yes | <= 64, hostname |
| 2 | `p` | tstr | yes | <= 64, path-only URL (section 14.2) |
| 3 | `ip` | array of tstr | yes | 1..4, literal addresses |
| 4 | `pin` | array of bstr(32) | yes | 1..4, SPKI sha256 |

A DoH entry's `h` MUST also appear as a mirror entry `h` in `mir`, per `01-DECISION.md` 5.5.4. The consequence, stated rather than hidden: the operator must run or front its own DoH resolver; a third-party public resolver cannot be named in a CSM/1 catalog.

`ip` is mandatory here and only here, because the R3 rung has to resolve without a resolver. The client connects to the literal address, sends `h` as SNI, and validates the certificate against `h` plus the SPKI pins. This is **not** the bare-IP mirror that `01-DECISION.md` 4.2 rejects: certificate validation is not disabled, and no IP-SAN certificate is required, because the name being validated is the hostname carried in SNI.

**Resource entry** (`rs` and `geo`; `01-DECISION.md` C3):

| Key | Name | Type | Mandatory | Cap |
|---|---|---|---|---|
| 1 | `n` | tstr | yes | <= 48, provider name |
| 2 | `u` | tstr | yes | <= 128, path-only URL (section 14.2) |
| 3 | `h` | bstr | yes | exactly 32, sha256 of the fetched bytes |
| 4 | `iv` | uint | no | 3600..604800, refresh interval seconds |

> A client MUST refuse to load a rule-set or geo file whose sha256 does not match `h`. Invariant 12.

This is the only integrity anyone provides for the data that decides which packets enter the tunnel. Verified as absent today: `libs/caramba-core/routing/routing.go:186-192` emits providers as `{type, behavior, format, url, interval}` with no hash, no signature and no pinning.

**Pin entry:**

| Key | Name | Type | Mandatory | Cap |
|---|---|---|---|---|
| 1 | `h` | tstr | yes | <= 64, hostname the pin applies to |
| 2 | `spki` | array of bstr(32) | yes | 1..4 |

**Route entry:**

| Key | Name | Type | Mandatory | Cap |
|---|---|---|---|---|
| 1 | `id` | tstr | yes | <= 32, closed vocabulary, `[a-z0-9-]` |
| 2 | `nm` | tstr | yes | <= 40, display name, inert |
| 3 | `rs` | array of tstr | yes | 0..32, resource entry names from `rs` |

#### 8.2.1 Node entry

This is the entry `01-DECISION.md` BC1 requires and the one that decides the catalog's size. It is sized against the panel's real `StreamInfo` (`apps/caramba-panel/src/singbox/subscription_generator.rs:91-114`) and its real per-protocol emission (`:224-437` for sing-box, `:1138-1340` for Clash), not against a sketch.

| Key | Name | Type | Mandatory | Cap | Source in the panel |
|---|---|---|---|---|---|
| 1 | `id` | tstr | yes | 1..24, charset `[0-9A-Za-z_-]` | `n<node_id>i<inbound_id>` |
| 2 | `pn` | tstr | yes | <= 64 | the verbatim mihomo proxy name, `subscription_generator.rs:1165` |
| 3 | `cc` | tstr | yes | exactly 2, uppercase | `NodeInfo.country_code` |
| 4 | `h` | tstr | yes | <= 64 | `frontend_url` if set, else `NodeInfo.address` |
| 5 | `p` | uint | yes | 1..65535 | `Inbound.listen_port` |
| 6 | `pr` | uint | yes | `pr` enum | `Inbound.protocol` |
| 7 | `nw` | uint | yes | `nw` enum | `StreamInfo.network` |
| 8 | `se` | uint | yes | `se` enum | `StreamInfo.security` |
| 9 | `sni` | tstr | no | <= 64 | `StreamInfo.sni` |
| 10 | `pbk` | bstr | no | exactly 32 | `StreamInfo.public_key`, **raw bytes** |
| 11 | `sid` | tstr | no | <= 16, hex | `StreamInfo.short_id` |
| 12 | `fp` | uint | no | `fp` enum, default 1 | `StreamInfo.fingerprint` |
| 13 | `fl` | uint | no | `fl` enum, default 0 | `StreamInfo.flow` |
| 14 | `pt` | tstr | no | <= 96 | `StreamInfo.ws_path` or `grpc_service` |
| 15 | `hst` | tstr | no | <= 64 | Host header override; omitted when equal to `sni` |
| 16 | `alp` | array of uint | no | 0..3, `alp` enum | ALPN list |
| 17 | `hop` | tstr | no | <= 32 | `StreamInfo.hy2_ports` |
| 18 | `obf` | tstr | no | <= 32 | `StreamInfo.hy2_obfs` |
| 19 | `cg` | uint | no | `cg` enum, default 1 | `StreamInfo.tuic_congestion_control` |
| 20 | `zr` | bool | no | default false | `StreamInfo.tuic_zero_rtt_handshake` |
| 21 | `ins` | bool | no | default false | skip-cert-verify |
| 22 | `rl` | tstr | no | <= 24 | id of the relay entry in `re` this exit chains through |
| 23 | `ssm` | uint | no | `ssm` enum | Shadowsocks method |
| 24 | `mtu` | uint | no | 576..1500 | wireguard MTU |

**`pbk` is carried as 32 raw bytes, not as the 43-character base64url text the panel stores.** Renderers re-encode with unpadded base64url (RFC 4648 section 5, no `=`). This saves 11 bytes per Reality node and the encoding is deterministic, so the Rust and Go renderers cannot diverge on it.

**`pn` is the Clash proxy name, and it is short.** `format_node_label` returns the country flag emoji alone (`subscription_generator.rs:156-158`) and the name is assembled as `format!("{} {}{}", node_label, proto_label, relay_suffix)` at `:1165`, giving strings like `🇩🇪 Stealth` (16 UTF-8 bytes). It is NOT the `"Node #{id} ({mbps} Mbps)"` string, which is a different surface entirely: that one is synthesized by `GET /api/v2/app/servers` at `apps/caramba-panel/src/api/v2/app.rs:250` and never appears in a config body. Getting this wrong is a 20-byte-per-node size error and, worse, breaks `Server.ID == Server.Name == the Clash proxy name`, which is the key for `Up(serverID)`, for `autotune.Candidate.ServerID` and for the mihomo prober's raw proxy map lookup (`libs/caramba-core/subscription/subscription.go:32-48`).

The Clash generator has no proxy-name uniquifier, unlike the sing-box path's `unique_tag` closure at `subscription_generator.rs:1566`. Two inbounds of the same protocol shape on the same country's node produce the same `pn`. `01-DECISION.md` 5.2.5 requires the uniquifier to be fixed in the same pass and the Go renderer to reuse the identical algorithm. Until it is, `pn` is not unique and `id` is the only safe key; `id` is mandatory and `pn` is not a key.

**Worked node entry, VLESS + Reality + TCP.** 129 bytes.

```
ae                          map, 14 pairs
01 65 6e 31 37 69 33        1 id   = "n17i3"                      7 bytes
02 70 f09f87a9 f09f87aa 20 53746561 6c7468
                            2 pn   = "🇩🇪 Stealth"               18 bytes
03 62 44 45                 3 cc   = "DE"                          4 bytes
04 71 64653...6e6574        4 h    = "de1.exa-nodes.net"          19 bytes
05 19 01 bb                 5 p    = 443                            4 bytes
06 01                       6 pr   = 1 (vless)                      2 bytes
07 01                       7 nw   = 1 (tcp)                        2 bytes
08 02                       8 se   = 2 (reality)                    2 bytes
09 71 7777772e...636f6d     9 sni  = "www.microsoft.com"           19 bytes
0a 58 20 <32 bytes>        10 pbk  = raw Reality public key        35 bytes
0b 68 36626138 35313739    11 sid  = "6ba85179"                    10 bytes
0c 01                      12 fp   = 1 (chrome)                     2 bytes
0d 01                      13 fl   = 1 (xtls-rprx-vision)           2 bytes
15 f4                      21 ins  = false                          2 bytes
                                                       map head:    1 byte
                                                            total: 129 bytes
```

**Measured entry sizes across the shapes the panel actually emits:**

| Shape | Bytes, this document's fixtures | Bytes, `05-TEST-VECTORS/` fixtures |
|---|---|---|
| Shadowsocks-2022 relay (no TLS, no SNI) | 60 | 62 |
| Hysteria2 + salamander obfs + port hopping | 114 | 103 |
| VLESS + Reality + TCP (the dominant exit) | 129 | 130 |
| Trojan + gRPC + Reality | 136 | 135 |
| VLESS + WS + TLS behind a CDN with a Host override | 142 | 111 |
| **mean** | **116** | **108** |

> **These are fixture measurements, not protocol constants, and neither column may be cited as one.** The two columns differ because the two sets of fixtures put different strings in `pn`, `h`, `sni` and `pt`, which are the only variable-length fields in the entry. The 129-byte figure for VLESS plus Reality is exact for the worked entry above and can be checked against it byte for byte; every other cell is a sample. The corpus records the divergence as `cor-5`. What is normative is the field table above it and the caps in it; what follows from the measurement is only the shape of the projection below, which is illustrative.

**Correction to `01-DECISION.md` BC1.** BC1 states "the honest entry is 220 to 280 bytes". Measured against this encoding it is 60 to 142 bytes, mean 116. The 220 to 280 figure is what the entry costs if protocol, network, security, fingerprint and flow are carried as text strings and `pbk` as base64url; enum-coding those five fields and carrying `pbk` raw saves roughly 90 bytes per entry. BC1's operational conclusion is unchanged and in fact strengthened: chunking is required in v1, not reserved for later. It is required at a far smaller catalog than BC1 assumed, because the binding constraint is the 4 KB response cap and not the 12288-byte chunk threshold. See Correction 2 in section 16 and the derivation in section 11.

**Catalog size, projected from the 116-byte mean. Illustrative, not normative:**

| Exits | `ex` array | payload with a typical tail | frame, `nsigs=1` | chunks at 2816 |
|---|---|---|---|---|
| 20 | 2324 | ~2504 | ~2588 | 1 |
| 40 | 4648 | ~4828 | ~4912 | 2 |
| 80 | 9296 | ~9476 | ~9560 | 4 |
| 120 | 13944 | ~14124 | ~14208 | 6 |

"typical tail" is 180 bytes: the 27-byte envelope, `tier`, `cap`, `ttl`, `jit`, `thr`, `pb`, `lad`, and a three-mirror pool with pins. A catalog with a large mirror pool, several DoH entries and full rule-set hashes adds roughly 500 bytes more. The corpus's own typical catalog, with 40 exits, 3 relays, 4 mirrors, DoH, resources and pins, measures 5589 bytes, which is what a real tail costs; the table above isolates the exit array and nothing else.

**Which inbounds become catalog entries.** The membership rule is `02-SPEC.md` 4.4 and it is not discretionary: a catalog and a legacy Clash body built from the same fleet must describe the same proxy set, or the byte-diff gate of `06-MIGRATION.md` 3.5 and the P9 identical-proxy-name fixture have no defined input.

**Worked encoding, minimal catalog.** One exit, no relays, no mirrors. Online-signed.

```
payload_len = 188     nsigs = 1     total = 272

0000  43 53 4d 31 02 00 bc ac 01 01 02 48 22 6e 8a 20
0010  f6 99 b9 64 03 07 04 1a 6a 97 67 00 05 1a 6a be
0020  f4 00 0a 01 0b 81 ae 01 65 6e 31 37 69 33 02 70
0030  f0 9f 87 a9 f0 9f 87 aa 20 53 74 65 61 6c 74 68
0040  03 62 44 45 04 71 64 65 31 2e 65 78 61 2d 6e 6f
0050  64 65 73 2e 6e 65 74 05 19 01 bb 06 01 07 01 08
0060  02 09 71 77 77 77 2e 6d 69 63 72 6f 73 6f 66 74
0070  2e 63 6f 6d 0a 58 20 00 01 02 03 04 05 06 07 08
0080  09 0a 0b 0c 0d 0e 0f 10 11 12 13 14 15 16 17 18
0090  19 1a 1b 1c 1d 1e 1f 0b 68 36 62 61 38 35 31 37
00a0  39 0c 01 0d 01 15 f4 0e 44 00 00 00 03 13 19 1c
00b0  20 14 14 15 a3 01 19 20 00 02 16 03 19 10 00 16
00c0  82 00 03 01 21 e3 e2 cc 0a 3b a7 77 e6 9c e1 4c
00d0  2c dd 6f 5c 4a 71 1c 7c e6 d6 4e 46 ea 1f 4e 74
00e0  65 53 15 83 ca 33 7e af 2f 40 52 39 bf c0 83 47
00f0  bb 62 e6 3b 17 c6 42 84 9a fb b3 96 ab ca 6b ac
0100  ed 79 d7 96 63 df 45 29 4d f2 ea be af 32 5e 08

sha256(frame) = eb5c33321940d11813848b8b8b03417e75fb36a82c8aa9c9567e1686f9df535d
cat_id        = XDE36CGS838HG4W4
```

The tail decodes as: `0e 44 00000003` is `cap` = bits 0 and 1 set (per-node material, sealed directives); `13 19 1c20` is `ttl` = 7200 seconds; `14 14` is `jit` = 20 percent; `15 a3 01 19 2000 02 16 03 19 1000` is `thr` = `{conn_bytes: 8192, conn_packets: 22, resp_max: 4096}`; `16 82 00 03` is `pb` = `[0, 3]`.

The `ver` here is 7 (`03 07`), so the envelope is 27 bytes; it grows to 28, 29 and 31 at the thresholds in section 8.0. `05-TEST-VECTORS/` records the earlier "31 bytes" claim as correction `cor-1`.

### 8.3 Directive, `doc_type = 0x03`

Online-signed, nonce-bound, one hour expiry, per device. In the deployed configuration it is never transmitted bare: it is carried inside a sealed `0x06` envelope (section 9). It is defined bare because that is what the signature covers and what the verifier ends up holding.

| Key | Name | Type | Mandatory | Cap | Meaning |
|---|---|---|---|---|---|
| 10 | `nonce` | bstr | yes | exactly 16 | echo of the client-supplied nonce |
| 11 | `dtp` | bstr | yes | exactly 16 | device thumbprint |
| 12 | `st` | uint | yes | `st` enum | status |
| 13 | `rc` | uint | no | `rc` registry, default 0 | machine reason code |
| 14 | `cat` | bstr | yes | exactly 32 | `chash` of the catalog this directive is bound to |
| 15 | `cn` | uint | yes | 1..64 | number of chunks that catalog is served in |
| 16 | `tier` | uint | yes | 1..1023 | plan tier |
| 17 | `cap` | bstr | yes | exactly 4 | capability bitfield echo |
| 18 | `sel` | map | no | | authoritative selection |
| 19 | `pol` | map | no | | settings echo with provenance |
| 20 | `ann` | tstr | no | <= 80 | announce text, inert, render-only |
| 21 | `sup` | tstr | no | <= 80 | support contact, inert, render-only |
| 22 | `ui` | array of hint entries | no | 0..4 | UI hints |
| 23 | `ttl` | uint | yes | 300..86400 | refresh cadence for the directive |
| 24 | `exph` | uint | no | 0..2592000 | offline grace window seconds |
| 25 | `loc` | tstr | yes | exactly 24 | the current locator |
| 26 | `traf` | map | no | | signed traffic counters |

**`sel`** (`01-DECISION.md` B6, 5.4.2, P6):

| Key | Name | Type | Cap | Meaning |
|---|---|---|---|---|
| 1 | `exit` | tstr | <= 24 | node entry `id` |
| 2 | `relay` | tstr | <= 24 | relay entry `id` |
| 3 | `preset` | tstr | <= 32 | route entry `id` |
| 4 | `variant` | uint | < 2^8 | protocol variant |
| 5 | `proto` | uint | `pr` enum | forced protocol, 0 for auto |
| 6 | `rcc` | tstr | exactly 2: an uppercase ISO 3166-1 alpha-2 code, or the literal `--` for none | resolved relay country |
| 7 | `nid` | uint | < 2^63 | the numeric `node_id` the legacy config fetch takes |

> **`sel` is mandatory in the directive whenever the panel flag `csm_geo_pinned` is on, and `rcc` and `nid` are mandatory inside it.** The earlier "mandatory in practice" hedge is withdrawn: a panel that omits `sel` is otherwise conformant and silently reintroduces the GeoIP dependence that `01-DECISION.md` P6 exists to remove, which is exactly the condition the byte-diff gate of `06-MIGRATION.md` 3.5 is built around. The field stays optional in the table so that a shadow-phase panel with the flag off can sign a directive at all.

> **Where `sel.rcc` is resolved from.** The panel MUST resolve it from the subscription's stored relay preference first, and from a geo lookup only at enrollment or at an explicit user write. It MUST NOT resolve it from the apparent source IP of a manifest fetch. The directive is fetched over the ladder by construction, so on rung R4 the apparent source is the exit node and on rung R2 it is whatever `X-Forwarded-For` the mirror sent, and `caramba-sub` forwards that header verbatim (`apps/caramba-sub/src/panel_client.rs:251-258`). Resolving from it would pin a wrong country into signed data on every ladder rung but the first. The same rule governs `sel.nid`.

Verified as the current behavior this replaces: `client_cc` comes from `x-country-code`, `cf-ipcountry` or a GeoIP lookup on the request IP (`apps/caramba-panel/src/subscription.rs:147-150`) and is in the cache key by design (`:692`). The ladder changes the apparent source IP by construction, so without P6 a config-hash mismatch is the normal case rather than the alarm case.

**Correction to an earlier form of this section: the no-relay sentinel is `--`, not `"NO"`.** `"NO"` was chosen on the stated ground that it "is not a valid ISO country in this position". `NO` is Norway. An operator with a Norwegian relay could not express it, and a user selecting one would silently get no relay at all, which is precisely the case the sentinel exists for. `--` is two characters, satisfies the exactly-2 cap, and is not an alpha-2 code under any registry. `02-SPEC.md` 7.3 and its Correction 4 bind for this value, `05-TEST-VECTORS/` `pos-m1-norelay` carries `--`, and the renderer maps `--` to the URL literal `none`, which `apps/caramba-panel/src/subscription.rs:754` accepts as `Some("none") | Some("NONE")`.

**Three states, not two.** `sel.rcc` is always a concrete resolution: an ISO code, or `--` meaning the operator resolved "no relay". The **request** side, `pol` key 3, carries the user's choice and has three states: a 2-character country code, the literal `--` meaning the user chose no relay, and the empty string meaning the user has expressed no choice and the operator resolves. When `pol[3]` is the empty string the panel MUST still emit a concrete `sel.rcc` and MUST mark `pol[3]` with `src = 3` (default). Collapsing the empty string onto `--` would remove relay chaining from every subscriber who has never touched the picker, which today is nearly all of them, because the live default falls back to `client_cc` rather than to no relay (`subscription.rs:753-758`).

**`pol`** (`01-DECISION.md` B2, 5.4.1). A map from setting key to a 2-element array `[value, src]`, where `src` is the `src` enum.

| Key | Setting | Value type | `CorePolicy` field |
|---|---|---|---|
| 1 | protocol | tstr | `protocol` |
| 2 | preset | tstr | `preset` |
| 3 | relay | tstr | `relay`; three states: a 2-letter code, `--` for an explicit no relay, or `""` for unset (see above) |
| 4 | stack | tstr | `stack` |
| 5 | mtu | uint | `mtu` |
| 6 | ipv6 | bool | `ipv6` |
| 7 | fakeIp | bool | `fakeIp` |
| 8 | killSwitch | bool | `killSwitch` |
| 9 | dns.nameservers | array of tstr | `dns.nameservers` |
| 10 | dns.fallback | array of tstr | `dns.fallback` |
| 11 | split.mode | tstr | `split.mode` |

The vocabulary is the `CorePolicy` string set (`apps/caramba-client/packages/caramba_vpn/lib/src/core_policy.dart:41-176`), never `CoreConfig` indices.

> `split.apps` MUST NOT appear in `pol`, MUST NOT be assigned a key in this table, and MUST NOT be transmitted in either direction. Invariant 15.

Because CBOR null is forbidden by rule C7, the "reset to operator default" state is carried as the sentinel text string `"default"` in a `want` request, per `01-DECISION.md` B6. This preserves the two-way `CorePolicy.toJson` contract, which omits nulls, and the Go `policyPatch` pointer contract, which cannot represent an explicit null today.

**Hint entry** (`ui`; `01-DECISION.md` B3):

| Key | Name | Type | Cap |
|---|---|---|---|
| 1 | `k` | uint | `ui.k` enum |
| 2 | `t` | tstr | <= 80 |

> Operator-supplied text MUST NOT carry a URL the app will open, in any storefront. URL-shaped substrings MUST be stripped at render. Operator text MUST NOT be rendered on the same surface as the verification chrome, MUST NOT be persisted, and MUST NOT be echoed. Invariants 10 and 11.

**`traf`:**

| Key | Name | Type | Meaning |
|---|---|---|---|
| 1 | `up` | uint | uploaded bytes |
| 2 | `dn` | uint | downloaded bytes |
| 3 | `tot` | uint | limit in bytes, 0 means unlimited |
| 4 | `exp` | uint | subscription expiry, Unix seconds |

This is the signed replacement for the `Subscription-Userinfo` header (`subscription.rs:832-835`), which keeps being emitted unchanged on `/sub/{uuid}` for Hiddify, v2rayNG and sing-box. Signed fields survive caching and mirrors in a way headers do not.

**Worked encoding, minimal directive.** Status active, no selection, no policy echo, no announce.

```
payload_len = 144     nsigs = 1     total = 228

0000  43 53 4d 31 03 00 90 ae 01 01 02 48 22 6e 8a 20
0010  f6 99 b9 64 03 19 01 9c 04 1a 6a 97 67 00 05 1a
0020  6a 97 75 10 0a 50 a3 f1 0c 94 b2 7e 5d 61 88 ff
0030  20 41 9c 73 ae 05 0b 50 4f 0f 22 56 95 64 aa b0
0040  9a 2d 1a 75 c1 32 d9 55 0c 03 0e 58 20 eb 5c 33
0050  32 19 40 d1 18 13 84 8b 8b 8b 03 41 7e 75 fb 36
0060  a8 2c 8a a9 c9 56 7e 16 86 f9 df 53 5d 0f 01 10
0070  01 11 44 00 00 00 03 17 19 1c 20 18 19 78 18 45
0080  41 33 42 38 53 4b 43 59 36 56 42 57 41 53 45 37
0090  41 4d 31 58 34 38 59 01 21 e3 e2 cc 0a 3b a7 77
00a0  e6 9c e1 4c 21 32 83 c0 5b b4 c5 dd 18 9a e9 0d
00b0  f9 36 c9 4c db dd 00 01 c5 64 2c 9a 0f ab ab c0
00c0  29 f9 30 13 12 4b cf 61 65 64 ae 68 ab 8b 7e 1f
00d0  4d 04 10 32 dd 00 c9 55 bf 96 07 39 af cb dc c7
00e0  17 21 fc 02

sha256(frame) = b1956c4ed3877c424c1f11b903ae75be4f9a24a1537f760bb43a618da74be600
```

Field walk of the doc-specific tail:

| Offset | Bytes | Meaning |
|---|---|---|
| 20 | `03 19 01 9c` | `ver` = 412 |
| 24 | `04 1a 6a 97 67 00` | `iat` = 1788307200 |
| 30 | `05 1a 6a 97 75 10` | `exp` = 1788310800, exactly `iat + 3600` |
| 36 | `0a 50 a3f1...ae05` | `nonce`, 16 bytes, the value this device sent as `?n=` |
| 54 | `0b 50 4f0f...d955` | `dtp`, 16 bytes |
| 72 | `0c 03` | `st` = 3, active |
| 74 | `0e 58 20 eb5c...535d` | `cat` = sha256 of the catalog frame in section 8.2 |
| 109 | `0f 01` | `cn` = 1 |
| 111 | `10 01` | `tier` = 1 |
| 113 | `11 44 00000003` | `cap` echo |
| 119 | `17 19 1c 20` | `ttl` = 7200 |
| 123 | `18 19 78 18 <24 bytes>` | key 25 (`loc`, note the 2-byte key head `18 19` and the 2-byte tstr head `78 18`), tstr of 24 characters, `EA3B8SKCY6VBWASE7AM1X48Y` |
| 151 | `01` | `nsigs` |

The `loc` field pays 2 bytes for its key because 25 is above 23. It is at 25 rather than in the sub-24 block deliberately: it is written once per rotation and read once per request, so it is a cold field, and the sub-24 slots are spent on `nonce`, `dtp`, `cat` and `cap`, which every directive carries.

### 8.4 Catalog chunk, `doc_type = 0x04`

A chunk carries a slice of a complete catalog **frame**, not of a catalog payload. Each chunk is independently signed, so a tampered chunk is caught before reassembly, and the reassembled bytes are a complete `0x02` frame that is then verified again in full. Two independent verifications, one code path.

| Key | Name | Type | Mandatory | Cap | Meaning |
|---|---|---|---|---|---|
| 10 | `cid` | bstr | yes | exactly 10 | `chash[0..10]` of the catalog being carried; the same bytes as `cat_id` before base32 |
| 11 | `i` | uint | yes | 0..63 | chunk index, zero-based |
| 12 | `n` | uint | yes | 1..64 | total chunk count; MUST equal the directive's `cn` |
| 13 | `tl` | uint | yes | <= 49152 | total length of the reassembled catalog frame |
| 14 | `d` | bstr | yes | 1..2816 | the slice |

Reassembly rules:

- Chunk `i` carries bytes `[i * 2816, min((i+1) * 2816, tl))` of the catalog frame. Every chunk except the last MUST have `len(d) == 2816`. The last MUST have `len(d) == tl - i * 2816`, which is in `1..2816`.
- All `n` chunks MUST carry identical `cid`, `n` and `tl`. A mismatch is `E_PARSE_FIELD`.
- After reassembly the client MUST check `sha256(reassembled)[0..10] == cid` and `len(reassembled) == tl`, then verify the reassembled bytes as a `0x02` frame from step P1.
- **Every catalog is chunked, including a one-chunk catalog.** There is no unchunked delivery path. `cn = 1` is the normal case for a small tenant and it goes through exactly the same code. This removes a size branch from three implementations, which is worth the chunk envelope it costs.

**Worked encoding, chunk 0 of 1** carrying the 272-byte catalog of section 8.2.

```
payload_len = 323     nsigs = 1     total = 407

0000  43 53 4d 31 04 01 43 aa 01 01 02 48 22 6e 8a 20
0010  f6 99 b9 64 03 07 04 1a 6a 97 67 00 05 1a 6a be
0020  f4 00 0a 4a eb 5c 33 32 19 40 d1 18 13 84 0b 00
0030  0c 01 0d 19 01 10 0e 59 01 10 43 53 4d 31 02 00
0040  bc ac 01 01 02 48 22 6e 8a 20 f6 99 b9 64 03 07
      ... the catalog frame of section 8.2, verbatim ...
0140  45 29 4d f2 ea be af 32 5e 08 01 21 e3 e2 cc 0a
0150  3b a7 77 e6 9c e1 4c 86 d4 32 40 c5 2c 38 d7 ed
0160  d8 d0 3f 8b 8d 97 4b 10 02 12 21 e8 ac 67 43 be
0170  1b fd 3e 1a 69 c9 01 78 19 32 eb fd 9a 24 b9 37
0180  6b 52 37 13 16 15 1e 8e 4f 79 20 5d 80 ab d0 6a
0190  b2 8b 37 03 2a af 08

sha256(frame) = 68d613af7e4f616464ad281a92739822361fa66600948cda6ede452b46237168
```

Reading the header: `0a 4a eb5c33321940d1181384` is `cid`, 10 bytes; `0b 00` is `i` = 0; `0c 01` is `n` = 1; `0d 19 0110` is `tl` = 272; `0e 59 0110` opens `d` as a 272-byte byte string, and the bytes that follow begin `43 53 4d 31 02`, the catalog frame's own magic and doc_type.

**Chunk envelope overhead, counted rather than estimated.** For this fixture it is **51 bytes**: `payload_len` 323 minus `d` 272. Field by field, 27 for the envelope at `ver < 24` (section 8.0) plus 12 for `cid` plus 2 for `i` plus 2 for `n` plus 4 for `tl` plus 4 for the `d` key and head. Across the legal range of `i`, `n`, `tl` and `ver` it reaches **59 bytes**: 31 for the envelope above `ver` 65535, 12 for `cid`, 3 each for `i` and `n` at their caps, 6 for `tl` above 65535, and 4 for the `d` head. An implementation MUST size its buffers at `d + 59`; the earlier figures of 40 and 43 in this section under-allocated by 12 to 19 bytes on every chunk and section 11.3 is corrected to match. `05-TEST-VECTORS/` `pos-c1c-min-0` is these exact 407 bytes and is the cheapest way to check an implementation against the number.

### 8.5 Bootstrap blob, `doc_type = 0x05`

Root-signed, out-of-band. The escape kit, and it MUST NOT live behind the host being escaped (`01-DECISION.md` C2). It carries the enrollment code, the mirror set, the DoH list and the pinned root key, and it makes the enrollment path itself ladder-aware.

| Key | Name | Type | Mandatory | Cap | Meaning |
|---|---|---|---|---|---|
| 10 | `org` | tstr | yes | <= 96 | enrollment origin, `https://host[:port]`, origin only, no path |
| 11 | `code` | tstr | yes | <= 32 | enrollment code with the pin folded in; hyphens are cosmetic |
| 12 | `rk` | bstr | yes | exactly 32 | root public key, so the blob is self-contained |
| 13 | `mir` | array of mirror entries | yes | 1..32 | |
| 14 | `doh` | array of DoH entries | yes | 1..8 | |
| 15 | `nm` | tstr | no | <= 40 | operator display name, inert |

`sha256(rk)[0..12]` MUST equal the `link_pin` the user was given out of band, and MUST equal `sha256` of the key the first key document presents under role `root`. A blob whose `rk` does not match the dictated pin MUST be rejected with a hard error and no "continue anyway" path.

> The blob MUST NOT carry prices, a bot handle, a purchase link or any "buy" call to action. `01-DECISION.md` 5.6.1.

**Worked encoding, minimal bootstrap blob.** One mirror, one DoH endpoint. Root-signed.

```
payload_len = 290     nsigs = 1     total = 374

0000  43 53 4d 31 05 01 22 ab 01 01 02 48 22 6e 8a 20
0010  f6 99 b9 64 03 01 04 1a 6a 97 67 00 05 1a 6a be
0020  f4 00 0a 78 19 68 74 74 70 73 3a 2f 2f 70 61 6e
0030  65 6c 2e 65 78 61 6d 70 6c 65 2e 6e 65 74 0b 6e
0040  4b 37 51 57 2d 33 4d 32 50 2d 39 58 52 54 0c 58
0050  20 8b 16 0c 71 c6 10 08 32 1c ae 0d 0d c9 a9 80
0060  b6 e5 9c b2 6d 0f 4d 1f a8 df c0 30 c3 67 5e 2b
0070  7c 0d 81 a5 01 72 6d 31 2e 65 78 61 6d 70 6c 65
0080  2d 63 64 6e 2e 6e 65 74 02 72 6d 31 2e 65 78 61
0090  6d 70 6c 65 2d 63 64 6e 2e 6e 65 74 03 81 58 20
00a0  20 21 22 23 24 25 26 27 28 29 2a 2b 2c 2d 2e 2f
00b0  30 31 32 33 34 35 36 37 38 39 3a 3b 3c 3d 3e 3f
00c0  04 19 61 6c 05 62 44 45 0e 81 a4 01 6f 64 6f 68
00d0  2e 65 78 61 6d 70 6c 65 2e 6e 65 74 02 6a 2f 64
00e0  6e 73 2d 71 75 65 72 79 03 81 6c 31 39 38 2e 35
00f0  31 2e 31 30 30 2e 37 04 81 58 20 40 41 42 43 44
0100  45 46 47 48 49 4a 4b 4c 4d 4e 4f 50 51 52 53 54
0110  55 56 57 58 59 5a 5b 5c 5d 5e 5f 0f 6c 45 78 61
0120  20 4e 65 74 77 6f 72 6b 73 01 22 6e 8a 20 f6 99
0130  b9 64 df b0 1e 86 fa 6d a0 9a ef 64 7c ce f4 27
0140  fc 7d 70 d1 0f 85 67 d2 20 22 d2 51 d8 a6 76 79
0150  57 b0 e6 e3 d1 63 e3 ae 7b 9b 9f 3e bb 61 66 86
0160  92 b8 6f 19 20 26 ec ca 33 51 bc d0 f9 e8 48 b7
0170  98 d8 09 7a 1c 0f

sha256(frame) = c78332c555152fd2572e7d5ec0f8bc2c1d48e2aedeccc99e3d4516eb05fc5247
```

Reading: `0a 78 19 ...` is key 10 with a 25-byte tstr, `https://panel.example.net`; `0b 6e 4b3751572d334d32502d39585254` is `code` = `K7QW-3M2P-9XRT`; `0c 58 20 8b16...2b7c` is `rk`, matching the root key of the fixture set, whose `link_pin` is `49Q8M87PK6WP9QXG3T30`; `04 19 616c` inside the mirror entry is `asn` = 24940.

> **The `code` in this dump is a retracted example and MUST NOT be copied.** `K7QW3M2P` is not `link_pin[0..8]`, so the blob does not satisfy the pin-folding rule of `01-DECISION.md` 5.1.6 and section 7.2. The hex is kept unchanged because five other documents cite its digest, and `05-TEST-VECTORS/` keeps it as the reference entry `pos-b1-wire85` for the same reason. The conforming form is `02-SPEC.md` 9.2, and the normative blob fixture is `pos-b1-min`, whose `code` is `49Q8-M87P-KQZ3-WFDG-ZTJX`. The 32-byte `code` cap accommodates the hyphenated 24-character form.

At 374 bytes the blob is one QR code (section 10) and it is dictatable in the degenerate case where the user has only the code and the pin.

### 8.6 Reserve pool, `doc_type = 0x08`

Root-signed, locator-scoped. Carries the mirror pool that is deliberately held out of the shared catalog, per `01-DECISION.md` A6 and A7.

| Key | Name | Type | Mandatory | Cap |
|---|---|---|---|---|
| 10 | `mir` | array of mirror entries | yes | 1..32 |
| 11 | `doh` | array of DoH entries | no | 0..8 |
| 12 | `coh` | uint | no | < 2^16 | cohort identifier this pool was drawn for (D7) |

It is a separate document type at a separate URL, root-signed, so that the pool survives a compromise of the online signing key and so that pulling it costs an adversary an enrolled subscription and a live locator rather than one anonymous request. See Correction 6.

---

## 9. Sealing: `doc_type = 0x06`

`01-DECISION.md` BC2 and 5.7.2. The per-device directive is HPKE-sealed so that a mirror, a CDN or an onion front holds only a ciphertext and a thumbprint.

### 9.1 Suite

| Component | RFC 9180 id | Value |
|---|---|---|
| Mode | `mode_base` | `0x00` |
| KEM | `DHKEM(P-256, HKDF-SHA256)` | `0x0010` |
| KDF | `HKDF-SHA256` | `0x0001` |
| AEAD | `ChaCha20Poly1305` | `0x0003` |

**Correction to `00-DESIGN-BRIEF.md` 4.7.** The brief proposes `DHKEM(X25519, HKDF-SHA256)`. That suite cannot be used, because the device key it seals to is P-256: `01-DECISION.md` 5.5.1 requires the device keypair to be generated non-exportable in Secure Enclave or StrongBox, and neither hardware store holds an X25519 key. The same constraint is visible in B4's own correction, which pins the hardware tier boundary at Android 12 because `PURPOSE_AGREE_KEY` is API 31, and `PURPOSE_AGREE_KEY` is precisely the P-256 ECDH capability this KEM needs. Using X25519 would move the sealing key out of hardware, which is the property the device binding exists to provide. The KEM is therefore `0x0010`.

`enc` is a 65-byte uncompressed P-256 point (`0x04 || X(32) || Y(32)`). A sender MUST emit the uncompressed form; a recipient MUST reject a compressed or hybrid point encoding and MUST reject a point not on the curve (`E_SEAL_SUITE`).

### 9.2 The `info` and `aad` strings, byte for byte

```
info = "CSM1-seal-v1"                                   12 ASCII bytes, no NUL
aad  = "CSM1" || 0x06 || pid(8) || dtp(16) || u32be(ver)  33 bytes
```

`ver` in the `aad` is the outer frame's `ver`, which equals the inner directive's `ver`. Both sides can compute `aad` before any encryption, from values that are also in the clear on the wire, so it binds the ciphertext to the tenant, the recipient device and the version without adding a secret dependency.

A recipient MUST recompute `aad` from the outer payload's own `pid`, `dtp` and `ver` fields and MUST NOT accept an `aad` supplied on the wire.

### 9.3 Outer payload fields

| Key | Name | Type | Mandatory | Cap | Meaning |
|---|---|---|---|---|---|
| 10 | `dtp` | bstr | yes | exactly 16 | recipient device thumbprint |
| 11 | `kem` | uint | yes | must equal 16 | RFC 9180 KEM id |
| 12 | `kdf` | uint | yes | must equal 1 | RFC 9180 KDF id |
| 13 | `aead` | uint | yes | must equal 3 | RFC 9180 AEAD id |
| 14 | `enc` | bstr | yes | exactly 65 | encapsulated key |
| 15 | `ct` | bstr | yes | 242..3072 | ciphertext with the 16-byte Poly1305 tag appended |
| 16 | `rkv` | uint | yes | < 2^16 | recipient key generation |

The plaintext of `ct` is a **complete `0x03` frame**, magic and signature slots included, not a bare payload.

The `ct` range is derived, not chosen. The smallest legal `0x03` frame carries only its mandatory fields with `ver < 24`: a 14-pair map of 142 payload bytes, so 226 frame bytes, plus the 16-byte tag gives 242. The upper end is `MAX_BSTR_BYTES` (section 3.2), which is what makes section 12.2's clamp on the inner directive necessary: an inner frame above 3056 bytes has no legal `ct` encoding and therefore no delivery path at all.

### 9.4 Order of operations on receipt

1. Parse the outer frame from step P1 (section 6.1).
2. Verify the outer frame's signature under role `online` (section 7.1). A mirror serving garbage is caught here, before any asymmetric work on the seal.
3. Check `dtp` equals this device's thumbprint. On mismatch, `E_SEAL_RECIPIENT`. This is not a security failure by itself, since the seal would fail anyway; it is a correctness failure and it must not be reported as tampering.
4. Check `kem`, `kdf` and `aead` equal the required suite. On mismatch, `E_SEAL_SUITE`.
5. Look up the private key for generation `rkv`. If the device does not hold it, `E_SEAL_RECIPIENT`.
6. `HPKE.Open` in base mode with `info` and `aad`. On failure, `E_SEAL_OPEN`. This is a security event.
7. Parse and verify the recovered plaintext as a `0x03` frame, in full, from step P1 through V14b. The outer verification does not shortcut any inner check. In particular the nonce check at V13 and the version rule at V9 run against the inner frame.

### 9.5 Recipient key rotation

**There are two HPKE keys in CSM/1 and they are not interchangeable.** The one this section rotates is the **device agreement key**: it is registered at enrollment, never published, held by the device, and its generation is `rkv`. It is the only recipient a `0x06` is ever sealed to.

> Catalog keys 25 and 26, `hpk` and `hpkv`, are the **panel's** own HPKE recipient key and its generation. A catalog is byte-identical for every subscriber on a tier, so a key carried there is a key shared by every subscriber on that tier; using it as the sealing recipient would let any subscriber on a tier open any other subscriber's directive, which destroys the property `01-DECISION.md` BC2 exists to provide. `hpk` MUST NOT be used as the recipient of a `0x06`, by any implementation, ever. Its single v1 use is the opposite direction, a client sealing a request body to the panel, and `02-SPEC.md` 10.2 owns that use, its gating, its storage and its rotation cadence.

Rotation of the panel key on a schedule is what stops a compelled or seized panel retroactively decrypting a long window of recorded client-to-panel traffic; rotation of the device key is what this section specifies.

A device MUST be able to replace its own recipient key without operator action. The rekey message is authenticated by the device **signing** key, which is a separate key from the agreement key per `01-DECISION.md` 5.5.1, and it exists from v1. There is no state in which a device is stuck with a key it cannot replace.

### 9.6 Sealed directive size

| Component | Bytes |
|---|---|
| outer envelope, `ver` in 256..65535 | 29 |
| `dtp` | 18 |
| `kem`, `kdf`, `aead` | 6 |
| `enc` | 68 |
| `ct` = 228-byte inner frame + 16-byte tag | 247 |
| `rkv` | 2 |
| **outer payload** | **370** |
| frame overhead `7 + 1 + 76` | 84 |
| **sealed frame, unpadded** | **454** |
| padded to the 256-byte grid, `r = 0` | **512** |

Each of `kem`, `kdf` and `aead` is a 2-byte pair: key `0b` with value `10`, key `0c` with value `01`, key `0d` with value `03`. They cost 6 bytes, not 7; the outer payload is 370 and the unpadded sealed frame is 454. Every sealed fixture in `05-TEST-VECTORS/` measures 454 and the corpus records the earlier figures as correction `cor-2`. The padded 512 is unaffected.

The general relation, for sizing a buffer: `sealed_frame = inner_frame + 226` while `inner_frame + 16` is at most 255, and `inner_frame + 227` above that, where the extra byte is the `ct` length head. It follows that an inner frame above **3056** bytes cannot be sealed at all, because `ct` would exceed `MAX_BSTR_BYTES`. Section 12.2 carries the clamp that keeps the signer inside that bound.

The 512-byte figure is the same one `01-DECISION.md` D3 objects to as a cross-tenant constant. It is a constant only when `pb` is `[0, 0]`. With the default `pb` of `[0, 3]` the directive response is drawn from `{512, 768, 1024, 1280}` per request, and the range itself is per tenant.

---

## 10. Armored text form and QR chunking

`01-DECISION.md` C1: the same bytes are a QR, a paste, a file and an HTTP body. This section defines the text form.

### 10.1 The byte stream

The armored form encodes a **frame stream**: the concatenation of one or more complete frames, in order. A reader parses frames sequentially, using each frame's own `payload_len` and `nsigs` to compute its exact length (section 1.1) and starting the next frame at the byte immediately after. Bytes remaining after the last complete frame, and a truncated final frame, are both `E_PARSE_FRAMING`.

The frame stream is the only place in CSM/1 where more than one frame appears in one artifact. It exists for two cases: a root rotation chain (section 7.3), and an offline snapshot carrying a key document plus a catalog plus a bootstrap blob.

Cap: a frame stream MUST contain at most 16 frames and MUST NOT exceed 65536 bytes.

The cap is sized against the largest snapshot the offline rung has to carry, and the arithmetic holds with room: the largest catalog this format admits measures 48589 bytes (`05-TEST-VECTORS/` `document_sizes.catalog.maximum`, 448 exits), a typical key document is 456 and a maximum bootstrap blob is 3836, so the full offline snapshot of `02-SPEC.md` 9.7, one blob plus one key document plus one catalog, is at most 52881 bytes and 3 frames. A root rotation chain of 8 key documents at the 4096-byte emission bound is 32768 bytes and 8 frames. Neither approaches either half of the cap.

### 10.2 Line format

```
CARCAP1.<bid>.<i>/<n>.<data>
```

| Part | Content |
|---|---|
| `CARCAP1` | literal, 7 characters |
| `.` | literal separator |
| `<bid>` | `base32_crockford(sha256(frame stream)[0..5])`, exactly 8 characters |
| `.` | literal separator |
| `<i>` | chunk ordinal, **1-based**, decimal, no leading zeros, 1..106 |
| `/` | literal separator |
| `<n>` | total chunk count, decimal, no leading zeros, 1..106 |
| `.` | literal separator |
| `<data>` | `base32_crockford` of this chunk's byte slice |

Chunk `i` (1-based) carries bytes `[(i-1) * 620, min(i * 620, len))` of the frame stream. 620 bytes is 4960 bits, which is exactly 992 base32 characters with no pad bits, so every chunk except the last encodes to exactly 992 characters and no chunk except the last has ambiguous trailing bits.

Maximum line length is `7 + 1 + 8 + 1 + 3 + 1 + 3 + 1 + 992 = 1017` characters. The 106-chunk cap follows from the 65536-byte stream cap: `ceil(65536 / 620) = 106`.

**Correction and extension to `01-DECISION.md` C1.** C1 writes the format as `CARCAP1.<i>/<n>.<chunk>`, with no bundle identifier. The `<bid>` field is added here. Without it a scanner cannot detect that the user has mixed chunks from two different bundles, which is the realistic failure when an operator prints a new sheet and the old one is still on the table, and a reader has no way to show scan progress against a fixed target. The cost is 9 characters per chunk. On reassembly a reader MUST check `base32_crockford(sha256(joined)[0..5]) == bid` before parsing, and MUST reject the set on mismatch.

### 10.3 Reader rules

- A reader MUST accept the whole set in any order and MUST detect a missing ordinal rather than concatenating what it has.
- A reader MUST reject a set in which any two lines disagree on `bid` or `n`.
- A reader MUST reject a set in which any ordinal appears twice with different `data`. It MAY silently accept a duplicate scan of an identical line.
- Every rejection in this section, and every rejection of section 4.1's alphabet and pad-bit rules when they are reached through the armored reader, returns `E_PARSE_FRAMING` (section 6.6). There is no separate armor error family.
- Every chunk except the one with `i == n` MUST decode to exactly 620 bytes.
- Whitespace, including line breaks, MUST be stripped from `<data>` before decoding. Hyphens MUST be ignored per section 4.1.
- The prefix `CARCAP1` is compared case-insensitively.

### 10.4 QR encoding

- The full line MUST be encoded in QR **alphanumeric mode**. Every character used by the format is in the alphanumeric character set: `0`-`9`, `A`-`Z`, and `.` and `/`. Byte mode MUST NOT be used; it costs roughly 45 percent more modules for the same content.
- A generator MUST emit uppercase.
- At 1017 characters a chunk fits QR version 40 at error correction level H, which holds 1852 alphanumeric characters, with substantial margin. A generator SHOULD select the lowest version that fits at level M or better; the common case, a 374-byte bootstrap blob in a single 620-character chunk, is well below that.

### 10.5 Worked example

The 374-byte bootstrap blob of section 8.5 is one chunk of one:

```
CARCAP1.RY1K5HAN.1/1.8D9MTC8504HAP08109424VMA43V9KEB40C0G86KAJXKG018TDAZF800AF0CPGX3ME1SKMBSFE1GPWSBC5SJQGRBDE1P6ABKECNT0PVJB6X8NEB9K9MS50B9SB195832R425HC33HRR80GCGWNR6GVJD9G2VEB75JDM7MT7X8VZ031GV7BRNQR3C1MM0Q4V9H5SJQGRBDE1P6ABB3CHQ2WVK5EG174V9H5SJQGRBDE1P6ABB3CHQ2WVK5EG1R2P1040GJ48S44MK2EA1958NJRB9E5WR32CHK6GTKCDSR74X3PF1X7RZG86B1DG2P4H251T0T80BFCHQPGBK5F1GPTW3CCMQ6WSBM09N2YS3EECPQ2XB5E9WG70BC64WKGBHN64Q32C1G5RVG90AR41042GJ38H2MCHT89554PK2D9S7N0MAJADA5ANJQB1CNMPTWBNF5Y3VC8NW6282ECNT7EVVJDDSG28KEH8GFD6DSCKFV07M6Z9PT16QFCHYCXX17ZHYQ1M8FGNKX4812T98XH9KPF5BV1SQ3T5HY7BKVKEFKXEV1CT395E3F34G2DV6A6D8VSM7SX14BF66R15X1R3R
```

Line length 620 characters: 21 of header and 599 of data, because 374 bytes is `ceil(374 * 8 / 5) = 599` characters.

### 10.6 Content type

An armored artifact carried over HTTP or written to a file uses media type `text/vnd.caramba.csm1-armor`. Lines are separated by `\n`. A trailing newline is permitted and ignored. The file extension is `.carcap`.

---

## 11. Size budget

The design constraint is not an implementation detail. `00-DESIGN-BRIEF.md` section 2.2: since June 2025 TSPU silently freezes TCP connections when three conditions align, HTTPS/TLS, a foreign datacenter IP, and a data threshold inside one connection, at roughly 25 packets or 15 to 20 KB depending on provider. No RST is sent. Cloudflare independently confirmed a per-connection limit near 16 KB applied via packet injection, affecting HTTP/1.1, HTTP/2 and HTTP/3 alike.

CSM/1 budgets **bytes and packets, with the TLS handshake counted**, per `01-DECISION.md` D5 and invariant 5.

### 11.1 Handshake accounting

A client cannot portably observe the exact byte count of its own TLS handshake, so the protocol charges a fixed debit for every new TCP connection.

| Constant | Value | Provisional |
|---|---|---|
| `HANDSHAKE_DEBIT_BYTES` | 2816 | yes |
| `HANDSHAKE_DEBIT_PACKETS` | 8 | yes |
| `PACKET_MTU_ASSUMED` | 1400 | yes |

Derivation, for a TLS 1.3 handshake against a manifest host provisioned per D5 (ECDSA leaf, shortest chain, no stapled OCSP):

| Element | Bytes | Data packets |
|---|---|---|
| TCP SYN, SYN-ACK, ACK | ~180 | 3 |
| ClientHello, uTLS Chrome profile, padded to the Chrome constant | 517 | 1 |
| ServerHello, ChangeCipherSpec, and the encrypted EncryptedExtensions + Certificate (2-cert ECDSA chain) + CertificateVerify + Finished flight | ~1583 | 2 |
| client ChangeCipherSpec + Finished | ~64 | 1 |
| two NewSessionTicket records | ~400 | 1 |
| **total** | **~2744** | **8** |

Rounded up to 2816, which is 11 × 256 and therefore lands on the padding grid.

The same handshake with an RSA-2048 leaf, an RSA intermediate and a stapled OCSP response runs 5.5 to 6.5 KB, which is 3 to 4 KB of the freeze budget spent on certificate bytes. **The provisioning rule is therefore normative for manifest hosts:** ECDSA leaf, shortest chain that validates, no OCSP stapling on the CSM routes.

**These three constants are provisional.** The measurement that changes them: capture the actual handshake byte and data-packet counts on the tenant's real certificate chain, from a Russian vantage point, on both mobile and home-broadband paths, and compare against the observed freeze point on the same path. Until that measurement exists the constants are conservative estimates and `thr.conn_bytes` is set below the observed 15 to 20 KB range with a factor of roughly two of margin.

### 11.2 The connection hygiene rule

> A client maintains, per TCP connection, a running total of accounted bytes and accounted data packets. The connection is opened with `HANDSHAKE_DEBIT_BYTES` bytes and `HANDSHAKE_DEBIT_PACKETS` packets already charged. Each request charges its request-line and header bytes; each response charges its response-line, header and body bytes. Packets are charged as `ceil(bytes / PACKET_MTU_ASSUMED)`, minimum 1, per message.
>
> A client MUST NOT begin a request on a connection when the projected total after that request and its maximum possible response (`thr.resp_max`) would exceed `thr.conn_bytes` or `thr.conn_packets`. A client MUST close a connection once either total is reached, rather than waiting for the peer.
>
> `thr.conn_bytes` and `thr.conn_packets` are signed catalog fields (section 8.2), not compiled constants. Invariant 5.

`01-DECISION.md` 5.3.4 requires this as connection hygiene inside the `FetchProfile` ladder loop; `component/resource/vehicle.go:87-183` in vendored mihomo is the working reference for the caching and size-limiting parts of the same loop.

### 11.3 Response ceiling and chunk size

| Constant | Value | Source |
|---|---|---|
| `RESP_MAX` | 4096 | invariant 5, signed as `thr.resp_max` |
| `PAYLOAD_MAX` | 49152 | frame field cap, section 1 |
| `CHUNK_PAYLOAD_MAX` | 2816 | derived below |
| `CHUNK_RESP_MAX` | 3584 | derived below |
| `PANEL_WARN` | 12288 | `01-DECISION.md` 5.2.7 |
| `PANEL_REFUSE` | 49152 | `01-DECISION.md` 5.2.7, invariant 6 |
| `DOC_FRAME_MAX` | 4096 | section 8.0.1, for `0x01`, `0x05`, `0x08` |
| `INNER_DIRECTIVE_MAX` | 2816 | section 12.2, largest padded `0x03` that a `0x06` can carry |

Derivation of `CHUNK_PAYLOAD_MAX`. A chunk frame carrying `d` bytes of catalog costs at most `d + 59` bytes of payload (section 8.4, measured, worst case across `i`, `n`, `tl` and `ver`) plus 84 bytes of frame overhead, so the frame is at most `d + 143`. It must then be padded up to a multiple of 256 and still leave room under `RESP_MAX` for the HTTP response line and headers. Setting `d = 2816` gives a frame of at most 2959 bytes, padding to 3072, and with the padding bucket range clamped so the padded frame does not exceed `CHUNK_RESP_MAX = 3584` there is 512 bytes of headroom under the 4096 cap for headers. `2816 = 11 * 256`. The fixture in section 8.4 sits at the low end of that range, `d + 135`, because its `ver`, `i`, `n` and `tl` are all small; an implementation MUST allocate for `d + 143`, not for the fixture.

**`PANEL_REFUSE` coincides with the format limit, and that is not an oversight.** `payload_len` is capped at 49152 by the frame header itself (section 1), so a payload above `PANEL_REFUSE` is already unencodable and the threshold can never fire as a distinct runtime check on encoded bytes. Invariant 6 nevertheless requires the panel to refuse rather than emit, so the check MUST be performed **before encoding**, against the projected payload size of the tier's content model, and it MUST name the tenant and the node count when it fires. The thresholds that bind in practice are `PANEL_WARN` at 12288, `DOC_FRAME_MAX` for the three unchunked types, and `CHUNK_RESP_MAX` for chunks.

### 11.4 The chunking rule

> Every catalog MUST be served as `0x04` chunks. `cn = ceil(len(catalog frame) / 2816)`, and `cn` is carried in the directive so the client knows how many to fetch before it starts. A catalog frame of 2816 bytes or fewer is `cn = 1` and goes through the same path.
>
> The panel MUST refuse to sign a catalog payload above `PANEL_REFUSE = 49152` bytes rather than emitting one. Invariant 6. At 12288 bytes it MUST log a warning naming the tenant and the node count.

**Correction to `01-DECISION.md` 5.2.7.** 5.2.7 states "a catalog payload above 12288 bytes MUST be chunked". Taken alone that is inconsistent with the same paragraph's "no single response above 4 KB", because a 12 KB frame cannot be delivered inside a 4 KB response. The 4 KB cap binds first and it binds much earlier: chunking is mandatory from roughly 2.9 KB of catalog frame, which for the mean node entry of 116 bytes is about 24 exits. 12288 is retained here as the panel's warning threshold and 49152 as its refusal threshold; both remain useful, neither is the chunking trigger. Making every catalog chunked, including one-chunk catalogs, removes the branch entirely.

### 11.5 Worked budgets

Directive fetch on a cold connection, `pb = [0, 3]` drawing a 1024-byte response:

| Element | Bytes | Packets |
|---|---|---|
| handshake debit | 2816 | 8 |
| `GET /sub/m1/{loc}?n=...&v=...` with 6 headers | ~320 | 1 |
| response line, 5 headers, 1024-byte body | ~1184 | 1 |
| **total** | **~4320** | **10** |

Comfortably inside 8192 and 22.

A second directive fetch on the same connection would bring the **actual** total to 5824 bytes and 12 packets. That is not the test. The rule in section 11.2 projects against the **maximum possible** response, `thr.resp_max`, so the projection is `4320 + 320 + 4096 = 8736`, which exceeds `thr.conn_bytes` of 8192. **At the default thresholds a directive fetch therefore gets its own connection**, and the hygiene rule enforces it with no directive-specific special case. An operator who wants directive refreshes to share a connection must raise `thr.conn_bytes`, and `04-THREAT-MODEL.md` section 4 caps how far it may be raised.

Catalog chunk fetch on a cold connection, response padded to `CHUNK_RESP_MAX`:

| Element | Bytes | Packets |
|---|---|---|
| handshake debit | 2816 | 8 |
| request | ~320 | 1 |
| response headers plus 3584-byte body | ~3744 | 3 |
| **total** | **~6880** | **12** |

A second chunk on the same connection projects to `6880 + 320 + 4096 = 11296` bytes under the section 11.2 rule, and to 10944 in actual bytes; both are far above `thr.conn_bytes`. **One catalog chunk per TCP connection** is therefore the rule that falls out of the numbers, and the client's hygiene rule enforces it without a chunk-specific special case.

Full cold start for a 40-exit tenant:

| Fetch | Connections | Bytes |
|---|---|---|
| key document | 1 | ~3900 |
| sealed directive | 1 | ~4320 |
| catalog, 2 chunks | 2 | ~13760 |
| **total** | **4** | **~21980** |

No single connection carries more than 6880 bytes or 12 data packets. Compare the artifact this replaces: the current Clash YAML for a 40-node fleet is 18 to 25 KB delivered in **one** connection, squarely inside the freeze window. That is why `apps/caramba-panel/src/subscription.rs:617-634` auto-pins a single node on first fetch, with the comment "Prevents dumping 40+ outbounds to the client on first subscription fetch". CSM/1 does not need that mitigation because it never puts the fleet in one connection, which is also why `01-DECISION.md` 4.13 refuses to remove the auto-pin until Connect clients are the majority: legacy clients still do.

### 11.6 Steady state

`ttl` default 7200 seconds with `jit` 20 percent gives roughly 12 jittered fetches a day instead of the roughly 720 that a fixed two-minute period produces. Each is one directive: about 4.3 KB on one connection. The catalog is fetched only when the directive's `cat` changes, which for a stable fleet is rare, and its chunks are content-addressed and cacheable for a day (section 13.4).

The mirror set refreshes on its own faster cadence, independent of `ttl`, so the rescue channel is not slowed by the privacy win (`01-DECISION.md` 5.3.6).

---

## 12. Padding

`01-DECISION.md` D3 and invariant 7.

### 12.1 Padding is inside the payload

> Padding MUST be carried in the common envelope field `pd` (key 9), a byte string of `0x00` bytes inside the signed payload. Padding MUST NOT be appended after a frame.

This follows directly from the exact-length rule: a byte appended after the last signature slot makes `total_len != 7 + payload_len + 1 + 76 * nsigs` and is rejected at P7. There is no legal place for out-of-frame padding, so padding is signed data.

A decoder MUST accept `pd` and MUST ignore its contents. A decoder MUST reject a `pd` containing any non-zero byte (`E_PARSE_FIELD`), so the field cannot be used as a covert channel by a hostile signer.

### 12.2 The bucket rule

```
PAD_UNIT = 256
```

Let `L0` be the frame length without `pd`, and let `r` be drawn uniformly at random from `[pb[0], pb[1]]` inclusive, where `pb` is the per-tenant range in the signed catalog (section 8.2), default `[0, 3]`.

```
T = PAD_UNIT * (ceil(L0 / PAD_UNIT) + r)
D = T - L0
```

`pd` carries `N` zero bytes where `N` is chosen so the encoded field costs exactly `D` bytes. The field costs `2 + N` for `N` in 0..23, `3 + N` for 24..255, and `4 + N` for 256..3072. Therefore:

| `D` | `N` |
|---|---|
| 0 | omit `pd` entirely |
| 2..25 | `D - 2` |
| 27..258 | `D - 3` |
| 260..3076 | `D - 4` |
| 1, 26, 259 | unreachable: use `T + PAD_UNIT` and recompute |

The three unreachable values are a consequence of CBOR head sizes and are enumerated here so that three implementations do not each discover them separately at a different time.

**Clamp.** The signer MUST reduce `r` as needed so that a padded response never breaks the ceiling that will carry it:

| Document | `T` MUST NOT exceed |
|---|---|
| `0x01` key document, `0x05` bootstrap blob, `0x08` reserve pool | `min(thr.resp_max, DOC_FRAME_MAX)` = 4096 (section 8.0.1) |
| `0x04` catalog chunk | `CHUNK_RESP_MAX` = 3584 |
| `0x02` catalog | not padded per request; padded once at signing, and bounded by `PANEL_REFUSE` rather than by a response ceiling, because it is delivered only as chunks |
| `0x03` directive, **inner** | `INNER_DIRECTIVE_MAX` = 2816 |
| `0x06` sealed directive, **outer** | `thr.resp_max` = 4096 |

> The `0x03` row is the one an implementer gets wrong. A directive is never transmitted bare (section 8.3), so its own padded length is not what has to fit under `thr.resp_max`: what has to fit is the sealed frame that carries it, which is `inner + 227` before the outer padding (section 9.6). The binding constraint is tighter still and comes from the encoding: `ct` carries `inner + 16` and `MAX_BSTR_BYTES` is 3072, so `inner` cannot exceed 3056 and the largest value on the 256-byte grid below that is `INNER_DIRECTIVE_MAX = 2816`. A signer MUST clamp the inner `r` to that bound and MUST then reduce the outer `r` so the sealed frame stays under `thr.resp_max`.
>
> A padded `0x03` above 2816 bytes is a legal, verifiable frame with no delivery path in v1. `05-TEST-VECTORS/` ships one, `pos-m1-max` at 3840 bytes, and it is correct that a verifier MUST accept it: it exercises the parser and the `pd` field at their limits. It is not a delivery precedent and a signer MUST NOT emit one.

### 12.3 What is padded per request and what is not

| Document | Padding drawn | Why |
|---|---|---|
| `0x03` directive, `0x06` sealed directive | per request | signed per request anyway, since it carries the nonce |
| `0x01` key document | once at signing | one document serves every subscriber |
| `0x02` catalog, `0x04` chunk | once at signing | content-addressed and byte-identical per tier; per-request padding would change `chash` |
| `0x05` bootstrap blob, `0x08` reserve pool | once at signing | |

**Residual, stated rather than claimed away.** Catalog chunk responses are a per-tier constant size. An observer who can count bytes learns which tier a subscriber is on and, from `cn`, roughly how large the fleet is. This is not mitigated in v1. It is bounded by the fact that catalog chunks are cacheable and mirrorable, so the observation may be of a CDN rather than of the panel, and by `pb` being per tenant so the constant is not shared across operators. The directive, which is the dominant flow, is padded per request and does not have this property.

### 12.4 Transport hygiene that padding depends on

> A CSM response MUST NOT carry a `Content-Encoding`. Compression defeats the padding by making the on-wire size a function of the plaintext.

Verified as a live hazard: the panel's root router applies `tower_http::compression::CompressionLayer::new()` to every response at `apps/caramba-panel/src/main.rs:1626`, and five `SetResponseHeaderLayer::overriding` layers at `:1630-1651` stamp `X-Content-Type-Options`, `X-Frame-Options`, `X-XSS-Protection`, `Strict-Transport-Security` and `Referrer-Policy` on everything. The CSM routes MUST be layered separately, outside all six, and a test MUST assert `Content-Encoding` is absent and that the response header set is exactly the one in section 13.4. The constant five-header stamp is otherwise a cross-tenant fingerprint identifying any panel as a Caramba panel from one response.

---

## 13. Endpoints

### 13.1 Where they live

The CSM read routes live on the panel's **root** router, beside the existing `/sub/{uuid}` and `/rulesets/{name}` registrations at `apps/caramba-panel/src/main.rs:1584-1595`. They are not nested under `/api`, so they do not acquire the `/api` and `/caramba-api` double mount that `main.rs:1582-1583` gives every API route, and they do not pass through `require_app_jwt`.

**How, not only where.** Registering them "beside" the existing routes is not sufficient and would make section 13.4 unimplementable. `CompressionLayer` is applied at `main.rs:1626` and the five `SetResponseHeaderLayer::overriding` layers at `:1630`, `:1634`, `:1638`, `:1645` and `:1649`, all after `.with_state(state)` at `:1625`, so in axum they wrap **every** route on that router, including any newly registered one. Compression additionally defeats the padding-bucket invariant (section 12.4).

> The CSM routes MUST be built as their own `Router`, carrying no compression layer and none of the five constant security-header layers, and that router MUST be merged into the top-level service **after** those layers have been applied to the existing router, not before. Equivalently: layer the existing router first, then `Router::new().merge(layered_existing).merge(csm_router)`. A test MUST assert the exact response header set of section 13.4 on each CSM route, and MUST assert `Content-Encoding` is absent when the request carries `Accept-Encoding: gzip, br`.

The exceptions are the two authenticated account actions, the settings write and device enrollment, which belong under `/api/v2/app/` (section 13.2). They inherit the panel's normal API layering, including its headers and compression, because they carry a CBOR request body rather than a padded frame in the request direction; their **responses** are frames and MUST be produced by the same unlayered CSM handler stack, which in axum means the handler sets its own headers and the route is excluded from the compression layer by a per-route `Layer` opt-out or by living on the CSM router with an explicit `require_app_jwt` `route_layer`. The panel MUST pick one and assert the header set either way.

### 13.2 The table

| Method | Path | Auth | Response | Cacheable |
|---|---|---|---|---|
| GET | `/sub/k1` | none, rate-limited | one `0x01` frame | yes, 300 s |
| GET | `/sub/k1?since=N` | none, rate-limited | frame stream, `0x01` versions `N+1`..`N+8` | yes, 300 s |
| GET | `/sub/b1/{code}` | the enrollment code is the credential | one `0x05` frame | no |
| GET | `/sub/r1/{loc}` | locator | one `0x08` frame | no |
| GET | `/sub/c1/{cat_id}/{i}` | locator, via `X-CSM-Loc` | one `0x04` frame | yes, **private**, 86400 s, immutable, `Vary: X-CSM-Loc` |
| GET | `/sub/m1/{loc}?n=&v=&d=` | locator plus device thumbprint | one `0x06` frame | no |
| POST | `/api/v2/app/csm/enroll/code` | enrollment code in the body; **public** group, no JWT | one `0x06` frame | no |
| POST | `/api/v2/app/csm/enroll/device` | account JWT; **protected** group | one `0x06` frame | no |
| PUT | `/api/v2/app/preferences` | account JWT plus body-bound proof | one `0x06` frame | no |

`{i}` is the zero-based chunk index and MUST be decimal with no leading zeros.

**Why the chunk route is `private` and carries `Vary`.** Admission is by request header, and every shared cache keys on the URL alone, so `public, immutable` on a header-admitted route hands the tenant's full node fleet to any caller who has observed a `cat_id`, which is the anonymous fleet dump `01-DECISION.md` 4.6 rejected and 5.2.4 promised to close. The response is therefore `Cache-Control: private, max-age=86400, immutable` and `Vary: X-CSM-Loc` is the one header section 13.4 admits beyond its base set. The client's own ETag cache is unaffected, which is where the 86400 seconds actually pay; a mirror that honors `Vary` may still cache per locator.

**Why enrollment is two routes.** `/api/v2/app/*` on the panel is a `public` router merged with a `protected` router carrying `.route_layer(require_app_jwt)` (`apps/caramba-panel/src/api/v2/mod.rs:145-157`, `:160-207`, merged at `:209`). One path cannot be in both groups, and a first enrollment redeems a code and has no JWT. `enroll/code` goes in the public group with its own fail-closed limiter (section 13.3); `enroll/device` goes in the protected group and is the second-device bridge of `01-DECISION.md` 5.5.5, gated on capability bit 9.

**A device that is not registered to the locator's subscription.** `GET /sub/m1/{loc}?d=` whose `d` names a well-formed thumbprint that is not registered against that locator's subscription MUST return **404**, byte-identical to an unknown locator, and MUST NOT return 400 or 403. Distinguishing the two would turn the route into an oracle for which thumbprints belong to which subscription.

**Query parameters on `/sub/m1/{loc}`:**

| Name | Form | Required | Meaning |
|---|---|---|---|
| `n` | 26 base32 Crockford characters | yes | the client nonce, `base32_crockford(nonce16)` |
| `v` | decimal | yes | the highest directive version this device has accepted; 0 if none |
| `d` | 26 base32 Crockford characters | yes | `base32_crockford(dtp)`, names the recipient device for sealing |

`v` is the entire client-side state report. `01-DECISION.md` 5.4.6: it gives server-side rollback detection at a privacy cost of one integer whose upper bound the operator already knows. The client MUST NOT report which transport rung carried the request. That is a live map of which circumvention rungs still work, per device and per ASN, volunteered to a party who may be compelled.

**`since` on `/sub/k1`:** decimal, the client's currently trusted key document version.

> The response carries a frame stream (section 10.1) beginning at `N+1`, in ascending order with no gaps, containing **as many consecutive versions as fit under `thr.resp_max`, and at most 8**. The panel MUST stop adding frames before the stream would exceed `thr.resp_max`, and MUST always include at least `N+1`; if `N+1` alone does not fit, the panel has signed a document above `DOC_FRAME_MAX` and section 8.0.1 has already been violated. A client that receives fewer than the full chain MUST repeat the request with `since` set to the highest version it now trusts, and MUST NOT treat a short stream as an error. Versions MUST NOT be skipped, per section 7.3.

The earlier claim that "the 8-frame cap keeps the response under `RESP_MAX`" does not hold and is withdrawn: a typical key document measures 456 bytes, so 8 of them fit only by luck, and the largest deliverable key document is 4089 bytes, so two of those break the ceiling on their own. The cap that binds is the size, and 8 is only an upper bound on top of it.

### 13.3 Rate limiting

> `/sub/m1/{loc}`, `/sub/r1/{loc}`, `/sub/c1/{cat_id}/{i}`, `/sub/b1/{code}`, `/sub/k1` and both enrollment routes MUST have their own rate limits, and every one of them MUST fail **closed** when Redis errors.

| Route | Key | Limit |
|---|---|---|
| `/sub/m1/{loc}` | locator **and** source IP | 60 per 3600 s per locator, 600 per 3600 s per IP |
| `/sub/r1/{loc}` | locator **and** source IP | 24 per 3600 s per locator, 240 per 3600 s per IP |
| `/sub/c1/{cat_id}/{i}` | `X-CSM-Loc` locator **and** source IP | 128 per 3600 s per locator, 1280 per 3600 s per IP |
| `/sub/b1/{code}` | source IP, and a global counter per code | 10 per 3600 s per IP, 20 lifetime per code |
| `/sub/k1` | source IP | 60 per 3600 s |
| `/api/v2/app/csm/enroll/code` | source IP, and a counter per code | 10 per 3600 s per IP, 10 lifetime per code |
| `/api/v2/app/csm/enroll/device` | account id | 10 per 86400 s |

The per-locator figures are `ttl`-derived: at the default `ttl` of 7200 with 20 percent jitter a well-behaved device fetches a directive at most twice an hour, so 60 is roughly thirty devices' worth of headroom on one subscription. `/sub/b1/{code}` in particular is a 60-bit-secret oracle with an empty 404 body and it MUST NOT be left unlimited; the lifetime counter per code is what makes brute force cost an operator action rather than time.

This is `01-DECISION.md` P3 and it is a deliberate departure from every other limiter in the panel. The existing subscription limiter is `rate:sub:{uuid}` at 30 per 60 s, it lives inside `subscription_handler` (`apps/caramba-panel/src/subscription.rs:155-167`) so it does not apply to a new route, and it fails open. The two app-auth limiters at `app_auth.rs:253-288` also fail open. Fail-open on a locator-scoped route hands an adversary a free enumeration window every time Redis blips.

> `/sub/m1/{loc}` MUST NOT call `track_access` and MUST NOT enforce the device limit.

`01-DECISION.md` 5.5.2 and P3. The device limit is enforced today on the config fetch at `subscription.rs:203-262` through `get_active_ips`, that is by apparent source IP. Every ladder rung with a different egress burns a slot, and under fetch-through-tunnel the apparent IP is the exit node's, shared by every user of that node. Counting on the manifest path would make the ladder self-defeating. `00-DESIGN-BRIEF.md` R9 is retired only when the manifest path stops counting **and** the config path counts by thumbprint instead of by IP.

### 13.4 Response shape

Every CSM response body is exactly one frame, or for `?since=` exactly one frame stream. Nothing precedes or follows it.

| Header | Value |
|---|---|
| `Content-Type` | `application/vnd.caramba.csm1` |
| `Content-Length` | the frame length |
| `Cache-Control` | per the table in 13.2; `no-store` where not cacheable |
| `ETag` | chunks only: `"<cat_id>-<i>"`, a strong validator |
| `Vary` | chunks only: exactly `X-CSM-Loc` |
| `Date` | as emitted by the server |

> No other response header may be present. In particular: no `Content-Encoding`, no `Profile-Title`, no `Profile-Update-Interval`, no `Subscription-Userinfo`, and none of the five constant security headers the panel's root router stamps. A test MUST assert the exact set, per route, and MUST run with `Accept-Encoding: gzip, br` on the request so a compression layer that is still attached fails the assertion rather than passing it.

The legacy headers keep being emitted on `/sub/{uuid}` exactly as today (`subscription.rs:826-845`), unchanged, forever. `Profile-Update-Interval` keeps saying `"2"`. Invariant 7.

Chunks are content-addressed, so `Cache-Control: private, max-age=86400, immutable` with `Vary: X-CSM-Loc` is correct and a conditional GET with `If-None-Match` MUST return 304 with no body. The ladder's ETag caching (`01-DECISION.md` 5.3.4) uses this. `private` rather than `public` is the admission fix of section 13.2: the route is authorized by a request header, and a `public` entry keyed only on the URL is a fleet dump waiting for the first shared cache.

### 13.5 Status codes

| Code | Meaning | Body |
|---|---|---|
| 200 | success | one frame or frame stream |
| 304 | not modified, conditional GET on a chunk | empty |
| 400 | malformed locator, nonce, thumbprint or index; a request body that is not valid CBOR under the strict profile | empty |
| 401 | missing or invalid `X-CSM-Loc` on a chunk fetch; missing, invalid or expired account JWT; invalid `X-CSM-Proof` | empty |
| 404 | unknown locator, `cat_id`, chunk index, enrollment code, or a well-formed `d=` naming a device not registered to that locator's subscription; a `cat_id` the locator's tier does not own | empty |
| 409 | `If-Match` stale on `PUT /api/v2/app/preferences`; body is a freshly signed and freshly sealed `0x06` directive | one frame |
| 410 | an enrollment code that exists but is revoked, expired or exhausted | empty |
| 429 | rate limited | empty |
| 503 | signing key unavailable, or no imported root-signed key document exists | empty |

> A `cat_id` that exists but is not owned by the tier the presented locator resolves to MUST return **404**, not 401 and not 403. The two are indistinguishable to the caller by design: distinguishing them turns the chunk route into a cross-tier enumeration oracle.
>
> The 409 body MUST be signed and sealed for **this** request, against the nonce in the request body and the `dtp` it names. A cached directive frame reused as a 409 body fails V13 at the client and looks to the user like tampering. The panel MUST NOT reuse one.

> A non-200 response body MUST be empty, except for 409. No status text, no JSON error object, no HTML.

This replaces the current bare 403 text body on the config path (`subscription.rs:183`, `:263`). Refusal reasons are signed fields, not status text: a refusal that the client must act on arrives as `st` and `rc` inside a 200 directive, because signed fields survive caching and mirrors in a way a status line does not (`01-DECISION.md` 5.2.8). A `revoked` subscription still gets a signed directive that says `st = 5`; it does not get a 403.

### 13.6 The settings write

`PUT /api/v2/app/preferences`, `01-DECISION.md` B5, B6 and 5.4.2.

Request body: CBOR under the same strict profile, **not** a frame, because the client does not sign with a role key. Fields:

| Key | Name | Type | Mandatory | Meaning |
|---|---|---|---|---|
| 1 | `v` | uint | yes | must equal 1 |
| 2 | `nonce` | bstr(16) | yes | fresh per request, echoed in the signed response |
| 3 | `dtp` | bstr(16) | yes | this device |
| 4 | `want` | map | yes | same key space as the directive `pol` map (section 8.3), values without the provenance wrapper |
| 5 | `sel` | map | no | same key space as the directive `sel` map |

Headers: `If-Match` carrying the `ver` of the directive the client is amending, as a decimal string. `X-CSM-Proof`, base64url without padding, an ECDSA P-256 signature by the device **signing** key over:

```
sha256("csm1-write" || 0x00 || method || 0x00 || path || 0x00 || sha256(request body))
```

with the following fixed exactly, because none of them is unambiguous on this panel:

| Term | Value |
|---|---|
| `method` | the uppercase HTTP method in ASCII, `PUT` for the settings write and `POST` for both enrollment routes |
| `path` | the **canonical literal** for the endpoint, independent of the mount the request arrived on: `/api/v2/app/preferences`, `/api/v2/app/csm/enroll/code`, `/api/v2/app/csm/enroll/device`. It is never the received path. |
| the signature | ECDSA on P-256 over the message above, hashed with SHA-256 by the signing operation itself, that is the platforms' **message** APIs: `.ecdsaSignatureMessageX962SHA256` on Apple, `SHA256withECDSA` on Android. The signer MUST NOT pre-hash and then sign the digest as a message. |
| the encoding | fixed 64 bytes, `r \|\| s`, each a 32-byte big-endian integer, **not** ASN.1 DER. `s` MUST be normalized to the low half of the order (`s <= n/2`); a verifier MUST reject a high `s` rather than normalizing it. |
| the header | that 64-byte string, base64url without padding, 86 characters |

The canonical path literal is required because `api_routes` is mounted twice, `.nest("/api", api_routes.clone())` and `.nest("/caramba-api", api_routes)` at `apps/caramba-panel/src/main.rs:1582-1583`, and `caramba-sub` rebuilds the upstream URL as `format!("http://127.0.0.1:3000/api/{}", path)` (`apps/caramba-sub/src/handlers/proxy.rs:47`). A verifier that signed or checked the received path would reject a client that spelled the other mount, and the failure would look exactly like tampering.

The encoding is fixed because the two platforms disagree by default: `SecKeyCreateSignature` and Android's `Signature` both return ASN.1 DER X9.62, and a Rust or Go verifier reading raw `r || s` would reject every real device. Both platforms can convert DER to `r || s` in a dozen lines; converting at the edge is cheaper than admitting two encodings, and fixing `r || s` with low-`s` normalization also removes the signature-malleability question rather than leaving it open.

The same three rules, message-hashed, `r || s`, low `s`, govern the rekey proof of `02-SPEC.md` 10.3 and both enrollment bodies. There is exactly one device-signature construction in CSM/1.

**Worked pre-image, so three implementations can agree before any of them holds a device key.** For the settings write with an empty `want`, `method = PUT`, `path = /api/v2/app/preferences` and a request body whose `sha256` is `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (the digest of the empty string, used here only to make the example checkable), the signed message is:

```
63 73 6d 31 2d 77 72 69 74 65        "csm1-write"
00
50 55 54                             "PUT"
00
2f 61 70 69 2f 76 32 2f 61 70 70 2f 70 72 65 66 65 72 65 6e 63 65 73   "/api/v2/app/preferences"
00
e3 b0 c4 42 98 fc 1c 14 9a fb f4 c8 99 6f b9 24 27 ae 41 e4 64 9b 93 4c a4 95 99 1b 78 52 b8 55
```

71 bytes. As one string:

```
63736d312d777269746500505554002f6170692f76322f6170702f707265666572656e63657300
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
```

Its SHA-256, which is what the ECDSA operation signs internally, is `45ae6e7f2d6e63113532b04a140183d21319b6d3af86993c3360c31eb80b3716`. `05-TEST-VECTORS/` carries no ECDSA fixture, because ECDSA signing is randomized and a deterministic one would require an RFC 6979 implementation the corpus does not have; the gap is real and the pre-image above is what closes the part of it that actually diverges.

**Idempotency.** `caramba-sub` retries a failed upstream against a second target (`apps/caramba-sub/src/handlers/proxy.rs:151-197`), so at-most-once delivery is not available. The panel MUST make write redemption idempotent on the request `nonce`: a second write carrying a `nonce` already redeemed within the 300-second nonce lifetime MUST return the same signed response as the first and MUST NOT apply the change twice.

> The proof MUST cover the request body. The signed echo in the response MUST cover **every** field the write can set, not a subset.

`01-DECISION.md` B5 names the failure this exists to avoid: B's own `Caravan-Proof` covered method, path, nonce and session but not the body, and three of the four fields a state-CA MITM would rewrite were absent from the signed echo. Under the state-CA premise the brief documents, with the Russian Trusted Root in 28.6 percent of RuStore apps by count and 81.4 percent by weighted download share, an `If-Match` header alone is rewritable in flight, which is exactly what `PUT /api/v2/app/preferences` would rely on otherwise.

Three-way semantics: a key absent from `want` means unchanged, the sentinel text `"default"` means reset to operator default, any other value means set. On a stale `If-Match` the response is 409 carrying the current signed directive, which is a full directive frame, not a diff.

Success is 200 with a `0x06` sealed directive whose `nonce` echoes the request nonce and whose `pol` and `sel` are the authoritative post-write state with provenance.

> A cross-device write MUST NOT be able to set `killSwitch`, `dns`, `split.mode` or the enabled transport set on a sibling device without that device raising the Keep or Revert card unconditionally.

`01-DECISION.md` 5.4.4 and invariant 22. The card fires on any narrowing of the user's security posture regardless of provenance.

### 13.7 `caramba-sub` routing requirements

Verified in source: `apps/caramba-sub/src/main.rs:71-88` registers exactly `/health`, `/sub/{uuid}`, `/app`, `/app/`, `/app/{*path}` and `any /api/{*path}`. There is no fallback and no wildcard beyond `/api`. Both crates are on axum 0.8, whose router gives static path segments precedence over path parameters.

Consequences, and the work that follows from each:

1. **`/sub/k1` collides with `/sub/{uuid}`.** Static wins once `/sub/k1` is registered explicitly, so registering it explicitly is mandatory in both the panel and `caramba-sub`. Without the explicit route the request is handled as a subscription fetch for a subscription whose uuid is the literal string `k1`, which returns a 404 with a text body rather than the key document.
2. **`/sub/b1/{code}`, `/sub/r1/{loc}`, `/sub/m1/{loc}` and `/sub/c1/{cat_id}/{i}` all 404 today.** They have 3 or 4 segments and match nothing. Add explicit routes.
3. **The config cache MUST NOT apply to CSM routes.** The cache key is `sub:config:{uuid}:{client}:{relay}:{node}` (`apps/caramba-sub/src/handlers/subscription.rs:124-130`) and it omits country and variant, while the panel's own key is `sub_config_v5:{uuid}:{client}:{node}:{variant}:{cc}:{relay}` (`subscription.rs:692`). Two subscribers in different countries share one `caramba-sub` entry, which produces config-hash mismatches through the sub path that succeed direct. CSM responses either bypass the cache or use a key that includes every varying input; bypass is simpler and the documents are small.
4. **`variant` is silently dropped.** `caramba-sub`'s `SubParams` carries only `client`, `relay_country` and `node_id` (`handlers/subscription.rs:11-19`). Until `variant` is forwarded, a directive whose `sel.variant` is non-default causes the sub path to return the default variant deterministically and every config-hash check to fail. Capability bit 10 (section 5.1) exists to signal when this is fixed; the panel MUST NOT set it before the forwarding lands.
5. **The `subscription_domain` 308 redirect lives inside the handler, not the router.** `subscription.rs:113-137` issues it unconditionally when the `Host` header does not match, so the new routes do NOT inherit it and it must be reimplemented or extracted. `caramba-sub` additionally converts any upstream 3xx into a fatal 502 (`handlers/subscription.rs:180-189`), so a redirect on a CSM route is an outage, not a hop.
6. **The whole query string MUST be forwarded verbatim**, including `n`, `v`, `d`, `since` and `If-None-Match` handling. `caramba-sub` today reconstructs the upstream URL from three named parameters and drops everything else.
7. **`X-CSM-Loc` and `X-CSM-Proof` MUST be forwarded.** Only `profile-title`, `profile-update-interval` and `subscription-userinfo` are forwarded on the response side today (`apps/caramba-sub/src/handlers/subscription.rs:196-203`); CSM responses need none of those and need the header set of section 13.4 passed through unmodified.
8. **Body fidelity is already correct and needs no work.** `proxy_handler` streams `res.bytes_stream()` and copies headers verbatim, and reqwest is built without gzip or brotli in both crates, so no transparent decompression can desynchronize a copied `Content-Encoding`. This is the one item on the list that is already done.

The client-side companion to point 5: `01-DECISION.md` 5.1.8 permits exactly one redirect hop, and only when the target host equals the tenant's configured `subscription_domain`, after which the profile URL is normalized to the target so the hop disappears.

### 13.8 The enrollment bodies

Both enrollment routes take a CBOR request body under the same strict profile as section 13.6, **not** a frame, and both return one `0x06` sealed directive on success. They are separate routes because they sit in different auth groups (section 13.2), and they differ only in how the caller is authorized and in whether key 2 is present.

| Key | Name | Type | `enroll/code` | `enroll/device` | Meaning |
|---|---|---|---|---|---|
| 1 | `v` | uint | yes | yes | must equal 1 |
| 2 | `code` | tstr | yes | MUST be absent | enrollment code, 20 characters, hyphens ignored (section 4.1) |
| 3 | `nonce` | bstr | yes | yes | exactly 16, fresh per request, echoed in the signed response at V13 |
| 4 | `spki` | bstr | yes | yes | 1..128, the device signing key as a DER `SubjectPublicKeyInfo`; `dtp = sha256(spki)[0..16]` |
| 5 | `agree` | bstr | yes | yes | exactly 65, the device agreement public key, P-256 uncompressed |
| 6 | `tier` | uint | yes | yes | hardware tier: `1` Secure Enclave, `2` StrongBox or TEE, `3` software |
| 7 | `rkv` | uint | yes | yes | the generation this device assigns its agreement key; starts at 1 |
| 8 | `label` | tstr | no | no | <= 40, a device name the user typed; inert, never echoed into a signed document |

`X-CSM-Proof` is mandatory on both, computed exactly as in section 13.6 over the canonical path literal for the route. The proof is by the very key the body is registering, which proves possession of the private half; it is not an authorization and does not replace the code or the JWT.

> The panel MUST verify, in this order: the CBOR body under the strict profile; `sha256(spki)[0..16]` against nothing yet, but computed and stored as `dtp`; the `X-CSM-Proof` signature against the `spki` in the body; then the credential, which is the code for `enroll/code` and the account JWT for `enroll/device`. Verifying the proof before the credential means a request that cannot prove key possession never reaches the code table, which is the same ordering rule section `02-SPEC.md` 9.2 applies to the pin prefix.

The panel stores, per device: `dtp`, the signing SPKI, the agreement public key, `rkv`, the hardware tier, the subscription id, the account id, the creation time and the last-seen time. That store is a named deliverable (`06-MIGRATION.md` 3.2, P3) and it is the store `02-SPEC.md` 9.6 requires `GET /api/v2/app/devices` to surface.

Status codes are those of section 13.5. In particular a code that exists but is revoked, expired or exhausted is **410** and an unknown code is **404**, because a code the panel has never issued and a code it has retired are different operational facts for the operator and neither leaks anything the caller did not already supply.

---

## 14. URL and path constraints

`01-DECISION.md` 5.5.4 and invariant 8.

### 14.1 Hostnames

A hostname field (`mir.h`, `mir.sni`, `doh.h`, `pin.h`, node `h`, node `sni`, node `hst`) MUST satisfy all of:

- 1 to 64 bytes, ASCII only.
- Labels separated by `.`, each label 1 to 63 characters from `[a-z0-9-]`, not beginning or ending with `-`.
- Lowercase. A verifier MUST reject uppercase rather than normalizing it, so that two spellings of one host cannot produce two `chash` values for one catalog.
- No trailing dot, no userinfo, no port, no path, no scheme.
- An IP literal is permitted **only** in `mir.ip` and `doh.ip`, and only as a dotted-quad IPv4 address or a lowercase RFC 5952 IPv6 address.
- Internationalized names MUST be carried in A-label (punycode) form. A U-label MUST be rejected.

Node `h` additionally MAY be an IP literal, because `NodeInfo.address` is the node's IP today (`subscription_generator.rs:41`) and `frontend_url` is the domain when one exists.

### 14.2 Path-only fields

A path field (`doh.p`, resource `u`) MUST satisfy all of:

- 1 to 128 bytes, ASCII only.
- Begins with exactly one `/`. A second leading `/` is rejected, because `//host/path` is a scheme-relative URL.
- Contains no scheme, no `://`, no authority, no `@`, no `\`, no whitespace, no control characters.
- Contains no `..` as a complete path segment.
- Contains no `%2f` or `%2F`, in any case, so that a decoder cannot be tricked into producing a segment separator after validation.
- Consists only of unreserved characters `A-Z a-z 0-9 - . _ ~`, the sub-delimiters `! $ & ' ( ) * + , ; =`, plus `/`, `:`, `@`, `?`, and correctly formed `%XX` escapes.

> Path fields are resolved **only** against the pinned enrollment origin or a host drawn from the signed mirror list. A signed document can name a path; it can never name a host that is not already in the pool.

### 14.3 Absolute URLs

There are none. No signed CSM/1 field carries an absolute URL. `org` in the bootstrap blob is an **origin**, `scheme://host[:port]` with no path, no query and no fragment, and its scheme MUST be `https`. DoH endpoints are a host plus a path plus literal addresses, assembled by the client, never a URL string.

### 14.4 Scheme

> `http://` MUST be refused for any manifest, config, rule-set or geo fetch. The only non-TLS exception is `.onion`, because onion addresses are self-authenticating. Invariant 8.

`EnrollLink.normalizePanelUrl` accepts plain `http://` today (`apps/caramba-client/lib/data/models/enrollment.dart:66-67`, verified: the scheme test is `if (scheme != 'https' && scheme != 'http') return null;`) and MUST stop. `fetchSubscriptionBody` sets `followRedirects: true` with no size cap and no scheme check (`apps/caramba-client/lib/data/subscription_fetch.dart:22-49`, verified) and gets `followRedirects: false` with per-hop scheme and origin validation and a body size cap in the same change.

### 14.5 The subscription uuid is not a Connect credential

> The subscription uuid MUST NOT appear in any signed CSM/1 document. Invariant, from `01-DECISION.md` BC2 and 5.5.4.

The reason is verifiable in one function: `subscription_user_uuid` returns `sub.vless_uuid` falling back to `sub.subscription_uuid` (`apps/caramba-panel/src/services/subscription_service.rs:191-205`), and that same value becomes the VLESS uuid, the Trojan password, the TUIC uuid and part of the Hysteria2 password (`subscription_generator.rs:229`, `:332`, `:391`). Putting it in a directive that is deliberately exposed to mirrors and to `caramba-sub` would publish the tunnel credential to every host in the mirror pool, and it would forbid using any mirror the operator does not fully control, which is the opposite of the diversity the design needs.

The client already knows its own uuid and assembles the legacy config URL locally. Decoupling the uuid from the tunnel credential is a larger change against live users and is scheduled after cutover; until it lands, the client MUST NOT present any control that claims to rotate access when it only rotates a link.

### 14.6 Operator-supplied text

Every operator-supplied text field in this document (`ann`, `sup`, `ui.t`, `nm`, route `nm`, node `pn`) is inert.

- Capped at the byte limit in its field table, enforced at decode.
- Rendered as text, never as a link, never as markup.
- URL-shaped substrings stripped at render.
- Never rendered on the same surface as the verification chrome.
- Never persisted and never echoed, except `pn` and `id`, which are validated against a closed charset and are machine identity rather than operator prose.

> The client MUST refuse to open any URL supplied by an operator. Invariant 10.

Verified as the live exposure this closes: `GET /api/v2/app/branding` is registered in the public group before `route_layer(require_app_jwt)` (`apps/caramba-panel/src/api/v2/mod.rs:154-157`), is unauthenticated and unsigned, returns operator-controlled `support_url` and `bot_url` (`apps/caramba-panel/src/api/v2/app_branding.rs:33-34,55-56`), and the client opens them with `LaunchMode.externalApplication` (`apps/caramba-client/lib/features/branding/powered_by.dart:126`). That is a pin bypass on the one field an attacker most wants. Those fields move into the signed surface as `sup` and `nm`, and neither is ever opened.

---

## 15. Reproducing the fixtures

Every hex dump in this document was produced by a generator written independently of the panel signer, with a from-scratch RFC 8032 implementation, and every signature in it verifies. `01-DECISION.md` X1 requires exactly this: vectors computed independently, not emitted by the Rust signer alone, or all three implementations agree on the same wrong value and CI stays green.

Inputs:

```
root   private seed = sha256("csm1-doc-example-root")
                    = 9aa4b46c96a0ed2aabe0391f899737224ad96032e8ca6bd53fd9daf5614d05ed
root   public key   = 8b160c71c61008321cae0d0dc9a980b6e59cb26d0f4d1fa8dfc030c3675e2b7c
online private seed = sha256("csm1-doc-example-online")
                    = 3e395bd70b7b39edf135a4610ed77446cf6b964e13daa8a9eae29402de45ff57
online public key   = 75f350b3eb21344a96de195d82079e45f0a56fecdc736c16b61d56619afd5653

pid        = 226e8a20f699b964
kid root   = 226e8a20f699b964dfb01e86
kid online = 21e3e2cc0a3ba777e69ce14c
link_pin   = 49Q8M87PK6WP9QXG3T30
iat        = 1788307200   (2026-09-02T00:00:00Z)
dtp        = 4f0f22569564aab09a2d1a75c132d955   = sha256("csm1-doc-example-device-spki")[0..16]
nonce      = a3f10c94b27e5d6188ff20419c73ae05
loc        = EA3B8SKCY6VBWASE7AM1X48Y
```

`loc` is `base32_crockford(HMAC-SHA256(K, "csm1-loc" || 0x00 || "9f3c1d02-5b8e-4a17-9d44-0e7a6c11b3f8" || 00000001)[0..15])` with `K = sha256("csm1-doc-example-loc-secret")`. The uuid is the 36-byte ASCII text and `gen = 1` is four big-endian bytes.

Frame digests, for cross-checking a reimplementation:

| Document | Total bytes | `sha256(frame)` |
|---|---|---|
| key document | 257 | `671eaaaf6729274419faefe0cb44430126d6421e2f0f628bc4f1fab376bdad35` |
| catalog | 272 | `eb5c33321940d11813848b8b8b03417e75fb36a82c8aa9c9567e1686f9df535d` |
| catalog chunk 0/1 | 407 | `68d613af7e4f616464ad281a92739822361fa66600948cda6ede452b46237168` |
| directive | 228 | `b1956c4ed3877c424c1f11b903ae75be4f9a24a1537f760bb43a618da74be600` |
| bootstrap blob | 374 | `c78332c555152fd2572e7d5ec0f8bc2c1d48e2aedeccc99e3d4516eb05fc5247` |

These five frames, the sealed directive of section 9.6, the armored line of section 10.5, and the full negative corpus become the positive half of `05-TEST-VECTORS/`. The negative half is enumerated in `01-DECISION.md` section 9 and every entry there maps onto an error code from section 6.6 of this document.

Independence discipline, restated so it is not lost: at least one of the three implementations MUST be validated against vectors it did not generate, and the Rust signer MUST NOT be the sole source of any vector. The fixtures above satisfy that for all three, because none of them came from the panel.

---

## 16. Corrections to the inputs

Each item is a place where this document departs from `01-DECISION.md` or `00-DESIGN-BRIEF.md`. The code is followed where they disagree with it, and the arithmetic is followed where they disagree with itself.

**Correction 1: the node entry is 60 to 142 bytes, not 220 to 280.** `01-DECISION.md` BC1 sizes the honest node entry at 220 to 280 bytes. Measured against the encoding in section 8.2.1, across the five node shapes the panel actually emits, it is 60 to 142 bytes with a mean of 116. The difference is enum-coding `pr`, `nw`, `se`, `fp` and `fl` instead of carrying them as text, and carrying `pbk` as 32 raw bytes instead of 43 base64url characters. BC1's conclusion is unchanged: chunking is required in v1.

**Correction 2: the chunking threshold is the 4 KB response cap, not 12288 bytes.** `01-DECISION.md` 5.2.7 states both "no single response above 4 KB" and "a catalog payload above 12288 bytes MUST be chunked" in the same paragraph. These are inconsistent, because a 12 KB frame cannot be delivered inside a 4 KB response. The response cap binds, and it binds from roughly 2.9 KB of catalog frame, which is about 24 exits. This specification makes every catalog chunked, including one-chunk catalogs, so there is no size branch anywhere. 12288 is retained as the panel's warning threshold and 49152 as its refusal threshold.

**Correction 3: padding cannot be appended to a frame.** `01-DECISION.md` D3 calls for padding buckets drawn per request, and invariant 2 forbids trailing bytes after the signature slots. Both hold only if padding is a field inside the signed payload, which is what section 12 specifies as `pd` at common key 9. The second-order consequence is that catalog padding is necessarily fixed at signing time rather than per request, because per-request padding would change `chash` and break content addressing. That residual is stated in section 12.3 rather than papered over.

**Correction 4: the HPKE KEM is `DHKEM(P-256, HKDF-SHA256)`, not X25519.** `00-DESIGN-BRIEF.md` 4.7 proposes `DHKEM(X25519, HKDF-SHA256)`. The device key is P-256 because `01-DECISION.md` 5.5.1 requires it to be non-exportable in Secure Enclave or StrongBox, and neither holds an X25519 key. B4's own Android 12 boundary is the same constraint seen from the other side, since `PURPOSE_AGREE_KEY` is the P-256 ECDH capability. Using X25519 would move the sealing key out of hardware.

**Correction 5: `pn` is the flag-emoji Clash proxy name, not the `/servers` display name.** `00-DESIGN-BRIEF.md` 1.1 records that `GET /servers` returns synthesized names `"Node #{id} ({mbps} Mbps)"` (`apps/caramba-panel/src/api/v2/app.rs:250`). That string never appears in a config body. The Clash proxy name is built at `subscription_generator.rs:1165` as `format!("{} {}{}", node_label, proto_label, relay_suffix)` where `format_node_label` returns the country flag emoji alone (`:156-158`), producing strings like `🇩🇪 Stealth`. Both statements in the brief are true; they describe different strings, and `pn` is the second one. Confusing them breaks `Server.ID == Server.Name == the Clash proxy name`, which is the key for `Up(serverID)`, for `autotune.Candidate.ServerID` and for the mihomo prober.

**Correction 6: the reserve mirror pool cannot live in a public key document.** `01-DECISION.md` A6 puts the reserve mirror set in the signed key document, "7 day expiry, its own URL, root-signed". A7 requires the reserve pool to be held out of anything an anonymous request can pull. The key document must be publicly fetchable, because a client needs it before it has a locator. Reconciled here by making the reserve pool its own root-signed document type, `0x08`, at its own locator-scoped URL `/sub/r1/{loc}`, which is what A6's "its own URL" already implies. Key 14 of the key document is reserved and MUST NOT be used.

**Correction 7: the version rule needs an equality case.** `01-DECISION.md` invariant 9 refuses a document "whose version is at or below the stored high-water mark". Read literally, a client can never re-read its own cached catalog, since a re-fetch carries the same `ver`. Section 6.3 states the exact rule: below is refused, equal is accepted only when the frame is byte-identical to the stored frame, above is accepted after every other check passes. This does not weaken the invariant; it makes it implementable.

**Correction 8: `01-DECISION.md` C1's armored format needs a bundle identifier.** `CARCAP1.<i>/<n>.<chunk>` cannot detect chunks mixed between two bundles and gives a scanner no fixed target for progress. Section 10.2 adds an 8-character `bid` derived from the stream digest. Nine characters per chunk.

**Correction 9: the common envelope is 27 bytes, not the 31 an early draft of this document stated.** Recorded here because the catalog projection table in section 8.2.1 depends on it: map head 1, `v` 2, `pid` 10, `ver` 2, `iat` 6, `exp` 6. The stray "31 bytes" sentence beside the catalog hex dump in section 8.2 is corrected in place; `05-TEST-VECTORS/` carries it as `cor-1`.

**Correction 10: the chunk envelope is 51 to 59 bytes, not 40 and not 43.** Two different figures appeared in section 8.4 and a third was implied by section 11.3's `d + 127`. The fixture in section 8.4 declares `payload_len = 323` around a `d` of 272, which is 51, and the field-by-field sum agrees. The worst case across `i`, `n`, `tl` and `ver` is 59. Section 11.3's derivation is `d + 143` for the frame, giving 2959 at `d = 2816`, which still pads to 3072 and still sits under `CHUNK_RESP_MAX`. The conclusions are unchanged; the buffer sizes are not, and three implementations allocating `d + 127` would have under-allocated on every chunk.

**Correction 11: determinism does not survive a re-sign on its own.** Section 1.5 originally read as though pure Ed25519 were sufficient to make an unchanged catalog re-sign to identical bytes. It is not, because `iat` is mandatory and moves. The panel persists the signed frame keyed by a content digest and re-signs only on a content change; sections 1.5 and 6.3 carry the rule and `06-MIGRATION.md` 2.2 exit criterion 4 is restated against it.

**Correction 12: the field caps do not imply a deliverable document.** Section 8.0.1 adds `DOC_FRAME_MAX` because the key document's own caps admit 5118 bytes against a 4096-byte response ceiling, with no chunking path for `0x01`. The caps bound a decoder; the emission bound binds the signer; `05-TEST-VECTORS/` records the same finding as `cor-4`.

**Correction 13: the sealed-directive size table over-counted by one byte.** `kem`, `kdf` and `aead` are 2 bytes each, so 6 rather than 7, and the outer payload and unpadded sealed frame are 370 and 454 rather than 371 and 455. Every sealed fixture in `05-TEST-VECTORS/` measures 454; the corpus records it as `cor-2`. This was the one place where a document and a shipped fixture disagreed on a number an implementer would use as a self-check.

**Correction 14: the version rule is inert for catalogs and the threat model must not claim otherwise.** `cat_id` is derived from the catalog's own bytes, so V9's scope is unique per catalog and an older catalog always meets an empty high-water mark. Section 6.3 states this and names the real bound, V14a plus the directive's own monotonic `ver`. Retaining `cat_id` as the scope is deliberate; the alternative would refuse a legitimate operator revert to a previously published catalog.

Everything else in `01-DECISION.md` sections 5 and 6 is encoded here as written.

---

## 17. Constant summary

Every number this document specifies, in one place. Values marked provisional carry the measurement that would change them.

| Constant | Value | Where | Provisional |
|---|---|---|---|
| magic | `43 53 4D 31` | section 1 | no |
| frame overhead, `nsigs = 1` | 84 bytes | section 1 | no |
| signature slot | 76 bytes (12 + 64) | section 1.4 | no |
| `nsigs` range | 1..4 | section 1.4 | no |
| `payload_len` range | 1..49152 | section 1 | no |
| `MAX_DEPTH` | 6 | section 3.2 | no |
| `MAX_MAP_PAIRS` | 64 | section 3.2 | no |
| `MAX_ARRAY_ITEMS` | 512 | section 3.2 | no |
| `MAX_TSTR_BYTES` | 256 | section 3.2 | no |
| `MAX_BSTR_BYTES` | 3072 | section 3.2 | no |
| `MAX_UINT` | 2^53 - 1 | section 3.2 | no |
| critical key range | 1..63 | section 3.3 | no |
| non-critical key range | 64..1023 | section 3.3 | no |
| `pid` | 8 bytes | section 4 | no |
| `keyid_trunc` | 12 bytes | section 4 | no |
| `link_pin` | 96 bits, 20 characters | section 4 | no |
| `loc` | 120 bits, 24 characters | section 4 | no |
| `dtp` | 128 bits, 16 bytes | section 4 | no |
| `cat_id` | 80 bits, 16 characters | section 4 | no |
| `bid` | 40 bits, 8 characters | section 4 | no |
| clock skew tolerance | 300 seconds | section 6.2 | no |
| key document lifetime | 604800 s | section 8.0 | no |
| catalog lifetime | 2592000 s | section 8.0 | no |
| directive lifetime | 3600 s | section 8.0 | no |
| `PAD_UNIT` | 256 | section 12.2 | no |
| default `pb` | `[0, 3]` | section 8.2 | no |
| `RESP_MAX` (`thr.resp_max`) | 4096 | section 11.3 | no, invariant 5 |
| `thr.conn_bytes` | 8192 | section 11.3 | **yes**: RU per-ASN freeze measurement |
| `thr.conn_packets` | 22 | section 11.3 | **yes**: same measurement |
| `HANDSHAKE_DEBIT_BYTES` | 2816 | section 11.1 | **yes**: real chain capture from an RU vantage point |
| `HANDSHAKE_DEBIT_PACKETS` | 8 | section 11.1 | **yes**: same capture |
| `PACKET_MTU_ASSUMED` | 1400 | section 11.1 | **yes**: same capture |
| `CHUNK_PAYLOAD_MAX` | 2816 | section 11.3 | derived from `RESP_MAX` |
| `CHUNK_RESP_MAX` | 3584 | section 11.3 | derived from `RESP_MAX` |
| chunk envelope overhead | 51 bytes at the fixture, 59 worst case | section 8.4 | no |
| `DOC_FRAME_MAX` for `0x01`, `0x05`, `0x08` | 4096 | section 8.0.1 | no, equals `RESP_MAX` |
| `INNER_DIRECTIVE_MAX` | 2816 | section 12.2 | no, derived from `MAX_BSTR_BYTES` |
| smallest legal `0x03` frame | 226 bytes | section 9.3 | no |
| `BUILD_EPOCH` plausibility window | `[BUILD_EPOCH, BUILD_EPOCH + 315360000]` | section 6.4 | no |
| `PANEL_WARN` | 12288 | section 11.3 | no |
| `PANEL_REFUSE` | 49152 | section 11.3 | no, invariant 6; checked before encoding |
| default `ttl` | 7200 s | section 11.6 | no |
| default `jit` | 20 percent | section 11.6 | no |
| armored bytes per chunk | 620 | section 10.2 | no |
| armored characters per full chunk | 992 data, 1017 line | section 10.2 | no |
| armored stream cap | 16 frames, 65536 bytes, 106 chunks | section 10.1 | no |
| `?since=` frame cap | at most 8, and only as many as fit under `thr.resp_max` | section 13.2 | no |
| key document cache | 300 s | section 13.2 | no |
| chunk cache | 86400 s, immutable | section 13.2 | no |
| HPKE suite | `0x0000` mode, KEM `0x0010`, KDF `0x0001`, AEAD `0x0003` | section 9.1 | no |
| HPKE `info`, panel to device | `"CSM1-seal-v1"`, 12 bytes | section 9.2 | no |
| HPKE `aad`, panel to device | 33 bytes, second byte `0x06` | section 9.2 | no |
| HPKE `info`, client to panel | `"CSM1-seal-w1"`, 12 bytes | `02-SPEC.md` 10.2 | no |
| HPKE `aad`, client to panel | 33 bytes, second byte `0xFF` | `02-SPEC.md` 10.2 | no |
| sealed frame, unpadded, from the 228-byte fixture | 454 bytes | section 9.6 | no |
| device write proof | ECDSA P-256, message-hashed SHA-256, `r \|\| s` 64 bytes, low `s` | section 13.6 | no |
| minimum deprecation notice | 15552000 s (180 days) | section 8.1 | no |
| operator free text cap | 80 bytes | section 8.3 | no |

Four constants are provisional and all four fall out of one measurement: the real TLS handshake byte and packet cost against the tenant's certificate chain, and the real freeze point, both taken from a Russian vantage point on mobile and home broadband. That measurement is the field-measurement plan named in `01-DECISION.md` section 9 as feeding A4, A10 and this budget. Until it exists, `thr.conn_bytes` sits at 8192 against an observed 15 to 20 KB trigger, which is roughly a factor of two of margin, and `thr` is a signed catalog field precisely so the correction ships as data rather than as an app release. The client-side ceilings that bound how far a signer may move those fields are in `02-SPEC.md` section 14, and two of them are provisional against the same measurement.

---

## Changelog

One review pass, 2026-09-02, by three reviewers reading the whole set for cross-document consistency, panel implementability and client implementability. What it changed in this document:

**Blocking**

- The no-relay sentinel for `sel.rcc` is `--`, not `"NO"`. Section 8.3, matching `02-SPEC.md` 7.3 and the shipped corpus, which already carried `--`. `NO` is Norway.
- Step V11 and the `time_floor` definition in section 6.4 are amended in place to the corrected forms rather than left as notes in another document, because section 0 tells the reader to assume they have only this document. V11 now carries the `LIFETIME_MAX` term and the floor no longer touches the `Date` header. A first-trust clock-plausibility rule was added alongside them.
- V14 is split into V14a, the unconditional `cat` equality, and V14b, the tier-hash equality conditional on `tiers[tier]` being present, with the "fleet not root-anchored" chrome requirement. As written before, a conforming verifier rejected every catalog on a tenant that publishes no tier hashes.
- Catalog keys 25 and 26 are named as the **panel's** HPKE key, never the recipient of a `0x06`, in both 8.2 and 9.5.
- Chunk envelope overhead corrected from 40 and 43 to 51 measured and 59 worst case, and the 11.3 derivation from `d + 127` to `d + 143`, giving 2959 rather than 2943 at the maximum chunk.
- `cap` precedence between the catalog and the directive is now stated in 5.1, where both other documents said it was missing: the freshest verified, unexpired directive wins, and a directive cannot grant a content-presence bit whose backing array is absent from the bound catalog.
- Section 13.1 states how the CSM routes are constructed, not only where, since the panel's compression and header layers otherwise wrap them and make 13.4 unsatisfiable.
- Section 8.2 gained a normative array-ordering rule, without which content addressing rests on Postgres row order.
- Section 1.5 gained the persisted-frame rule: determinism alone cannot make a re-sign byte-identical, because `iat` moves.
- Section 8.0.1 adds `DOC_FRAME_MAX`, because the key document's own caps admit 5118 bytes against a 4096-byte ceiling with no chunking path for `0x01`.
- The chunk route's caching and its admission were mutually unimplementable: it is now `private` with `Vary: X-CSM-Loc`, and `Vary` is admitted to the 13.4 header set.

**Serious**

- `?since=` is size-derived: at most 8 frames, and only as many as fit under `thr.resp_max`.
- Sealed-directive size table: 6 rather than 7 for the suite fields, 370 and 454 rather than 371 and 455.
- `ct` range corrected to 242..3072, derived rather than asserted, and section 12.2's clamp restated per document type with `INNER_DIRECTIVE_MAX`.
- The 11.5 budget arithmetic corrected to 5824, and its conclusion reversed: the hygiene rule of 11.2 forbids the connection reuse the section previously permitted.
- Section 6.3 states that V9 is inert for `0x02` and `0x04`, keeps the `cat_id` scope deliberately, and names the real anti-rollback bound.
- Armored-reader failures map to `E_PARSE_FRAMING`, stated in 6.6 and 10.3, matching what the corpus already assigns.
- Section 13.6 fixes the write-proof signature encoding, the message-versus-digest question and the canonical path literal, and adds idempotency on the nonce plus a worked pre-image.
- Section 13.8 adds the enrollment body schema, and 13.2 splits enrollment into a public code route and a protected device route, because one path cannot sit in both of the panel's auth groups.
- Section 13.3 gives concrete limits for every CSM route, including `/sub/b1/{code}` and `/sub/k1`, all fail-closed.
- Parse step P12 validates the `pk` values inside a key document, which no step reached before.

**Minor**

- The "envelope is 31 bytes" note beside the catalog hex dump corrected to 27.
- The `loc` field walk in 8.3 now quotes the full four-byte prefix `18 19 78 18`.
- V1 carries no error code, because P3 has already made it unreachable.
- Section 7.2's enrollment-code grouping updated to a conforming code; the retracted `K7QW-3M2P-9XRT` inside the 8.5 dump is now labelled as retracted rather than presented as guidance.
- The node-entry size table is marked as fixture measurement, with the corpus's independent column beside it, and the projection table is marked illustrative.
- `PANEL_REFUSE` is noted as coinciding with the format limit, with the check moved before encoding so invariant 6 has something to do.
- Section 4 names `CSM_LOC_SECRET` as the locator HMAC key, with an explicit prohibition on reusing `SESSION_SECRET` or `APP_JWT_SECRET`.

Six new corrections, 9 through 14, are recorded in section 16.

