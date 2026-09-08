package main

// The armored text form and the multi-frame QR chunking of 03-WIRE.md
// section 10.

import "fmt"

// armor renders a frame stream as CARCAP1 lines. Chunk i (1-based) carries
// bytes [(i-1)*620, min(i*620, len)).
func armor(stream []byte) []string {
	if len(stream) > armorStreamMax {
		panic(fmt.Sprintf("armor: stream of %d bytes exceeds the %d-byte cap", len(stream), armorStreamMax))
	}
	bid := bidOf(stream)
	n := (len(stream) + armorChunkBytes - 1) / armorChunkBytes
	if n == 0 {
		n = 1
	}
	if n > 106 {
		panic(fmt.Sprintf("armor: %d chunks exceeds the 106-chunk cap", n))
	}
	out := make([]string, 0, n)
	for i := 1; i <= n; i++ {
		lo := (i - 1) * armorChunkBytes
		hi := lo + armorChunkBytes
		if hi > len(stream) {
			hi = len(stream)
		}
		out = append(out, fmt.Sprintf("CARCAP1.%s.%d/%d.%s", bid, i, n, crock(stream[lo:hi])))
	}
	return out
}

// armorSelfCheck reverses the encoding and confirms the bytes round-trip and
// that the bid matches, which is the check a reader MUST perform.
func armorSelfCheck(lines []string, want []byte) {
	var joined []byte
	for i, ln := range lines {
		var bid string
		var idx, n int
		var data string
		if _, err := fmt.Sscanf(ln, "CARCAP1.%8s.%d/%d.%s", &bid, &idx, &n, &data); err != nil {
			panic(fmt.Sprintf("armor: line %d unparsable: %v", i+1, err))
		}
		if idx != i+1 || n != len(lines) {
			panic("armor: ordinal or count mismatch in self-check")
		}
		raw, err := crockDecode(data)
		if err != nil {
			panic(fmt.Sprintf("armor: chunk %d: %v", idx, err))
		}
		if idx != n && len(raw) != armorChunkBytes {
			panic(fmt.Sprintf("armor: non-final chunk %d decoded to %d bytes, want %d", idx, len(raw), armorChunkBytes))
		}
		joined = append(joined, raw...)
	}
	if len(joined) != len(want) {
		panic(fmt.Sprintf("armor: round trip length %d, want %d", len(joined), len(want)))
	}
	for i := range joined {
		if joined[i] != want[i] {
			panic("armor: round trip byte mismatch")
		}
	}
	if bid := bidOf(want); !hasBid(lines, bid) {
		panic("armor: bid mismatch")
	}
}

func hasBid(lines []string, bid string) bool {
	for _, ln := range lines {
		if len(ln) < 16 || ln[8:16] != bid {
			return false
		}
	}
	return true
}

// splitFrames walks a frame stream using each frame's own payload_len and
// nsigs, which is the reader rule of 03-WIRE.md 10.1. It is used to confirm
// the generator's own streams are walkable.
func splitFrames(stream []byte) ([][]byte, error) {
	var out [][]byte
	off := 0
	for off < len(stream) {
		if len(stream)-off < 8 {
			return nil, fmt.Errorf("truncated frame at offset %d", off)
		}
		pl := int(stream[off+5])<<8 | int(stream[off+6])
		if off+7+pl+1 > len(stream) {
			return nil, fmt.Errorf("truncated payload at offset %d", off)
		}
		ns := int(stream[off+7+pl])
		total := 7 + pl + 1 + 76*ns
		if off+total > len(stream) {
			return nil, fmt.Errorf("truncated signature slots at offset %d", off)
		}
		out = append(out, stream[off:off+total])
		off += total
	}
	return out, nil
}
