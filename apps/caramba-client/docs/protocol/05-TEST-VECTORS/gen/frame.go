package main

// Frame assembly, base32 Crockford, derived identifiers and the deterministic
// bit source. 03-WIRE.md sections 1, 4 and 4.1.

import (
	"bytes"
	"crypto/ed25519"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"sort"
)

var magic = []byte{0x43, 0x53, 0x4d, 0x31} // "CSM1"

// Document types, 03-WIRE.md section 1.2.
const (
	dtKey       = 0x01
	dtCatalog   = 0x02
	dtDirective = 0x03
	dtChunk     = 0x04
	dtBootstrap = 0x05
	dtSealed    = 0x06
	dtReserve   = 0x08
)

// Lifetimes, 03-WIRE.md section 8.0.
const (
	lifeKey       = 604800
	lifeCatalog   = 2592000
	lifeDirective = 3600
	lifeBootstrap = 2592000
	lifeReserve   = 604800
)

// Size constants, 03-WIRE.md section 17.
const (
	payloadLenMax   = 49152
	padUnit         = 256
	respMax         = 4096
	chunkPayloadMax = 2816
	chunkRespMax    = 3584
	panelWarn       = 12288
	panelRefuse     = 49152
	armorChunkBytes = 620
	armorStreamMax  = 65536
	armorFrameMax   = 16
)

// signer pairs an Ed25519 private key with its truncated key id.
type signer struct {
	name string
	sk   ed25519.PrivateKey
	pk   ed25519.PublicKey
	kid  []byte // sha256(pk)[0..12]
}

func newSigner(name, seedLabel string) signer {
	seed := sha256.Sum256([]byte(seedLabel))
	sk := ed25519.NewKeyFromSeed(seed[:])
	pk := sk.Public().(ed25519.PublicKey)
	h := sha256.Sum256(pk)
	return signer{name: name, sk: sk, pk: pk, kid: append([]byte(nil), h[:12]...)}
}

// preImage is the signed byte string: magic || doc_type || u16be(len) || payload.
// 03-WIRE.md section 1.3. Nothing else is ever signed.
func preImage(docType byte, payload []byte) []byte {
	if len(payload) < 1 || len(payload) > payloadLenMax {
		panic(fmt.Sprintf("frame: payload_len %d outside 1..%d", len(payload), payloadLenMax))
	}
	out := make([]byte, 0, 7+len(payload))
	out = append(out, magic...)
	out = append(out, docType)
	out = append(out, byte(len(payload)>>8), byte(len(payload)))
	return append(out, payload...)
}

// buildFrame produces a complete, conforming frame. Slots are sorted by
// keyid_trunc ascending as section 1.4 requires; a caller that passes signers
// out of order still gets a conforming frame, so slot-order negatives have to
// be built by splicing rather than by mis-ordering the input.
func buildFrame(docType byte, payload []byte, signers []signer) []byte {
	if len(signers) < 1 || len(signers) > 4 {
		panic(fmt.Sprintf("frame: nsigs %d outside 1..4", len(signers)))
	}
	pre := preImage(docType, payload)
	ss := append([]signer(nil), signers...)
	sort.Slice(ss, func(i, j int) bool { return bytes.Compare(ss[i].kid, ss[j].kid) < 0 })
	for i := 1; i < len(ss); i++ {
		if bytes.Equal(ss[i-1].kid, ss[i].kid) {
			panic("frame: duplicate keyid_trunc in signer set")
		}
	}
	out := append([]byte(nil), pre...)
	out = append(out, byte(len(ss)))
	for _, s := range ss {
		out = append(out, s.kid...)
		out = append(out, ed25519.Sign(s.sk, pre)...)
	}
	if len(out) != 7+len(payload)+1+76*len(ss) {
		panic("frame: exact-length rule violated by the builder itself")
	}
	return out
}

func frameSHA(f []byte) []byte { h := sha256.Sum256(f); return h[:] }
func hexs(x []byte) string     { return hex.EncodeToString(x) }

// ---------------------------------------------------------------- base32

const crockAlphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

// crock encodes bytes as base32 Crockford, most significant bit first, with
// zero pad bits and no "=" padding. 03-WIRE.md section 4.1.
func crock(in []byte) string {
	out := make([]byte, 0, (len(in)*8+4)/5)
	var acc uint32
	var bits uint
	for _, c := range in {
		acc = acc<<8 | uint32(c)
		bits += 8
		for bits >= 5 {
			bits -= 5
			out = append(out, crockAlphabet[(acc>>bits)&31])
		}
	}
	if bits > 0 {
		out = append(out, crockAlphabet[(acc<<(5-bits))&31])
	}
	return string(out)
}

// crockDecode is the inverse, with the I/L/O folding and hyphen skipping the
// reader rules require. It is used to self-check the encoder and to build the
// armored negative fixtures.
func crockDecode(s string) ([]byte, error) {
	var acc uint32
	var bits uint
	out := []byte{}
	for _, r := range s {
		if r == '-' {
			continue
		}
		var v uint32
		switch {
		case r >= '0' && r <= '9':
			v = uint32(r - '0')
		case r == 'I' || r == 'i' || r == 'L' || r == 'l':
			v = 1
		case r == 'O' || r == 'o':
			v = 0
		default:
			up := r
			if up >= 'a' && up <= 'z' {
				up -= 32
			}
			idx := -1
			for i := 0; i < len(crockAlphabet); i++ {
				if rune(crockAlphabet[i]) == up {
					idx = i
					break
				}
			}
			if idx < 0 {
				return nil, fmt.Errorf("crockford: illegal character %q", r)
			}
			v = uint32(idx)
		}
		acc = acc<<5 | v
		bits += 5
		if bits >= 8 {
			bits -= 8
			out = append(out, byte(acc>>bits))
		}
	}
	if bits > 0 && acc&((1<<bits)-1) != 0 {
		return nil, fmt.Errorf("crockford: non-zero trailing pad bits")
	}
	return out, nil
}

// ---------------------------------------------------------------- derivations

func pidOf(rootPK ed25519.PublicKey) []byte { h := sha256.Sum256(rootPK); return h[:8] }
func kidOf(pk ed25519.PublicKey) []byte     { h := sha256.Sum256(pk); return h[:12] }
func linkPin(rootPK ed25519.PublicKey) string {
	h := sha256.Sum256(rootPK)
	return crock(h[:12])
}
func catID(chash []byte) string { return crock(chash[:10]) }
func bidOf(stream []byte) string {
	h := sha256.Sum256(stream)
	return crock(h[:5])
}

// locator, 03-WIRE.md section 4. The uuid is the 36-byte lowercase ASCII text
// with hyphens, not the 16 raw bytes, and gen is four big-endian bytes.
func locator(secret []byte, uuid string, gen uint32) string {
	mac := hmac.New(sha256.New, secret)
	mac.Write([]byte("csm1-loc"))
	mac.Write([]byte{0x00})
	mac.Write([]byte(uuid))
	var g [4]byte
	binary.BigEndian.PutUint32(g[:], gen)
	mac.Write(g[:])
	return crock(mac.Sum(nil)[:15])
}

// ---------------------------------------------------------------- bit source

// detReader is the deterministic bit source used everywhere the corpus needs
// bytes that look random. crypto/rand is not seedable in Go, so a SHA-256
// counter stream over a fixed label is used instead; see README "Determinism".
// No fixture depends on an entropy source.
type detReader struct {
	label string
	ctr   uint64
	buf   []byte
}

func newDet(label string) *detReader { return &detReader{label: label} }

func (d *detReader) Read(p []byte) (int, error) {
	for len(p) > len(d.buf) {
		var c [8]byte
		binary.BigEndian.PutUint64(c[:], d.ctr)
		d.ctr++
		h := sha256.New()
		h.Write([]byte("csm1-det:"))
		h.Write([]byte(d.label))
		h.Write([]byte{0x00})
		h.Write(c[:])
		d.buf = append(d.buf, h.Sum(nil)...)
	}
	n := copy(p, d.buf)
	d.buf = d.buf[n:]
	return n, nil
}

func (d *detReader) bytes(n int) []byte {
	out := make([]byte, n)
	d.Read(out)
	return out
}

// padTo appends a pd field so the finished frame lands on the 256-byte grid
// plus r buckets. 03-WIRE.md section 12.2. Returns the padded payload.
// The caller passes the payload WITHOUT pd; this function re-encodes.
func padTo(c *cmap, docType byte, nsigs int, r int) []byte {
	if c.has(9) {
		panic("padTo: pd already present")
	}
	base := encode(c)
	l0 := 7 + len(base) + 1 + 76*nsigs
	target := padUnit * ((l0+padUnit-1)/padUnit + r)
	d := target - l0
	n, ok := padBytesFor(d)
	if !ok {
		target += padUnit
		d = target - l0
		n, ok = padBytesFor(d)
		if !ok {
			panic("padTo: no reachable padding size")
		}
	}
	if d == 0 {
		return base
	}
	c.set(9, b(make([]byte, n)))
	out := encode(c)
	got := 7 + len(out) + 1 + 76*nsigs
	if got != target {
		panic(fmt.Sprintf("padTo: landed on %d, wanted %d", got, target))
	}
	return out
}
