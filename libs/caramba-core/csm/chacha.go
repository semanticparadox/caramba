package csm

import (
	"crypto/subtle"
	"encoding/binary"
	"errors"
	"math/big"
)

// ChaCha20-Poly1305 по RFC 8439. Написан здесь, а не взят из
// golang.org/x/crypto, по двум причинам: сборка libs/caramba-core по
// умолчанию не тянет ни одной сторонней зависимости кроме yaml.v3, и правило
// 6 задания требует не заводить новых. Алгоритм полностью задан RFC, а корпус
// приносит векторы RFC 9180 A.5, на которых реализация проверяется.
//
// Poly1305 считается на math/big: сообщения здесь не длиннее нескольких
// килобайт, а ключ разовый и выводится из HPKE, так что читаемость важнее
// скорости. Сравнение тега постоянного времени.

var errAEADOpen = errors.New("csm: ChaCha20-Poly1305 authentication failed")

func chachaQuarterRound(s *[16]uint32, a, b, c, d int) {
	s[a] += s[b]
	s[d] ^= s[a]
	s[d] = s[d]<<16 | s[d]>>16
	s[c] += s[d]
	s[b] ^= s[c]
	s[b] = s[b]<<12 | s[b]>>20
	s[a] += s[b]
	s[d] ^= s[a]
	s[d] = s[d]<<8 | s[d]>>24
	s[c] += s[d]
	s[b] ^= s[c]
	s[b] = s[b]<<7 | s[b]>>25
}

// chachaBlock вычисляет один 64-байтовый блок гаммы.
func chachaBlock(key []byte, counter uint32, nonce []byte, out *[64]byte) {
	var s [16]uint32
	s[0], s[1], s[2], s[3] = 0x61707865, 0x3320646e, 0x79622d32, 0x6b206574
	for i := 0; i < 8; i++ {
		s[4+i] = binary.LittleEndian.Uint32(key[i*4:])
	}
	s[12] = counter
	for i := 0; i < 3; i++ {
		s[13+i] = binary.LittleEndian.Uint32(nonce[i*4:])
	}
	w := s
	for i := 0; i < 10; i++ {
		chachaQuarterRound(&w, 0, 4, 8, 12)
		chachaQuarterRound(&w, 1, 5, 9, 13)
		chachaQuarterRound(&w, 2, 6, 10, 14)
		chachaQuarterRound(&w, 3, 7, 11, 15)
		chachaQuarterRound(&w, 0, 5, 10, 15)
		chachaQuarterRound(&w, 1, 6, 11, 12)
		chachaQuarterRound(&w, 2, 7, 8, 13)
		chachaQuarterRound(&w, 3, 4, 9, 14)
	}
	for i := 0; i < 16; i++ {
		binary.LittleEndian.PutUint32(out[i*4:], w[i]+s[i])
	}
}

// chachaXOR накладывает гамму, начиная с указанного счётчика.
func chachaXOR(key []byte, counter uint32, nonce, in []byte) []byte {
	out := make([]byte, len(in))
	var block [64]byte
	for off := 0; off < len(in); off += 64 {
		chachaBlock(key, counter, nonce, &block)
		counter++
		n := len(in) - off
		if n > 64 {
			n = 64
		}
		for i := 0; i < n; i++ {
			out[off+i] = in[off+i] ^ block[i]
		}
	}
	return out
}

var poly1305P = func() *big.Int {
	p := new(big.Int).Lsh(big.NewInt(1), 130)
	return p.Sub(p, big.NewInt(5))
}()

// poly1305 вычисляет 16-байтовый тег по RFC 8439 раздел 2.5.
func poly1305(key, msg []byte) []byte {
	rBytes := make([]byte, 16)
	copy(rBytes, key[:16])
	rBytes[3] &= 15
	rBytes[7] &= 15
	rBytes[11] &= 15
	rBytes[15] &= 15
	rBytes[4] &= 252
	rBytes[8] &= 252
	rBytes[12] &= 252

	r := leToBig(rBytes)
	s := leToBig(key[16:32])
	acc := new(big.Int)

	for off := 0; off < len(msg); off += 16 {
		n := len(msg) - off
		if n > 16 {
			n = 16
		}
		buf := make([]byte, n+1)
		copy(buf, msg[off:off+n])
		buf[n] = 1
		acc.Add(acc, leToBig(buf))
		acc.Mul(acc, r)
		acc.Mod(acc, poly1305P)
	}
	acc.Add(acc, s)

	tag := make([]byte, 16)
	b := acc.Bytes()
	// acc может быть шире 16 байт: берём младшие 16 в little-endian.
	for i := 0; i < 16 && i < len(b); i++ {
		tag[i] = b[len(b)-1-i]
	}
	return tag
}

// leToBig читает little-endian целое.
func leToBig(b []byte) *big.Int {
	be := make([]byte, len(b))
	for i := range b {
		be[len(b)-1-i] = b[i]
	}
	return new(big.Int).SetBytes(be)
}

func pad16(n int) int {
	if n%16 == 0 {
		return 0
	}
	return 16 - n%16
}

// chachaPolyOpen расшифровывает и проверяет AEAD ChaCha20-Poly1305.
// ct несёт 16-байтовый тег Poly1305 в конце.
func chachaPolyOpen(key, nonce, aad, ct []byte) ([]byte, error) {
	if len(key) != 32 || len(nonce) != 12 {
		return nil, errAEADOpen
	}
	if len(ct) < 16 {
		return nil, errAEADOpen
	}
	body := ct[:len(ct)-16]
	tag := ct[len(ct)-16:]

	var block [64]byte
	chachaBlock(key, 0, nonce, &block)
	polyKey := make([]byte, 32)
	copy(polyKey, block[:32])

	mac := make([]byte, 0, len(aad)+pad16(len(aad))+len(body)+pad16(len(body))+16)
	mac = append(mac, aad...)
	mac = append(mac, make([]byte, pad16(len(aad)))...)
	mac = append(mac, body...)
	mac = append(mac, make([]byte, pad16(len(body)))...)
	var lens [16]byte
	binary.LittleEndian.PutUint64(lens[0:], uint64(len(aad)))
	binary.LittleEndian.PutUint64(lens[8:], uint64(len(body)))
	mac = append(mac, lens[:]...)

	want := poly1305(polyKey, mac)
	if subtle.ConstantTimeCompare(want, tag) != 1 {
		return nil, errAEADOpen
	}
	return chachaXOR(key, 1, nonce, body), nil
}

// chachaPolySeal шифрует. Нужен только тестам пакета: верификатор запечатывать
// не умеет и не должен.
func chachaPolySeal(key, nonce, aad, pt []byte) []byte {
	var block [64]byte
	chachaBlock(key, 0, nonce, &block)
	polyKey := make([]byte, 32)
	copy(polyKey, block[:32])

	body := chachaXOR(key, 1, nonce, pt)

	mac := make([]byte, 0, len(aad)+pad16(len(aad))+len(body)+pad16(len(body))+16)
	mac = append(mac, aad...)
	mac = append(mac, make([]byte, pad16(len(aad)))...)
	mac = append(mac, body...)
	mac = append(mac, make([]byte, pad16(len(body)))...)
	var lens [16]byte
	binary.LittleEndian.PutUint64(lens[0:], uint64(len(aad)))
	binary.LittleEndian.PutUint64(lens[8:], uint64(len(body)))
	mac = append(mac, lens[:]...)

	return append(body, poly1305(polyKey, mac)...)
}
