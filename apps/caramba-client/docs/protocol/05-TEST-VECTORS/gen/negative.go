package main

// The negative corpus. Every fixture here is produced by splicing bytes into
// output from the strict encoder, never by weakening the encoder, so there is
// exactly one code path that emits conforming bytes.

import (
	"crypto/ecdh"
	"fmt"
	"strings"
)

func minDirectiveMap(ver uint64) *cmap {
	return buildDirective(directiveOpts{
		pid: pid, ver: ver, iat: fixIAT, nonce: nonce, dtp: dtp, st: 3,
		cat: frameSHA(catMinFrame), cn: 1, tier: fixTier,
		cap: []byte{0, 0, 0, 3}, ttl: fixTTL, loc: loc,
	})
}

func minCatalogMap(ver, tier uint64) *cmap {
	return buildCatalog(catalogOpts{
		pid: pid, ver: ver, iat: fixIAT, tier: tier,
		ex:  []node{nodeVlessReality("n17i3", "\U0001F1E9\U0001F1EA Stealth", "DE", "de1.exa-nodes.net", 443, "6ba85179")},
		cap: []byte{0, 0, 0, 3}, ttl: fixTTL, jit: fixJit,
		thr: [3]uint64{8192, 22, 4096}, pb: [2]uint64{0, 3},
	})
}

// signDir re-signs a mutated directive payload. The signature is valid over
// the mutated bytes, so the fixture tests the decoder and not the signer.
func signDir(payload []byte) []byte { return buildFrame(dtDirective, payload, []signer{online}) }

func buildAnchors() {
	rev := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 1, iat: fixIAT,
		keys:   []keyEntry{{root.kid, root.pk}, {online.kid, online.pk}},
		roles:  []roleSpec{{1, [][]byte{root.kid}, 1}, {2, [][]byte{online.kid}, 1}},
		revKid: [][]byte{online.kid},
	})
	put("bin/anchors/k1_rev_online.bin", buildFrame(dtKey, encode(rev), []signer{root}))

	thr2 := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 1, iat: fixIAT,
		keys:  []keyEntry{{root.kid, root.pk}, {online.kid, online.pk}, {online2.kid, online2.pk}},
		roles: []roleSpec{{1, [][]byte{root.kid}, 1}, {2, [][]byte{online.kid, online2.kid}, 2}},
	})
	put("bin/anchors/k1_online_thr2.bin", buildFrame(dtKey, encode(thr2), []signer{root}))

	// A tenant that has published a root role and no online role at all. It
	// is legal under every field rule of 03-WIRE.md 8.1, and it is the only
	// way E_VERIFY_ROLE is reachable at step V3.
	rootOnly := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 1, iat: fixIAT,
		keys:  []keyEntry{{root.kid, root.pk}},
		roles: []roleSpec{{1, [][]byte{root.kid}, 1}},
	})
	put("bin/anchors/k1_root_only.bin", buildFrame(dtKey, encode(rootOnly), []signer{root}))
}

func buildNegatives() {
	dp := encode(minDirectiveMap(fixDirV))
	base := signDir(dp)

	// ------------------------------------------------ frame, steps P1..P8
	neg("neg-parse-short", "parse_short.bin", dtDirective, clone(base[:7]),
		"E_PARSE_SHORT", "P1", "Seven bytes. total_len >= 8 fails before anything is read out of the header.")

	badMagic := clone(base)
	badMagic[3] = 0x32 // "CSM2"
	neg("neg-parse-magic", "parse_magic.bin", dtDirective, badMagic,
		"E_PARSE_MAGIC", "P2", "Magic changed to CSM2. The magic is inside the signing pre-image, so a future version can never have its signatures replayed under this one.")

	for _, dt := range []struct {
		v    byte
		name string
		why  string
	}{
		{0x00, "parse_doctype_00", "doc_type 0x00 is the invalid value"},
		{0x07, "parse_doctype_07", "doc_type 0x07 is the reserved timestamp role of 01-DECISION.md A2 and is not emitted in v1"},
		{0xf0, "parse_doctype_f0", "doc_type 0xF0 is private experimentation space and MUST NOT be emitted"},
	} {
		f := clone(base)
		f[4] = dt.v
		neg("neg-"+strings.ReplaceAll(dt.name, "_", "-"), dt.name+".bin", int(dt.v), f,
			"E_PARSE_DOCTYPE", "P3", dt.why+". An unknown doc_type is a parse failure, not a verification failure: the verifier cannot resolve which role should have signed it and guessing is the confusion the domain separator exists to prevent.")
	}

	lenZero := clone(base)
	lenZero[5], lenZero[6] = 0x00, 0x00
	neg("neg-parse-len-zero", "parse_len_zero.bin", dtDirective, lenZero,
		"E_PARSE_LEN", "P4", "payload_len declared 0. The legal range is 1..49152 inclusive.")

	lenOver := clone(base)
	lenOver[5], lenOver[6] = 0xc0, 0x01 // 49153
	neg("neg-parse-len-over", "parse_len_over.bin", dtDirective, lenOver,
		"E_PARSE_LEN", "P4", "payload_len declared 49153, one above the cap. A reader MUST reject on the header, before allocating a buffer of the declared size.")

	neg("neg-parse-short-body", "parse_short_body.bin", dtDirective, clone(base[:100]),
		"E_PARSE_SHORT", "P5", "Header declares payload_len 144 but only 93 payload bytes follow. This is the truncated-response case a captive portal produces, and it MUST advance the ladder rather than raise a tampering claim.")

	nsigs0 := clone(base[:7+len(dp)+1])
	nsigs0[7+len(dp)] = 0x00
	neg("neg-parse-nsigs-zero", "parse_nsigs_zero.bin", dtDirective, nsigs0,
		"E_PARSE_NSIGS", "P6", "nsigs is 0. The legal range is 1..4.")

	four := buildFrame(dtDirective, dp, []signer{online, online2, root, rootB})
	five := clone(four)
	five[7+len(dp)] = 0x05
	five = append(five, five[len(five)-76:]...)
	neg("neg-parse-nsigs-five", "parse_nsigs_five.bin", dtDirective, five,
		"E_PARSE_NSIGS", "P6", "Five signature slots present and nsigs = 5, so the framing arithmetic is exact and only the 1..4 range check catches it. The cap exists so a hostile frame cannot force 255 Ed25519 verifications.")

	neg("neg-parse-framing-trailing", "parse_framing_trailing.bin", dtDirective, append(clone(base), 0x00),
		"E_PARSE_FRAMING", "P7", "One 0x00 appended after the last signature slot. Trailing bytes are a rejection, never an ignored suffix, and this is checked before any signature work. 01-DECISION.md invariant 2 and graft C4.")

	neg("neg-parse-framing-truncated", "parse_framing_truncated.bin", dtDirective, clone(base[:len(base)-1]),
		"E_PARSE_FRAMING", "P7", "One byte removed from the end. total_len is 227 where 7 + 144 + 1 + 76 = 228.")

	inflated := clone(base)
	inflated[7+len(dp)] = 0x02
	neg("neg-parse-nsigs-inflated", "parse_nsigs_inflated.bin", dtDirective, inflated,
		"E_PARSE_FRAMING", "P7", "nsigs claims 2 with one slot present. This is the attack the exact-length rule exists to stop, and it is why nsigs can safely be left outside the signing pre-image: it cannot be raised without changing total_len.")

	dual := buildFrame(dtCatalog, encode(minCatalogMap(11, 5)), []signer{online, online2})
	pl := len(dual) - 84 - 76
	s0 := clone(dual[8+pl : 8+pl+76])
	s1 := clone(dual[8+pl+76 : 8+pl+152])
	swapped := clone(dual)
	copy(swapped[8+pl:], s1)
	copy(swapped[8+pl+76:], s0)
	negCtx("neg-parse-slotorder-desc", "parse_slotorder_desc.bin", dtCatalog, swapped,
		"E_PARSE_SLOTORDER", "P8", "online_thr2", nil,
		"Two valid signatures, slots in descending keyid_trunc order. A verifier MUST reject rather than sort: ordering is what makes a frame with a given signer set byte-unique, which is what allows a catalog to be content-addressed.")

	dupSlot := clone(dual)
	copy(dupSlot[8+pl+76:], s0)
	negCtx("neg-parse-slotorder-dup", "parse_slotorder_dup.bin", dtCatalog, dupSlot,
		"E_PARSE_SLOTORDER", "P8", "online_thr2", nil,
		"The same slot twice. A verifier MUST reject rather than count it twice toward the threshold; counting it twice turns a two-of-two role into a one-of-two role.")

	// ------------------------------------------------ CBOR, step P9
	cb := func(id, name string, payload []byte, note string) {
		neg("neg-cbor-"+id, "cbor_"+name+".bin", dtDirective, signDir(payload), "E_PARSE_CBOR", "P9", note)
	}

	indefMap := clone(dp)
	indefMap[0] = 0xbf
	indefMap = append(indefMap, 0xff)
	cb("indefinite-map", "indefinite_map", indefMap,
		"Top-level map opened with 0xbf and closed with 0xff. Rule C3: definite lengths only.")

	cb("indefinite-bstr", "indefinite_bstr",
		mut(dp, append([]byte{0x0a, 0x50}, nonce...), append(append([]byte{0x0a, 0x5f, 0x50}, nonce...), 0xff)),
		"The nonce byte string re-encoded as an indefinite-length chunked bstr. Rule C3 applies to strings, not only to containers.")

	cb("nonminimal-uint", "nonminimal_uint", mut(dp, []byte{0x01, 0x01}, []byte{0x01, 0x18, 0x01}),
		"v = 1 encoded as 0x18 0x01 instead of 0x01. Rule C4: values 0..23 MUST be inline.")

	cb("nonminimal-len", "nonminimal_len", mut(dp, []byte{0x02, 0x48}, []byte{0x02, 0x58, 0x08}),
		"The 8-byte pid string headed 0x58 0x08 instead of 0x48. Rule C4 governs length arguments as well as values, and this is the counterexample 03-WIRE.md 3.4 names.")

	dup := clone(dp)
	dup[0] = 0xaf
	dup = append(dup, 0x01, 0x01)
	cb("duplicate-key", "duplicate_key", dup,
		"Key 1 appears twice, the second occurrence appended after key 25. Rule C10 catches it as an ordering violation, which is why duplicate detection cannot be forgotten independently.")

	// dp is: ae | 01 01 | 02 48 <pid 8> | 03 ... ; transpose the first two pairs.
	unsorted := []byte{0xae}
	unsorted = append(unsorted, dp[3:13]...) // key 2, the pid pair
	unsorted = append(unsorted, dp[1:3]...)  // key 1, the v pair
	unsorted = append(unsorted, dp[13:]...)
	cb("unsorted-keys", "unsorted_keys", unsorted,
		"Keys 1 and 2 transposed, so the map opens with key 2. Rule C10 requires strictly ascending order and forbids satisfying it by sorting after decode.")

	cb("text-key", "text_key", mut(dp, []byte{0x0c, 0x03}, []byte{0x62, 0x73, 0x74, 0x03}),
		"The status pair rewritten with the text key \"st\". Rule C9: map keys MUST be unsigned integers.")

	cb("negative-int", "negative_int", mut(dp, []byte{0x0c, 0x03}, []byte{0x0c, 0x20}),
		"st carries the negative integer -1. Rule C8: major type 1 is rejected in v1, in any position.")

	cb("tag", "tag", mut(dp, []byte{0x03, 0x19, 0x01, 0x9c}, []byte{0x03, 0xc2, 0x42, 0x01, 0x9c}),
		"ver wrapped in tag 2, the bignum tag. Rule C5 rejects tags in any position, tag 2 and tag 3 included, so a decoder cannot be steered into arbitrary-precision arithmetic.")

	cb("float", "float", mut(dp, []byte{0x0c, 0x03}, []byte{0x0c, 0xf9, 0x42, 0x00}),
		"st carries the half-precision float 3.0. Rule C6.")

	cb("null", "null", mut(dp, []byte{0x0c, 0x03}, []byte{0x0c, 0xf6}),
		"st carries CBOR null. Rule C7 permits only 0xf4 and 0xf5, which is why the reset-to-default state is the text sentinel \"default\" and not a null (03-WIRE.md 8.3).")

	cb("simple-other", "simple_other", mut(dp, []byte{0x0c, 0x03}, []byte{0x0c, 0xf8, 0x20}),
		"st carries simple value 32. Rule C7.")

	badUTF := clone(dp)
	i := indexOf(badUTF, []byte(loc))
	badUTF[i] = 0xff
	cb("invalid-utf8", "invalid_utf8", badUTF,
		"The first byte of the loc text string replaced by 0xff, which is not a legal UTF-8 lead byte. Rule C11 requires validation and forbids substituting replacement characters, because a substituting decoder makes two different byte strings compare equal.")

	cb("trailing-in-payload", "trailing_in_payload", append(clone(dp), 0x00),
		"One 0x00 inside payload_len, after the top-level map ends. Rule C2: the item MUST consume exactly payload_len bytes. This is distinct from neg-parse-framing-trailing, which appends after the signature slots.")

	cb("toplevel-array", "toplevel_array", []byte{0x81, 0x01},
		"Payload is an array, not a map. Rule C1.")

	cb("depth-7", "depth_7", []byte{0xa2, 0x01, 0x01, 0x18, 0x40, 0x81, 0x81, 0x81, 0x81, 0x81, 0x81, 0x01},
		"Six arrays nested inside the top-level map, which is depth 7. Rule C12 caps depth at 6 with the top-level map counted as depth 1. The envelope is also incomplete, but P9 runs before P10 so the expected code is the CBOR one.")

	keyZero := clone(dp)
	keyZero[0] = 0xaf
	keyZero = append([]byte{0xaf, 0x00, 0x01}, dp[1:]...)
	cb("key-zero", "key_zero", keyZero,
		"Map key 0, in ascending position. 03-WIRE.md 3.3: key 0 MUST be rejected. A decoder can decide this without knowing the doc_type, which is why the code is the CBOR one and not the field one.")

	keyBig := clone(dp)
	keyBig[0] = 0xaf
	keyBig = append(keyBig, 0x19, 0x04, 0x00, 0x01)
	cb("key-1024", "key_1024", keyBig,
		"Map key 1024, above the non-critical range. 03-WIRE.md 3.3.")

	big65 := []byte{0xb8, 0x41}
	for k := 1; k <= 65; k++ {
		big65 = append(big65, head(0, uint64(k))...)
		big65 = append(big65, 0x01)
	}
	cb("map-65-pairs", "map_65_pairs", big65,
		"A 65-pair map. MAX_MAP_PAIRS is 64.")

	long := append([]byte{0xa2, 0x01, 0x01, 0x18, 0x40, 0x79, 0x01, 0x01}, []byte(strings.Repeat("x", 257))...)
	cb("tstr-257", "tstr_257", long,
		"A 257-byte text string in non-critical key 64. MAX_TSTR_BYTES is 256, and the limit applies everywhere, including in a key a v1 decoder would otherwise ignore.")

	bigb := append([]byte{0xa2, 0x01, 0x01, 0x18, 0x40, 0x59, 0x0c, 0x01}, make([]byte, 3073)...)
	cb("bstr-3073", "bstr_3073", bigb,
		"A 3073-byte byte string. MAX_BSTR_BYTES is 3072, sized to admit a full catalog chunk payload of 2816 bytes and the padding field, and nothing larger.")

	cb("uint-over-max", "uint_over_max",
		mut(dp, []byte{0x04, 0x1a, 0x6a, 0x97, 0x67, 0x00}, []byte{0x04, 0x1b, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00}),
		"iat set to 2^53. MAX_UINT is 2^53 - 1 so that a Dart or JavaScript-hosted decoder never silently loses precision.")

	// ------------------------------------------------ envelope, step P10
	env := func(id, name string, payload []byte, note string) {
		neg("neg-env-"+id, "env_"+name+".bin", dtDirective, signDir(payload), "E_PARSE_ENVELOPE", "P10", note)
	}
	env("v2", "v2", mut(dp, []byte{0x01, 0x01}, []byte{0x01, 0x02}),
		"v = 2. A v1 verifier MUST reject rather than attempt a best-effort parse; a spec_version bump is a re-review, not a patch (01-DECISION.md section 6).")

	noExp := clone(dp)
	noExp = mut(noExp, []byte{0x05, 0x1a, 0x6a, 0x97, 0x75, 0x10}, nil)
	noExp[0] = 0xad
	env("missing-exp", "missing_exp", noExp,
		"exp removed and the map head reduced to 13 pairs. Envelope keys 1..5 are mandatory in every document of every type.")

	shortPid := mut(dp, append([]byte{0x02, 0x48}, pid...), append([]byte{0x02, 0x47}, pid[:7]...))
	env("pid-7-bytes", "pid_7_bytes", shortPid,
		"pid encoded as a 7-byte string. The cap is exactly 8; a length-tolerant verifier would let a prefix of one tenant's pid match another's.")

	// ------------------------------------------------ field, step P11
	fld := func(id, name string, dtype int, f []byte, note string) {
		neg("neg-field-"+id, "field_"+name+".bin", dtype, f, "E_PARSE_FIELD", "P11", note)
	}

	noLoc := minDirectiveMap(fixDirV)
	noLocEnc := encode(noLoc)
	noLocEnc = mut(noLocEnc, append([]byte{0x18, 0x19, 0x78, 0x18}, []byte(loc)...), nil)
	noLocEnc[0] = 0xad
	fld("missing-loc", "missing_loc", dtDirective, signDir(noLocEnc),
		"The mandatory loc field removed. Without it the client cannot re-fetch, and a document that omits a mandatory field MUST be rejected rather than defaulted.")

	crit := minDirectiveMap(fixDirV)
	crit.set(8, u(1))
	fld("unknown-critical-key", "unknown_critical_key", dtDirective, signDir(encode(crit)),
		"Key 8, which 03-WIRE.md 8.0 reserves in the critical range and forbids in v1. Critical-range keys fail closed: an old client refuses rather than ignores, which is what lets a future security-relevant field be added safely. See correction cor-3 on which error code this is.")

	padBad := minDirectiveMap(fixDirV)
	nz := make([]byte, 16)
	nz[7] = 0x01
	padBad.set(9, b(nz))
	fld("pd-nonzero", "pd_nonzero", dtDirective, signDir(encode(padBad)),
		"pd contains a non-zero byte. A decoder MUST ignore pd's contents but MUST reject a non-zero one, so a hostile signer cannot use the padding field as a covert channel (03-WIRE.md 12.1).")

	kidBad := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 2, iat: fixIAT,
		keys:  []keyEntry{{flip(root.kid, 11), root.pk}, {online.kid, online.pk}},
		roles: []roleSpec{{1, [][]byte{flip(root.kid, 11)}, 1}, {2, [][]byte{online.kid}, 1}},
	})
	fld("kid-mismatch", "kid_mismatch", dtKey, buildFrame(dtKey, encode(kidBad), []signer{root}),
		"A key entry whose kid is not sha256(pk)[0..12]. A verifier MUST recompute and MUST reject; carrying kid unchecked would buy an attacker a free alias for a key the role table authorizes.")

	unref := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 2, iat: fixIAT,
		keys:  []keyEntry{{root.kid, root.pk}, {online.kid, online.pk}, {stranger.kid, stranger.pk}},
		roles: []roleSpec{{1, [][]byte{root.kid}, 1}, {2, [][]byte{online.kid}, 1}},
	})
	fld("key-unreferenced", "key_unreferenced", dtKey, buildFrame(dtKey, encode(unref), []signer{root}),
		"A key present in keys but named by no role. Role lives only in roles, so an unreferenced key is a key with no role, and 01-DECISION.md 5.1.3 forbids any path that yields a key without one.")

	thrBad := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 2, iat: fixIAT,
		keys:  []keyEntry{{root.kid, root.pk}, {online.kid, online.pk}},
		roles: []roleSpec{{1, [][]byte{root.kid}, 2}, {2, [][]byte{online.kid}, 1}},
	})
	fld("thr-exceeds-ks", "thr_exceeds_ks", dtKey, buildFrame(dtKey, encode(thrBad), []signer{root}),
		"roles[1] has thr = 2 over a one-key set. The cap is thr <= len(ks); a threshold that can never be met bricks the profile at the next rotation.")

	// Chunk with a short non-final slice.
	twoChunk := chunkPayloads(pid, 8, fixIAT, catTypFrame)
	shortChunk := envelope(pid, 8, fixIAT, lifeCatalog)
	shortChunk.set(10, b(frameSHA(catTypFrame)[:10]))
	shortChunk.set(11, u(0))
	shortChunk.set(12, u(uint64(len(twoChunk))))
	shortChunk.set(13, u(uint64(len(catTypFrame))))
	shortChunk.set(14, b(catTypFrame[:2000]))
	fld("chunk-short-nonfinal", "chunk_short_nonfinal", dtChunk,
		buildFrame(dtChunk, encode(shortChunk), []signer{online}),
		fmt.Sprintf("Chunk 0 of %d carrying 2000 bytes where every chunk but the last MUST carry exactly %d. Accepting it would let a mirror choose the reassembly offsets.", len(twoChunk), chunkPayloadMax))

	idBad := minCatalogMap(13, 6)
	fld("node-id-charset", "node_id_charset", dtCatalog,
		buildFrame(dtCatalog, mut(encode(idBad), []byte("n17i3"), []byte("n17 3")), []signer{online}),
		"A node id containing a space. The charset is [0-9A-Za-z_-] and the field is a closed vocabulary the client persists and echoes; invariant 11 forbids persisting an operator-supplied value that is not validated against one.")

	upperHost := buildCatalog(catalogOpts{
		pid: pid, ver: 14, iat: fixIAT, tier: 7,
		ex:  []node{nodeVlessReality("n17i3", "\U0001F1E9\U0001F1EA Stealth", "DE", "de1.exa-nodes.net", 443, "6ba85179")},
		cap: []byte{0, 0, 0, 3}, ttl: fixTTL, jit: fixJit,
		thr: [3]uint64{8192, 22, 4096}, pb: [2]uint64{0, 3},
		mir: []mirror{{h: "M1.Example-Cdn.net", sni: "M1.Example-Cdn.net", pin: [][]byte{rng(0x20, 32)}, asn: 24940, cc: "DE"}},
	})
	fld("host-uppercase", "host_uppercase", dtCatalog,
		buildFrame(dtCatalog, encode(upperHost), []signer{online}),
		"A mirror hostname with uppercase letters. A verifier MUST reject rather than lowercase it, so two spellings of one host cannot produce two chash values for one catalog (03-WIRE.md 14.1).")

	for _, pc := range []struct{ id, name, path, why string }{
		{"path-double-slash", "path_double_slash", "//evil.example.net/dns", "a second leading slash makes it a scheme-relative URL pointing at an authority the pool never authorized"},
		{"path-dotdot", "path_dotdot", "/a/../dns-query", "a complete .. segment escapes the path the operator published"},
		{"path-pct2f", "path_pct2f", "/dns%2fquery", "a percent-encoded slash can be decoded into a segment separator after validation"},
	} {
		bad := buildCatalog(catalogOpts{
			pid: pid, ver: 15, iat: fixIAT, tier: 8,
			ex:  []node{nodeVlessReality("n17i3", "\U0001F1E9\U0001F1EA Stealth", "DE", "de1.exa-nodes.net", 443, "6ba85179")},
			cap: []byte{0, 0, 0, 3}, ttl: fixTTL, jit: fixJit,
			thr: [3]uint64{8192, 22, 4096}, pb: [2]uint64{0, 3},
			mir: []mirror{fixMirror()},
			doh: []dohEntry{{h: "m1.example-cdn.net", p: pc.path, ip: []string{"198.51.100.7"}, pin: [][]byte{rng(0x40, 32)}}},
		})
		fld(pc.id, pc.name, dtCatalog, buildFrame(dtCatalog, encode(bad), []signer{online}),
			"A DoH path field of \""+pc.path+"\": "+pc.why+". 03-WIRE.md 14.2.")
	}

	stBad := minDirectiveMap(fixDirV)
	fld("st-out-of-vocabulary", "st_out_of_vocabulary", dtDirective,
		signDir(mut(encode(stBad), []byte{0x0c, 0x03}, []byte{0x0c, 0x09})),
		"st = 9, outside the closed vocabulary of eight values. Unlike rc, which is deliberately open-ended extension space, st is a critical-range closed vocabulary and a value outside it is a parse failure (03-WIRE.md section 5).")

	capBad := minDirectiveMap(fixDirV)
	fld("cap-wrong-length", "cap_wrong_length", dtDirective,
		signDir(mut(encode(capBad), []byte{0x11, 0x44, 0x00, 0x00, 0x00, 0x03}, []byte{0x11, 0x43, 0x00, 0x00, 0x03})),
		"cap encoded as a 3-byte string. The cap is exactly 4, and a short bitfield would silently clear the high capability bits rather than fail.")

	flZero := minCatalogMap(16, 9)
	fld("fl-zero-emitted", "fl_zero_emitted", dtCatalog,
		buildFrame(dtCatalog, mut(encode(flZero), []byte{0x0d, 0x01}, []byte{0x0d, 0x00}), []signer{online}),
		"A node carrying fl = 0 explicitly. Value 0 of the fl enum means \"absent; the renderer MUST omit the key entirely, not emit an empty string\", so emitting the key at all is the error. The panel comment at subscription_generator.rs:230-232 records that an empty flow breaks Happ, and the Go renderer must reproduce the omission.")

	// ------------------------------------------------ verification, V1..V14
	vr := func(id, name string, dtype int, f []byte, code, step, context string, over *ctx, note string) {
		negCtx("neg-verify-"+id, "verify_"+name+".bin", dtype, f, code, step, context, over, note)
	}

	kdWrong := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 2, iat: fixIAT,
		keys:  []keyEntry{{root.kid, root.pk}, {online.kid, online.pk}},
		roles: []roleSpec{{1, [][]byte{root.kid}, 1}, {2, [][]byte{online.kid}, 1}},
	})
	vr("keydoc-signed-by-online", "keydoc_signed_by_online", dtKey,
		buildFrame(dtKey, encode(kdWrong), []signer{online}),
		"E_VERIFY_UNAUTHORIZED", "V4", "default", nil,
		"A key document signed by the ONLINE key. This is the attack the role table exists to stop: an attacker holding the online key mints a key document at N+1 naming only their own key under role root, the high-water mark advances, and the operator can never recover. The role is resolved from doc_type and the key set is read from the PREVIOUSLY TRUSTED document, so the online kid is simply not in roles[1].ks.")

	vr("catalog-signed-by-root", "catalog_signed_by_root", dtCatalog,
		buildFrame(dtCatalog, encode(minCatalogMap(17, 10)), []signer{root}),
		"E_VERIFY_UNAUTHORIZED", "V4", "default", nil,
		"A catalog signed by the ROOT key. The wrong-role pair in the other direction: root is a real, trusted, non-revoked key of this tenant, and it is still not authorized for doc_type 0x02.")

	vr("directive-signed-by-root", "directive_signed_by_root", dtDirective,
		buildFrame(dtDirective, encode(minDirectiveMap(440)), []signer{root}),
		"E_VERIFY_UNAUTHORIZED", "V4", "default", nil,
		"A directive signed by the root key.")

	vr("signed-by-stranger", "signed_by_stranger", dtCatalog,
		buildFrame(dtCatalog, encode(minCatalogMap(18, 11)), []signer{stranger}),
		"E_VERIFY_UNAUTHORIZED", "V4", "default", nil,
		"Signed by a well-formed key that is in no role of the trusted document. The frame MUST be rejected whole; an unauthorized slot is never skipped, because a frame carrying one is a frame someone tried to launder.")

	vr("revoked-signer", "revoked_signer", dtCatalog,
		buildFrame(dtCatalog, encode(minCatalogMap(19, 12)), []signer{online}),
		"E_VERIFY_REVOKED", "V5", "rev_online", nil,
		"A validly signed catalog whose signer appears in the anchor's rev.kids. A client that sees a kid in rev MUST reject that key and every document it signed, cached copies on disk included, immediately.")

	vr("below-threshold", "below_threshold", dtCatalog,
		buildFrame(dtCatalog, encode(minCatalogMap(20, 13)), []signer{online}),
		"E_VERIFY_THRESHOLD", "V7", "online_thr2", nil,
		"One valid online signature against a role whose thr is 2. The signature verifies, the signer is authorized and not revoked, and the document is still rejected.")

	flipped := clone(base)
	flipped[40] ^= 0x01
	vr("signature-payload-bitflip", "signature_payload_bitflip", dtDirective, flipped,
		"E_VERIFY_SIG", "V6", "default", nil,
		"One bit flipped inside the payload after signing. The frame parses cleanly, so this exercises the signature check and nothing else.")

	badS := clone(base)
	sOff := len(base) - 32
	sPlus := addL(badS[sOff:])
	if sPlus != nil {
		copy(badS[sOff:], sPlus)
		vr("noncanonical-s", "noncanonical_s", dtDirective, badS,
			"E_VERIFY_SIG", "V6", "default", nil,
			"S replaced by S + L, which is the classic malleability case. A verifier that reduces S mod L instead of rejecting turns one signature into several, and a catalog is content-addressed by sha256 of the whole frame, so several spellings of one signature are several catalogs.")
	}

	pidBad := minDirectiveMap(441)
	vr("pid-mismatch", "pid_mismatch", dtDirective,
		signDir(mut(encode(pidBad), append([]byte{0x02, 0x48}, pid...), append([]byte{0x02, 0x48}, pidOf(rootB.pk)...))),
		"E_VERIFY_PID", "V8", "default", nil,
		"A validly signed directive carrying another tenant's pid. Two profiles for two operators can and do live on one device, so the pinned-pid equality check is what stops operator A's directive being applied to operator B's profile.")

	vr("version-regression", "version_regression", dtDirective,
		signDir(encode(minDirectiveMap(410))),
		"E_VERIFY_VERSION", "V9", "default", nil,
		"ver = 410 against a stored high-water mark of 411. Below the mark is refused outright.")

	vr("version-equal-different-bytes", "version_equal_different_bytes", dtCatalog,
		buildFrame(dtCatalog, encode(minCatalogMap(7, 99)), []signer{online}),
		"E_VERIFY_VERSION", "V9", "default",
		&ctx{HWM: map[string]uint64{"2": 7}, StoredFrame: "bin/positive/c1_min.bin",
			Note: "The stored frame at this version is c1_min.bin. This document carries the same ver with a different tier and therefore different bytes."},
		"Same ver as the stored document, different bytes. 03-WIRE.md Correction 7 makes the equality case explicit: equal is accepted ONLY when the frame is byte-identical to the stored frame, which is what lets a client re-read its own cache without weakening the monotonic rule.")

	rotSkip := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 3, iat: fixIAT,
		keys:  []keyEntry{{root.kid, root.pk}, {online.kid, online.pk}},
		roles: []roleSpec{{1, [][]byte{root.kid}, 1}, {2, [][]byte{online.kid}, 1}},
	})
	vr("rotation-skip", "rotation_skip", dtKey,
		buildFrame(dtKey, encode(rotSkip), []signer{root}),
		"E_VERIFY_ROTATION", "V10", "default", nil,
		"A key document at version 3 against a trusted version 1. Clients MUST refuse to skip a version, and the code is E_VERIFY_ROTATION rather than E_VERIFY_VERSION so that a skip is distinguishable from a rollback in the attempt history.")

	rotSingle := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 2, iat: fixIAT + 60,
		keys:  []keyEntry{{rootB.kid, rootB.pk}, {online.kid, online.pk}},
		roles: []roleSpec{{1, [][]byte{rootB.kid}, 1}, {2, [][]byte{online.kid}, 1}},
	})
	vr("rotation-new-key-only", "rotation_new_key_only", dtKey,
		buildFrame(dtKey, encode(rotSingle), []signer{rootB}),
		"E_VERIFY_ROTATION", "V10", "rotation_v1", nil,
		"A rotation to rootB signed only by rootB. It satisfies the key set of the document under verification and not the key set of the currently trusted one. Both MUST pass. Accepting this is exactly the takeover the rotation rule prevents: anyone who can serve bytes could otherwise install their own root.")

	vr("iat-future", "iat_future", dtDirective,
		buildFrame(dtDirective, encode(buildDirective(directiveOpts{
			pid: pid, ver: 442, iat: fixIAT + 86400, nonce: nonce, dtp: dtp, st: 3,
			cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: []byte{0, 0, 0, 3},
			ttl: fixTTL, loc: loc})), []signer{online}),
		"E_VERIFY_IAT", "V11", "default", nil,
		"iat one day in the future against now = iat_fixture + 300. Skew tolerance is 300 seconds in both directions.")

	vr("iat-below-floor", "iat_below_floor", dtDirective,
		buildFrame(dtDirective, encode(buildDirective(directiveOpts{
			pid: pid, ver: 443, iat: fixIAT - 86400, nonce: nonce, dtp: dtp, st: 3,
			cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: []byte{0, 0, 0, 3},
			ttl: fixTTL, loc: loc})), []signer{online}),
		"E_VERIFY_IAT", "V11", "default", nil,
		"iat one day below time_floor. This document had already expired when the profile last heard from the panel, so it is refused under the literal V11 of 03-WIRE.md and under the amended V11 of 02-SPEC.md Correction 2 alike. Contrast pos-c1-stale-but-live, where the two forms disagree.")

	vr("expired", "expired", dtDirective,
		buildFrame(dtDirective, encode(buildDirective(directiveOpts{
			pid: pid, ver: 444, iat: fixIAT - 7200, nonce: nonce, dtp: dtp, st: 3,
			cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: []byte{0, 0, 0, 3},
			ttl: fixTTL, loc: loc})), []signer{online}),
		"E_VERIFY_EXPIRED", "V12", "default", nil,
		"exp is 3300 seconds before now, past the 300 second skew. The rejection means the document is refused for NEW instructions and NEW status only. Invariant 16 is absolute: an expired document MUST NOT disconnect a user, tear down a tunnel or clear a cached configuration, and a harness that treats this fixture as a disconnect trigger has implemented the wrong thing.")

	vr("nonce-mismatch", "nonce_mismatch", dtDirective,
		signDir(mut(encode(minDirectiveMap(445)), append([]byte{0x0a, 0x50}, nonce...),
			append([]byte{0x0a, 0x50}, flip(nonce, 0)...))),
		"E_VERIFY_NONCE", "V13", "default", nil,
		"The echoed nonce is not the one this device just sent. The nonce is the only freshness mechanism that survives a wrong clock, which is the normal case after a factory reset and common when DNS blocking prevents NTP.")

	vr("device-mismatch", "device_mismatch", dtDirective,
		signDir(mut(encode(minDirectiveMap(446)), append([]byte{0x0b, 0x50}, dtp...),
			append([]byte{0x0b, 0x50}, dtpOther...))),
		"E_VERIFY_DEVICE", "V13", "default", nil,
		"dtp names another device. Accepting it would apply another device's selection and status to this one.")

	vr("cathash-mismatch", "cathash_mismatch", dtCatalog,
		buildFrame(dtCatalog, encode(minCatalogMap(21, 14)), []signer{online}),
		"E_VERIFY_CATHASH", "V14", "default",
		&ctx{HWM: map[string]uint64{"2": 6},
			Note: "The trusted directive names cat = sha256(c1_min.bin). This catalog is a different document with a different frame digest."},
		"A validly signed catalog whose sha256 is not the cat the trusted directive named. The directive binds the catalog by hash, so a compromised mirror cannot substitute a different but validly signed catalog; per 01-DECISION.md A1, a compromised ONLINE KEY still can, and that is an accepted risk rather than a defect.")
}

func buildAnchorNegatives() {
	negCtx("neg-verify-role-unresolvable", "verify_role_unresolvable.bin", dtCatalog,
		buildFrame(dtCatalog, encode(minCatalogMap(22, 15)), []signer{online}),
		"E_VERIFY_ROLE", "V3", "root_only", nil,
		"A catalog verified against a trusted key document that publishes no online role. doc_type 0x02 resolves to role online at V1 and the trusted document has no roles[2] to read a key set and a threshold from, so V3 fails. This is the only shape in which E_VERIFY_ROLE is reachable: every defined doc_type has a role in the table, so V1 itself cannot fail, and an unknown doc_type is a parse failure at P3.")

	negCtx("neg-verify-no-anchor", "verify_no_anchor.bin", dtCatalog,
		buildFrame(dtCatalog, encode(minCatalogMap(23, 16)), []signer{online}),
		"E_VERIFY_NOANCHOR", "V2", "first_trust", nil,
		"A catalog offered before any key document has been trusted. link_pin anchors doc_type 0x01 and 0x05 only; there is nothing a catalog can be verified against, and a client MUST NOT bootstrap trust from a catalog. This is what a mirror serving a plausible catalog to a fresh install looks like.")

	blobWrongRoot := buildBlob(blobOpts{
		pid: pid, ver: 1, iat: fixIAT,
		org: "https://panel.example.net", code: enrollCode(), rk: rootB.pk,
		mir: []mirror{fixMirror()}, doh: []dohEntry{fixDoH()}, nm: "Exa Networks",
	})
	negCtx("neg-verify-blob-pin-mismatch", "verify_blob_pin_mismatch.bin", dtBootstrap,
		buildFrame(dtBootstrap, encode(blobWrongRoot), []signer{rootB}),
		"E_VERIFY_UNAUTHORIZED", "V4", "first_trust", nil,
		"A self-consistent bootstrap blob: rk is rootB and the frame is signed by rootB, so every internal check passes. It is rejected because sha256(rootB)[0..12] is not the link_pin the user was dictated. A blob whose rk does not match the dictated pin MUST be rejected with a hard error and there MUST NOT be a continue-anyway path; this fixture is the one that proves a client is checking the pin rather than checking the blob against itself.")
}

func flip(x []byte, i int) []byte {
	out := clone(x)
	out[i] ^= 0xff
	return out
}

func indexOf(hay, needle []byte) int {
	for i := 0; i+len(needle) <= len(hay); i++ {
		ok := true
		for j := range needle {
			if hay[i+j] != needle[j] {
				ok = false
				break
			}
		}
		if ok {
			return i
		}
	}
	panic("indexOf: not found")
}

// addL returns the little-endian encoding of S + L when it still fits in 32
// bytes, and nil otherwise.
func addL(s []byte) []byte {
	v := leToBig(s)
	v.Add(v, edL)
	if v.BitLen() > 256 {
		return nil
	}
	return edScalarEncode(v)
}

// ---------------------------------------------------------------- sealing

func buildSeals() {
	dev := derivePrivP256("csm1-doc-example-device-agree")
	other := derivePrivP256("csm1-doc-example-device-agree-other")
	eph := derivePrivP256("csm1-doc-example-hpke-ephemeral")
	info := []byte(hpkeInfoStr)

	sealFrame := func(ver uint64, recipient *ecdh.PublicKey, aadDtp []byte, inner []byte, outDtp []byte, kem, kdf, aead, rkv uint64, mangleEnc, mangleCt bool, pad bool) []byte {
		aad := sealAAD(pid, aadDtp, uint32(ver))
		enc, ct := hpkeSeal(eph, recipient, info, aad, inner)
		if mangleEnc {
			enc = clone(enc)
			enc[0] = 0x06
		}
		if mangleCt {
			ct = clone(ct)
			ct[20] ^= 0x40
		}
		o := buildSealed(sealedOpts{pid: pid, ver: ver, iat: fixIAT, dtp: outDtp,
			kem: kem, kdf: kdf, aead: aead, enc: enc, ct: ct, rkv: rkv})
		var payload []byte
		if pad {
			payload = padTo(o, dtSealed, 1, 0)
		} else {
			payload = encode(o)
		}
		return buildFrame(dtSealed, payload, []signer{online})
	}

	f := sealFrame(fixDirV, dev.PublicKey(), dtp, dirMinFrame, dtp, 16, 1, 3, 1, false, false, false)
	pos("pos-m1s-min", "m1s_min.bin", dtSealed, f,
		fmt.Sprintf("Sealed directive, unpadded, %d bytes. The plaintext of ct is the COMPLETE 0x03 frame of pos-m1-min, magic and signature slots included; a recipient that recovers a plaintext not beginning 43 53 4d 31 03 MUST treat it as E_SEAL_OPEN and MUST NOT attempt a partial parse. See correction cor-2: 03-WIRE.md 9.6 gives 455 for this frame.", len(f)))
	size("sealed_directive", "unpadded", len(f))

	fp := sealFrame(fixDirV, dev.PublicKey(), dtp, dirMinFrame, dtp, 16, 1, 3, 1, false, false, true)
	pos("pos-m1s-padded", "m1s_padded.bin", dtSealed, fp,
		fmt.Sprintf("The same sealed directive padded onto the 256-byte grid at r = 0: %d bytes. With the default pb of [0,3] the response is drawn per request from a four-value set, and the range itself is per tenant, which is what stops 512 becoming the cross-tenant constant 01-DECISION.md D3 objects to.", len(fp)))
	size("sealed_directive", "padded_r0", len(fp))

	negCtx("neg-seal-wrong-recipient", "seal_wrong_recipient.bin", dtSealed,
		sealFrame(447, other.PublicKey(), dtpOther, dirMinFrame, dtpOther, 16, 1, 3, 1, false, false, false),
		"E_SEAL_RECIPIENT", "seal step 3", "default", nil,
		"Sealed to another device's agreement key and addressed to that device's dtp. The recipient check fires before any asymmetric work. This is a correctness failure, not a security event, and MUST NOT be reported as tampering: it is what a client sees when a mirror serves a cached response for a different device.")

	negCtx("neg-seal-unknown-rkv", "seal_unknown_rkv.bin", dtSealed,
		sealFrame(448, dev.PublicKey(), dtp, dirMinFrame, dtp, 16, 1, 3, 9, false, false, false),
		"E_SEAL_RECIPIENT", "seal step 5", "default", nil,
		"rkv names generation 9, which this device does not hold. The client MUST respond by rekeying its agreement key and re-requesting, per 02-SPEC.md 10.3, not by failing the fetch permanently.")

	negCtx("neg-seal-wrong-kem", "seal_wrong_kem.bin", dtSealed,
		sealFrame(449, dev.PublicKey(), dtp, dirMinFrame, dtp, 32, 1, 3, 1, false, false, false),
		"E_SEAL_SUITE", "seal step 4", "default", nil,
		"kem = 32, which is DHKEM(X25519, HKDF-SHA256), the suite 00-DESIGN-BRIEF.md 4.7 proposed. It MUST be rejected: the device key is P-256 because it has to live in Secure Enclave or StrongBox, and neither holds an X25519 key (03-WIRE.md Correction 4).")

	negCtx("neg-seal-hybrid-enc", "seal_hybrid_enc.bin", dtSealed,
		sealFrame(450, dev.PublicKey(), dtp, dirMinFrame, dtp, 16, 1, 3, 1, true, false, false),
		"E_SEAL_SUITE", "seal step 4", "default", nil,
		"enc is 65 bytes but its leading byte is 0x06, a hybrid point encoding. A sender MUST emit the uncompressed form 0x04 || X || Y; a recipient MUST reject compressed and hybrid encodings and any point not on the curve.")

	negCtx("neg-seal-ct-tampered", "seal_ct_tampered.bin", dtSealed,
		sealFrame(451, dev.PublicKey(), dtp, dirMinFrame, dtp, 16, 1, 3, 1, false, true, false),
		"E_SEAL_OPEN", "seal step 6", "default", nil,
		"One bit flipped inside ct. The outer Ed25519 signature is valid over the tampered outer payload, so this is caught by the AEAD tag and nothing else. It is a security event and MUST be surfaced.")

	// aad tampering: seal under ver 452, then re-sign the outer frame at 453
	// so the recipient recomputes a different aad.
	aadOK := sealAAD(pid, dtp, 452)
	enc, ct := hpkeSeal(eph, dev.PublicKey(), info, aadOK, dirMinFrame)
	o := buildSealed(sealedOpts{pid: pid, ver: 453, iat: fixIAT, dtp: dtp,
		kem: 16, kdf: 1, aead: 3, enc: enc, ct: ct, rkv: 1})
	negCtx("neg-seal-aad-version", "seal_aad_version.bin", dtSealed,
		buildFrame(dtSealed, encode(o), []signer{online}),
		"E_SEAL_OPEN", "seal step 6", "default", nil,
		"The ciphertext was sealed with ver 452 in its aad and the outer payload says 453. A recipient MUST recompute aad from the outer payload's own pid, dtp and ver and MUST NOT accept an aad supplied on the wire, which is what makes this a rejection rather than a silent version downgrade.")

	// Outer opens, inner directive fails its own nonce check.
	innerBad := buildFrame(dtDirective, encode(buildDirective(directiveOpts{
		pid: pid, ver: 454, iat: fixIAT, nonce: flip(nonce, 3), dtp: dtp, st: 3,
		cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: []byte{0, 0, 0, 3},
		ttl: fixTTL, loc: loc})), []signer{online})
	negCtx("neg-seal-inner-nonce", "seal_inner_nonce.bin", dtSealed,
		sealFrame(454, dev.PublicKey(), dtp, innerBad, dtp, 16, 1, 3, 1, false, false, false),
		"E_VERIFY_NONCE", "seal step 7, inner V13", "default", nil,
		"The seal opens correctly and the recovered inner 0x03 frame carries the wrong nonce. The outer verification shortcuts no inner check: the inner frame is parsed and verified in full from P1 through V14, and this fixture is the one that proves a harness actually does that rather than trusting the envelope.")
}

// ---------------------------------------------------------------- armor

func buildArmor() {
	writeArmor := func(id, name string, stream []byte, frames int, carries []string, note string) []string {
		lines := armor(stream)
		armorSelfCheck(lines, stream)
		if got, err := splitFrames(stream); err != nil || len(got) != frames {
			panic("armor: frame stream is not walkable: " + fmt.Sprint(err))
		}
		p := put("armor/"+name, []byte(strings.Join(lines, "\n")+"\n"))
		armors = append(armors, armorVec{ID: id, File: p, Frames: frames,
			Stream: len(stream), Chunks: len(lines), BID: bidOf(stream),
			Verdict: "accept", Carries: carries, Note: note})
		return lines
	}

	single := writeArmor("arm-b1-single", "b1_min.carcap", blobConform, 1,
		[]string{"pos-b1-min"},
		fmt.Sprintf("The bootstrap blob as one armored chunk: %d characters of line for %d bytes of frame. Every character of the format is inside the QR alphanumeric set, so it encodes in alphanumeric mode rather than byte mode, which is roughly 45 percent fewer modules.", len(armor(blobConform)[0]), len(blobConform)))

	snapshot := clone(keyMinFrame)
	snapshot = append(snapshot, catMinFrame...)
	snapshot = append(snapshot, blobConform...)
	snapshot = append(snapshot, rotV2...)
	snapLines := writeArmor("arm-offline-snapshot", "offline_snapshot.carcap", snapshot, 4,
		[]string{"pos-k1-min", "pos-c1-min", "pos-b1-min", "pos-k1-rot-v2"},
		"An offline snapshot: key document, catalog, bootstrap blob and the version 2 key document as one frame stream. A reader walks it using each frame's own payload_len and nsigs and starts the next frame at the byte immediately after; bytes left over after the last complete frame, and a truncated final frame, are both E_PARSE_FRAMING.")

	chain := append(clone(rotV2), rotV3...)
	writeArmor("arm-rotation-chain", "rotation_chain.carcap", chain, 2,
		[]string{"pos-k1-rot-v2", "pos-k1-rot-v3"},
		"The root rotation chain versions 2 and 3, which is what GET /sub/k1?since=1 returns and what lets the out-of-band rung survive a rotation. A client MUST walk the intermediates in order and MUST NOT skip either.")

	wire85 := writeArmor("arm-b1-wire85", "b1_wire_8_5.carcap", blobWireFrame, 1,
		[]string{"pos-b1-wire85"},
		"The armored form of the exact 374-byte blob printed in 03-WIRE.md 8.5, kept so an implementer can diff against the worked example in 03-WIRE.md 10.5. Its bid and its single line are the published ones; see published_digest_check for the comparison.")
	armorPublishedLine = wire85[0]

	writeArmor("arm-b1-max", "b1_max.carcap", blobMaxFrame, 1,
		[]string{"pos-b1-max"},
		fmt.Sprintf("The maximum bootstrap blob across %d chunks. Every chunk but the last carries exactly 620 bytes, which is 4960 bits and therefore exactly 992 base32 characters with no pad bits.", len(armor(blobMaxFrame))))

	negArmor := func(id, name string, lines []string, code, note string) {
		p := put("armor/"+name, []byte(strings.Join(lines, "\n")+"\n"))
		armors = append(armors, armorVec{ID: id, File: p, Frames: 0,
			Chunks: len(lines), Verdict: "reject", Code: code, Note: note})
	}

	if len(snapLines) < 3 {
		panic("armor: the offline snapshot must span at least three chunks for the negative set")
	}
	mixed := append([]string{}, snapLines...)
	mixed[1] = "CARCAP1." + bidOf(blobConform) + mixed[1][16:]
	negArmor("arm-neg-mixed-bid", "neg_mixed_bid.carcap", mixed, "E_PARSE_FRAMING",
		"Chunk 2 carries the bid of a different bundle. A reader MUST reject a set in which any two lines disagree on bid or n. This is the realistic failure: an operator prints a new sheet and the old one is still on the table. The bid is the extension 03-WIRE.md Correction 8 adds to 01-DECISION.md C1, which had no bundle identifier at all.")

	missing := append([]string{}, snapLines[0], snapLines[2])
	negArmor("arm-neg-missing-ordinal", "neg_missing_ordinal.carcap", missing, "E_PARSE_FRAMING",
		"Ordinal 2 of 3 is absent. A reader MUST detect the missing ordinal rather than concatenating what it has, and MUST accept the remaining chunks in any order.")

	disagree := append([]string{}, snapLines...)
	disagree[len(disagree)-1] = strings.Replace(disagree[len(disagree)-1],
		fmt.Sprintf(".%d/%d.", len(snapLines), len(snapLines)),
		fmt.Sprintf(".%d/%d.", len(snapLines), len(snapLines)+1), 1)
	negArmor("arm-neg-count-disagree", "neg_count_disagree.carcap", disagree, "E_PARSE_FRAMING",
		"The last line claims a different total than the others.")

	short := append([]string{}, snapLines...)
	short[0] = short[0][:len(short[0])-8]
	negArmor("arm-neg-short-nonfinal", "neg_short_nonfinal.carcap", short, "E_PARSE_FRAMING",
		"A non-final chunk that decodes to fewer than 620 bytes. Every chunk except the one with i == n MUST decode to exactly 620.")

	badChar := append([]string{}, single...)
	badChar[0] = badChar[0][:len(badChar[0])-1] + "U"
	negArmor("arm-neg-illegal-character", "neg_illegal_character.carcap", badChar, "E_PARSE_FRAMING",
		"The letter U, which the Crockford alphabet excludes by construction along with I, L and O. A decoder MUST map I, i, L and l to 1 and O and o to 0, MUST ignore hyphens anywhere, and MUST reject every other character.")

	padBits := append([]string{}, single...)
	last := padBits[0]
	padBits[0] = last[:len(last)-1] + bumpPad(last[len(last)-1])
	negArmor("arm-neg-nonzero-pad-bits", "neg_nonzero_pad_bits.carcap", padBits, "E_PARSE_FRAMING",
		"The final character carries non-zero trailing pad bits. A decoder MUST reject it, so that each byte string has exactly one accepted spelling and a QR set cannot be re-spelled into a different bid.")
}

// bumpPad returns a Crockford character whose low pad bits are set, given the
// last character of an encoding whose pad bits are zero.
func bumpPad(c byte) string {
	for i := 0; i < len(crockAlphabet); i++ {
		if crockAlphabet[i] == c {
			return string(crockAlphabet[i|1])
		}
	}
	panic("bumpPad: character not in alphabet")
}
