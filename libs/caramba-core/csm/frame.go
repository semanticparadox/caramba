package csm

import (
	"bytes"
	"crypto/sha256"
	"errors"
	"fmt"
)

// Кадр CSM/1, 03-WIRE.md раздел 1.
//
//	offset  size            field
//	0       4               magic       = "CSM1"
//	4       1               doc_type
//	5       2               payload_len u16 big-endian, 1..49152
//	7       payload_len     payload     строгий CBOR
//	7+L     1               nsigs       1..4
//	8+L     76 * nsigs      signatures  nsigs * { keyid_trunc(12) || sig(64) }

// Константы кадра, 03-WIRE.md раздел 17.
const (
	MagicLen      = 4
	HeaderLen     = 7  // magic + doc_type + payload_len
	SlotLen       = 76 // keyid_trunc(12) + sig(64)
	KeyIDTruncLen = 12
	SigLen        = 64
	PIDLen        = 8
	MinPayloadLen = 1
	MaxPayloadLen = 49152
	MinNsigs      = 1
	MaxNsigs      = 4

	PadUnit           = 256
	RespMax           = 4096
	DocFrameMax       = 4096
	ChunkPayloadMax   = 2816
	ChunkRespMax      = 3584
	InnerDirectiveMax = 2816
	PanelWarn         = 12288
	PanelRefuse       = 49152
	SkewSeconds       = 300
)

// Magic это первые четыре байта каждого кадра. Они входят в подписываемый
// прообраз, поэтому подпись v1 нельзя воспроизвести под будущим magic.
var Magic = [MagicLen]byte{'C', 'S', 'M', '1'}

// Типы документов, 03-WIRE.md 1.2.
const (
	DocKey       uint8 = 0x01 // k1,  ключевой документ
	DocCatalog   uint8 = 0x02 // c1,  каталог
	DocDirective uint8 = 0x03 // m1,  директива
	DocChunk     uint8 = 0x04 // c1c, фрагмент каталога
	DocBootstrap uint8 = 0x05 // b1,  bootstrap blob
	DocSealed    uint8 = 0x06 // m1s, запечатанная директива
	DocReserve   uint8 = 0x08 // r1,  резервный пул зеркал
)

// docTypeDefined это множество значений doc_type, определённых в v1.
// 0x00 недопустим, 0x07 зарезервирован под роль timestamp и не выпускается,
// 0x09..0xEF зарезервированы, 0xF0..0xFF частное пространство.
var docTypeDefined = map[uint8]bool{
	DocKey: true, DocCatalog: true, DocDirective: true, DocChunk: true,
	DocBootstrap: true, DocSealed: true, DocReserve: true,
}

// DocTypeName возвращает короткое имя типа документа.
func DocTypeName(dt uint8) string {
	switch dt {
	case DocKey:
		return "k1"
	case DocCatalog:
		return "c1"
	case DocDirective:
		return "m1"
	case DocChunk:
		return "c1c"
	case DocBootstrap:
		return "b1"
	case DocSealed:
		return "m1s"
	case DocReserve:
		return "r1"
	}
	return fmt.Sprintf("0x%02x", dt)
}

// SigSlot это один слот подписи, 03-WIRE.md 1.4. keyid_trunc это подсказка
// для поиска, а не авторизация: авторизацию даёт таблица ролей раздела 7.
type SigSlot struct {
	KeyID [KeyIDTruncLen]byte
	Sig   [SigLen]byte
}

// Frame это разобранный кадр. Payload, PreImage и Raw это срезы одного и того
// же входного буфера: пакет проверяет подпись по принятым байтам и никогда не
// пересобирает их.
type Frame struct {
	Raw      []byte
	DocType  uint8
	Payload  []byte
	PreImage []byte // Raw[:7+payload_len], ровно то, что подписано
	Sigs     []SigSlot
}

// Hash возвращает sha256 всего кадра, слоты подписей включительно. Это chash
// каталога (03-WIRE.md раздел 4): переподпись даёт другой каталог.
func (f *Frame) Hash() [32]byte { return sha256.Sum256(f.Raw) }

// ParseFrame выполняет шаги P1..P8 раздела 6.1. Разбор полезной нагрузки
// (P9..P12) делает ParseDocument.
func ParseFrame(raw []byte) (*Frame, error) {
	// P1
	if len(raw) < HeaderLen+1 {
		return nil, errf(EParseShort, "P1", "total_len %d is below the 8 byte minimum", len(raw))
	}
	// P2
	if !bytes.Equal(raw[0:MagicLen], Magic[:]) {
		return nil, errf(EParseMagic, "P2", "magic %x is not CSM1", raw[0:MagicLen])
	}
	// P3
	dt := raw[4]
	if !docTypeDefined[dt] {
		return nil, errf(EParseDocType, "P3", "doc_type 0x%02x is undefined, reserved or private", dt)
	}
	// P4
	payloadLen := int(raw[5])<<8 | int(raw[6])
	if payloadLen < MinPayloadLen || payloadLen > MaxPayloadLen {
		return nil, errf(EParseLen, "P4", "payload_len %d is outside 1..%d", payloadLen, MaxPayloadLen)
	}
	// P5
	if len(raw) < HeaderLen+payloadLen+1 {
		return nil, errf(EParseShort, "P5", "total_len %d is below 8 + payload_len %d", len(raw), payloadLen)
	}
	// P6
	nsigs := int(raw[HeaderLen+payloadLen])
	if nsigs < MinNsigs || nsigs > MaxNsigs {
		return nil, errf(EParseNsigs, "P6", "nsigs %d is outside 1..%d", nsigs, MaxNsigs)
	}
	// P7, точная длина. Проверяется до любой работы с подписями, и это то,
	// что делает безопасным вынос nsigs за пределы прообраза.
	want := HeaderLen + payloadLen + 1 + SlotLen*nsigs
	if len(raw) != want {
		return nil, errf(EParseFraming, "P7", "total_len %d, exact rule requires %d", len(raw), want)
	}

	f := &Frame{
		Raw:      raw,
		DocType:  dt,
		Payload:  raw[HeaderLen : HeaderLen+payloadLen],
		PreImage: raw[:HeaderLen+payloadLen],
		Sigs:     make([]SigSlot, nsigs),
	}
	off := HeaderLen + payloadLen + 1
	for i := 0; i < nsigs; i++ {
		copy(f.Sigs[i].KeyID[:], raw[off:off+KeyIDTruncLen])
		copy(f.Sigs[i].Sig[:], raw[off+KeyIDTruncLen:off+SlotLen])
		off += SlotLen
	}
	// P8, строго возрастающий порядок слотов. Верификатор обязан отвергнуть,
	// а не отсортировать: порядок делает кадр с данным набором подписантов
	// байт-уникальным, чем и живёт адресация каталога по содержимому.
	for i := 1; i < nsigs; i++ {
		switch bytes.Compare(f.Sigs[i-1].KeyID[:], f.Sigs[i].KeyID[:]) {
		case 0:
			return nil, errf(EParseSlotOrder, "P8", "duplicate keyid_trunc %x in slots %d and %d",
				f.Sigs[i].KeyID[:], i-1, i)
		case 1:
			return nil, errf(EParseSlotOrder, "P8", "slots %d and %d are not in ascending keyid_trunc order", i-1, i)
		}
	}
	return f, nil
}

// BuildFrame собирает кадр. Слоты сортируются по возрастанию keyid_trunc, как
// требует 1.4; дубликат keyid это ошибка сборки, а не молчаливая склейка.
func BuildFrame(docType uint8, payload []byte, slots []SigSlot) ([]byte, error) {
	if !docTypeDefined[docType] {
		return nil, fmt.Errorf("csm: doc_type 0x%02x is not emittable", docType)
	}
	if len(payload) < MinPayloadLen || len(payload) > MaxPayloadLen {
		return nil, fmt.Errorf("csm: payload_len %d is outside 1..%d", len(payload), MaxPayloadLen)
	}
	if len(slots) < MinNsigs || len(slots) > MaxNsigs {
		return nil, fmt.Errorf("csm: nsigs %d is outside 1..%d", len(slots), MaxNsigs)
	}
	sorted := make([]SigSlot, len(slots))
	copy(sorted, slots)
	for i := 1; i < len(sorted); i++ {
		for j := i; j > 0 && bytes.Compare(sorted[j-1].KeyID[:], sorted[j].KeyID[:]) > 0; j-- {
			sorted[j-1], sorted[j] = sorted[j], sorted[j-1]
		}
	}
	for i := 1; i < len(sorted); i++ {
		if bytes.Equal(sorted[i-1].KeyID[:], sorted[i].KeyID[:]) {
			return nil, errors.New("csm: two signature slots carry the same keyid_trunc")
		}
	}

	out := make([]byte, 0, HeaderLen+len(payload)+1+SlotLen*len(sorted))
	out = append(out, Magic[:]...)
	out = append(out, docType)
	out = append(out, byte(len(payload)>>8), byte(len(payload)))
	out = append(out, payload...)
	out = append(out, byte(len(sorted)))
	for _, s := range sorted {
		out = append(out, s.KeyID[:]...)
		out = append(out, s.Sig[:]...)
	}
	return out, nil
}

// SigningPreImage возвращает подписываемые байты для полезной нагрузки:
// magic || doc_type || u16be(payload_len) || payload (03-WIRE.md 1.3).
// nsigs и слоты в прообраз не входят.
func SigningPreImage(docType uint8, payload []byte) []byte {
	out := make([]byte, 0, HeaderLen+len(payload))
	out = append(out, Magic[:]...)
	out = append(out, docType)
	out = append(out, byte(len(payload)>>8), byte(len(payload)))
	return append(out, payload...)
}

// SplitFrameStream разбирает поток кадров (03-WIRE.md 10.1): последовательность
// полных кадров, где длина каждого вычисляется из его собственных payload_len и
// nsigs. Остаток после последнего полного кадра и обрезанный последний кадр
// одинаково дают E_PARSE_FRAMING.
const (
	MaxStreamFrames = 16
	MaxStreamBytes  = 65536
)

func SplitFrameStream(stream []byte) ([][]byte, error) {
	if len(stream) > MaxStreamBytes {
		return nil, errf(EParseFraming, "10.1", "frame stream of %d bytes exceeds the %d byte cap", len(stream), MaxStreamBytes)
	}
	var out [][]byte
	off := 0
	for off < len(stream) {
		if len(out) == MaxStreamFrames {
			return nil, errf(EParseFraming, "10.1", "frame stream exceeds the %d frame cap", MaxStreamFrames)
		}
		rest := stream[off:]
		if len(rest) < HeaderLen+1 {
			return nil, errf(EParseFraming, "10.1", "truncated final frame: %d bytes left", len(rest))
		}
		payloadLen := int(rest[5])<<8 | int(rest[6])
		if payloadLen < MinPayloadLen || payloadLen > MaxPayloadLen {
			return nil, errf(EParseFraming, "10.1", "frame %d declares payload_len %d", len(out), payloadLen)
		}
		if len(rest) < HeaderLen+payloadLen+1 {
			return nil, errf(EParseFraming, "10.1", "truncated final frame")
		}
		nsigs := int(rest[HeaderLen+payloadLen])
		if nsigs < MinNsigs || nsigs > MaxNsigs {
			return nil, errf(EParseFraming, "10.1", "frame %d declares nsigs %d", len(out), nsigs)
		}
		total := HeaderLen + payloadLen + 1 + SlotLen*nsigs
		if len(rest) < total {
			return nil, errf(EParseFraming, "10.1", "truncated final frame: need %d bytes, have %d", total, len(rest))
		}
		out = append(out, rest[:total])
		off += total
	}
	if len(out) == 0 {
		return nil, errf(EParseFraming, "10.1", "empty frame stream")
	}
	return out, nil
}
