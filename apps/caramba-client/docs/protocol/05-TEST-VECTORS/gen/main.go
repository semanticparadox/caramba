package main

// Generator for the CSM/1 test-vector corpus.
//
//	cd gen && go run .
//
// Writes ../vectors.json, ../bin/**.bin and ../armor/**.carcap, then verifies
// its own output: it re-reads every file, re-hashes it, and refuses to finish
// if any entry disagrees with the bytes on disk or if the five frame digests
// published in 03-WIRE.md section 15 do not reproduce.

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math/big"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// ---------------------------------------------------------------- records

type ctx struct {
	Anchor      string            `json:"anchor,omitempty"`
	PinnedPID   string            `json:"pinned_pid,omitempty"`
	LinkPin     string            `json:"link_pin,omitempty"`
	Now         uint64            `json:"now,omitempty"`
	TimeFloor   uint64            `json:"time_floor,omitempty"`
	Nonce       string            `json:"expected_nonce,omitempty"`
	DeviceDTP   string            `json:"device_dtp,omitempty"`
	DeviceAgree string            `json:"device_agreement_sk,omitempty"`
	HWM         map[string]uint64 `json:"hwm,omitempty"`
	StoredFrame string            `json:"stored_frame,omitempty"`
	Note        string            `json:"note,omitempty"`
}

type vec struct {
	ID      string `json:"id"`
	Group   string `json:"group"`
	File    string `json:"file"`
	DocType int    `json:"doc_type"`
	Bytes   int    `json:"bytes"`
	SHA256  string `json:"sha256"`
	Verdict string `json:"verdict"`
	Code    string `json:"code,omitempty"`
	Step    string `json:"step,omitempty"`
	Context string `json:"context"`
	Over    *ctx   `json:"context_override,omitempty"`
	Note    string `json:"note,omitempty"`
}

type armorVec struct {
	ID      string   `json:"id"`
	File    string   `json:"file"`
	Frames  int      `json:"frames"`
	Stream  int      `json:"stream_bytes"`
	Chunks  int      `json:"chunks"`
	BID     string   `json:"bid"`
	Verdict string   `json:"verdict"`
	Code    string   `json:"code,omitempty"`
	Carries []string `json:"carries,omitempty"`
	Note    string   `json:"note,omitempty"`
}

type keyVec struct {
	ID        string `json:"id"`
	PublicKey string `json:"public_key"`
	Verdict   string `json:"verdict"`
	Clause    string `json:"clause,omitempty"`
	Note      string `json:"note"`
}

type sigVec struct {
	ID        string `json:"id"`
	PublicKey string `json:"public_key"`
	Message   string `json:"message_hex"`
	Signature string `json:"signature"`
	Verdict   string `json:"verdict"`
	Clause    string `json:"clause,omitempty"`
	Note      string `json:"note"`
}

type derivVec struct {
	Name   string `json:"name"`
	Input  string `json:"input"`
	Output string `json:"output"`
	Rule   string `json:"rule"`
}

type transportVec struct {
	ID      string `json:"id"`
	Subject string `json:"subject"`
	Verdict string `json:"verdict"`
	Code    string `json:"code,omitempty"`
	Rule    string `json:"rule"`
}

type shapeVec struct {
	Shape string `json:"shape"`
	Bytes int    `json:"bytes"`
}

type corpus struct {
	Corpus         string                    `json:"corpus"`
	CorpusVersion  int                       `json:"corpus_version"`
	Generator      string                    `json:"generator"`
	Determinism    string                    `json:"determinism"`
	Documents      map[string]string         `json:"source_documents"`
	FixtureKeys    map[string]string         `json:"fixture_keys"`
	Contexts       map[string]ctx            `json:"contexts"`
	Published      []publishedDigest         `json:"published_digest_check"`
	PublishedArmor map[string]any            `json:"published_armor_check"`
	Vectors        []vec                     `json:"vectors"`
	Armor          []armorVec                `json:"armor"`
	Ed25519Keys    []keyVec                  `json:"ed25519_public_key_ingest"`
	Ed25519Sigs    []sigVec                  `json:"ed25519_signature"`
	HPKE           map[string]any            `json:"hpke"`
	Derivations    []derivVec                `json:"derivations"`
	Transport      []transportVec            `json:"transport"`
	NodeShapes     []shapeVec                `json:"node_entry_shapes"`
	Counts         map[string]int            `json:"counts"`
	ErrorCodes     map[string][]string       `json:"error_code_registry"`
	Corrections    []map[string]string       `json:"corrections"`
	Sizes          map[string]map[string]int `json:"document_sizes"`
}

type publishedDigest struct {
	Document string `json:"document"`
	File     string `json:"file"`
	Bytes    int    `json:"bytes"`
	Expected string `json:"expected_by_wire_15"`
	Actual   string `json:"actual"`
	Match    bool   `json:"match"`
}

// ---------------------------------------------------------------- state

var (
	files    = map[string][]byte{}
	vectors  []vec
	armors   []armorVec
	root     = newSigner("root", "csm1-doc-example-root")
	online   = newSigner("online", "csm1-doc-example-online")
	rootB    = newSigner("rootB", "csm1-doc-example-root-b")
	rootC    = newSigner("rootC", "csm1-doc-example-root-c")
	online2  = newSigner("online2", "csm1-doc-example-online-2")
	stranger = newSigner("stranger", "csm1-doc-example-stranger")
	pid      []byte
	dtp      []byte
	dtpOther []byte
	nonce    []byte
	loc      string
	outDir   string
)

func put(path string, data []byte) string {
	if _, dup := files[path]; dup {
		panic("duplicate output path " + path)
	}
	files[path] = data
	return path
}

func add(v vec) {
	if v.Context == "" {
		v.Context = "default"
	}
	vectors = append(vectors, v)
}

func emit(id, group, dir, name string, docType int, f []byte, verdict, code, step, context string, over *ctx, note string) {
	p := put(filepath.Join(dir, name), f)
	add(vec{
		ID: id, Group: group, File: p, DocType: docType, Bytes: len(f),
		SHA256: hexs(frameSHA(f)), Verdict: verdict, Code: code, Step: step,
		Context: context, Over: over, Note: note,
	})
}

func pos(id, name string, docType int, f []byte, note string) {
	emit(id, "positive", "bin/positive", name, docType, f, "accept", "", "", "default", nil, note)
}

func neg(id, name string, docType int, f []byte, code, step, note string) {
	emit(id, "negative", "bin/negative", name, docType, f, "reject", code, step, "default", nil, note)
}

func negCtx(id, name string, docType int, f []byte, code, step, context string, over *ctx, note string) {
	emit(id, "negative", "bin/negative", name, docType, f, "reject", code, step, context, over, note)
}

func clone(x []byte) []byte { return append([]byte(nil), x...) }

// mut replaces the first occurrence of find with repl. It panics when find is
// absent or ambiguous, so a fixture can never silently stop testing what it
// was written to test.
func mut(in, find, repl []byte) []byte {
	i := bytes.Index(in, find)
	if i < 0 {
		panic("mut: pattern not found: " + hexs(find))
	}
	if bytes.Index(in[i+1:], find) >= 0 {
		panic("mut: pattern is ambiguous: " + hexs(find))
	}
	out := clone(in[:i])
	out = append(out, repl...)
	return append(out, in[i+len(find):]...)
}

func h(s string) []byte {
	x, err := hex.DecodeString(s)
	if err != nil {
		panic(err)
	}
	return x
}

// ---------------------------------------------------------------- main

func main() {
	checkRFC9180()

	pid = pidOf(root.pk)
	d := sha256.Sum256([]byte(fixDtpLabel))
	dtp = d[:16]
	d2 := sha256.Sum256([]byte("csm1-doc-example-device-spki-other"))
	dtpOther = d2[:16]
	nonce = h(fixNonceHex)
	locSecret := sha256.Sum256([]byte(fixLocSecret))
	loc = locator(locSecret[:], fixSubUUID, fixGen)

	if hexs(pid) != "226e8a20f699b964" {
		panic("pid does not match 03-WIRE.md section 15")
	}
	if linkPin(root.pk) != "49Q8M87PK6WP9QXG3T30" || loc != "EA3B8SKCY6VBWASE7AM1X48Y" {
		panic("link_pin or loc does not match 03-WIRE.md section 15")
	}

	outDir = ".."

	published := buildPositives()
	buildAnchors()
	buildNegatives()
	buildAnchorNegatives()
	buildSeals()
	buildArmor()

	c := corpus{
		Corpus:        "CSM/1 test vectors",
		CorpusVersion: 1,
		Generator:     "05-TEST-VECTORS/gen (Go, standard library only, hand-written strict CBOR encoder)",
		Determinism:   "No entropy source is used. crypto/rand is not seedable in Go, so bytes that must look random are drawn from a SHA-256 counter stream over a fixed label (gen/frame.go detReader). Re-running the generator reproduces every byte.",
		Documents: map[string]string{
			"decision":     "../01-DECISION.md",
			"wire":         "../03-WIRE.md",
			"spec":         "../02-SPEC.md",
			"threat_model": "../04-THREAT-MODEL.md",
		},
		FixtureKeys: fixtureKeys(),
		Contexts:    contexts(),
		Published:   published,
		PublishedArmor: map[string]any{
			"source":   "03-WIRE.md section 10.5, the worked armored example",
			"expected": wirePublishedArmorLine,
			"actual":   armorPublishedLine,
			"match":    armorPublishedLine == wirePublishedArmorLine,
			"note":     "The armored form of the 374-byte bootstrap blob of 03-WIRE.md 8.5, including its bid. The generator refuses to finish on a mismatch.",
		},
		Vectors:     vectors,
		Armor:       armors,
		Ed25519Keys: ed25519KeyVectors(),
		Ed25519Sigs: ed25519SigVectors(),
		HPKE:        hpkeSection(),
		Derivations: derivations(),
		Transport:   transportVectors(),
		NodeShapes:  nodeShapes,
		ErrorCodes: map[string][]string{
			"parse":  {"E_PARSE_SHORT", "E_PARSE_MAGIC", "E_PARSE_DOCTYPE", "E_PARSE_LEN", "E_PARSE_NSIGS", "E_PARSE_FRAMING", "E_PARSE_SLOTORDER", "E_PARSE_CBOR", "E_PARSE_ENVELOPE", "E_PARSE_FIELD"},
			"verify": {"E_VERIFY_ROLE", "E_VERIFY_NOANCHOR", "E_VERIFY_UNAUTHORIZED", "E_VERIFY_REVOKED", "E_VERIFY_SIG", "E_VERIFY_THRESHOLD", "E_VERIFY_PID", "E_VERIFY_VERSION", "E_VERIFY_ROTATION", "E_VERIFY_IAT", "E_VERIFY_EXPIRED", "E_VERIFY_NONCE", "E_VERIFY_DEVICE", "E_VERIFY_CATHASH"},
			"seal":   {"E_SEAL_RECIPIENT", "E_SEAL_SUITE", "E_SEAL_OPEN"},
		},
		Corrections: corrections,
		Sizes:       docSizes,
	}
	counts := map[string]int{"vectors": len(vectors), "armor": len(armors),
		"ed25519_public_key_ingest": len(c.Ed25519Keys), "ed25519_signature": len(c.Ed25519Sigs),
		"transport": len(c.Transport), "derivations": len(c.Derivations)}
	for _, v := range vectors {
		counts[v.Group]++
	}
	counts["files"] = len(files)
	c.Counts = counts

	// Write everything.
	for path, data := range files {
		full := filepath.Join(outDir, path)
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			panic(err)
		}
		if err := os.WriteFile(full, data, 0o644); err != nil {
			panic(err)
		}
	}
	sort.Slice(c.Vectors, func(i, j int) bool { return c.Vectors[i].ID < c.Vectors[j].ID })
	js, err := json.MarshalIndent(c, "", "  ")
	if err != nil {
		panic(err)
	}
	js = append(js, '\n')
	if err := os.WriteFile(filepath.Join(outDir, "vectors.json"), js, 0o644); err != nil {
		panic(err)
	}

	verifyOnDisk(c)

	total := len(js)
	for _, d := range files {
		total += len(d)
	}
	fmt.Printf("wrote %d fixture files + vectors.json, %d bytes total\n", len(files), total)
	fmt.Printf("vectors: %d positive, %d negative, %d armor, %d ed25519 key, %d ed25519 sig, %d transport\n",
		counts["positive"], counts["negative"], len(armors), len(c.Ed25519Keys), len(c.Ed25519Sigs), len(c.Transport))
	for _, p := range published {
		status := "MATCH"
		if !p.Match {
			status = "DIFFERS"
		}
		fmt.Printf("  03-WIRE.md 15 %-18s %4d bytes  %s\n", p.Document, p.Bytes, status)
	}
}

// verifyOnDisk re-reads every artifact and re-checks length and digest.
func verifyOnDisk(c corpus) {
	check := func(rel string, wantLen int, wantSHA string) {
		data, err := os.ReadFile(filepath.Join(outDir, rel))
		if err != nil {
			panic(err)
		}
		if wantLen >= 0 && len(data) != wantLen {
			panic(fmt.Sprintf("%s: on disk %d bytes, vectors.json says %d", rel, len(data), wantLen))
		}
		if wantSHA != "" && hexs(frameSHA(data)) != wantSHA {
			panic(fmt.Sprintf("%s: digest mismatch", rel))
		}
	}
	referenced := map[string]bool{}
	for _, v := range c.Vectors {
		check(v.File, v.Bytes, v.SHA256)
		cx, ok := c.Contexts[v.Context]
		if !ok {
			panic(v.ID + ": names undefined context " + v.Context)
		}
		referenced[v.File] = true
		if cx.Anchor != "" {
			referenced[cx.Anchor] = true
		}
		if v.Over != nil && v.Over.StoredFrame != "" {
			referenced[v.Over.StoredFrame] = true
		}
	}
	for _, cx := range c.Contexts {
		if cx.Anchor != "" {
			check(cx.Anchor, -1, "")
			referenced[cx.Anchor] = true
		}
	}
	for _, a := range c.Armor {
		referenced[a.File] = true
	}
	for path := range files {
		if !referenced[path] {
			panic("orphan artifact, written but referenced by nothing: " + path)
		}
	}
	for _, a := range c.Armor {
		check(a.File, -1, "")
	}
	for _, p := range c.Published {
		if !p.Match {
			panic("published digest mismatch for " + p.Document)
		}
	}
	if armorPublishedLine != wirePublishedArmorLine {
		panic("armored line does not reproduce the worked example of 03-WIRE.md 10.5")
	}
}

func fixtureKeys() map[string]string {
	mk := func(s signer) string { return hexs(s.pk) }
	seed := func(label string) string { x := sha256.Sum256([]byte(label)); return hexs(x[:]) }
	agree := derivePrivP256("csm1-doc-example-device-agree")
	agreeOther := derivePrivP256("csm1-doc-example-device-agree-other")
	return map[string]string{
		"root_seed":                    seed("csm1-doc-example-root"),
		"root_public":                  mk(root),
		"root_kid":                     hexs(root.kid),
		"online_seed":                  seed("csm1-doc-example-online"),
		"online_public":                mk(online),
		"online_kid":                   hexs(online.kid),
		"online2_seed":                 seed("csm1-doc-example-online-2"),
		"online2_public":               mk(online2),
		"online2_kid":                  hexs(online2.kid),
		"rootB_seed":                   seed("csm1-doc-example-root-b"),
		"rootB_public":                 mk(rootB),
		"rootB_kid":                    hexs(rootB.kid),
		"rootC_seed":                   seed("csm1-doc-example-root-c"),
		"rootC_public":                 mk(rootC),
		"rootC_kid":                    hexs(rootC.kid),
		"stranger_seed":                seed("csm1-doc-example-stranger"),
		"stranger_public":              mk(stranger),
		"stranger_kid":                 hexs(stranger.kid),
		"pid":                          hexs(pid),
		"link_pin":                     linkPin(root.pk),
		"loc":                          loc,
		"loc_hmac_secret":              seed("csm1-doc-example-loc-secret"),
		"subscription_uuid_for_loc":    fixSubUUID,
		"dtp":                          hexs(dtp),
		"dtp_other_device":             hexs(dtpOther),
		"nonce":                        hexs(nonce),
		"device_agreement_sk":          hexs(agree.Bytes()),
		"device_agreement_pk":          hexs(agree.PublicKey().Bytes()),
		"device_agreement_sk_other":    hexs(agreeOther.Bytes()),
		"device_agreement_pk_other":    hexs(agreeOther.PublicKey().Bytes()),
		"panel_hpke_sk":                hexs(derivePrivP256("csm1-doc-example-panel-hpke").Bytes()),
		"panel_hpke_pk":                hexs(derivePrivP256("csm1-doc-example-panel-hpke").PublicKey().Bytes()),
		"hpke_ephemeral_sk":            hexs(derivePrivP256("csm1-doc-example-hpke-ephemeral").Bytes()),
		"enrollment_code":              enrollCode(),
		"enrollment_code_unhyphenated": strings.ReplaceAll(enrollCode(), "-", ""),
	}
}

func contexts() map[string]ctx {
	base := ctx{
		Anchor:      "bin/positive/k1_min.bin",
		PinnedPID:   hexs(pid),
		LinkPin:     linkPin(root.pk),
		Now:         fixIAT + 300,
		TimeFloor:   fixIAT,
		Nonce:       hexs(nonce),
		DeviceDTP:   hexs(dtp),
		DeviceAgree: hexs(derivePrivP256("csm1-doc-example-device-agree").Bytes()),
		HWM:         map[string]uint64{"1": 1, "2": 6, "3": 411, "4": 6, "5": 0, "8": 0},
		Note:        "The trust anchor is k1_min. hwm is per (pid, doc_type, scope); a vector that needs a different high-water mark carries a context_override.",
	}
	rev := base
	rev.Anchor = "bin/anchors/k1_rev_online.bin"
	rev.Note = "Trust anchor revokes the online kid. Every document that kid signed MUST be rejected, cached copies included."
	thr := base
	thr.Anchor = "bin/anchors/k1_online_thr2.bin"
	thr.Note = "Trust anchor sets roles[2] to {ks:[online,online2], thr:2}. One online signature is below threshold."
	rot := base
	rot.Anchor = "bin/positive/k1_rot_v1.bin"
	rot.HWM = map[string]uint64{"1": 1}
	rot.Note = "Trust anchor is version 1 of the rotation chain. Only version 2 may be accepted next (03-WIRE.md 7.3)."
	rot2 := base
	rot2.Anchor = "bin/positive/k1_rot_v2.bin"
	rot2.HWM = map[string]uint64{"1": 2}
	rot2.Note = "Trust anchor is version 2 of the rotation chain, whose root role holds rootB only."
	rootOnly := base
	rootOnly.Anchor = "bin/anchors/k1_root_only.bin"
	rootOnly.Note = "Trust anchor publishes a root role and no online role. Legal, and the only shape in which V3 cannot resolve a key set."
	first := base
	first.Anchor = ""
	first.HWM = map[string]uint64{"1": 0}
	first.Note = "First trust. There is no previously trusted key document; the anchor is link_pin (03-WIRE.md 7.2)."
	return map[string]ctx{
		"default":     base,
		"rev_online":  rev,
		"online_thr2": thr,
		"rotation_v1": rot,
		"rotation_v2": rot2,
		"root_only":   rootOnly,
		"first_trust": first,
	}
}

func enrollCode() string {
	// 02-SPEC.md 9.2: code = link_pin[0..8] || 12 characters of base32 over
	// 60 bits. 02-SPEC.md Correction 5 requires the corpus to regenerate the
	// bootstrap blob with a conforming code; 03-WIRE.md 8.5's own example
	// does not fold in the pin.
	secret := sha256.Sum256([]byte("csm1-doc-example-enroll-secret"))
	body := crock(secret[:8])[:12]
	full := linkPin(root.pk)[:8] + body
	var b strings.Builder
	for i := 0; i < len(full); i += 4 {
		if i > 0 {
			b.WriteByte('-')
		}
		b.WriteString(full[i : i+4])
	}
	return b.String()
}

func derivations() []derivVec {
	agree := derivePrivP256("csm1-doc-example-device-agree")
	return []derivVec{
		{"pid", "sha256(root_public_key)", hexs(pid), "03-WIRE.md 4: sha256(root_ed25519_public_key)[0..8]"},
		{"kid_root", "sha256(root_public_key)", hexs(root.kid), "03-WIRE.md 4: sha256(pk)[0..12]"},
		{"kid_online", "sha256(online_public_key)", hexs(online.kid), "03-WIRE.md 4"},
		{"link_pin", "sha256(root_public_key)[0..12]", linkPin(root.pk), "03-WIRE.md 4: base32_crockford, 20 characters, 4 pad bits"},
		{"loc", "HMAC-SHA256(secret, \"csm1-loc\" || 0x00 || \"" + fixSubUUID + "\" || u32be(1))[0..15]", loc, "03-WIRE.md 4: uuid is the 36-byte lowercase ASCII text, not 16 raw bytes"},
		{"dtp", "sha256(\"" + fixDtpLabel + "\")[0..16]", hexs(dtp), "03-WIRE.md 4: sha256(device_signing_SPKI_DER)[0..16]; the fixture label stands in for a real SPKI"},
		{"enrollment_code", "link_pin[0..8] || base32_crockford(sha256(\"csm1-doc-example-enroll-secret\")[0..8])[0..12]", enrollCode(), "02-SPEC.md 9.2, and Correction 5 to 03-WIRE.md 8.5"},
		{"device_agreement_pk", "P-256 public of the fixture device agreement key", hexs(agree.PublicKey().Bytes()), "02-SPEC.md 9.4: signing and agreement are separate keys"},
		{"crockford_roundtrip", "base32_crockford(0x00..0x0f)", crock([]byte{0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15}), "03-WIRE.md 4.1: 16 bytes give 26 characters with 2 pad bits"},
	}
}

func transportVectors() []transportVec {
	return []transportVec{
		{"tr-content-encoding", "a CSM response carrying Content-Encoding: gzip", "reject", "", "03-WIRE.md 12.4: a CSM response MUST NOT carry a Content-Encoding. Compression makes the on-wire size a function of the plaintext and defeats the padding. The client MUST refuse the response rather than decompress it, so there is no decompression bound to get wrong."},
		{"tr-body-over-resp-max", "a response body of 4097 bytes for /sub/m1 or /sub/k1", "reject", "", "03-WIRE.md 11.3 and 01-DECISION.md invariant 5: thr.resp_max is 4096 and is a signed catalog field. The reader MUST use io.LimitReader at resp_max + 1 and reject at overflow rather than buffering."},
		{"tr-chunk-body-over-cap", "a response body of 3585 bytes for /sub/c1/{cat_id}/{i}", "reject", "", "03-WIRE.md 11.3: CHUNK_RESP_MAX is 3584."},
		{"tr-payload-len-over-cap", "a frame header declaring payload_len 49153", "reject", "E_PARSE_LEN", "03-WIRE.md 6.1 P4: 1 <= payload_len <= 49152. The reader MUST reject on the header before allocating."},
		{"tr-armor-stream-over-cap", "an armored frame stream of 65537 bytes", "reject", "E_PARSE_FRAMING", "03-WIRE.md 10.1: a frame stream MUST contain at most 16 frames and MUST NOT exceed 65536 bytes."},
		{"tr-armor-chunk-count", "an armored set claiming n = 107", "reject", "", "03-WIRE.md 10.2: the 106-chunk cap follows from ceil(65536 / 620)."},
		{"tr-redirect-cross-origin", "a 302 from the enrolled origin to another host", "reject", "", "01-DECISION.md 5.1.8: fetchSubscriptionBody gets followRedirects: false with per-hop scheme and origin validation. One hop MAY be followed only when the target host equals the tenant's configured subscription_domain."},
		{"tr-http-scheme", "any manifest, config, rule-set or geo URL with scheme http", "reject", "", "03-WIRE.md 14.4 and invariant 8. The only non-TLS exception is .onion."},
	}
}

func hpkeSection() map[string]any {
	agree := derivePrivP256("csm1-doc-example-device-agree")
	return map[string]any{
		"suite": map[string]any{
			"mode": hpkeModeBase, "kem_id": hpkeKEM, "kdf_id": hpkeKDF, "aead_id": hpkeAEAD,
			"name": "mode_base, DHKEM(P-256, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305",
			"rule": "03-WIRE.md 9.1, and its Correction 4 to 00-DESIGN-BRIEF.md 4.7: the KEM is P-256, not X25519, because the device key must live in Secure Enclave or StrongBox.",
		},
		"info":                        hexs([]byte(hpkeInfoStr)),
		"info_ascii":                  hpkeInfoStr,
		"aad_rule":                    "aad = \"CSM1\" || 0x06 || pid(8) || dtp(16) || u32be(ver), 33 bytes, recomputed by the recipient from the outer payload and never accepted from the wire (03-WIRE.md 9.2).",
		"aad_fixture":                 hexs(sealAAD(pid, dtp, fixDirV)),
		"recipient_pk":                hexs(agree.PublicKey().Bytes()),
		"recipient_sk":                hexs(agree.Bytes()),
		"rkv":                         1,
		"rfc9180_key_schedule_vector": rfc9180A5,
		"rfc9180_note":                "Imported verbatim from RFC 9180 Appendix A.5 (the CSM/1 suite). The generator re-derives every field of it on each run as a self-test and refuses to emit a corpus if any field disagrees; the values themselves are the RFC's, not this generator's.",
	}
}

// ---------------------------------------------------------------- ed25519 vectors

func ed25519KeyVectors() []keyVec {
	var out []keyVec
	addKey := func(id string, pk []byte, note string) {
		r := edDecode(pk)
		verdict, clause := "accept", ""
		switch {
		case !r.Canonical:
			verdict, clause = "reject", "2.1 clause 1 (canonical encoding, y < p)"
		case !r.OnCurve:
			verdict, clause = "reject", "2.1 clause 2 (decompresses to a curve point)"
		case r.SmallOrder:
			verdict, clause = "reject", "2.1 clause 3 ([8]A is not the identity)"
		}
		out = append(out, keyVec{ID: id, PublicKey: hexs(pk), Verdict: verdict, Clause: clause, Note: note})
	}
	addKey("ed-key-valid-root", root.pk, "The fixture root key. Accepted by all three clauses.")
	addKey("ed-key-valid-online", online.pk, "The fixture online key.")
	tors := edTorsion()
	for i, p := range tors {
		addKey(fmt.Sprintf("ed-key-small-order-%d", i),
			edEncode(p),
			"Canonical encoding of a point of order dividing 8, computed as [L]P for an off-subgroup P. 03-WIRE.md 2.1 clause 3 requires the [8]A identity test, not a transcribed blacklist.")
	}
	// Non-canonical spellings: y + p is still under 2^255 only when y < 19.
	for i, p := range tors {
		if p.y.Cmp(big.NewInt(19)) >= 0 {
			continue
		}
		yp := new(big.Int).Add(p.y, edP)
		enc := edEncode(edPoint{p.x, yp})
		addKey(fmt.Sprintf("ed-key-noncanonical-y-%d", i), enc,
			"Non-canonical spelling of small-order point "+fmt.Sprint(i)+": y + p re-encoded. Clause 1 rejects it before clause 3 is reached.")
	}
	// y = p exactly, and y = 2^255 - 1.
	addKey("ed-key-y-equals-p", edEncode(edPoint{big.NewInt(0), edP}),
		"y = p exactly. Clause 1 requires y strictly less than p.")
	allOnes := bytes.Repeat([]byte{0xff}, 32)
	addKey("ed-key-y-all-ones", allOnes,
		"All bits set. With the sign bit masked this is y = 2^255 - 1, which exceeds p.")
	// A canonical, on-curve, non-small-order point that is nonetheless not a
	// valid Ed25519 public key for any signature here: still accepted at ingest.
	B := edBasePoint()
	addKey("ed-key-basepoint", edEncode(B),
		"The Ed25519 basepoint. Canonical, on curve, order L, so ingest accepts it. Ingest is not an authorization check; authorization is the role table of 03-WIRE.md 7.1.")
	// Not on the curve at all.
	notOn := clone(edEncode(B))
	notOn[0] ^= 0x01
	if edDecode(notOn).OnCurve {
		notOn[1] ^= 0x03
	}
	addKey("ed-key-not-on-curve", notOn,
		"Basepoint encoding with one bit flipped in y; decompression fails.")
	return out
}

func ed25519SigVectors() []sigVec {
	msg := []byte("CSM1 test vector message")
	sig := ed25519.Sign(online.sk, msg)
	out := []sigVec{{
		ID: "ed-sig-valid", PublicKey: hexs(online.pk), Message: hexs(msg),
		Signature: hexs(sig), Verdict: "accept",
		Note: "Deterministic RFC 8032 pure Ed25519 signature. 03-WIRE.md 1.5 forbids Ed25519ph, Ed25519ctx and randomized nonces.",
	}}
	// S + L, still 32 bytes: the canonical non-canonical-S case.
	S := new(big.Int)
	for i := 63; i >= 32; i-- {
		S.Lsh(S, 8)
		S.Or(S, big.NewInt(int64(sig[i])))
	}
	sPlusL := new(big.Int).Add(S, edL)
	if sPlusL.BitLen() <= 256 {
		bad := clone(sig)
		copy(bad[32:], edScalarEncode(sPlusL))
		out = append(out, sigVec{
			ID: "ed-sig-noncanonical-S", PublicKey: hexs(online.pk), Message: hexs(msg),
			Signature: hexs(bad), Verdict: "reject", Clause: "2.2 clause 1 (S < L)",
			Note: "S replaced by S + L. A verifier MUST reject rather than reduce; a reducing verifier turns one signature into many.",
		})
	}
	bad := clone(sig)
	copy(bad[32:], edScalarEncode(edL))
	out = append(out, sigVec{
		ID: "ed-sig-S-equals-L", PublicKey: hexs(online.pk), Message: hexs(msg),
		Signature: hexs(bad), Verdict: "reject", Clause: "2.2 clause 1 (S < L, strictly)",
		Note: "S = L exactly. The bound is strict, so this is a rejection.",
	})
	bad2 := clone(sig)
	bad2[63] |= 0x80
	out = append(out, sigVec{
		ID: "ed-sig-S-high-bit", PublicKey: hexs(online.pk), Message: hexs(msg),
		Signature: hexs(bad2), Verdict: "reject", Clause: "2.2 clause 1",
		Note: "High bit of the last S byte set, which puts S far above L.",
	})
	tors := edTorsion()
	bad3 := clone(sig)
	copy(bad3[:32], edEncode(tors[1]))
	out = append(out, sigVec{
		ID: "ed-sig-R-small-order", PublicKey: hexs(online.pk), Message: hexs(msg),
		Signature: hexs(bad3), Verdict: "reject", Clause: "2.2 clause 3 (the cofactorless equation fails)",
		Note: "R replaced by a small-order point. 03-WIRE.md 2.2 clause 2 permits small-order R structurally, so the rejection must come from the verification equation, not from a shape check.",
	})
	bad4 := clone(sig)
	bad4[10] ^= 0x40
	out = append(out, sigVec{
		ID: "ed-sig-bitflip", PublicKey: hexs(online.pk), Message: hexs(msg),
		Signature: hexs(bad4), Verdict: "reject", Clause: "2.2 clause 3",
		Note: "One bit flipped inside R.",
	})
	if pk, s, ok := edCofactorDivergence(msg); ok {
		out = append(out, sigVec{
			ID: "ed-sig-cofactored-only", PublicKey: hexs(pk), Message: hexs(msg),
			Signature: hexs(s), Verdict: "reject", Clause: "2.2 clause 3 (cofactorless [S]B == R + [h]A)",
			Note: "Constructed so that a COFACTORED verifier accepts and a COFACTORLESS verifier rejects. The public key carries an order-8 torsion component, so it passes ingest clause 3, and S was solved for the cofactored equation. This is the single vector that catches the divergence 03-WIRE.md 2.2 clause 3 forbids; an implementation that accepts it has the wrong verification equation even though every other vector in this corpus passes.",
		})
	} else {
		panic("failed to construct the cofactor divergence vector")
	}
	return out
}

// wirePublishedArmorLine is the worked example printed in 03-WIRE.md 10.5,
// transcribed so the generator can prove it reproduces it.
const wirePublishedArmorLine = "CARCAP1.RY1K5HAN.1/1.8D9MTC8504HAP08109424VMA43V9KEB40C0G86KAJXKG018TDAZF800AF0CPGX3ME1SKMBSFE1GPWSBC5SJQGRBDE1P6ABKECNT0PVJB6X8NEB9K9MS50B9SB195832R425HC33HRR80GCGWNR6GVJD9G2VEB75JDM7MT7X8VZ031GV7BRNQR3C1MM0Q4V9H5SJQGRBDE1P6ABB3CHQ2WVK5EG174V9H5SJQGRBDE1P6ABB3CHQ2WVK5EG1R2P1040GJ48S44MK2EA1958NJRB9E5WR32CHK6GTKCDSR74X3PF1X7RZG86B1DG2P4H251T0T80BFCHQPGBK5F1GPTW3CCMQ6WSBM09N2YS3EECPQ2XB5E9WG70BC64WKGBHN64Q32C1G5RVG90AR41042GJ38H2MCHT89554PK2D9S7N0MAJADA5ANJQB1CNMPTWBNF5Y3VC8NW6282ECNT7EVVJDDSG28KEH8GFD6DSCKFV07M6Z9PT16QFCHYCXX17ZHYQ1M8FGNKX4812T98XH9KPF5BV1SQ3T5HY7BKVKEFKXEV1CT395E3F34G2DV6A6D8VSM7SX14BF66R15X1R3R"
