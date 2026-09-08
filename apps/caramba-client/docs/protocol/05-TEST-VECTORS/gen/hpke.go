package main

// HPKE, RFC 9180, restricted to the one CSM/1 suite:
//
//   mode_base 0x00, DHKEM(P-256, HKDF-SHA256) 0x0010,
//   HKDF-SHA256 0x0001, ChaCha20Poly1305 0x0003
//
// which is RFC 9180 Appendix A.5. 03-WIRE.md section 9.1.
//
// ChaCha20-Poly1305 is written out here because it is not in the Go standard
// library and the corpus takes no third-party dependency. The implementation
// is checked against the RFC 9180 A.5 base-mode vector on every run: if the
// generator cannot reproduce the published enc, shared_secret, key,
// base_nonce and sequence-0 ciphertext, it refuses to emit a corpus.

import (
	"crypto/ecdh"
	"crypto/hkdf"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"math/big"
)

const (
	hpkeModeBase = 0x00
	hpkeKEM      = 0x0010
	hpkeKDF      = 0x0001
	hpkeAEAD     = 0x0003
	hpkeNsecret  = 32
	hpkeNk       = 32
	hpkeNn       = 12
	hpkeNh       = 32
	hpkeInfoStr  = "CSM1-seal-v1"
)

func i2osp2(v uint16) []byte { return []byte{byte(v >> 8), byte(v)} }

func kemSuiteID() []byte { return append([]byte("KEM"), i2osp2(hpkeKEM)...) }
func hpkeSuiteID() []byte {
	out := append([]byte("HPKE"), i2osp2(hpkeKEM)...)
	out = append(out, i2osp2(hpkeKDF)...)
	return append(out, i2osp2(hpkeAEAD)...)
}

func labeledExtract(suiteID, salt []byte, label string, ikm []byte) []byte {
	msg := append([]byte("HPKE-v1"), suiteID...)
	msg = append(msg, []byte(label)...)
	msg = append(msg, ikm...)
	prk, err := hkdf.Extract(sha256.New, msg, salt)
	if err != nil {
		panic(err)
	}
	return prk
}

func labeledExpand(suiteID, prk []byte, label string, info []byte, l int) []byte {
	lab := i2osp2(uint16(l))
	lab = append(lab, []byte("HPKE-v1")...)
	lab = append(lab, suiteID...)
	lab = append(lab, []byte(label)...)
	lab = append(lab, info...)
	out, err := hkdf.Expand(sha256.New, prk, string(lab), l)
	if err != nil {
		panic(err)
	}
	return out
}

// dhkemEncapDeterministic performs DHKEM(P-256, HKDF-SHA256) Encap with a
// caller-supplied ephemeral private key, which is what makes the corpus
// reproducible and what lets the RFC vector be re-derived from its skEm.
func dhkemEncap(skE *ecdh.PrivateKey, pkR *ecdh.PublicKey) (shared, enc []byte) {
	dh, err := skE.ECDH(pkR)
	if err != nil {
		panic(err)
	}
	enc = skE.PublicKey().Bytes() // 65-byte uncompressed point
	kemContext := append(append([]byte{}, enc...), pkR.Bytes()...)
	eaePrk := labeledExtract(kemSuiteID(), nil, "eae_prk", dh)
	shared = labeledExpand(kemSuiteID(), eaePrk, "shared_secret", kemContext, hpkeNsecret)
	return shared, enc
}

type hpkeCtx struct {
	key            []byte
	baseNonce      []byte
	exporterSecret []byte
	ksContext      []byte
	secret         []byte
}

func keySchedule(shared, info []byte) hpkeCtx {
	sid := hpkeSuiteID()
	pskIDHash := labeledExtract(sid, nil, "psk_id_hash", nil)
	infoHash := labeledExtract(sid, nil, "info_hash", info)
	ksContext := append([]byte{hpkeModeBase}, pskIDHash...)
	ksContext = append(ksContext, infoHash...)
	secret := labeledExtract(sid, shared, "secret", nil)
	return hpkeCtx{
		key:            labeledExpand(sid, secret, "key", ksContext, hpkeNk),
		baseNonce:      labeledExpand(sid, secret, "base_nonce", ksContext, hpkeNn),
		exporterSecret: labeledExpand(sid, secret, "exp", ksContext, hpkeNh),
		ksContext:      ksContext,
		secret:         secret,
	}
}

// hpkeSeal is Setup + Seal at sequence number 0, which is all CSM/1 uses:
// one directive per encapsulation, never a stream.
func hpkeSeal(skE *ecdh.PrivateKey, pkR *ecdh.PublicKey, info, aad, pt []byte) (enc, ct []byte) {
	shared, enc := dhkemEncap(skE, pkR)
	ctx := keySchedule(shared, info)
	return enc, chachaPolySeal(ctx.key, ctx.baseNonce, aad, pt)
}

// sealAAD is the 33-byte additional data of 03-WIRE.md section 9.2:
//
//	aad = "CSM1" || 0x06 || pid(8) || dtp(16) || u32be(ver)
func sealAAD(pid, dtp []byte, ver uint32) []byte {
	out := append([]byte("CSM1"), dtSealed)
	out = append(out, pid...)
	out = append(out, dtp...)
	var v [4]byte
	binary.BigEndian.PutUint32(v[:], ver)
	return append(out, v[:]...)
}

// derivePrivP256 turns a label into a fixed, valid P-256 private key by
// hashing with a counter until the scalar is in range.
func derivePrivP256(label string) *ecdh.PrivateKey {
	for i := 0; i < 256; i++ {
		h := sha256.Sum256([]byte(fmt.Sprintf("%s#%d", label, i)))
		if k, err := ecdh.P256().NewPrivateKey(h[:]); err == nil {
			return k
		}
	}
	panic("derivePrivP256: no valid scalar found")
}

// ---------------------------------------------------------- ChaCha20-Poly1305

func rotl(x uint32, n uint) uint32 { return x<<n | x>>(32-n) }

func chachaBlock(key []byte, counter uint32, nonce []byte) [64]byte {
	var s [16]uint32
	s[0], s[1], s[2], s[3] = 0x61707865, 0x3320646e, 0x79622d32, 0x6b206574
	for i := 0; i < 8; i++ {
		s[4+i] = binary.LittleEndian.Uint32(key[4*i:])
	}
	s[12] = counter
	for i := 0; i < 3; i++ {
		s[13+i] = binary.LittleEndian.Uint32(nonce[4*i:])
	}
	w := s
	qr := func(a, b, c, d int) {
		w[a] += w[b]
		w[d] ^= w[a]
		w[d] = rotl(w[d], 16)
		w[c] += w[d]
		w[b] ^= w[c]
		w[b] = rotl(w[b], 12)
		w[a] += w[b]
		w[d] ^= w[a]
		w[d] = rotl(w[d], 8)
		w[c] += w[d]
		w[b] ^= w[c]
		w[b] = rotl(w[b], 7)
	}
	for i := 0; i < 10; i++ {
		qr(0, 4, 8, 12)
		qr(1, 5, 9, 13)
		qr(2, 6, 10, 14)
		qr(3, 7, 11, 15)
		qr(0, 5, 10, 15)
		qr(1, 6, 11, 12)
		qr(2, 7, 8, 13)
		qr(3, 4, 9, 14)
	}
	var out [64]byte
	for i := 0; i < 16; i++ {
		binary.LittleEndian.PutUint32(out[4*i:], w[i]+s[i])
	}
	return out
}

func chachaXOR(key []byte, counter uint32, nonce, in []byte) []byte {
	out := make([]byte, len(in))
	for off := 0; off < len(in); off += 64 {
		blk := chachaBlock(key, counter+uint32(off/64), nonce)
		n := len(in) - off
		if n > 64 {
			n = 64
		}
		for i := 0; i < n; i++ {
			out[off+i] = in[off+i] ^ blk[i]
		}
	}
	return out
}

var poly1305P = new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), 130), big.NewInt(5))

func poly1305Mac(otk, msg []byte) []byte {
	rb := make([]byte, 16)
	copy(rb, otk[:16])
	rb[3] &= 15
	rb[7] &= 15
	rb[11] &= 15
	rb[15] &= 15
	rb[4] &= 252
	rb[8] &= 252
	rb[12] &= 252
	r := new(big.Int).SetBytes(revBytes(rb))
	s := new(big.Int).SetBytes(revBytes(otk[16:32]))
	a := new(big.Int)
	for off := 0; off < len(msg); off += 16 {
		n := len(msg) - off
		if n > 16 {
			n = 16
		}
		blk := make([]byte, n+1)
		copy(blk, msg[off:off+n])
		blk[n] = 1
		a.Add(a, new(big.Int).SetBytes(revBytes(blk)))
		a.Mul(a, r)
		a.Mod(a, poly1305P)
	}
	a.Add(a, s)
	tag := make([]byte, 16)
	ab := a.Bytes()
	le := revBytes(ab)
	copy(tag, le)
	return tag
}

func revBytes(x []byte) []byte {
	out := make([]byte, len(x))
	for i := range x {
		out[len(x)-1-i] = x[i]
	}
	return out
}

func pad16(n int) []byte {
	if n%16 == 0 {
		return nil
	}
	return make([]byte, 16-n%16)
}

// chachaPolySeal is AEAD_CHACHA20_POLY1305 of RFC 8439 section 2.8.
func chachaPolySeal(key, nonce, aad, pt []byte) []byte {
	otk := chachaBlock(key, 0, nonce)
	ct := chachaXOR(key, 1, nonce, pt)
	var mac []byte
	mac = append(mac, aad...)
	mac = append(mac, pad16(len(aad))...)
	mac = append(mac, ct...)
	mac = append(mac, pad16(len(ct))...)
	var l [16]byte
	binary.LittleEndian.PutUint64(l[0:], uint64(len(aad)))
	binary.LittleEndian.PutUint64(l[8:], uint64(len(ct)))
	mac = append(mac, l[:]...)
	return append(ct, poly1305Mac(otk[:32], mac)...)
}

// ------------------------------------------------- RFC 9180 A.5, imported

// rfc9180A5 holds the published base-setup vector of RFC 9180 Appendix A.5,
// transcribed verbatim from https://www.rfc-editor.org/rfc/rfc9180.txt.
// It is IMPORTED, not computed: the corpus ships it so all three verifiers
// test their HPKE against the RFC rather than against this generator.
// checkRFC9180 re-derives it as a self-test of the code above.
type rfcVector struct {
	Source             string `json:"source"`
	Section            string `json:"section"`
	Status             string `json:"status"`
	Mode               int    `json:"mode"`
	KemID              int    `json:"kem_id"`
	KdfID              int    `json:"kdf_id"`
	AeadID             int    `json:"aead_id"`
	Info               string `json:"info"`
	SkEm               string `json:"skEm"`
	PkEm               string `json:"pkEm"`
	SkRm               string `json:"skRm"`
	PkRm               string `json:"pkRm"`
	Enc                string `json:"enc"`
	SharedSecret       string `json:"shared_secret"`
	KeyScheduleContext string `json:"key_schedule_context"`
	Secret             string `json:"secret"`
	Key                string `json:"key"`
	BaseNonce          string `json:"base_nonce"`
	ExporterSecret     string `json:"exporter_secret"`
	Seq0Pt             string `json:"seq0_pt"`
	Seq0Aad            string `json:"seq0_aad"`
	Seq0Nonce          string `json:"seq0_nonce"`
	Seq0Ct             string `json:"seq0_ct"`
	Seq1Aad            string `json:"seq1_aad"`
	Seq1Nonce          string `json:"seq1_nonce"`
	Seq1Ct             string `json:"seq1_ct"`
	ExportL32Empty     string `json:"export_L32_empty_context"`
	ExportL32Ctx00     string `json:"export_L32_context_00"`
	ExportL32CtxTest   string `json:"export_L32_context_TestContext"`
}

var rfc9180A5 = rfcVector{
	Source:  "https://www.rfc-editor.org/rfc/rfc9180.txt",
	Section: "Appendix A.5.1 / A.5.1.1 / A.5.1.2",
	Status:  "imported verbatim from RFC 9180; not computed by this generator",
	Mode:    0, KemID: 16, KdfID: 1, AeadID: 3,
	Info:               "4f6465206f6e2061204772656369616e2055726e",
	SkEm:               "7550253e1147aae48839c1f8af80d2770fb7a4c763afe7d0afa7e0f42a5b3689",
	PkEm:               "04c07836a0206e04e31d8ae99bfd549380b072a1b1b82e563c935c095827824fc1559eac6fb9e3c70cd3193968994e7fe9781aa103f5b50e934b5b2f387e381291",
	SkRm:               "a4d1c55836aa30f9b3fbb6ac98d338c877c2867dd3a77396d13f68d3ab150d3b",
	PkRm:               "04a697bffde9405c992883c5c439d6cc358170b51af72812333b015621dc0f40bad9bb726f68a5c013806a790ec716ab8669f84f6b694596c2987cf35baba2a006",
	Enc:                "04c07836a0206e04e31d8ae99bfd549380b072a1b1b82e563c935c095827824fc1559eac6fb9e3c70cd3193968994e7fe9781aa103f5b50e934b5b2f387e381291",
	SharedSecret:       "806520f82ef0b03c823b7fc524b6b55a088f566b9751b89551c170f4113bd850",
	KeyScheduleContext: "00b738cd703db7b4106e93b4621e9a19c89c838e55964240e5d3f331aaf8b0d58b2e986ea1c671b61cf45eec134dac0bae58ec6f63e790b1400b47c33038b0269c",
	Secret:             "fe891101629aa355aad68eff3cc5170d057eca0c7573f6575e91f9783e1d4506",
	Key:                "a8f45490a92a3b04d1dbf6cf2c3939ad8bfc9bfcb97c04bffe116730c9dfe3fc",
	BaseNonce:          "726b4390ed2209809f58c693",
	ExporterSecret:     "4f9bd9b3a8db7d7c3a5b9d44fdc1f6e37d5d77689ade5ec44a7242016e6aa205",
	Seq0Pt:             "4265617574792069732074727574682c20747275746820626561757479",
	Seq0Aad:            "436f756e742d30",
	Seq0Nonce:          "726b4390ed2209809f58c693",
	Seq0Ct:             "6469c41c5c81d3aa85432531ecf6460ec945bde1eb428cb2fedf7a29f5a685b4ccb0d057f03ea2952a27bb458b",
	Seq1Aad:            "436f756e742d31",
	Seq1Nonce:          "726b4390ed2209809f58c692",
	Seq1Ct:             "f1564199f7e0e110ec9c1bcdde332177fc35c1adf6e57f8d1df24022227ffa8716862dbda2b1dc546c9d114374",
	ExportL32Empty:     "9b13c510416ac977b553bf1741018809c246a695f45eff6d3b0356dbefe1e660",
	ExportL32Ctx00:     "6c8b7be3a20a5684edecb4253619d9051ce8583baf850e0cb53c402bdcaf8ebb",
	ExportL32CtxTest:   "477a50d804c7c51941f69b8e32fe8288386ee1a84905fe4938d58972f24ac938",
}

func mustHex(s string) []byte {
	x, err := hex.DecodeString(s)
	if err != nil {
		panic(err)
	}
	return x
}

// checkRFC9180 re-derives the imported vector. It is a self-test of this
// generator's HKDF labeling, DHKEM and ChaCha20-Poly1305, and it is fatal.
func checkRFC9180() {
	v := rfc9180A5
	skE, err := ecdh.P256().NewPrivateKey(mustHex(v.SkEm))
	if err != nil {
		panic(err)
	}
	pkR, err := ecdh.P256().NewPublicKey(mustHex(v.PkRm))
	if err != nil {
		panic(err)
	}
	shared, enc := dhkemEncap(skE, pkR)
	must := func(name, want string, got []byte) {
		if hexs(got) != want {
			panic(fmt.Sprintf("RFC 9180 A.5 self-test failed on %s:\n  want %s\n  got  %s", name, want, hexs(got)))
		}
	}
	must("pkEm", v.PkEm, skE.PublicKey().Bytes())
	must("enc", v.Enc, enc)
	must("shared_secret", v.SharedSecret, shared)
	ctx := keySchedule(shared, mustHex(v.Info))
	must("key_schedule_context", v.KeyScheduleContext, ctx.ksContext)
	must("secret", v.Secret, ctx.secret)
	must("key", v.Key, ctx.key)
	must("base_nonce", v.BaseNonce, ctx.baseNonce)
	must("exporter_secret", v.ExporterSecret, ctx.exporterSecret)
	must("seq0 ct", v.Seq0Ct, chachaPolySeal(ctx.key, mustHex(v.Seq0Nonce), mustHex(v.Seq0Aad), mustHex(v.Seq0Pt)))
	must("seq1 ct", v.Seq1Ct, chachaPolySeal(ctx.key, mustHex(v.Seq1Nonce), mustHex(v.Seq1Aad), mustHex(v.Seq0Pt)))
	sid := hpkeSuiteID()
	must("export L=32 empty context", v.ExportL32Empty, labeledExpand(sid, ctx.exporterSecret, "sec", nil, 32))
	must("export L=32 context 00", v.ExportL32Ctx00, labeledExpand(sid, ctx.exporterSecret, "sec", []byte{0x00}, 32))
	must("export L=32 context TestContext", v.ExportL32CtxTest, labeledExpand(sid, ctx.exporterSecret, "sec", []byte("TestContext"), 32))
}
