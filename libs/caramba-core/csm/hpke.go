package csm

import (
	"crypto/ecdh"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/binary"
	"errors"
)

// HPKE, RFC 9180, режим base, набор DHKEM(P-256, HKDF-SHA256) + HKDF-SHA256 +
// ChaCha20Poly1305 (03-WIRE.md 9.1). Реализована только сторона получателя:
// верификатор распечатывает, но никогда не запечатывает.
//
// Коррекция 4 раздела 16: KEM это P-256, а не X25519, потому что ключ
// согласования устройства обязан жить в Secure Enclave или StrongBox, а ни то
// ни другое не держит X25519.

const (
	hpkeNsecret = 32
	hpkeNk      = 32
	hpkeNn      = 12
	hpkeNh      = 32
)

var errHPKE = errors.New("csm: HPKE open failed")

// hkdfExtract это HKDF-Extract на SHA-256: PRK = HMAC(salt, ikm).
func hkdfExtract(salt, ikm []byte) []byte {
	if len(salt) == 0 {
		salt = make([]byte, sha256.Size)
	}
	m := hmac.New(sha256.New, salt)
	m.Write(ikm)
	return m.Sum(nil)
}

// hkdfExpand это HKDF-Expand на SHA-256.
func hkdfExpand(prk, info []byte, length int) []byte {
	out := make([]byte, 0, length)
	var prev []byte
	for i := 1; len(out) < length; i++ {
		m := hmac.New(sha256.New, prk)
		m.Write(prev)
		m.Write(info)
		m.Write([]byte{byte(i)})
		prev = m.Sum(nil)
		out = append(out, prev...)
	}
	return out[:length]
}

func i2osp2(n int) []byte {
	var b [2]byte
	binary.BigEndian.PutUint16(b[:], uint16(n))
	return b[:]
}

// kemSuiteID это "KEM" || I2OSP(kem_id, 2).
func kemSuiteID() []byte {
	return append([]byte("KEM"), i2osp2(int(HPKEKemID))...)
}

// hpkeSuiteID это "HPKE" || kem_id || kdf_id || aead_id.
func hpkeSuiteID() []byte {
	out := []byte("HPKE")
	out = append(out, i2osp2(int(HPKEKemID))...)
	out = append(out, i2osp2(int(HPKEKdfID))...)
	return append(out, i2osp2(int(HPKEAeadID))...)
}

func labeledExtract(suiteID, salt []byte, label string, ikm []byte) []byte {
	buf := make([]byte, 0, 7+len(suiteID)+len(label)+len(ikm))
	buf = append(buf, []byte("HPKE-v1")...)
	buf = append(buf, suiteID...)
	buf = append(buf, []byte(label)...)
	buf = append(buf, ikm...)
	return hkdfExtract(salt, buf)
}

func labeledExpand(suiteID, prk []byte, label string, info []byte, length int) []byte {
	buf := make([]byte, 0, 2+7+len(suiteID)+len(label)+len(info))
	buf = append(buf, i2osp2(length)...)
	buf = append(buf, []byte("HPKE-v1")...)
	buf = append(buf, suiteID...)
	buf = append(buf, []byte(label)...)
	buf = append(buf, info...)
	return hkdfExpand(prk, buf, length)
}

// dhkemDecap выводит общий секрет на стороне получателя.
// enc это 65-байтовая несжатая точка отправителя, skR это 32-байтовый скаляр.
func dhkemDecap(skR, enc []byte) ([]byte, error) {
	curve := ecdh.P256()
	priv, err := curve.NewPrivateKey(skR)
	if err != nil {
		return nil, errHPKE
	}
	pub, err := curve.NewPublicKey(enc)
	if err != nil {
		return nil, errHPKE
	}
	dh, err := priv.ECDH(pub)
	if err != nil {
		return nil, errHPKE
	}
	suite := kemSuiteID()
	kemContext := make([]byte, 0, len(enc)+65)
	kemContext = append(kemContext, enc...)
	kemContext = append(kemContext, priv.PublicKey().Bytes()...)

	eaePrk := labeledExtract(suite, nil, "eae_prk", dh)
	return labeledExpand(suite, eaePrk, "shared_secret", kemContext, hpkeNsecret), nil
}

// hpkeKeySchedule это KeySchedule<mode_base> RFC 9180 5.1.
// Возвращает key, base_nonce, exporter_secret и key_schedule_context.
func hpkeKeySchedule(sharedSecret, info []byte) (key, baseNonce, exporter, ksc []byte) {
	suite := hpkeSuiteID()
	pskIDHash := labeledExtract(suite, nil, "psk_id_hash", nil)
	infoHash := labeledExtract(suite, nil, "info_hash", info)

	ksc = make([]byte, 0, 1+len(pskIDHash)+len(infoHash))
	ksc = append(ksc, HPKEModeBase)
	ksc = append(ksc, pskIDHash...)
	ksc = append(ksc, infoHash...)

	secret := labeledExtract(suite, sharedSecret, "secret", nil)
	key = labeledExpand(suite, secret, "key", ksc, hpkeNk)
	baseNonce = labeledExpand(suite, secret, "base_nonce", ksc, hpkeNn)
	exporter = labeledExpand(suite, secret, "exp", ksc, hpkeNh)
	return key, baseNonce, exporter, ksc
}

// hpkeNonce накладывает номер сообщения на base_nonce.
func hpkeNonce(baseNonce []byte, seq uint64) []byte {
	out := make([]byte, len(baseNonce))
	copy(out, baseNonce)
	var s [8]byte
	binary.BigEndian.PutUint64(s[:], seq)
	for i := 0; i < 8; i++ {
		out[len(out)-8+i] ^= s[i]
	}
	return out
}

// hpkeOpenBase распечатывает одно сообщение с номером 0 в режиме base.
func hpkeOpenBase(skR, enc, info, aad, ct []byte) ([]byte, error) {
	shared, err := dhkemDecap(skR, enc)
	if err != nil {
		return nil, err
	}
	key, baseNonce, _, _ := hpkeKeySchedule(shared, info)
	pt, err := chachaPolyOpen(key, hpkeNonce(baseNonce, 0), aad, ct)
	if err != nil {
		return nil, errHPKE
	}
	return pt, nil
}
