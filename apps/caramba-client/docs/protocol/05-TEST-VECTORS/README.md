# CSM/1 Test Vectors

Status: normative corpus, 2026-09-02. Companion to `../02-SPEC.md` (protocol behavior), `../03-WIRE.md` (byte layout) and `../01-DECISION.md` (rationale, which is cited here and never repeated).

This directory is the merge gate that `01-DECISION.md` X1 and invariant 3 require. Three implementations must agree byte for byte: a Rust signer and verifier in `apps/caramba-panel`, a Go verifier in `libs/caramba-core`, a Dart verifier in `apps/caramba-client`. Agreeing that a fixture fails is not sufficient. All three MUST return the same error code for the same fixture, because two implementations failing for different reasons is how a real divergence hides.

Key words MUST, MUST NOT, SHOULD and MAY carry their RFC 2119 meanings and apply to the three harnesses, not only to the protocol.

---

## 1. What is here

```
05-TEST-VECTORS/
  README.md                this file
  vectors.json             the index; every fixture, its expected verdict and its exact reason code
  gen/                     the generator, Go 1.26, standard library only
    go.mod  cbor.go  frame.go  docs.go  edwards.go  hpke.go
    positive.go  negative.go  armor.go  main.go
  bin/
    positive/   54 files   documents a conforming verifier MUST accept
    negative/   84 files   documents a conforming verifier MUST reject
    anchors/     3 files   trust anchors that a vector's context names; never verified as vectors themselves
  armor/        11 files   CARCAP1 text form, 5 accepted sets and 6 rejected ones
```

153 files, 306173 bytes, `vectors.json` included. The generator is 10 files and 161638 bytes of Go source.

The corpus is fully deterministic. Running the generator twice produces byte-identical output; the check is in section 3.

---

## 2. Regenerating

```
cd apps/caramba-client/docs/protocol/05-TEST-VECTORS/gen
go run .
```

Go 1.26 or later, no network, no third-party modules. The program writes `../vectors.json`, `../bin/**` and `../armor/**`, re-reads every file it wrote, re-hashes it, and fails loudly rather than emitting a corpus it cannot vouch for. Expected output:

```
wrote 152 fixture files + vectors.json, 306173 bytes total
vectors: 52 positive, 84 negative, 11 armor, 17 ed25519 key, 7 ed25519 sig, 8 transport
  03-WIRE.md 15 key document        257 bytes  MATCH
  03-WIRE.md 15 catalog             272 bytes  MATCH
  03-WIRE.md 15 catalog chunk 0/1   407 bytes  MATCH
  03-WIRE.md 15 directive           228 bytes  MATCH
  03-WIRE.md 15 bootstrap blob      374 bytes  MATCH
```

The generator aborts if any of the following does not hold, so a green run is itself evidence:

1. The RFC 9180 Appendix A.5 base-setup vector re-derives, field by field, from its own `skEm`: `pkEm`, `enc`, `shared_secret`, `key_schedule_context`, `secret`, `key`, `base_nonce`, `exporter_secret`, the sequence 0 and sequence 1 ciphertexts, and all three exported values. This is the self-test of the hand-written HKDF labeling, DHKEM(P-256) and ChaCha20-Poly1305.
2. `pid`, `link_pin` and `loc` equal the values published in `03-WIRE.md` section 15.
3. All five frame digests of `03-WIRE.md` section 15 reproduce exactly.
4. The armored line of `03-WIRE.md` section 10.5 reproduces exactly, all 620 characters including the `bid` `RY1K5HAN`.
5. Every file listed in `vectors.json` exists on disk at the stated length with the stated `sha256`.
6. Every vector names a context that exists, and every artifact written is referenced by a vector, a context anchor or an armor entry. An orphan file is a build failure.

### Why the generator is written the way it is

**Standard library only, hand-written CBOR.** The encoder in `gen/cbor.go` has no indefinite-length mode, no tag support, no float support, no negative integers, no null, no text or byte-string map keys, and it panics on a duplicate or out-of-range map key. It cannot express anything the strict profile of `03-WIRE.md` section 3 forbids. That inability is the point: it is the existence proof that the profile is implementable and unambiguous, since the whole positive corpus round-trips through it. Malformed fixtures are produced by splicing bytes into encoder output (`gen/negative.go`), never by weakening the encoder, so there is exactly one code path in the generator that emits conforming bytes.

**Nothing comes from the Rust signer.** `01-DECISION.md` X1 requires vectors computed independently, or all three implementations agree on the same wrong value and CI stays green. Ed25519 signing here is `crypto/ed25519`; the small-order and canonicity vectors are computed from a from-scratch Edwards25519 implementation in `gen/edwards.go`; HPKE and ChaCha20-Poly1305 are written out in `gen/hpke.go`; and the RFC 9180 key-schedule vector is imported from the RFC and re-derived as a check, never invented. The panel is not in the loop.

**Determinism without a seedable RNG.** `crypto/rand` in Go cannot be seeded and there is no standard-library API that makes it deterministic. The generator therefore uses no entropy source at all. Values that must look unpredictable are either fixed constants transcribed from `03-WIRE.md` section 15 or drawn from a SHA-256 counter stream over a fixed label (`detReader` in `gen/frame.go`). See correction `cor-6` in `vectors.json`.

---

## 3. Determinism check

```
cd 05-TEST-VECTORS
find bin armor vectors.json -type f | sort | xargs shasum -a 256 | shasum -a 256
```

At the corpus version recorded here this prints:

```
b36a2e11704936400f2cedb21d5cbc59c4f31c9fefad6ce90429e5552e12512b  -
```

CI SHOULD run the generator and assert this aggregate is unchanged, which catches an accidental edit to a fixture more cheaply than diffing 152 files. Changing a fixture on purpose changes this value, and the change belongs in the same commit as the reason for it.

---

## 4. `vectors.json`

One object, 21 top-level keys. The sections a harness reads are these.

| Key | Contents |
|---|---|
| `fixture_keys` | Every seed, public key, key id, `pid`, `link_pin`, `loc`, `dtp`, nonce, device agreement key pair and enrollment code, as hex or text. A harness that wants to sign its own extra fixtures has everything it needs here. |
| `contexts` | Named verification contexts (section 5). |
| `published_digest_check` | The five frame digests of `03-WIRE.md` section 15, expected against actual, with a boolean. |
| `published_armor_check` | The armored line of `03-WIRE.md` section 10.5, expected against actual. |
| `vectors` | 138 frame fixtures. |
| `armor` | 11 CARCAP1 sets. |
| `ed25519_public_key_ingest` | 17 raw public keys with a verdict and the clause of `03-WIRE.md` 2.1 that decides it. |
| `ed25519_signature` | 7 (key, message, signature) triples with a verdict and the clause of 2.2 that decides it. |
| `hpke` | The suite, the `info` and `aad` strings, the fixture recipient key pair, and the imported RFC 9180 A.5 vector. |
| `derivations` | 9 identifier derivations with input, output and the rule. |
| `transport` | 8 rules that are not frames: `Content-Encoding`, response size caps, redirect handling, scheme. |
| `node_entry_shapes` | Measured byte size of the five node shapes `03-WIRE.md` 8.2.1 enumerates. |
| `document_sizes` | Minimum, typical and maximum measured size per document type. |
| `error_code_registry` | The registry of `03-WIRE.md` 6.6, so a harness can assert it handles every code. |
| `corrections` | Nine places where the corpus departs from its inputs, with evidence and resolution (section 8). |

### Fields of a `vectors` entry

```json
{
  "id": "neg-verify-keydoc-signed-by-online",
  "group": "negative",
  "file": "bin/negative/verify_keydoc_signed_by_online.bin",
  "doc_type": 1,
  "bytes": 257,
  "sha256": "…",
  "verdict": "reject",
  "code": "E_VERIFY_UNAUTHORIZED",
  "step": "V4",
  "context": "default",
  "context_override": { "hwm": { "2": 7 } },
  "note": "…"
}
```

`verdict` is `accept` or `reject`. `code` is the exact identifier from the registry of `03-WIRE.md` 6.6 and is what all three implementations MUST return. `step` is the step of `03-WIRE.md` 6.1 or 6.2 that decides it, and it is diagnostic rather than normative: a harness MUST match `code`, and SHOULD report `step` when it fails so a divergence is readable without a debugger.

`group` is `positive`, `negative` or `reference`. A `reference` entry is not a conformance vector. There are two, both kept so an implementer can diff against a document: `pos-b1-wire85`, which is the exact bootstrap blob printed in `03-WIRE.md` 8.5 and whose enrollment code does not fold in the pin, and `pos-k1-max-caps`, which is a key document at every field cap and therefore larger than the response ceiling allows. A harness SHOULD parse both and MUST NOT treat either as a size or format precedent.

---

## 5. The context model

A frame does not carry the state a verifier checks it against. `contexts` supplies that state, and a vector names one; `context_override` replaces named fields of it.

| Context | Anchor | What it sets up |
|---|---|---|
| `default` | `bin/positive/k1_min.bin` | The ordinary case: root key pinned, `hwm` at `{k1:1, c1:6, m1:411, c1c:6, b1:0, r1:0}`, `now = iat + 300`, `time_floor = iat`, the outstanding nonce and this device's `dtp`. |
| `first_trust` | none | No trusted key document. The anchor is `link_pin` (`03-WIRE.md` 7.2). Only `0x01` and `0x05` can be verified here. |
| `rev_online` | `bin/anchors/k1_rev_online.bin` | The online key id appears in `rev.kids`. |
| `online_thr2` | `bin/anchors/k1_online_thr2.bin` | `roles[2]` is `{ks:[online, online2], thr:2}`. |
| `rotation_v1` | `bin/positive/k1_rot_v1.bin` | Trusted version 1 of the rotation chain, `hwm` 1. |
| `rotation_v2` | `bin/positive/k1_rot_v2.bin` | Trusted version 2, whose root role holds `rootB` only. |
| `root_only` | `bin/anchors/k1_root_only.bin` | A tenant with a root role and no online role. The only shape in which `E_VERIFY_ROLE` is reachable. |

Notes a harness MUST honor:

- `hwm` is per `(pid, doc_type, scope)`, where scope is the locator for `0x03` and `0x08`, the `cat_id` for `0x02` and `0x04`, and empty for `0x01` and `0x05` (`03-WIRE.md` 6.3). The values in a context are per `doc_type`; unless a vector's `context_override` says otherwise, each fixture is verified against a fresh scope, so two catalogs at the same `ver` do not collide.
- That fresh-scope rule is not a harness convenience, it is the shape of the protocol: `cat_id` is derived from the catalog's own bytes, so **V9 is inert for `0x02` and `0x04` by construction** and every catalog meets an empty high-water mark. `03-WIRE.md` 6.3 and `02-SPEC.md` 5.1 now state this, and `04-THREAT-MODEL.md` 2.1 names the real anti-rollback bound for a catalog, which is V14a plus the monotonicity of the directive that named it. The two version negatives in the corpus are directive and key-document vectors for that reason, and a harness MUST NOT read them as evidence that a catalog rollback is caught at V9.
- `now` and `time_floor` are fixture values, not the wall clock. A harness MUST inject them. A harness that uses the real clock will pass today and fail on 2026-09-09, which is the worst possible failure mode for a merge gate.
- The anchors in `bin/anchors/` are inputs, not vectors. They do not appear in `vectors` and MUST NOT be run as conformance fixtures.
- Several key documents in the corpus are alternative version 2 documents against the `default` anchor (`pos-k1-typical`, `pos-k1-max-deliverable`, `pos-k1-max-caps`). They are mutually exclusive: each is verified from a fresh profile sitting at version 1, never in sequence.

---

## 6. Coverage

### 6.1 Positive frames, 52

Every document type at minimum, typical and maximum size, plus the shapes the protocol distinguishes.

| Document type | Fixtures |
|---|---|
| `0x01` key document | minimum (the `03-WIRE.md` 8.1 fixture), typical (two online keys in overlap, revocation, tier hashes, a dated deprecation), maximum deliverable under `thr.resp_max`, and the three-document rotation chain v1, v2, v3 |
| `0x02` catalog | minimum (the 8.2 fixture), typical (40 exits, 3 relays, 4 mirrors over 4 ASNs, DoH, rule-set and geo hashes, pins, ladder defaults, panel HPKE key), maximum (448 exits, 48589 bytes), dual signature, and a stale but still live catalog |
| `0x03` directive | minimum (the 8.3 fixture), typical, maximum (every optional field, padded to 3840), padded at `r = 0` and at `r = 3`, the `--` no-relay sentinel, all eight `st` values, and one carrying an unknown non-critical key |
| `0x04` catalog chunk | chunk 0 of 1 (the 8.4 fixture), both chunks of the typical catalog, all 18 chunks of the maximum catalog |
| `0x05` bootstrap blob | conforming minimum, maximum (32 mirrors, 8 DoH), and the `03-WIRE.md` 8.5 reproduction as a reference entry |
| `0x06` sealed directive | unpadded (454 bytes) and padded to 512 |
| `0x08` reserve pool | minimum, three mirrors over three ASNs and three countries, cohort 4 |

Two positives deserve attention because they are the ones a lazy implementation fails.

**`pos-m1-noncritical-key`** carries map key 64, the agreement-key rekey slot of `02-SPEC.md` 10.3. A v1 verifier MUST ignore it and MUST still accept the document. It is the only vector that distinguishes a correct verifier from one that rejects everything it does not recognize, and without it the extension mechanism of `03-WIRE.md` 3.3 is untested.

**`pos-c1-stale-but-live`** has `iat` 20 days before `time_floor` and `exp` 10 days after `now`. The earlier form of `03-WIRE.md` 6.2 step V11, `iat >= time_floor`, rejects it; the corrected form, `iat + LIFETIME_MAX[doc_type] + 300 >= time_floor`, accepts it. **Since the review pass, `03-WIRE.md` 6.2 and 6.4 carry the corrected form in place** rather than leaving the amendment as a note in `02-SPEC.md`, so a verifier written from either document now agrees with this fixture. The expected verdict is **accept**, and a harness that implements the retracted literal V11 fails here, which is the intended signal.

**`pos-m1-max`**, at 3840 bytes, is a valid `0x03` frame padded to the `pd` limit and a verifier MUST accept it. It is **not** a delivery precedent, and `03-WIRE.md` 12.2 now says so: a directive is never transmitted bare, and the sealed `0x06` that carries one can hold an inner frame of at most 3056 bytes because `ct` is bounded by `MAX_BSTR_BYTES`, so `INNER_DIRECTIVE_MAX` is 2816 and a signer MUST NOT emit a padded directive above it. The fixture exercises the parser and the padding field at their limits, which is what a corpus is for; the clamp is what a signer is held to.

### 6.2 Negative frames, 84

Every code in the registry of `03-WIRE.md` 6.6 is exercised at least once, and no vector uses a code outside it.

| Code | Step | Count |
|---|---|---|
| `E_PARSE_SHORT` | P1, P5 | 2 |
| `E_PARSE_MAGIC` | P2 | 1 |
| `E_PARSE_DOCTYPE` | P3 | 3 |
| `E_PARSE_LEN` | P4 | 2 |
| `E_PARSE_NSIGS` | P6 | 2 |
| `E_PARSE_FRAMING` | P7 | 3 |
| `E_PARSE_SLOTORDER` | P8 | 2 |
| `E_PARSE_CBOR` | P9 | 22 |
| `E_PARSE_ENVELOPE` | P10 | 3 |
| `E_PARSE_FIELD` | P11 | 15 |
| `E_VERIFY_NOANCHOR` | V2 | 1 |
| `E_VERIFY_ROLE` | V3 | 1 |
| `E_VERIFY_UNAUTHORIZED` | V4 | 5 |
| `E_VERIFY_REVOKED` | V5 | 1 |
| `E_VERIFY_SIG` | V6 | 2 |
| `E_VERIFY_THRESHOLD` | V7 | 1 |
| `E_VERIFY_PID` | V8 | 1 |
| `E_VERIFY_VERSION` | V9 | 2 |
| `E_VERIFY_ROTATION` | V10 | 2 |
| `E_VERIFY_IAT` | V11 | 2 |
| `E_VERIFY_EXPIRED` | V12 | 1 |
| `E_VERIFY_NONCE` | V13, and inner V13 through a seal | 2 |
| `E_VERIFY_DEVICE` | V13 | 1 |
| `E_VERIFY_CATHASH` | V14b | 1 |
| `E_SEAL_RECIPIENT` | seal steps 3 and 5 | 2 |
| `E_SEAL_SUITE` | seal step 4 | 2 |
| `E_SEAL_OPEN` | seal step 6 | 2 |

The named cases from the document plan of `01-DECISION.md` section 9 map as follows. Wrong-role signing pairs are four vectors (`keydoc_signed_by_online`, `catalog_signed_by_root`, `directive_signed_by_root`, `signed_by_stranger`), and they return `E_VERIFY_UNAUTHORIZED` rather than `E_VERIFY_ROLE`, because the role resolves correctly and it is the key set membership test at V4 that fails. Threshold violation, version regression, expiry, inexact framing, trailing bytes, inflated `nsigs`, duplicate CBOR map keys, non-minimal integers, indefinite lengths, unknown tags, floats and invalid UTF-8 each have a vector under those names. Small-order and non-canonical Ed25519 public keys and non-canonical `S` are in the key-material sections rather than as frames, for the reason in section 6.4. HPKE wrong-recipient and tampered AAD are `neg-seal-wrong-recipient` and `neg-seal-aad-version`. Oversized payload is `neg-parse-len-over` plus the transport entry `tr-payload-len-over-cap`. Decompression bounds are the transport entry `tr-content-encoding`: CSM/1 has no compression, `03-WIRE.md` 12.4 forbids `Content-Encoding` on a CSM response, and the correct behavior is to refuse the response rather than to bound a decompressor, so there is no decompression bound to get wrong and the vector states that rather than inventing one.

Note the distinction between `neg-cbor-trailing-in-payload` and `neg-parse-framing-trailing`. The first appends a byte inside `payload_len`, after the top-level map ends, and is a CBOR rule C2 failure. The second appends a byte after the last signature slot and is an exact-length failure at P7. An implementation that conflates them will pass one and fail the other.

### 6.3 Armored form, 11

Accepted: the bootstrap blob as one chunk, the `03-WIRE.md` 8.5 blob as one chunk (whose line is compared against the published example character for character), an offline snapshot of four frames across three chunks, the root rotation chain versions 2 and 3 as the `GET /sub/k1?since=1` response, and the maximum blob across seven chunks.

Rejected: mixed `bid` between two bundles, a missing ordinal, lines disagreeing on `n`, a non-final chunk shorter than 620 bytes, an illegal character (`U`, which the Crockford alphabet excludes along with `I`, `L` and `O`), and non-zero trailing pad bits.

An armor entry carries `stream_bytes`, `frames`, `chunks` and `bid`. A harness MUST, for an accepted set: strip whitespace, decode, check `base32_crockford(sha256(joined)[0..5]) == bid` before parsing, then walk the frame stream using each frame's own `payload_len` and `nsigs` and confirm it lands exactly on the end.

### 6.4 Key and signature material

`ed25519_public_key_ingest` holds 17 raw 32-byte public keys, 3 accepted and 14 rejected, each with the clause of `03-WIRE.md` 2.1 that decides it. The eight small-order encodings were computed, not transcribed: the generator finds an off-subgroup point, multiplies it by `L` to land in the torsion subgroup, and walks the resulting group of order 8. `03-WIRE.md` 2.1 clause 3 explicitly forbids implementing the test as a hardcoded blacklist, and the corpus takes that seriously enough not to ship one as its own source of truth. The non-canonical entries are the `y + p` spelling of each small-order point whose `y` is below 19, plus `y = p` exactly and an all-ones encoding.

These are key material rather than frames because clause 2.1 governs a key entering the trusted set, and `03-WIRE.md` section 6 has no step that validates the `pk` values inside a key document's `keys` array. That gap is correction `cor-3`; the corpus resolves it by assigning `E_PARSE_FIELD` and recommending an explicit step, and it also ships `neg-field-kid-mismatch` so the `keys` array is not entirely untested at frame level.

`ed25519_signature` holds 7 triples, 1 accepted and 6 rejected: `S + L`, `S = L` exactly, `S` with the high bit set, `R` replaced by a small-order point, a bit flip inside `R`, and one more that is worth its own paragraph.

**`ed-sig-cofactored-only`** is a public key and signature constructed so that a cofactored verifier accepts and a cofactorless verifier rejects. The public key is `[a]B + T` where `T` has order 8, so it passes ingest clause 3, and `S` was solved for the cofactored equation. `03-WIRE.md` 2.2 clause 3 mandates the cofactorless equation, so the expected verdict is **reject**. This is the only vector in the corpus that catches that particular divergence: an implementation with the wrong verification equation passes every other fixture here. It is also the one most likely to be quietly wrong in Dart, where `03-WIRE.md` 2.3 already warns that most available Ed25519 packages perform none of the required checks.

### 6.5 HPKE

The suite is `mode_base`, `DHKEM(P-256, HKDF-SHA256)`, `HKDF-SHA256`, `ChaCha20Poly1305`, which is RFC 9180 Appendix A.5.

`hpke.rfc9180_key_schedule_vector` is **imported verbatim** from `https://www.rfc-editor.org/rfc/rfc9180.txt`, Appendix A.5.1, A.5.1.1 and A.5.1.2. It is marked as imported in the JSON. The generator re-derives every field of it on each run as a self-test and refuses to emit a corpus on any mismatch, but the values shipped are the RFC's, so a harness that passes them is agreeing with the RFC and not with this generator.

`hpke.aad_fixture` is the 33-byte `aad` of the fixture sealed directive. A recipient MUST recompute `aad` from the outer payload's own `pid`, `dtp` and `ver` and MUST NOT accept one from the wire; `neg-seal-aad-version` is the fixture that proves it, and `neg-seal-inner-nonce` is the fixture that proves the recovered inner `0x03` frame is parsed and verified in full rather than trusted because the envelope opened.

---

## 7. How the three harnesses consume this

All three load the same `vectors.json` and the same bytes. The shape is identical in each language:

1. Parse `vectors.json`.
2. For each entry of `vectors`: read `file`, assert its length equals `bytes` and its `sha256` matches, build the verifier state from `contexts[context]` with `context_override` applied, run parse then verify, and assert the outcome equals `verdict` and, on a rejection, that the returned code equals `code`.
3. Run `ed25519_public_key_ingest` against the key ingest path, `ed25519_signature` against the signature path, and `hpke.rfc9180_key_schedule_vector` against the HPKE key schedule.
4. For each entry of `armor`, run the reader.
5. For each entry of `derivations`, recompute the value from the stated input.
6. Assert `published_digest_check` and `published_armor_check` all report `match: true`, which turns a stale corpus into a test failure rather than a silent regression.

Every harness MUST fail the build on any disagreement, and the three CI jobs are a merge gate (`01-DECISION.md` invariant 3). A harness MUST NOT skip a vector it does not yet implement; it marks the whole suite failing until the code exists. Skipping is how a gate stops being one.

### Rust, `cargo test` in `apps/caramba-panel`

Path from the crate root is `../caramba-client/docs/protocol/05-TEST-VECTORS/`. Load it through a build-time constant rather than a relative path computed at run time, so a test run from a different working directory does not silently find nothing and pass.

Ed25519 MUST use `ed25519_dalek::VerifyingKey::verify_strict`, which implements 2.1 clause 3 and 2.2 clause 1. `VerifyingKey::verify` MUST NOT be used. The dependency already exists at `libs/caramba-shared/Cargo.toml:16`, feature-gated behind `license`. Read correction `cor-9` before reusing the existing licensing helper: `libs/caramba-shared/src/license.rs:205` builds the key with `VerifyingKey::from_bytes`, which performs no small-order or canonicity check in dalek v2, and `:225` verifies with `verifying_key.verify(&msg, &signature)`. That is the API `03-WIRE.md` 2.3 forbids, in the exact code `03-WIRE.md` 2.3 points at as the precedent to reuse. `ed-sig-cofactored-only` and `ed-key-small-order-*` are the fixtures that will fail if it is copied.

The panel is also the signer, so its harness carries an obligation the other two do not: for each positive fixture it MUST re-sign the same payload with the same fixture key and assert the resulting frame is byte-identical. Ed25519 is deterministic by `03-WIRE.md` 1.5, so this is a strict test, and it is what proves the panel's encoder agrees with the corpus rather than merely accepting it.

### Go, `go test` in `libs/caramba-core`

Path from the module root is `../../apps/caramba-client/docs/protocol/05-TEST-VECTORS/`. Prefer copying the corpus into the module with `go:embed` if a build ever needs to work from a module cache.

`crypto/ed25519.Verify` performs 2.2 clause 1 and clause 3 but does NOT perform 2.1 clause 3, so the small-order test MUST be added explicitly at key ingest. Read correction `cor-8` first: `03-WIRE.md` 2.3 says to use `filippo.io/edwards25519` "which is already in the module graph through mihomo", and it is not. The only edwards25519 module present is `github.com/metacubex/edwards25519 v1.2.0`, declared indirect at `libs/caramba-core/go.mod:61`; a search for `filippo.io/edwards25519` across every `go.mod` and `go.sum` in the repository returns nothing. Use the module that is there, or add the other one deliberately and say so.

The Go side is also the renderer under `01-DECISION.md` BC1 and P9, so `pos-c1-typical` and `pos-c1-max` do double duty: they are the node sets the identical-proxy-name fixture of `01-DECISION.md` 5.2.5 should render against once the Rust and Go renderers exist.

### Dart, `flutter test` in `apps/caramba-client`

Path from the package root is `docs/protocol/05-TEST-VECTORS/`, which is inside the package, so no `assets` entry is needed for a `dart:io` read in a test.

`apps/caramba-client/pubspec.yaml` declares no cryptography, CBOR or base32 package today; verified, a search for `crypto`, `pointycastle`, `cryptography`, `cbor` and `base32` in that file returns nothing. Adding one is a prerequisite, per `00-DESIGN-BRIEF.md` build item 4. **The acceptance test for whichever package is chosen is this corpus**, and specifically `ed25519_public_key_ingest` and `ed25519_signature` in full, before the dependency is accepted. `03-WIRE.md` 2.3 notes that most Dart Ed25519 implementations perform none of the required checks; running the seven signature vectors and seventeen key vectors against a candidate takes an afternoon and settles it.

Dart has one hazard the other two do not. `MAX_UINT` is `2^53 - 1` precisely so a Dart or JavaScript-hosted decoder never silently loses precision, and `neg-cbor-uint-over-max` is the fixture that checks it. On Flutter web, `int` is a double, and a decoder that accepts a larger integer will round it and then compare `iat` values that are not the values on the wire.

### The migration phases

During the shadow phase of `01-DECISION.md` C7, verification failures against live traffic are logged and never fatal. That does not extend to this corpus. These fixtures are fatal from the first commit that lands a verifier, in all three languages, because the whole point of a synthetic corpus is that it is available before the traffic is.

---

## 8. Corrections to the inputs

Nine items, carried in full in `vectors.json` under `corrections` with evidence and resolution. Summarized here.

| Id | Subject | Finding |
|---|---|---|
| `cor-1` | `03-WIRE.md` 8.2 | The note under the catalog fixture says the envelope is 31 bytes because `ver` is 7. It is 27, as section 8.0 and Correction 9 of section 16 both state and as the hex dump shows. 31 would be right only above version 65535. |
| `cor-2` | `03-WIRE.md` 9.6 | The sealed-directive size table over-counts the suite fields by one byte: `kem`, `kdf` and `aead` cost 2 bytes each, 6 in total, not 7. The outer payload is 370 bytes and the unpadded sealed frame is 454, not 371 and 455. The corpus measures 454. The padded figure of 512 is unaffected. |
| `cor-3` | `03-WIRE.md` 6.1 P11 versus 3.3 | Two error-code decisions were left to the implementer: which code an unrecognized critical key returns, and which step validates the `pk` values inside a key document. **Both are now closed in `03-WIRE.md`.** The first was already answered by P11 and the corpus's split, `E_PARSE_CBOR` for what a decoder decides without a `doc_type` and `E_PARSE_FIELD` for everything doc-type-dependent, was the right reading of it. The second was a genuine gap and `03-WIRE.md` 6.1 now carries **step P12**, which validates every `pk` inside a key document against all three clauses of 2.1 and returns `E_PARSE_FIELD`, exactly as this corpus recommended. |
| `cor-4` | `03-WIRE.md` 8.1 versus invariant 5 | A key document at every stated field cap is 5118 bytes, 1022 above `thr.resp_max`. The caps and the response ceiling are not jointly satisfiable, and `GET /sub/k1` is the one endpoint a client must reach before it has anything else. **Closed by `03-WIRE.md` 8.0.1**, which adds `DOC_FRAME_MAX = 4096` as an emission bound on `0x01`, `0x05` and `0x08`: the caps bound a decoder, the emission bound binds the signer, and the corpus's 4089-byte `pos-k1-max-deliverable` is what a conforming panel may sign while `pos-k1-max-caps` stays a `reference` entry it may not. `03-WIRE.md` 13.2 also replaces the `?since=` 8-frame claim with a size-derived rule, which was the same defect on the rotation path. |
| `cor-5` | `01-DECISION.md` BC1 node size | Re-measured across the same five shapes: 62 to 135 bytes, mean 108, against `03-WIRE.md` Correction 1's 60 to 142, mean 116. The conclusion is unchanged. The residual is how much text a fixture puts in `pn` and `h`, so the mean is fixture choice and should not be cited as a protocol constant. |
| `cor-6` | The instruction to seed `crypto/rand` | Not possible in Go. The generator uses no entropy source at all; see section 2. |
| `cor-7` | `03-WIRE.md` 12.2 versus `MAX_BSTR_BYTES` | `pd` is capped at 3072 bytes, so reachable padded size is bounded by `L0 + 3076` as well as by `thr.resp_max`. A 228-byte directive cannot reach the 4096 ceiling. Not a defect and no change proposed, since the default `pb` of `[0,3]` never approaches the cap, but an implementer who reads only the clamp sentence will size a buffer that never fills. |
| `cor-8` | `03-WIRE.md` 2.3, Go | `filippo.io/edwards25519` is not in the module graph. `github.com/metacubex/edwards25519 v1.2.0` is, indirect, at `libs/caramba-core/go.mod:61`. |
| `cor-9` | `03-WIRE.md` 2.3, Rust | The licensing code cited as the precedent to reuse uses the forbidden API: `VerifyingKey::from_bytes` at `libs/caramba-shared/src/license.rs:205` and `verifying_key.verify(...)` at `:225`. |

Two further items are carried by `02-SPEC.md` and honored here rather than re-argued. `02-SPEC.md` Correction 5 requires the bootstrap blob to be regenerated with a conforming 20-character enrollment code whose first eight characters are `link_pin[0..8]`; the corpus does that in `pos-b1-min`, with code `49Q8-M87P-KQZ3-WFDG-ZTJX`, and keeps the published bytes as the reference entry `pos-b1-wire85`. `02-SPEC.md` Correction 4 replaces the no-relay sentinel `"NO"`, which is Norway, with `"--"`; `pos-m1-norelay` carries `"--"` and a verifier MUST accept it and MUST NOT treat it as a country. **Both are now settled in every document of the set**: `03-WIRE.md` 8.3 and 7.2, and `06-MIGRATION.md` 4.5 and 7.6, were the remaining sites carrying the retracted forms and have been amended, so a signer built from any document in the set now emits what this corpus expects.

**One generator decision that was undocumented and is now derived.** The six armored negatives, `arm-neg-mixed-bid`, `arm-neg-missing-ordinal`, `arm-neg-count-disagree`, `arm-neg-short-nonfinal`, `arm-neg-illegal-character` and `arm-neg-nonzero-pad-bits`, all carry `E_PARSE_FRAMING`, including the base32 alphabet violation that never reaches framing. No error registry covered any of them when they were written, so the assignment was a generator choice. `03-WIRE.md` 6.6 and 10.3 now state the rule the corpus was already following: every failure of the armored reader maps to `E_PARSE_FRAMING`, there is no `E_ARMOR_*` family, and the specific condition belongs in the reader's log message rather than in its code. The corpus is unchanged; what changed is that the choice is now derivable from a document rather than from this file.

Nothing else in `03-WIRE.md` proved unimplementable. The strict CBOR profile of section 3 encoded cleanly with no ambiguity and no discretionary choices, which is the result section 3 claims and which the encoder in `gen/cbor.go` now demonstrates rather than asserts.

---

## 9. Adding a vector

1. Add the construction to `gen/positive.go` or `gen/negative.go`. Build negatives by splicing into encoder output with `mut`, which panics if its pattern is absent or ambiguous, so a fixture cannot silently stop testing what it was written to test.
2. Give it an `id` in the existing scheme: `pos-<doctype>-<what>` or `neg-<layer>-<what>`, where layer is `parse`, `cbor`, `env`, `field`, `verify` or `seal`.
3. Write the `note` for the implementer who has only this directory. Say what the fixture does, which rule decides it, and what a wrong implementation would do instead. The notes are the documentation of the corpus and they are worth more than the bytes.
4. Run `go run .`, confirm the five published digests still match, and update the aggregate digest in section 3 of this file in the same commit.
5. Add the vector to all three harnesses in the same change, or the gate has a hole in it for as long as the follow-up takes.

---

## 10. Known gaps in this corpus

Two things a harness cannot check here, named so nobody assumes coverage that does not exist.

**No ECDSA fixture for the device write proof.** `03-WIRE.md` 13.6 defines `X-CSM-Proof` as an ECDSA P-256 signature by the device signing key, and the review pass fixed the three things about it that would otherwise have diverged: the signature is over the **message**, hashed with SHA-256 by the signing operation itself rather than pre-hashed; the encoding is a fixed 64-byte `r || s` with low-`s` normalization, not ASN.1 DER; and the signed `path` is a canonical literal independent of which of the panel's two API mounts the request arrived on. What the corpus cannot ship is a signature fixture, because ECDSA signing is randomized and a deterministic one would need an RFC 6979 implementation this generator does not have. `03-WIRE.md` 13.6 carries a worked **pre-image** instead, 71 bytes with its SHA-256, which is the part three implementations actually disagree on. A harness SHOULD assert its own construction of that pre-image against the published bytes.

**No fixture for the enrollment or rekey bodies.** `03-WIRE.md` 13.8 gives the enrollment request body a field table under the same strict CBOR profile as everything else here, and `02-SPEC.md` 10.3 puts the rekey message in non-critical key 64 of a `want` map. Neither is a frame, so neither is in `vectors`, and a harness that wants coverage builds it from `fixture_keys` and the strict encoder in `gen/cbor.go`. `pos-m1-noncritical-key` is the closest thing here: it proves a v1 verifier ignores key 64 rather than rejecting the document, which is the property the rekey path depends on.

---

## Changelog

One review pass, 2026-09-02, over the whole specification set. **The corpus itself did not change**: the generator was re-run and reproduces byte-identically, the aggregate digest in section 3 is unchanged at `b36a2e11704936400f2cedb21d5cbc59c4f31c9fefad6ce90429e5552e12512b`, and all five published digests still match. What changed is that several of this file's corrections are now closed by the documents they were raised against, and this file says so:

- `cor-3`'s open half is closed by `03-WIRE.md` step **P12**, which validates key material inside a key document and returns `E_PARSE_FIELD`, exactly as recommended here.
- `cor-4` is closed by `03-WIRE.md` **`DOC_FRAME_MAX`** and by the size-derived `?since=` rule.
- `cor-1`, `cor-2` and `cor-5` are corrected in place in `03-WIRE.md` 8.2, 9.6 and 8.2.1, the last with this corpus's independent measurement printed beside the document's own and both marked as fixture measurements rather than protocol constants.
- The `--` sentinel and the conforming enrollment code, which this corpus already carried against the documents, are now what every document in the set says.
- The armored-reader error-code assignment, previously an undocumented generator choice, is now derivable from `03-WIRE.md` 6.6 and 10.3.
- Section 5 notes that V9 is inert for `0x02` and `0x04`, section 6.1 notes that `pos-m1-max` is a parser-limit fixture and not a delivery precedent, and section 10 is new and names the two gaps this corpus does not cover.
