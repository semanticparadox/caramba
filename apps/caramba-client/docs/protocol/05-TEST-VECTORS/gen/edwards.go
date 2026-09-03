package main

// Edwards25519 arithmetic, written from scratch over math/big, used only to
// COMPUTE the Ed25519 edge-case vectors rather than transcribe them from
// memory or from another project's fixture file.
//
// It produces three things the corpus needs and that no standard library
// exposes:
//
//  1. the eight canonical encodings of the points of order dividing 8, found
//     by multiplying an off-subgroup point by L, so 03-WIRE.md 2.1 clause 3
//     can be tested against real small-order keys;
//  2. the non-canonical spellings (y + p) that clause 1 must reject;
//  3. a (public key, signature) pair that a COFACTORED verifier accepts and a
//     COFACTORLESS verifier rejects, which is the divergence 03-WIRE.md 2.2
//     clause 3 forbids and which nothing else in the corpus can detect.
//
// Speed is irrelevant: this runs a few hundred point operations once.

import (
	"crypto/sha512"
	"math/big"
)

var (
	edP = new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), 255), big.NewInt(19))
	edL = func() *big.Int {
		l, _ := new(big.Int).SetString("7237005577332262213973186563042994240857116359379907606001950938285454250989", 10)
		return l
	}()
	edD = func() *big.Int {
		// d = -121665 * inv(121666) mod p
		num := new(big.Int).Neg(big.NewInt(121665))
		den := new(big.Int).ModInverse(big.NewInt(121666), edP)
		return new(big.Int).Mod(new(big.Int).Mul(num, den), edP)
	}()
	edSqrtM1 = func() *big.Int {
		// sqrt(-1) = 2^((p-1)/4) mod p
		e := new(big.Int).Rsh(new(big.Int).Sub(edP, big.NewInt(1)), 2)
		return new(big.Int).Exp(big.NewInt(2), e, edP)
	}()
)

type edPoint struct{ x, y *big.Int }

func edIdentity() edPoint { return edPoint{big.NewInt(0), big.NewInt(1)} }

func (p edPoint) isIdentity() bool {
	return p.x.Sign() == 0 && p.y.Cmp(big.NewInt(1)) == 0
}

func (p edPoint) equal(q edPoint) bool {
	return p.x.Cmp(q.x) == 0 && p.y.Cmp(q.y) == 0
}

func fmul(a, b *big.Int) *big.Int { return new(big.Int).Mod(new(big.Int).Mul(a, b), edP) }
func fadd(a, b *big.Int) *big.Int { return new(big.Int).Mod(new(big.Int).Add(a, b), edP) }
func fsub(a, b *big.Int) *big.Int { return new(big.Int).Mod(new(big.Int).Sub(a, b), edP) }
func finv(a *big.Int) *big.Int    { return new(big.Int).ModInverse(a, edP) }

func edAdd(p, q edPoint) edPoint {
	// Twisted Edwards addition, a = -1, complete for this curve.
	t := fmul(fmul(edD, fmul(p.x, q.x)), fmul(p.y, q.y))
	x := fmul(fadd(fmul(p.x, q.y), fmul(q.x, p.y)), finv(fadd(big.NewInt(1), t)))
	y := fmul(fadd(fmul(p.y, q.y), fmul(p.x, q.x)), finv(fsub(big.NewInt(1), t)))
	return edPoint{x, y}
}

func edDouble(p edPoint) edPoint { return edAdd(p, p) }

func edScalarMul(k *big.Int, p edPoint) edPoint {
	r := edIdentity()
	acc := p
	kk := new(big.Int).Set(k)
	for kk.Sign() > 0 {
		if kk.Bit(0) == 1 {
			r = edAdd(r, acc)
		}
		acc = edDouble(acc)
		kk.Rsh(kk, 1)
	}
	return r
}

// edEncode produces the 32-byte little-endian encoding with the sign of x in
// bit 255.
func edEncode(p edPoint) []byte {
	out := make([]byte, 32)
	yb := p.y.Bytes()
	for i, c := range yb {
		out[len(yb)-1-i] = c
	}
	if p.x.Bit(0) == 1 {
		out[31] |= 0x80
	}
	return out
}

type decodeResult struct {
	OK         bool
	Canonical  bool // y < p
	OnCurve    bool
	SmallOrder bool // [8]A == identity
	P          edPoint
}

// edDecode implements the three ingest clauses of 03-WIRE.md 2.1 separately,
// so a fixture can say which clause rejects it.
func edDecode(in []byte) decodeResult {
	var r decodeResult
	if len(in) != 32 {
		return r
	}
	b := make([]byte, 32)
	copy(b, in)
	sign := b[31] >> 7
	b[31] &= 0x7f
	y := new(big.Int)
	for i := 31; i >= 0; i-- {
		y.Lsh(y, 8)
		y.Or(y, big.NewInt(int64(b[i])))
	}
	// Clause 1: canonical encoding.
	r.Canonical = y.Cmp(edP) < 0
	yy := fmul(y, y)
	uu := fsub(yy, big.NewInt(1))
	vv := fadd(fmul(edD, yy), big.NewInt(1))
	if vv.Sign() == 0 {
		return r
	}
	// x = (u/v)^((p+3)/8), then fix up.
	e := new(big.Int).Rsh(new(big.Int).Add(edP, big.NewInt(3)), 3)
	x := new(big.Int).Exp(fmul(uu, finv(vv)), e, edP)
	switch {
	case fmul(vv, fmul(x, x)).Cmp(new(big.Int).Mod(uu, edP)) == 0:
	case fmul(vv, fmul(x, x)).Cmp(new(big.Int).Mod(new(big.Int).Neg(uu), edP)) == 0:
		x = fmul(x, edSqrtM1)
	default:
		return r // clause 2: not on the curve
	}
	if x.Sign() == 0 && sign == 1 {
		return r
	}
	if x.Bit(0) != uint(sign) {
		x = fsub(edP, x)
	}
	r.OnCurve = true
	r.P = edPoint{x, y}
	// Clause 3: not of small order.
	r.SmallOrder = edDouble(edDouble(edDouble(r.P))).isIdentity()
	r.OK = r.Canonical && r.OnCurve && !r.SmallOrder
	return r
}

// edBasePoint recovers B by decompressing y = 4/5 and self-checks [L]B == O.
func edBasePoint() edPoint {
	y := fmul(big.NewInt(4), finv(big.NewInt(5)))
	enc := edEncode(edPoint{big.NewInt(0), y}) // sign bit 0
	d := edDecode(enc)
	if !d.OnCurve {
		panic("edwards: basepoint recovery failed")
	}
	if !edScalarMul(edL, d.P).isIdentity() {
		panic("edwards: [L]B is not the identity")
	}
	return d.P
}

// edTorsion returns the eight points of order dividing 8, computed by
// multiplying an off-subgroup point by L and walking the resulting cyclic
// group of order 8.
func edTorsion() []edPoint {
	for cand := int64(2); cand < 1000; cand++ {
		enc := edEncode(edPoint{big.NewInt(0), big.NewInt(cand)})
		d := edDecode(enc)
		if !d.OnCurve {
			continue
		}
		t := edScalarMul(edL, d.P)
		if t.isIdentity() {
			continue
		}
		// Order must be exactly 8 for the walk to enumerate all eight.
		if !edDouble(edDouble(edDouble(t))).isIdentity() {
			continue
		}
		if edDouble(edDouble(t)).isIdentity() {
			continue // order 4, keep looking for a generator
		}
		out := []edPoint{edIdentity()}
		acc := t
		for i := 0; i < 7; i++ {
			out = append(out, acc)
			acc = edAdd(acc, t)
		}
		return out
	}
	panic("edwards: no order-8 torsion generator found")
}

// edHashScalar is SHA-512(R || A || M) reduced mod L, the h of RFC 8032.
func edHashScalar(r, a, msg []byte) *big.Int {
	h := sha512.New()
	h.Write(r)
	h.Write(a)
	h.Write(msg)
	sum := h.Sum(nil)
	le := new(big.Int)
	for i := 63; i >= 0; i-- {
		le.Lsh(le, 8)
		le.Or(le, big.NewInt(int64(sum[i])))
	}
	return le.Mod(le, edL)
}

func edScalarEncode(s *big.Int) []byte {
	out := make([]byte, 32)
	sb := s.Bytes()
	for i, c := range sb {
		out[len(sb)-1-i] = c
	}
	return out
}

// edCofactorDivergence builds a public key and a signature that a COFACTORED
// verifier accepts and a COFACTORLESS verifier rejects.
//
// Construction, with B the basepoint, T a point of order 8, a and k fixed
// scalars, M a fixed message:
//
//	A = [a]B + T          canonical, on curve, NOT of small order
//	R = [k]B              torsion free
//	h = H(R || A || M)
//	S = k + h*a  mod L
//
// Cofactorless: [S]B = [k + h a]B, while R + [h]A = [k + h a]B + [h]T, and
// [h]T is not the identity whenever h is not a multiple of 8, so the two
// sides differ and verification MUST fail.
// Cofactored: [8][S]B = [8](R + [h]A) because [8][h]T = O, so it succeeds.
//
// 03-WIRE.md 2.2 clause 3 requires the cofactorless equation, so the expected
// verdict for this vector is reject in all three implementations.
func edCofactorDivergence(msg []byte) (pub, sig []byte, ok bool) {
	B := edBasePoint()
	tors := edTorsion()
	var T edPoint
	for _, p := range tors {
		if !edDouble(edDouble(edDouble(p))).isIdentity() {
			continue
		}
		if p.isIdentity() || edDouble(edDouble(p)).isIdentity() {
			continue
		}
		T = p // order exactly 8
		break
	}
	a := new(big.Int).SetInt64(0x5c51c1)
	k := new(big.Int).SetInt64(0x2f0a77)
	A := edAdd(edScalarMul(a, B), T)
	if edDouble(edDouble(edDouble(A))).isIdentity() {
		return nil, nil, false
	}
	R := edScalarMul(k, B)
	encA := edEncode(A)
	encR := edEncode(R)
	h := edHashScalar(encR, encA, msg)
	S := new(big.Int).Mod(new(big.Int).Add(k, new(big.Int).Mul(h, a)), edL)
	// Confirm the two equations really do disagree before shipping the vector.
	lhs := edScalarMul(S, B)
	rhs := edAdd(R, edScalarMul(h, A))
	if lhs.equal(rhs) {
		return nil, nil, false // no divergence, do not emit
	}
	l8 := edScalarMul(big.NewInt(8), lhs)
	r8 := edScalarMul(big.NewInt(8), rhs)
	if !l8.equal(r8) {
		return nil, nil, false // cofactored would reject too, not the case we want
	}
	sig = append(append([]byte{}, encR...), edScalarEncode(S)...)
	return encA, sig, true
}

// leToBig reads a little-endian byte string as an integer.
func leToBig(x []byte) *big.Int {
	v := new(big.Int)
	for i := len(x) - 1; i >= 0; i-- {
		v.Lsh(v, 8)
		v.Or(v, big.NewInt(int64(x[i])))
	}
	return v
}
