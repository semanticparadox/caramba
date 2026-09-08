package main

// The positive corpus: every document type at minimum, typical and maximum
// size, single and dual signature, a full root rotation chain, the chunked
// catalog and the bootstrap blob.

import (
	"bytes"
	"fmt"
	"strings"
)

var (
	catMinFrame        []byte
	catTypFrame        []byte
	catMaxFrame        []byte
	dirMinFrame        []byte
	keyMinFrame        []byte
	blobConform        []byte
	blobMaxFrame       []byte
	blobWireFrame      []byte
	armorPublishedLine string
	rotV1, rotV2       []byte
	rotV3              []byte
	nodeShapes         []shapeVec
	docSizes           = map[string]map[string]int{}
	corrections        []map[string]string
)

func rng(lo, n int) []byte {
	out := make([]byte, n)
	for i := range out {
		out[i] = byte(lo + i)
	}
	return out
}

func size(doc, variant string, n int) {
	if docSizes[doc] == nil {
		docSizes[doc] = map[string]int{}
	}
	docSizes[doc][variant] = n
}

func correction(id, subject, finding, action string) {
	corrections = append(corrections, map[string]string{
		"id": id, "subject": subject, "finding": finding, "resolution": action,
	})
}

// ------------------------------------------------------------ fixture parts

func fixMirror() mirror {
	return mirror{h: "m1.example-cdn.net", sni: "m1.example-cdn.net",
		pin: [][]byte{rng(0x20, 32)}, asn: 24940, cc: "DE"}
}

func fixDoH() dohEntry {
	return dohEntry{h: "doh.example.net", p: "/dns-query",
		ip: []string{"198.51.100.7"}, pin: [][]byte{rng(0x40, 32)}}
}

var mirrorPool = []mirror{
	{h: "m1.example-cdn.net", sni: "m1.example-cdn.net", pin: [][]byte{rng(0x20, 32)}, asn: 24940, cc: "DE", w: 10},
	{h: "m2.example-edge.net", sni: "m2.example-edge.net", pin: [][]byte{rng(0x60, 32)}, asn: 13335, cc: "NL", w: 20, ip: []string{"203.0.113.9"}},
	{h: "m3.example-relay.org", sni: "m3.example-relay.org", pin: [][]byte{rng(0x80, 32)}, asn: 16509, cc: "FI", w: 5},
	{h: "m4.example-alt.net", sni: "m4.example-alt.net", pin: [][]byte{rng(0xa0, 32)}, asn: 20473, cc: "SE", w: 5},
}

// The five node shapes 03-WIRE.md 8.2.1 measures.
func shapeNode(i int, idx int) node {
	id := fmt.Sprintf("n%di%d", 100+idx, 1+idx%7)
	switch i % 5 {
	case 0: // Shadowsocks-2022 relay, no TLS, no SNI
		return node{id: id, pn: "\U0001F1F3\U0001F1F1 Relay", cc: "NL",
			h: "nl-r1.exa-nodes.net", p: 8388, pr: 6, nw: 1, se: 0, ssm: 2}
	case 1: // Hysteria2 + salamander obfs + port hopping
		return node{id: id, pn: "\U0001F1EB\U0001F1EE Turbo", cc: "FI",
			h: "fi1.exa-nodes.net", p: 443, pr: 4, nw: 6, se: 1,
			sni: "cdn.example.net", hop: "20000-25000", obf: "salamander", alp: []uint64{3}}
	case 2: // VLESS + Reality + TCP, the dominant exit
		return nodeVlessReality(id, "\U0001F1E9\U0001F1EA Stealth", "DE", "de1.exa-nodes.net", 443, "6ba85179")
	case 3: // Trojan + gRPC + Reality
		return node{id: id, pn: "\U0001F1F8\U0001F1EA Grpc", cc: "SE",
			h: "se1.exa-nodes.net", p: 443, pr: 3, nw: 3, se: 2,
			sni: "www.microsoft.com", pbk: realityPBK, sid: "1f2e3d4c",
			fp: 1, pt: "grpc-svc", insV: true}
	default: // VLESS + WS + TLS behind a CDN with a Host override
		return node{id: id, pn: "\U0001F1FA\U0001F1F8 Edge", cc: "US",
			h: "us1.exa-cdn.net", p: 443, pr: 1, nw: 2, se: 1,
			sni: "cdn.example.net", pt: "/ws-tunnel", hst: "front.example.net",
			fp: 1, alp: []uint64{1, 2}, insV: true}
	}
}

// ------------------------------------------------------------ positives

func buildPositives() []publishedDigest {
	capBits := []byte{0x00, 0x00, 0x00, 0x03}

	// ---- 0x01 key document, minimum. Reproduces 03-WIRE.md 8.1 exactly.
	kdMin := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 1, iat: fixIAT,
		keys: []keyEntry{{root.kid, root.pk}, {online.kid, online.pk}},
		roles: []roleSpec{
			{1, [][]byte{root.kid}, 1},
			{2, [][]byte{online.kid}, 1},
		},
	})
	keyMinFrame = buildFrame(dtKey, encode(kdMin), []signer{root})
	emit("pos-k1-min", "positive", "bin/positive", "k1_min.bin", dtKey, keyMinFrame,
		"accept", "", "", "first_trust", nil,
		"Minimum key document, one root key and one online key, threshold 1 each. Byte-for-byte the fixture of 03-WIRE.md 8.1 and the trust anchor for the default context.")
	size("key_document", "minimum", len(keyMinFrame))

	// ---- 0x02 catalog, minimum. Reproduces 03-WIRE.md 8.2 exactly.
	catMin := buildCatalog(catalogOpts{
		pid: pid, ver: fixCatalogV, iat: fixIAT, tier: fixTier,
		ex:  []node{nodeVlessReality("n17i3", "\U0001F1E9\U0001F1EA Stealth", "DE", "de1.exa-nodes.net", 443, "6ba85179")},
		cap: capBits, ttl: fixTTL, jit: fixJit,
		thr: [3]uint64{8192, 22, 4096}, pb: [2]uint64{0, 3},
	})
	catMinFrame = buildFrame(dtCatalog, encode(catMin), []signer{online})
	pos("pos-c1-min", "c1_min.bin", dtCatalog, catMinFrame,
		"Minimum catalog, one exit, no relays and no mirrors. Byte-for-byte the fixture of 03-WIRE.md 8.2; cat_id is "+catID(frameSHA(catMinFrame))+".")
	size("catalog", "minimum", len(catMinFrame))

	// ---- 0x03 directive, minimum. Reproduces 03-WIRE.md 8.3 exactly.
	dirMin := buildDirective(directiveOpts{
		pid: pid, ver: fixDirV, iat: fixIAT, nonce: nonce, dtp: dtp, st: 3,
		cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: capBits,
		ttl: fixTTL, loc: loc,
	})
	dirMinFrame = buildFrame(dtDirective, encode(dirMin), []signer{online})
	pos("pos-m1-min", "m1_min.bin", dtDirective, dirMinFrame,
		"Minimum directive, status active, no selection and no policy echo. Byte-for-byte the fixture of 03-WIRE.md 8.3.")
	size("directive", "minimum", len(dirMinFrame))

	// ---- 0x04 chunk 0 of 1. Reproduces 03-WIRE.md 8.4 exactly.
	chMin := chunkPayloads(pid, fixCatalogV, fixIAT, catMinFrame)
	chMinFrame := buildFrame(dtChunk, encode(chMin[0]), []signer{online})
	pos("pos-c1c-min-0", "c1c_min_0.bin", dtChunk, chMinFrame,
		"Chunk 0 of 1 carrying the minimum catalog frame. Byte-for-byte the fixture of 03-WIRE.md 8.4. Every catalog is chunked, including a one-chunk catalog.")
	size("catalog_chunk", "minimum", len(chMinFrame))

	// ---- 0x05 bootstrap blob, exactly as 03-WIRE.md 8.5 prints it.
	blobWire := buildBlob(blobOpts{
		pid: pid, ver: 1, iat: fixIAT,
		org: "https://panel.example.net", code: "K7QW-3M2P-9XRT", rk: root.pk,
		mir: []mirror{fixMirror()}, doh: []dohEntry{fixDoH()}, nm: "Exa Networks",
	})
	blobWireFrame = buildFrame(dtBootstrap, encode(blobWire), []signer{root})
	emit("pos-b1-wire85", "reference", "bin/positive", "b1_wire_8_5.bin", dtBootstrap, blobWireFrame,
		"accept", "", "", "default", nil,
		"Reproduction of the exact bytes printed in 03-WIRE.md 8.5, kept only so an implementer can diff against the document. Its code field does NOT fold in the pin, so 02-SPEC.md Correction 5 supersedes it; use pos-b1-min for conformance.")

	// ---- 0x05 bootstrap blob, conforming code (02-SPEC.md 9.2, Correction 5).
	blob := buildBlob(blobOpts{
		pid: pid, ver: 1, iat: fixIAT,
		org: "https://panel.example.net", code: enrollCode(), rk: root.pk,
		mir: []mirror{fixMirror()}, doh: []dohEntry{fixDoH()}, nm: "Exa Networks",
	})
	blobConform = buildFrame(dtBootstrap, encode(blob), []signer{root})
	pos("pos-b1-min", "b1_min.bin", dtBootstrap, blobConform,
		"Minimum bootstrap blob with a conforming 20-character enrollment code ("+enrollCode()+"), whose first eight characters are link_pin[0..8]. This is the normative blob fixture; 02-SPEC.md Correction 5 requires it.")
	size("bootstrap_blob", "minimum", len(blobConform))

	// ---- 0x08 reserve pool.
	res := buildReserve(pid, 1, fixIAT, mirrorPool[:3], []dohEntry{fixDoH()}, 4)
	resFrame := buildFrame(dtReserve, encode(res), []signer{root})
	pos("pos-r1-min", "r1_min.bin", dtReserve, resFrame,
		"Reserve pool, root-signed and locator-scoped. Three mirrors across three ASNs and three countries, cohort 4. Correction 6 of 03-WIRE.md 16 is why this is a separate document type rather than a key document field.")
	size("reserve_pool", "minimum", len(resFrame))

	// ---- typical catalog: 40 exits, 3 relays, mirrors, DoH, resources, pins.
	var ex40 []node
	for i := 0; i < 40; i++ {
		ex40 = append(ex40, shapeNode(i, i))
	}
	re3 := []node{
		{id: "r1i1", pn: "\U0001F1F3\U0001F1F1 Relay", cc: "NL", h: "nl-r1.exa-nodes.net", p: 8388, pr: 6, nw: 1, se: 0, ssm: 2},
		{id: "r2i1", pn: "\U0001F1EB\U0001F1EE Relay", cc: "FI", h: "fi-r1.exa-nodes.net", p: 8388, pr: 6, nw: 1, se: 0, ssm: 2},
		{id: "r3i1", pn: "\U0001F1F8\U0001F1EA Relay", cc: "SE", h: "se-r1.exa-nodes.net", p: 8388, pr: 6, nw: 1, se: 0, ssm: 2},
	}
	agree := derivePrivP256("csm1-doc-example-panel-hpke")
	catTyp := buildCatalog(catalogOpts{
		pid: pid, ver: 8, iat: fixIAT, tier: fixTier,
		ex: ex40, re: re3,
		ro: []routeEntry{
			{id: "global", nm: "Everything through the tunnel", rs: []string{}},
			{id: "bypass-ru", nm: "Bypass domestic", rs: []string{"ru-direct"}},
		},
		cap: []byte{0x00, 0x00, 0x0f, 0xff}, mir: mirrorPool, doh: []dohEntry{fixDoH()},
		rs: []resource{
			{n: "ru-direct", u: "/rulesets/ru-direct.srs", h: rng(0xc0, 32), iv: 86400},
			{n: "ads", u: "/rulesets/ads.srs", h: rng(0xe0, 32), iv: 86400},
		},
		geo: []resource{{n: "geoip", u: "/geo/geoip.dat", h: rng(0x10, 32), iv: 604800}},
		ttl: fixTTL, jit: fixJit, thr: [3]uint64{8192, 22, 4096}, pb: [2]uint64{0, 3},
		lad: &ladder{ord: []uint64{0, 1, 2, 3, 4, 5, 6}, en: []uint64{0, 1, 2, 3, 6}},
		pin: []pinEntry{{h: "panel.example.net", spki: [][]byte{rng(0x30, 32), rng(0x50, 32)}}},
		hpk: agree.PublicKey().Bytes(), hpkv: 2,
	})
	catTypFrame = buildFrame(dtCatalog, encode(catTyp), []signer{online})
	pos("pos-c1-typical", "c1_typical.bin", dtCatalog, catTypFrame,
		fmt.Sprintf("Typical catalog: 40 exits across the five node shapes, 3 relays, 4 mirrors over 4 ASNs and 4 countries, 1 DoH endpoint, 2 rule-sets and 1 geo file with sha256, TLS SPKI pins, ladder defaults and a panel HPKE key. Serves in %d chunks.", (len(catTypFrame)+chunkPayloadMax-1)/chunkPayloadMax))
	size("catalog", "typical", len(catTypFrame))
	emitChunks("typ", catTypFrame, 8, "typical")

	// ---- maximum catalog: as many exits as fit under PANEL_REFUSE.
	n := 40
	for {
		var ex []node
		for i := 0; i < n+8; i++ {
			ex = append(ex, shapeNode(i, i))
		}
		trial := encode(buildCatalog(catalogOpts{
			pid: pid, ver: 9, iat: fixIAT, tier: 2, ex: ex, cap: capBits,
			ttl: fixTTL, jit: fixJit, thr: [3]uint64{8192, 22, 4096}, pb: [2]uint64{0, 3},
		}))
		if len(trial) > panelRefuse || n+8 > maxArrayItems {
			break
		}
		n += 8
	}
	var exMax []node
	for i := 0; i < n; i++ {
		exMax = append(exMax, shapeNode(i, i))
	}
	catMax := buildCatalog(catalogOpts{
		pid: pid, ver: 9, iat: fixIAT, tier: 2, ex: exMax, cap: capBits,
		ttl: fixTTL, jit: fixJit, thr: [3]uint64{8192, 22, 4096}, pb: [2]uint64{0, 3},
	})
	catMaxFrame = buildFrame(dtCatalog, encode(catMax), []signer{online})
	pos("pos-c1-max", "c1_max.bin", dtCatalog, catMaxFrame,
		fmt.Sprintf("Maximum catalog: %d exits, payload %d bytes, the largest that stays under the PANEL_REFUSE threshold of %d. Serves in %d chunks.",
			n, len(catMaxFrame)-84, panelRefuse, (len(catMaxFrame)+chunkPayloadMax-1)/chunkPayloadMax))
	size("catalog", "maximum", len(catMaxFrame))
	size("catalog", "maximum_exits", n)
	emitChunks("max", catMaxFrame, 9, "maximum")

	// ---- typical and maximum directive.
	dirTyp := buildDirective(directiveOpts{
		pid: pid, ver: 413, iat: fixIAT, nonce: nonce, dtp: dtp, st: 3,
		cat: frameSHA(catTypFrame), cn: uint64((len(catTypFrame) + chunkPayloadMax - 1) / chunkPayloadMax),
		tier: fixTier, cap: []byte{0x00, 0x00, 0x0f, 0xff},
		sel: &selection{exit: "n102i3", relay: "r1i1", preset: "bypass-ru", rcc: "NL", nid: 102},
		pol: []policyItem{
			{1, t("auto"), 2},
			{2, t("bypass-ru"), 1},
			{8, boolean(true), 1},
			{11, t("off"), 3},
		},
		ann: "Scheduled maintenance on Sunday 03:00 UTC.", sup: "support desk, in app",
		ui:  []hint{{1, "Your plan renews in 6 days."}},
		ttl: fixTTL, exph: 604800, loc: loc,
		traf: &traffic{up: 12884901888, dn: 96636764160, tot: 214748364800, exp: fixIAT + 2592000},
	})
	dirTypFrame := buildFrame(dtDirective, encode(dirTyp), []signer{online})
	pos("pos-m1-typical", "m1_typical.bin", dtDirective, dirTypFrame,
		"Typical directive: selection, four policy echoes with provenance, announce and support text at their caps' shape, one UI hint, an offline grace window and signed traffic counters. sel.rcc carries a real country here; see pos-m1-norelay for the no-relay sentinel.")
	size("directive", "typical", len(dirTypFrame))

	// Padded per request onto the 256-byte grid, r = 0. 03-WIRE.md 12.2.
	dirPadSrc := buildDirective(directiveOpts{
		pid: pid, ver: 414, iat: fixIAT, nonce: nonce, dtp: dtp, st: 3,
		cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: capBits, ttl: fixTTL, loc: loc,
	})
	padded := padTo(dirPadSrc, dtDirective, 1, 0)
	dirPadFrame := buildFrame(dtDirective, padded, []signer{online})
	pos("pos-m1-padded-r0", "m1_padded_r0.bin", dtDirective, dirPadFrame,
		fmt.Sprintf("Directive padded onto the 256-byte grid with r = 0: %d bytes exactly. pd is inside the signed payload, all bytes zero; padding after the last signature slot is impossible under the exact-length rule.", len(dirPadFrame)))
	dirPadSrc3 := buildDirective(directiveOpts{
		pid: pid, ver: 415, iat: fixIAT, nonce: nonce, dtp: dtp, st: 3,
		cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: capBits, ttl: fixTTL, loc: loc,
	})
	padded3 := padTo(dirPadSrc3, dtDirective, 1, 3)
	dirPad3Frame := buildFrame(dtDirective, padded3, []signer{online})
	pos("pos-m1-padded-r3", "m1_padded_r3.bin", dtDirective, dirPad3Frame,
		fmt.Sprintf("The same directive drawn at the top of the default pb range [0,3]: %d bytes. The four reachable sizes for this document are the bucket set an observer sees.", len(dirPad3Frame)))

	// Maximum directive: every optional field, then padded as far as the
	// MAX_BSTR_BYTES cap on pd permits under thr.resp_max.
	longest := strings.Repeat("x", 80)
	dirMaxOpts := directiveOpts{
		pid: pid, ver: 65535, iat: fixIAT, nonce: nonce, dtp: dtp, st: 7, rc: 3001,
		cat: frameSHA(catMaxFrame), cn: uint64((len(catMaxFrame) + chunkPayloadMax - 1) / chunkPayloadMax),
		// tier at its maximum, 1023. 03-WIRE.md 8.1 caps it there and not at
		// 65535: the tier id is a CBOR map key in tiers, and rule 3.3 rejects
		// every key at or above 1024, so a panel deriving 65535 signed a
		// document no conforming verifier could decode.
		tier: 1023, cap: []byte{0x00, 0x00, 0x0f, 0xff},
		// sel and pol MUST agree under the three self-contained predicates of
		// 02-SPEC.md 7.4, and a maximum fixture is no exception: pol[1] is the
		// protocol whose PROTO_WIRE value is sel.proto, pol[2] repeats
		// sel.preset, and sel.rcc is pol[3] uppercased. An earlier revision
		// violated all three and no implementation noticed, which is why the
		// predicates now have negative vectors of their own.
		//
		// pol[3] stays LOWERCASE deliberately. The wire predicate is satisfied
		// (sel.rcc is that code uppercased), and the value is still outside the
		// three states 02-SPEC.md 7.3 admits for pol[3], so a client that
		// applies it rather than ignoring the one key is caught. Agreement at
		// parse and vocabulary at merge are different rules and this fixture
		// exercises both at once.
		sel: &selection{exit: strings.Repeat("e", 24), relay: strings.Repeat("r", 24),
			preset: strings.Repeat("p", 32), variant: 255, proto: 8, rcc: "NL", nid: 1 << 40},
		pol: []policyItem{
			{1, t("AmneziaWG"), 1}, {2, t(strings.Repeat("p", 32)), 2}, {3, t("nl"), 1}, {4, t("system"), 3},
			{5, u(1420), 1}, {6, boolean(false), 2}, {7, boolean(true), 1}, {8, boolean(true), 1},
			{9, arr{t("https://doh.example.net/dns-query")}, 2},
			{10, arr{t("1.0.0.1")}, 3},
			{11, t("off"), 1},
		},
		ann: longest, sup: longest,
		ui:  []hint{{1, longest}, {2, longest}, {3, longest}, {4, longest}},
		ttl: 86400, exph: 2592000, loc: loc,
		traf: &traffic{up: 1 << 45, dn: 1 << 46, tot: 1 << 47, exp: fixIAT + 2592000},
	}
	dirMaxBase := buildDirective(dirMaxOpts)
	unpadded := len(encode(dirMaxBase)) + 84
	// Largest grid multiple reachable given pd <= MAX_BSTR_BYTES.
	r := 0
	for {
		probe := buildDirective(dirMaxOpts)
		l0 := len(encode(probe)) + 84
		target := padUnit * ((l0+padUnit-1)/padUnit + r + 1)
		if target > respMax {
			break
		}
		if d := target - l0; d-4 > maxBstrBytes {
			break
		}
		r++
	}
	dirMaxPayload := padTo(buildDirective(dirMaxOpts), dtDirective, 1, r)
	dirMaxFrame := buildFrame(dtDirective, dirMaxPayload, []signer{online})
	pos("pos-m1-max", "m1_max.bin", dtDirective, dirMaxFrame,
		fmt.Sprintf("Maximum directive: every optional field present, every capped text field at 80 bytes, all eleven policy keys, then padded to %d bytes, the largest 256-byte grid value that fits under thr.resp_max of %d. Unpadded it is %d bytes.", len(dirMaxFrame), respMax, unpadded))
	size("directive", "maximum", len(dirMaxFrame))
	size("directive", "maximum_unpadded", unpadded)

	// The no-relay sentinel, per 02-SPEC.md Correction 4.
	dirNoRelay := buildDirective(directiveOpts{
		pid: pid, ver: 416, iat: fixIAT, nonce: nonce, dtp: dtp, st: 3,
		cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: capBits,
		sel: &selection{exit: "n17i3", rcc: "--", nid: 17}, ttl: fixTTL, loc: loc,
	})
	pos("pos-m1-norelay", "m1_norelay.bin", dtDirective,
		buildFrame(dtDirective, encode(dirNoRelay), []signer{online}),
		"sel.rcc carries the no-relay sentinel \"--\". 03-WIRE.md 8.3 specifies \"NO\", which is Norway; 02-SPEC.md Correction 4 replaces it with \"--\" and the renderer maps it to the URL literal none. A verifier MUST accept \"--\" and MUST NOT treat it as a country.")

	// Every status value, so no implementation quietly special-cases one.
	for st := uint64(1); st <= 8; st++ {
		dv := buildDirective(directiveOpts{
			pid: pid, ver: 420 + st, iat: fixIAT, nonce: nonce, dtp: dtp, st: st,
			rc:  map[uint64]uint64{1: 1001, 2: 0, 3: 0, 4: 2001, 5: 4003, 6: 1002, 7: 3001, 8: 4001}[st],
			cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: capBits, ttl: fixTTL, loc: loc,
		})
		pos(fmt.Sprintf("pos-m1-st%d", st), fmt.Sprintf("m1_st%d.bin", st), dtDirective,
			buildFrame(dtDirective, encode(dv), []signer{online}),
			fmt.Sprintf("Directive with st = %d. All eight status values are positive vectors: an expired or revoked status is a signed, accepted document, and invariant 16 forbids disconnecting on it.", st))
	}

	// Non-critical unknown key must be ignored, not rejected.
	dirNC := buildDirective(directiveOpts{
		pid: pid, ver: 430, iat: fixIAT, nonce: nonce, dtp: dtp, st: 3,
		cat: frameSHA(catMinFrame), cn: 1, tier: fixTier, cap: capBits, ttl: fixTTL, loc: loc,
	})
	dirNC.set(64, m().set(1, b(rng(0x70, 65))).set(2, u(3)))
	pos("pos-m1-noncritical-key", "m1_noncritical_key.bin", dtDirective,
		buildFrame(dtDirective, encode(dirNC), []signer{online}),
		"Carries non-critical key 64, the agreement-key rekey slot of 02-SPEC.md 10.3. A v1 verifier MUST ignore it and MUST still accept the document. This is the extension mechanism of 03-WIRE.md 3.3 and the only vector that proves a verifier is not simply rejecting everything it does not recognize.")

	// Stale but still live catalog: the fixture 02-SPEC.md Correction 2 asks for.
	catStale := buildCatalog(catalogOpts{
		pid: pid, ver: 3, iat: fixIAT - 1728000, tier: 3,
		ex:  []node{nodeVlessReality("n17i3", "\U0001F1E9\U0001F1EA Stealth", "DE", "de1.exa-nodes.net", 443, "6ba85179")},
		cap: capBits, ttl: fixTTL, jit: fixJit, thr: [3]uint64{8192, 22, 4096}, pb: [2]uint64{0, 3},
	})
	staleFrame := buildFrame(dtCatalog, encode(catStale), []signer{online})
	emit("pos-c1-stale-but-live", "positive", "bin/positive", "c1_stale_but_live.bin", dtCatalog, staleFrame,
		"accept", "", "", "default",
		&ctx{HWM: map[string]uint64{"2": 0}},
		"iat is 20 days before time_floor and exp is 10 days after now, so the catalog is stale but not expired. 03-WIRE.md 6.2 V11 read literally (iat >= time_floor) REJECTS this; 02-SPEC.md Correction 2 amends V11 to iat + LIFETIME_MAX[doc_type] + 300 >= time_floor, which ACCEPTS it. This vector is the one that distinguishes the two forms, and the expected verdict is accept.")

	// ---- typical and maximum key document.
	kdTyp := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 2, iat: fixIAT,
		keys: []keyEntry{{root.kid, root.pk}, {online.kid, online.pk}, {online2.kid, online2.pk}},
		roles: []roleSpec{
			{1, [][]byte{root.kid}, 1},
			{2, [][]byte{online.kid, online2.kid}, 1},
		},
		revKid: [][]byte{stranger.kid},
		revNod: []string{"n99i1", "n98i2"},
		tiers:  map[uint64][]byte{1: frameSHA(catMinFrame), 2: frameSHA(catMaxFrame)},
		dep:    []depEntry{{"legacy.sub.v2ray", fixIAT + 15552000}},
		ttlk:   21600,
	})
	kdTypFrame := buildFrame(dtKey, encode(kdTyp), []signer{root})
	pos("pos-k1-typical", "k1_typical.bin", dtKey, kdTypFrame,
		"Typical key document: two online keys during an overlap window, a revoked stranger key, two revoked node ids, per-tier catalog hashes, one dated deprecation at exactly the 180 day minimum, and ttlk. The tier hashes are what make per-user catalog equivocation detectable (01-DECISION.md 5.2.4).")
	size("key_document", "typical", len(kdTypFrame))

	kdMaxFrame, kdMaxCapsFrame := buildMaxKeyDocs()
	pos("pos-k1-max-deliverable", "k1_max_deliverable.bin", dtKey, kdMaxFrame,
		fmt.Sprintf("The largest key document that fits under thr.resp_max of %d bytes: %d bytes. See correction cor-4: the field caps of 03-WIRE.md 8.1 admit a document far larger than the response ceiling permits, so the panel must enforce an emission bound the field table does not state.", respMax, len(kdMaxFrame)))
	size("key_document", "maximum_deliverable", len(kdMaxFrame))
	emit("pos-k1-max-caps", "reference", "bin/positive", "k1_max_caps.bin", dtKey, kdMaxCapsFrame,
		"accept", "", "", "default", nil,
		fmt.Sprintf("Every field of 03-WIRE.md 8.1 at its stated cap: 16 keys, 64 revoked kids, 256 revoked node ids, 16 tier hashes, 16 deprecations. %d bytes, which is %d bytes above thr.resp_max. A parser MUST accept it; a panel MUST NOT emit it over HTTP. Reference only.", len(kdMaxCapsFrame), len(kdMaxCapsFrame)-respMax))
	size("key_document", "maximum_at_field_caps", len(kdMaxCapsFrame))

	// ---- maximum bootstrap blob.
	var mir32 []mirror
	for i := 0; i < 32; i++ {
		src := mirrorPool[i%len(mirrorPool)]
		src.h = fmt.Sprintf("m%02d.example-cdn.net", i)
		src.sni = src.h
		src.asn = uint64(20000 + i)
		mir32 = append(mir32, src)
	}
	var doh8 []dohEntry
	for i := 0; i < 8; i++ {
		d := fixDoH()
		d.h = fmt.Sprintf("doh%d.example.net", i)
		doh8 = append(doh8, d)
	}
	blobMax := buildBlob(blobOpts{
		pid: pid, ver: 2, iat: fixIAT, org: "https://panel.example.net",
		code: enrollCode(), rk: root.pk, mir: mir32, doh: doh8, nm: "Exa Networks",
	})
	blobMaxFrame = buildFrame(dtBootstrap, encode(blobMax), []signer{root})
	pos("pos-b1-max", "b1_max.bin", dtBootstrap, blobMaxFrame,
		fmt.Sprintf("Maximum bootstrap blob: 32 mirrors and 8 DoH endpoints, %d bytes. Above thr.resp_max, which is correct and not a defect: the blob is the out-of-band rung and travels as a QR set, a file or a paste, never as a GET /sub/b1 response at this size.", len(blobMaxFrame)))
	size("bootstrap_blob", "maximum", len(blobMaxFrame))

	// ---- dual signature: catalog under a two-of-two online role.
	catDual := buildCatalog(catalogOpts{
		pid: pid, ver: 10, iat: fixIAT, tier: 4,
		ex:  []node{nodeVlessReality("n17i3", "\U0001F1E9\U0001F1EA Stealth", "DE", "de1.exa-nodes.net", 443, "6ba85179")},
		cap: capBits, ttl: fixTTL, jit: fixJit, thr: [3]uint64{8192, 22, 4096}, pb: [2]uint64{0, 3},
	})
	catDualFrame := buildFrame(dtCatalog, encode(catDual), []signer{online, online2})
	emit("pos-c1-dual-sig", "positive", "bin/positive", "c1_dual_sig.bin", dtCatalog, catDualFrame,
		"accept", "", "", "online_thr2", nil,
		"Catalog with two signature slots, verified against an anchor whose online role is {ks:[online,online2], thr:2}. nsigs = 2 and total = 7 + payload_len + 1 + 152. Slots are ordered by ascending keyid_trunc, which for this pair is "+slotOrder(online, online2)+".")
	size("catalog", "dual_signature", len(catDualFrame))

	// ---- root rotation chain, v1 -> v2 -> v3.
	rv1 := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 1, iat: fixIAT,
		keys:  []keyEntry{{root.kid, root.pk}, {online.kid, online.pk}},
		roles: []roleSpec{{1, [][]byte{root.kid}, 1}, {2, [][]byte{online.kid}, 1}},
	})
	rotV1 = buildFrame(dtKey, encode(rv1), []signer{root})
	emit("pos-k1-rot-v1", "positive", "bin/positive", "k1_rot_v1.bin", dtKey, rotV1,
		"accept", "", "", "first_trust", nil,
		"Version 1 of the rotation chain. Accepted at first trust because it holds exactly one key under role root whose sha256[0..12] matches link_pin "+linkPin(root.pk)+".")
	rv2 := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 2, iat: fixIAT + 60,
		keys:  []keyEntry{{rootB.kid, rootB.pk}, {online.kid, online.pk}},
		roles: []roleSpec{{1, [][]byte{rootB.kid}, 1}, {2, [][]byte{online.kid}, 1}},
	})
	rotV2 = buildFrame(dtKey, encode(rv2), []signer{root, rootB})
	emit("pos-k1-rot-v2", "positive", "bin/positive", "k1_rot_v2.bin", dtKey, rotV2,
		"accept", "", "", "rotation_v1", nil,
		"Version 2 rotates root from rootA to rootB. Dual-signed and verified twice over the same pre-image: once against roles[1] of the trusted v1, once against roles[1] of this document. Both MUST pass (03-WIRE.md 7.3). Note that pid does not change: pid is sha256 of the ORIGINAL root key and stays pinned across rotation.")
	rv3 := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 3, iat: fixIAT + 120,
		keys:  []keyEntry{{rootC.kid, rootC.pk}, {online.kid, online.pk}},
		roles: []roleSpec{{1, [][]byte{rootC.kid}, 1}, {2, [][]byte{online.kid}, 1}},
	})
	rotV3 = buildFrame(dtKey, encode(rv3), []signer{rootB, rootC})
	emit("pos-k1-rot-v3", "positive", "bin/positive", "k1_rot_v3.bin", dtKey, rotV3,
		"accept", "", "", "rotation_v2", nil,
		"Version 3 rotates rootB to rootC, signed by both. Walking v1 to v3 in one GET /sub/k1?since=1 response is the armored stream arm-rotation-chain.")

	// Node shape measurements, checked against 03-WIRE.md 8.2.1.
	names := []string{"Shadowsocks-2022 relay (no TLS, no SNI)", "Hysteria2 + salamander obfs + port hopping",
		"VLESS + Reality + TCP (the dominant exit)", "Trojan + gRPC + Reality",
		"VLESS + WS + TLS behind a CDN with a Host override"}
	total := 0
	for i, nm := range names {
		sz := len(nodeVal(shapeNode(i, i)).enc(2))
		nodeShapes = append(nodeShapes, shapeVec{Shape: nm, Bytes: sz})
		total += sz
	}
	nodeShapes = append(nodeShapes, shapeVec{Shape: "mean of the five shapes", Bytes: total / len(names)})

	registerCorrections(len(kdMaxCapsFrame), total/len(names))

	return []publishedDigest{
		pubCheck("key document", "bin/positive/k1_min.bin", keyMinFrame, "671eaaaf6729274419faefe0cb44430126d6421e2f0f628bc4f1fab376bdad35"),
		pubCheck("catalog", "bin/positive/c1_min.bin", catMinFrame, "eb5c33321940d11813848b8b8b03417e75fb36a82c8aa9c9567e1686f9df535d"),
		pubCheck("catalog chunk 0/1", "bin/positive/c1c_min_0.bin", chMinFrame, "68d613af7e4f616464ad281a92739822361fa66600948cda6ede452b46237168"),
		pubCheck("directive", "bin/positive/m1_min.bin", dirMinFrame, "b1956c4ed3877c424c1f11b903ae75be4f9a24a1537f760bb43a618da74be600"),
		pubCheck("bootstrap blob", "bin/positive/b1_wire_8_5.bin", blobWireFrame, "c78332c555152fd2572e7d5ec0f8bc2c1d48e2aedeccc99e3d4516eb05fc5247"),
	}
}

func pubCheck(name, file string, f []byte, want string) publishedDigest {
	return publishedDigest{Document: name, File: file, Bytes: len(f),
		Expected: want, Actual: hexs(frameSHA(f)), Match: hexs(frameSHA(f)) == want}
}

func emitChunks(tag string, catFrame []byte, ver uint64, label string) {
	chs := chunkPayloads(pid, ver, fixIAT, catFrame)
	for i, c := range chs {
		f := buildFrame(dtChunk, encode(c), []signer{online})
		pos(fmt.Sprintf("pos-c1c-%s-%d", tag, i),
			fmt.Sprintf("c1c_%s_%d.bin", tag, i), dtChunk, f,
			fmt.Sprintf("Chunk %d of %d of the %s catalog. Carries a slice of the catalog FRAME, not of its payload; every chunk but the last carries exactly %d bytes and the reassembled bytes are verified again in full as a 0x02 frame.", i, len(chs), label, chunkPayloadMax))
	}
	size("catalog_chunk", label+"_count", len(chs))
}

func buildMaxKeyDocs() (deliverable, atCaps []byte) {
	// Every field at its 03-WIRE.md 8.1 cap.
	var keys []keyEntry
	var ks [][]byte
	sgs := []signer{root, online, online2, rootB, rootC, stranger}
	for i := 0; i < 16; i++ {
		s := sgs[i%len(sgs)]
		if i >= len(sgs) {
			s = newSigner(fmt.Sprintf("filler%d", i), fmt.Sprintf("csm1-doc-example-filler-%d", i))
		}
		keys = append(keys, keyEntry{s.kid, s.pk})
		ks = append(ks, s.kid)
	}
	var revK [][]byte
	for i := 0; i < 64; i++ {
		s := newSigner("rev", fmt.Sprintf("csm1-doc-example-revoked-%d", i))
		revK = append(revK, s.kid)
	}
	var revN []string
	for i := 0; i < 256; i++ {
		revN = append(revN, fmt.Sprintf("n%di%d", 1000+i, i%9))
	}
	tiers := map[uint64][]byte{}
	for i := uint64(1); i <= 16; i++ {
		x := rng(int(i), 32)
		tiers[i] = x
	}
	var dep []depEntry
	for i := 0; i < 16; i++ {
		dep = append(dep, depEntry{fmt.Sprintf("legacy.surface.%02d", i), fixIAT + 15552000 + uint64(i)})
	}
	atCapsDoc := buildKeyDoc(keyDocOpts{
		pid: pid, ver: 2, iat: fixIAT, keys: keys,
		roles:  []roleSpec{{1, ks[:8], 2}, {2, ks[8:], 2}},
		revKid: revK, revNod: revN, tiers: tiers, dep: dep, ttlk: 86400,
	})
	atCaps = buildFrame(dtKey, encode(atCapsDoc), []signer{root, online})
	// Now the largest that fits under respMax: keep 16 keys and both roles,
	// then add revoked node ids until the frame would exceed the ceiling.
	for n := 0; n <= 256; n++ {
		doc := buildKeyDoc(keyDocOpts{
			pid: pid, ver: 2, iat: fixIAT, keys: keys,
			roles:  []roleSpec{{1, ks[:8], 2}, {2, ks[8:], 2}},
			revKid: revK[:imin(n, 64)], revNod: revN[:n], tiers: nil, dep: nil, ttlk: 86400,
		})
		f := buildFrame(dtKey, encode(doc), []signer{root, online})
		if len(f) > respMax {
			break
		}
		deliverable = f
	}
	if deliverable == nil {
		panic("no deliverable maximum key document found")
	}
	return deliverable, atCaps
}

func imin(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func registerCorrections(kdMaxCaps, nodeMean int) {
	correction("cor-1",
		"03-WIRE.md 8.2, the catalog worked example",
		"The note under the minimum catalog says \"The ver here is 7 (03 07), which is why the envelope is 31 bytes rather than the 33 it would be past version 255.\" Section 8.0 and Correction 9 of section 16 both put the envelope at 27 bytes for ver < 24, and the hex dump agrees: map head 1 + v 2 + pid 10 + ver 2 + iat 6 + exp 6 = 27. The 31 and 33 figures are wrong for this document, and 31 would be right only for a ver above 65535.",
		"The corpus follows the hex dump and section 8.0. The catalog fixture reproduces the published digest exactly, so nothing downstream is affected; 03-WIRE.md 8.2 should have that sentence corrected to 27 bytes, and to 29 for ver in 256..65535.")
	correction("cor-2",
		"03-WIRE.md 9.6, the sealed directive size table",
		"The row \"kem, kdf, aead | 7\" over-counts by one. The three fields encode as 0b 10, 0c 01 and 0d 03, which is 2 bytes each and 6 in total, so the outer payload is 370 bytes and the unpadded sealed frame is 454, not 371 and 455.",
		"The corpus emits the sealed directive with its measured length and records both numbers in document_sizes. The padded figure of 512 in that table is unaffected: 454 and 455 both round to the same 256-byte grid value.")
	correction("cor-3",
		"03-WIRE.md 6.1 P11 versus 3.3, unknown critical keys and key material",
		"Two decisions are left to the implementer. First, an unrecognized key in the critical range 1..63 is a parse failure under 3.3 and also under P11, but 3.3 sits inside the CBOR profile (code E_PARSE_CBOR) and P11 is the field step (code E_PARSE_FIELD). A CBOR decoder cannot evaluate 3.3 because it does not know the doc_type. Second, section 2.1 requires the small-order and canonical-encoding checks when a key ENTERS the trusted set, but the step tables of section 6 have no step that validates the pk values inside a key document's keys array; V6 covers only signature slots.",
		"The corpus assigns E_PARSE_CBOR to what a decoder can decide without a doc_type (key 0, keys at or above 1024, every structural rule) and E_PARSE_FIELD to everything doc-type-dependent, including an unrecognized critical key and a key document whose keys array carries a small-order or non-canonical pk. 03-WIRE.md should add a step P12 stating the second rule explicitly with code E_PARSE_FIELD, otherwise Rust, Go and Dart will each pick a different code for the same fixture and the shared-code requirement of section 6.6 is unmet.")
	correction("cor-4",
		"03-WIRE.md 8.1 field caps versus invariant 5",
		fmt.Sprintf("A key document with every field at its stated cap (16 keys, 64 revoked kids, 256 revoked node ids, 16 tier hashes, 16 deprecations) encodes to %d bytes, which is %d bytes above thr.resp_max of %d. The caps and the response ceiling are not jointly satisfiable, and GET /sub/k1 is the one endpoint a client must reach before it has anything else.", kdMaxCaps, kdMaxCaps-respMax, respMax),
		"The corpus ships both: pos-k1-max-caps as a parser vector and pos-k1-max-deliverable as the largest that fits the ceiling. The panel needs an emission bound that the field table does not state, or the key document needs the same chunking the catalog has. Chunking is the better answer, because the revocation list is the field that grows and it is the one field a client must not be denied.")
	correction("cor-5",
		"01-DECISION.md BC1 node entry size, re-measured",
		fmt.Sprintf("03-WIRE.md Correction 1 revises BC1's 220 to 280 bytes down to 60 to 142 with a mean of 116. Measured against this generator's encoder over the same five shapes the mean is %d bytes. The individual shapes are in node_entry_shapes.", nodeMean),
		"03-WIRE.md Correction 1's conclusion stands unchanged and so does its range. Any residual difference is in how much text a fixture puts in pn and h, which is fixture choice rather than encoding, so the mean is not a protocol constant and should not be cited as one.")
	correction("cor-6",
		"The task's instruction to seed crypto/rand",
		"crypto/rand.Reader in Go is not seedable and there is no standard-library API that makes it deterministic.",
		"The generator uses no entropy source at all. Values that must look unpredictable (padding content is all zero by rule, but nonces, thumbprints, pin bytes and Reality keys are not) are either fixed constants from 03-WIRE.md 15 or drawn from a SHA-256 counter stream over a fixed label. Re-running the generator reproduces every byte, which is the property the instruction was asking for.")
	correction("cor-8",
		"03-WIRE.md 2.3, the Go note on small-order key rejection",
		"03-WIRE.md 2.3 says the small-order test MUST be added at key ingest using filippo.io/edwards25519, \"which is already in the module graph through mihomo\". It is not. The only edwards25519 module in libs/caramba-core/go.mod is github.com/metacubex/edwards25519 v1.2.0, declared indirect at libs/caramba-core/go.mod:61, and a grep for filippo.io/edwards25519 across every go.mod and go.sum in the repository returns nothing.",
		"The Go verifier either takes metacubex/edwards25519, which is a fork of the same code under a different import path and is already vendored, or it adds filippo.io/edwards25519 as a new direct dependency and says so. The corpus does not care which: ed25519_public_key_ingest carries the eight small-order encodings and their non-canonical spellings as raw hex, so the test is the same either way. 03-WIRE.md 2.3 should name the module that is actually present.")
	correction("cor-9",
		"03-WIRE.md 2.3's Rust precedent already violates the profile it cites",
		"03-WIRE.md 2.3 points at the panel's existing licensing code as the Ed25519 precedent to reuse and then says VerifyingKey::verify MUST NOT be used. That code uses exactly it: libs/caramba-shared/src/license.rs:205 builds the key with VerifyingKey::from_bytes, which performs no small-order or canonicity check in ed25519-dalek v2, and :225 verifies with verifying_key.verify(&msg, &signature). The dependency is at libs/caramba-shared/Cargo.toml:16 and the import at license.rs:27, both as the wire document states.",
		"A CSM/1 signer and verifier in the panel MUST use VerifyingKey::verify_strict and MUST run the section 2.1 clause 3 test at key ingest, and reusing the license module's helper as-is would carry the defect into the protocol. The ed25519_public_key_ingest and ed25519_signature sections of this corpus fail against a verify-based implementation, ed-sig-noncanonical-S and ed-sig-cofactored-only in particular, which is the intended outcome: this is a defect the corpus is supposed to catch on day one. Whether the licensing path itself should be fixed is out of scope here and belongs to whoever owns libs/caramba-shared.")
	correction("cor-7",
		"03-WIRE.md 12.2 padding versus MAX_BSTR_BYTES",
		"pd is capped at MAX_BSTR_BYTES of 3072, so the reachable padded size of a document is bounded by L0 + 3076 as well as by thr.resp_max. A 230-byte directive cannot be padded to the 4096 ceiling; the largest grid value it can reach is 3328.",
		"Not a defect, and no change is proposed: the bucket rule draws r from pb, which defaults to [0,3] and therefore never approaches the cap. It is recorded because an implementer who reads only the clamp sentence in 12.2 will size a buffer for 4096 bytes of padding and never see it used.")
}

// slotOrder reports the ascending keyid_trunc order of two signers, so a
// fixture note states the real order rather than guessing at it.
func slotOrder(a, b signer) string {
	if bytes.Compare(a.kid, b.kid) < 0 {
		return hexs(a.kid) + " (" + a.name + ") then " + hexs(b.kid) + " (" + b.name + ")"
	}
	return hexs(b.kid) + " (" + b.name + ") then " + hexs(a.kid) + " (" + a.name + ")"
}
