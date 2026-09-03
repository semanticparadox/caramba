package csm

import (
	"crypto/ed25519"
	"errors"
	"math/big"
)

// Строгий профиль Ed25519, 03-WIRE.md раздел 2. Это требование соответствия,
// а не совет.
//
// crypto/ed25519.Verify из стандартной библиотеки выполняет пункт 1 раздела
// 2.2 (каноничность S) и пункт 3 (бескофакторное уравнение RFC 8032 5.1.7),
// но НЕ выполняет пункт 3 раздела 2.1 (отсев точек малого порядка). Поэтому
// приём ключа написан здесь явно.
//
// Арифметика на math/big намеренно не постоянного времени: она работает
// только над ОТКРЫТЫМИ ключами из принятых байтов, секрета в этом пути нет.
// Собственная реализация выбрана вместо новой зависимости: в графе модуля
// есть только github.com/metacubex/edwards25519 (косвенная), а
// filippo.io/edwards25519, на который ссылается 03-WIRE.md 2.3, отсутствует
// (коррекция cor-8 корпуса).

var (
	edP      *big.Int // 2^255 - 19
	edD      *big.Int // -121665/121666 mod p
	edSqrtM1 *big.Int // 2^((p-1)/4) mod p
	edPm5d8  *big.Int // (p-5)/8
)

func init() {
	one := big.NewInt(1)
	edP = new(big.Int).Lsh(one, 255)
	edP.Sub(edP, big.NewInt(19))

	num := big.NewInt(-121665)
	den := big.NewInt(121666)
	inv := new(big.Int).ModInverse(den, edP)
	edD = new(big.Int).Mul(num, inv)
	edD.Mod(edD, edP)

	e := new(big.Int).Sub(edP, one)
	e.Rsh(e, 2) // (p-1)/4
	edSqrtM1 = new(big.Int).Exp(big.NewInt(2), e, edP)

	edPm5d8 = new(big.Int).Sub(edP, big.NewInt(5))
	edPm5d8.Rsh(edPm5d8, 3)
}

// ErrPubKeyNonCanonical: пункт 1 раздела 2.1, y >= p.
var ErrPubKeyNonCanonical = errors.New("csm: ed25519 public key encoding is non-canonical (y >= p)")

// ErrPubKeyNotOnCurve: пункт 2 раздела 2.1, распаковка не даёт точку кривой.
var ErrPubKeyNotOnCurve = errors.New("csm: ed25519 public key does not decompress to a curve point")

// ErrPubKeySmallOrder: пункт 3 раздела 2.1, [8]A это нейтральный элемент.
var ErrPubKeySmallOrder = errors.New("csm: ed25519 public key is of small order ([8]A is the identity)")

// ErrPubKeyLength: длина не 32 байта.
var ErrPubKeyLength = errors.New("csm: ed25519 public key must be exactly 32 bytes")

// edPoint это точка в расширенных координатах (X:Y:Z:T), x = X/Z, y = Y/Z.
type edPoint struct{ X, Y, Z, T *big.Int }

// isIdentity: нейтральный элемент это (0:1), то есть X == 0 и Y == Z.
func (p *edPoint) isIdentity() bool {
	return p.X.Sign() == 0 && p.Y.Cmp(p.Z) == 0 && p.Z.Sign() != 0
}

// double это удвоение для скрученной кривой Эдвардса с a = -1 (dbl-2008-hwcd).
func (p *edPoint) double() *edPoint {
	mod := func(z *big.Int) *big.Int { return z.Mod(z, edP) }

	a := mod(new(big.Int).Mul(p.X, p.X))
	b := mod(new(big.Int).Mul(p.Y, p.Y))
	c := mod(new(big.Int).Lsh(new(big.Int).Mul(p.Z, p.Z), 1))
	h := mod(new(big.Int).Add(a, b))
	xy := mod(new(big.Int).Add(p.X, p.Y))
	e := mod(new(big.Int).Sub(h, mod(new(big.Int).Mul(xy, xy))))
	g := mod(new(big.Int).Sub(a, b))
	f := mod(new(big.Int).Add(c, g))

	return &edPoint{
		X: mod(new(big.Int).Mul(e, f)),
		Y: mod(new(big.Int).Mul(g, h)),
		T: mod(new(big.Int).Mul(e, h)),
		Z: mod(new(big.Int).Mul(f, g)),
	}
}

// edDecompress распаковывает 32-байтовую кодировку точки, применяя пункты 1 и
// 2 раздела 2.1.
func edDecompress(a []byte) (*edPoint, error) {
	if len(a) != 32 {
		return nil, ErrPubKeyLength
	}
	// Little-endian, старший бит последнего байта это знак x.
	le := make([]byte, 32)
	copy(le, a)
	sign := le[31] >> 7
	le[31] &= 0x7f
	be := make([]byte, 32)
	for i := 0; i < 32; i++ {
		be[i] = le[31-i]
	}
	y := new(big.Int).SetBytes(be)

	// Пункт 1: строго y < p.
	if y.Cmp(edP) >= 0 {
		return nil, ErrPubKeyNonCanonical
	}

	// Пункт 2: x^2 = (y^2 - 1) / (d*y^2 + 1).
	y2 := new(big.Int).Mul(y, y)
	y2.Mod(y2, edP)
	u := new(big.Int).Sub(y2, big.NewInt(1))
	u.Mod(u, edP)
	v := new(big.Int).Mul(edD, y2)
	v.Add(v, big.NewInt(1))
	v.Mod(v, edP)
	if v.Sign() == 0 {
		return nil, ErrPubKeyNotOnCurve
	}

	v3 := new(big.Int).Mul(v, v)
	v3.Mod(v3, edP)
	v3.Mul(v3, v)
	v3.Mod(v3, edP)
	v7 := new(big.Int).Mul(v3, v3)
	v7.Mod(v7, edP)
	v7.Mul(v7, v)
	v7.Mod(v7, edP)

	uv7 := new(big.Int).Mul(u, v7)
	uv7.Mod(uv7, edP)
	pow := new(big.Int).Exp(uv7, edPm5d8, edP)

	x := new(big.Int).Mul(u, v3)
	x.Mod(x, edP)
	x.Mul(x, pow)
	x.Mod(x, edP)

	check := new(big.Int).Mul(x, x)
	check.Mod(check, edP)
	check.Mul(check, v)
	check.Mod(check, edP)

	switch {
	case check.Cmp(u) == 0:
		// x годится как есть.
	default:
		negU := new(big.Int).Sub(edP, u)
		negU.Mod(negU, edP)
		if check.Cmp(negU) != 0 {
			return nil, ErrPubKeyNotOnCurve
		}
		x.Mul(x, edSqrtM1)
		x.Mod(x, edP)
	}

	if x.Sign() == 0 && sign == 1 {
		return nil, ErrPubKeyNotOnCurve
	}
	if byte(x.Bit(0)) != sign {
		x.Sub(edP, x)
		x.Mod(x, edP)
	}

	t := new(big.Int).Mul(x, y)
	t.Mod(t, edP)
	return &edPoint{X: x, Y: new(big.Int).Set(y), Z: big.NewInt(1), T: t}, nil
}

// CheckPublicKey применяет все три пункта раздела 2.1 к 32-байтовому открытому
// ключу Ed25519. Проверка выполняется на КАЖДОМ использовании ключа, а не
// кешируется битом: в P12 на каждом pk внутри ключевого документа, где
// материал входит в доверенное множество, и в V6 на ключе каждого слота.
func CheckPublicKey(pk []byte) error {
	p, err := edDecompress(pk)
	if err != nil {
		return err
	}
	// Пункт 3: [8]A не должно быть нейтральным элементом. Реализовано как три
	// удвоения и тест на нейтральность, а не как список запрещённых кодировок:
	// список легко переписать с ошибкой, предикат точен.
	q := p.double().double().double()
	if q.isIdentity() {
		return ErrPubKeySmallOrder
	}
	return nil
}

// VerifySignature проверяет подпись по разделу 2.2: каноничность S,
// корректность R и бескофакторное уравнение [S]B == R + [SHA-512(R||A||M)]A.
// Открытый ключ обязан быть предварительно пропущен через CheckPublicKey.
//
// crypto/ed25519.Verify реализует ровно этот набор: он отвергает S >= L и
// сравнивает сжатые кодировки, то есть кофакторную форму НЕ использует.
func VerifySignature(pk, msg, sig []byte) bool {
	if len(pk) != ed25519.PublicKeySize || len(sig) != ed25519.SignatureSize {
		return false
	}
	return ed25519.Verify(ed25519.PublicKey(pk), msg, sig)
}
