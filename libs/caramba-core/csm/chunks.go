package csm

import (
	"bytes"
	"crypto/sha256"
)

// Пересборка каталога из фрагментов, 03-WIRE.md 8.4.
//
// Фрагмент несёт срез полного КАДРА каталога, а не его полезной нагрузки.
// Каждый фрагмент подписан отдельно, поэтому подделанный фрагмент ловится ДО
// пересборки, а собранные байты это полный кадр 0x02, который затем
// проверяется ещё раз целиком. Две независимые проверки, один путь кода.
//
// Чанкуется КАЖДЫЙ каталог, включая односегментный: ветки по размеру нет
// нигде, и это стоит своего конверта.

// ChunkSet накапливает проверенные фрагменты одного каталога.
type ChunkSet struct {
	cid    []byte
	n      uint64
	tl     uint64
	wantN  uint64
	parts  map[uint64][]byte
	frames map[uint64][]byte
}

// NewChunkSet создаёт пустой накопитель без ожидаемого числа фрагментов.
func NewChunkSet() *ChunkSet {
	return &ChunkSet{parts: map[uint64][]byte{}, frames: map[uint64][]byte{}}
}

// NewChunkSetFor создаёт накопитель, знающий cn доверенной директивы.
// 03-WIRE.md 8.4 требует n == directive.cn как отдельное правило, и без него
// расхождение всплывало бы косвенно, отказом по индексу, то есть с верным
// исходом и неверной причиной.
func NewChunkSetFor(cn uint64) *ChunkSet {
	cs := NewChunkSet()
	cs.wantN = cn
	return cs
}

// Add проверяет кадр фрагмента целиком и запоминает его срез. Подпись
// проверяется ДО того, как байты попадут в буфер пересборки.
func (cs *ChunkSet) Add(raw []byte, st *TrustState) (*Result, error) {
	res, err := Verify(raw, st)
	if err != nil {
		return nil, err
	}
	ch, ok := res.Doc.(*CatalogChunk)
	if !ok {
		return nil, errf(EParseDocType, "P3", "frame is %s, not a catalog chunk", DocTypeName(res.Frame.DocType))
	}
	if cs.wantN != 0 && ch.N != cs.wantN {
		return nil, fieldErr("chunk %d says n %d, the trusted directive said cn %d", ch.I, ch.N, cs.wantN)
	}
	if cs.cid == nil {
		cs.cid, cs.n, cs.tl = ch.CID, ch.N, ch.TL
	} else {
		// Все n фрагментов обязаны нести одинаковые cid, n и tl.
		if !bytes.Equal(cs.cid, ch.CID) || cs.n != ch.N || cs.tl != ch.TL {
			return nil, fieldErr("chunk %d disagrees with the set on cid, n or tl", ch.I)
		}
	}
	if prev, ok := cs.parts[ch.I]; ok && !bytes.Equal(prev, ch.D) {
		return nil, fieldErr("chunk %d was already held with different bytes", ch.I)
	}
	cs.parts[ch.I] = ch.D
	cs.frames[ch.I] = raw
	return res, nil
}

// Complete сообщает, все ли фрагменты набора собраны.
func (cs *ChunkSet) Complete() bool {
	return cs.cid != nil && uint64(len(cs.parts)) == cs.n
}

// Missing перечисляет ещё не полученные индексы.
func (cs *ChunkSet) Missing() []uint64 {
	if cs.cid == nil {
		return nil
	}
	var out []uint64
	for i := uint64(0); i < cs.n; i++ {
		if _, ok := cs.parts[i]; !ok {
			out = append(out, i)
		}
	}
	return out
}

// Reassemble склеивает срезы и проверяет адресацию по содержимому:
// sha256(собранное)[0..10] == cid и len(собранное) == tl.
func (cs *ChunkSet) Reassemble() ([]byte, error) {
	if !cs.Complete() {
		return nil, fieldErr("chunk set is incomplete, %d of %d held", len(cs.parts), cs.n)
	}
	out := make([]byte, 0, cs.tl)
	for i := uint64(0); i < cs.n; i++ {
		out = append(out, cs.parts[i]...)
	}
	if uint64(len(out)) != cs.tl {
		return nil, fieldErr("reassembled %d bytes where tl declares %d", len(out), cs.tl)
	}
	sum := sha256.Sum256(out)
	if !bytes.Equal(sum[:10], cs.cid) {
		return nil, fieldErr("reassembled bytes hash to %x, cid declares %x", sum[:10], cs.cid)
	}
	return out, nil
}

// VerifyCatalogFromChunks проверяет каждый фрагмент, собирает каталог и
// проверяет собранный кадр 0x02 целиком, начиная с шага P1.
func VerifyCatalogFromChunks(chunkFrames [][]byte, st *TrustState) (*Result, error) {
	return VerifyCatalogFromChunksCN(chunkFrames, 0, st)
}

// VerifyCatalogFromChunksCN это то же самое с известным cn директивы, которое
// каждый фрагмент обязан подтвердить (03-WIRE.md 8.4, ключ 12).
func VerifyCatalogFromChunksCN(chunkFrames [][]byte, cn uint64, st *TrustState) (*Result, error) {
	cs := NewChunkSetFor(cn)
	for _, raw := range chunkFrames {
		if _, err := cs.Add(raw, st); err != nil {
			return nil, err
		}
	}
	frame, err := cs.Reassemble()
	if err != nil {
		return nil, err
	}
	return Verify(frame, st)
}
