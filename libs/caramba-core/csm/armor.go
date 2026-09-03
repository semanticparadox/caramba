package csm

import (
	"bytes"
	"fmt"
	"strings"
)

// Армированная текстовая форма и разбивка под QR, 03-WIRE.md раздел 10.
//
//	CARCAP1.<bid>.<i>/<n>.<data>
//
// Каждый отказ армированного читателя отображается в E_PARSE_FRAMING. Семейства
// E_ARMOR_* не существует и заводить его нельзя: для вызывающего все шесть
// условий раздела 10.3 означают одно и то же, перед ним не поток кадров, а
// конкретное условие место в журнале, а не в коде.

const (
	// ArmorChunkBytes это 620 байт, то есть 4960 бит, ровно 992 символа
	// base32 без битов заполнения.
	ArmorChunkBytes = 620
	// ArmorMaxChunks следует из потолка потока в 65536 байт.
	ArmorMaxChunks = 106
	armorPrefix    = "CARCAP1"
	// ArmorMediaType это тип содержимого армированного артефакта.
	ArmorMediaType = "text/vnd.caramba.csm1-armor"
)

func armorErr(format string, args ...any) *Error {
	return errf(EParseFraming, "10.3", format, args...)
}

// ArmorEncode кодирует поток кадров в набор строк CARCAP1.
func ArmorEncode(stream []byte) ([]string, error) {
	if len(stream) == 0 {
		return nil, armorErr("empty frame stream")
	}
	if len(stream) > MaxStreamBytes {
		return nil, armorErr("frame stream of %d bytes exceeds the %d byte cap", len(stream), MaxStreamBytes)
	}
	bid := BundleID(stream)
	n := (len(stream) + ArmorChunkBytes - 1) / ArmorChunkBytes
	if n > ArmorMaxChunks {
		return nil, armorErr("%d chunks exceed the %d chunk cap", n, ArmorMaxChunks)
	}
	lines := make([]string, 0, n)
	for i := 0; i < n; i++ {
		lo := i * ArmorChunkBytes
		hi := lo + ArmorChunkBytes
		if hi > len(stream) {
			hi = len(stream)
		}
		lines = append(lines, fmt.Sprintf("%s.%s.%d/%d.%s",
			armorPrefix, bid, i+1, n, Base32CrockfordEncode(stream[lo:hi])))
	}
	return lines, nil
}

type armorLine struct {
	bid  string
	i    int
	n    int
	data []byte
}

// parseDecimal читает десятичное число без ведущих нулей.
func parseDecimal(s string) (int, bool) {
	if s == "" || len(s) > 3 {
		return 0, false
	}
	if len(s) > 1 && s[0] == '0' {
		return 0, false
	}
	v := 0
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return 0, false
		}
		v = v*10 + int(s[i]-'0')
	}
	return v, true
}

func parseArmorLine(line string) (armorLine, error) {
	var out armorLine
	line = strings.TrimSpace(line)
	parts := strings.SplitN(line, ".", 4)
	if len(parts) != 4 {
		return out, armorErr("line does not have the four CARCAP1 fields")
	}
	if !strings.EqualFold(parts[0], armorPrefix) {
		return out, armorErr("line does not start with %s", armorPrefix)
	}
	if len(parts[1]) != 8 {
		return out, armorErr("bid is %d characters, must be 8", len(parts[1]))
	}
	out.bid = strings.ToUpper(parts[1])

	ord := strings.SplitN(parts[2], "/", 2)
	if len(ord) != 2 {
		return out, armorErr("ordinal field is not <i>/<n>")
	}
	i, ok := parseDecimal(ord[0])
	if !ok {
		return out, armorErr("chunk ordinal %q is not a decimal without leading zeros", ord[0])
	}
	n, ok := parseDecimal(ord[1])
	if !ok {
		return out, armorErr("chunk count %q is not a decimal without leading zeros", ord[1])
	}
	if i < 1 || n < 1 || n > ArmorMaxChunks || i > n {
		return out, armorErr("ordinal %d/%d is outside 1..%d", i, n, ArmorMaxChunks)
	}
	out.i, out.n = i, n

	// Пробелы, включая переносы строк, снимаются перед декодированием;
	// дефисы игнорируются декодером Crockford.
	data := strings.Map(func(r rune) rune {
		if r == ' ' || r == '\t' || r == '\r' || r == '\n' {
			return -1
		}
		return r
	}, parts[3])
	b, err := Base32CrockfordDecode(data)
	if err != nil {
		return out, armorErr("chunk %d: %v", i, err)
	}
	out.data = b
	return out, nil
}

// ArmorDecode собирает набор строк CARCAP1 обратно в поток кадров.
// Строки принимаются в любом порядке.
func ArmorDecode(lines []string) ([]byte, error) {
	parsed := make(map[int]armorLine)
	var bid string
	var total int

	seen := 0
	for _, raw := range lines {
		if strings.TrimSpace(raw) == "" {
			continue
		}
		l, err := parseArmorLine(raw)
		if err != nil {
			return nil, err
		}
		if seen == 0 {
			bid, total = l.bid, l.n
		} else {
			if l.bid != bid {
				return nil, armorErr("chunk %d carries bid %s, the set carries %s", l.i, l.bid, bid)
			}
			if l.n != total {
				return nil, armorErr("chunk %d claims a total of %d, the set claims %d", l.i, l.n, total)
			}
		}
		seen++
		if prev, ok := parsed[l.i]; ok {
			// Повторное сканирование идентичной строки допустимо.
			if !bytes.Equal(prev.data, l.data) {
				return nil, armorErr("ordinal %d appears twice with different data", l.i)
			}
			continue
		}
		parsed[l.i] = l
	}
	if seen == 0 {
		return nil, armorErr("no CARCAP1 lines")
	}

	var stream []byte
	for i := 1; i <= total; i++ {
		l, ok := parsed[i]
		if !ok {
			return nil, armorErr("ordinal %d of %d is missing", i, total)
		}
		if i != total && len(l.data) != ArmorChunkBytes {
			return nil, armorErr("non-final chunk %d decodes to %d bytes, must be exactly %d",
				i, len(l.data), ArmorChunkBytes)
		}
		if i == total && (len(l.data) == 0 || len(l.data) > ArmorChunkBytes) {
			return nil, armorErr("final chunk decodes to %d bytes", len(l.data))
		}
		stream = append(stream, l.data...)
	}

	// Проверка bid идёт ДО разбора: набор, смешавший две связки, отвергается
	// раньше, чем его байты попадут в парсер кадров.
	if got := BundleID(stream); got != bid {
		return nil, armorErr("recomputed bid %s does not match the declared %s", got, bid)
	}
	return stream, nil
}

// ArmorDecodeText принимает содержимое файла .carcap целиком.
func ArmorDecodeText(text string) ([]byte, error) {
	return ArmorDecode(strings.Split(text, "\n"))
}
