package csm

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/binary"
	"errors"
	"strings"
)

// Производные идентификаторы, 03-WIRE.md раздел 4. Ни один идентификатор в
// CSM/1 не выдаётся последовательностью базы: последовательность коррелирует
// арендаторов, а производное значение верификатор может пересчитать сам.

// CrockfordAlphabet это алфавит base32 Crockford, индексы 0..31.
// I, L, O и U отсутствуют по построению.
const CrockfordAlphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

// ErrCrockford это отказ декодера base32 Crockford. Через армированный
// читатель он отображается в E_PARSE_FRAMING (03-WIRE.md 6.6).
var ErrCrockford = errors.New("csm: invalid base32 Crockford input")

// Base32CrockfordEncode кодирует байты как поток бит, старший бит первым, по
// пять бит на символ. Хвост дополняется нулевыми битами, символов заполнения
// нет никогда.
func Base32CrockfordEncode(b []byte) string {
	var sb strings.Builder
	sb.Grow((len(b)*8 + 4) / 5)
	var acc uint32
	var bits uint
	for _, x := range b {
		acc = acc<<8 | uint32(x)
		bits += 8
		for bits >= 5 {
			bits -= 5
			sb.WriteByte(CrockfordAlphabet[(acc>>bits)&0x1f])
		}
	}
	if bits > 0 {
		sb.WriteByte(CrockfordAlphabet[(acc<<(5-bits))&0x1f])
	}
	return sb.String()
}

// crockfordValue отображает символ в значение. Строчные принимаются, I, i, L и
// l дают 1, O и o дают 0, дефис игнорируется вызывающим.
func crockfordValue(c byte) (uint32, bool) {
	switch {
	case c >= '0' && c <= '9':
		return uint32(c - '0'), true
	case c == 'O' || c == 'o':
		return 0, true
	case c == 'I' || c == 'i' || c == 'L' || c == 'l':
		return 1, true
	}
	up := c
	if up >= 'a' && up <= 'z' {
		up -= 32
	}
	for i := 0; i < len(CrockfordAlphabet); i++ {
		if CrockfordAlphabet[i] == up {
			return uint32(i), true
		}
	}
	return 0, false
}

// Base32CrockfordDecode декодирует строку. Дефисы игнорируются в любом месте,
// любой другой символ вне алфавита это отказ, и ненулевые хвостовые биты
// заполнения тоже отказ: у каждой строки байт ровно одно принимаемое написание.
func Base32CrockfordDecode(s string) ([]byte, error) {
	out := make([]byte, 0, len(s)*5/8+1)
	var acc uint32
	var bits uint
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c == '-' {
			continue
		}
		v, ok := crockfordValue(c)
		if !ok {
			return nil, ErrCrockford
		}
		acc = acc<<5 | v
		bits += 5
		if bits >= 8 {
			bits -= 8
			out = append(out, byte(acc>>bits))
		}
	}
	if bits > 0 && acc&((1<<bits)-1) != 0 {
		return nil, ErrCrockford
	}
	return out, nil
}

// PIDOf возвращает sha256(root_pk)[0..8], идентичность арендатора.
func PIDOf(rootPub []byte) []byte {
	h := sha256.Sum256(rootPub)
	return h[:PIDLen]
}

// KeyIDOf возвращает sha256(pk)[0..12], усечённый идентификатор ключа.
func KeyIDOf(pub []byte) []byte {
	h := sha256.Sum256(pub)
	return h[:KeyIDTruncLen]
}

// LinkPin возвращает base32_crockford(sha256(root_pk)[0..12]), 20 символов.
func LinkPin(rootPub []byte) string {
	return Base32CrockfordEncode(KeyIDOf(rootPub))
}

// LocatorOf выводит локатор, 24 символа:
// base32_crockford(HMAC-SHA256(secret, "csm1-loc" || 0x00 || uuid || u32be(gen))[0..15]).
// uuid это ASCII-текст ровно как его хранит панель, 36 байт в нижнем регистре
// с дефисами, а не 16 сырых байт.
func LocatorOf(secret []byte, subscriptionUUID string, gen uint32) string {
	mac := hmac.New(sha256.New, secret)
	mac.Write([]byte("csm1-loc"))
	mac.Write([]byte{0x00})
	mac.Write([]byte(subscriptionUUID))
	var g [4]byte
	binary.BigEndian.PutUint32(g[:], gen)
	mac.Write(g[:])
	sum := mac.Sum(nil)
	return Base32CrockfordEncode(sum[:15])
}

// DeviceThumbprint возвращает sha256(device_signing_SPKI_DER)[0..16].
func DeviceThumbprint(spkiDER []byte) []byte {
	h := sha256.Sum256(spkiDER)
	return h[:16]
}

// CatalogHash возвращает chash, sha256 полного кадра каталога.
func CatalogHash(frame []byte) []byte {
	h := sha256.Sum256(frame)
	return h[:]
}

// CatalogID возвращает cat_id, base32_crockford(chash[0..10]), 16 символов.
func CatalogID(chash []byte) string {
	return Base32CrockfordEncode(chash[:10])
}

// ValidCatalogID проверяет, что строка это cat_id: 16 символов канонического
// алфавита base32 Crockford (10 байт chash). Идентификатор попадает в путь
// URL и в путь файла, поэтому проверка набора символов принадлежит границе, а
// не месту склейки.
func ValidCatalogID(s string) bool {
	if len(s) != 16 {
		return false
	}
	for i := 0; i < len(s); i++ {
		if strings.IndexByte(CrockfordAlphabet, s[i]) < 0 {
			return false
		}
	}
	return true
}

// BundleID возвращает bid армированного потока,
// base32_crockford(sha256(stream)[0..5]), 8 символов.
func BundleID(stream []byte) string {
	h := sha256.Sum256(stream)
	return Base32CrockfordEncode(h[:5])
}
