package csm

import "unicode/utf8"

// Строгий профиль декодирования CBOR, 03-WIRE.md раздел 3.
//
// Каноничность в CSM/1 это локальный предикат над входными байтами, а не
// сравнение с результатом перекодирования. Декодер написан здесь, а не взят
// из библиотеки, потому что профиль это подмножество: библиотека общего
// назначения примет то, что профиль обязан отвергнуть (неопределённые длины,
// теги, float, null, отрицательные числа, неминимальные головы, дубли ключей).
//
// Все ограничения проверяются ПО ХОДУ разбора, до выделения памяти, чтобы
// враждебный документ не мог заставить нас выделить буфер по своему слову.

// Ограничения раздела 3.2.
const (
	MaxDepth      = 6
	MaxMapPairs   = 64
	MaxArrayItems = 512
	MaxTstrBytes  = 256
	MaxBstrBytes  = 3072
	MaxUint       = uint64(1)<<53 - 1
)

// Диапазоны ключей раздела 3.3.
const (
	CriticalKeyMin    = 1
	CriticalKeyMax    = 63
	NonCriticalKeyMin = 64
	NonCriticalKeyMax = 1023
)

// Kind это тип элемента CBOR, допустимый профилем.
type Kind uint8

const (
	KindUint Kind = iota
	KindBstr
	KindTstr
	KindArray
	KindMap
	KindBool
)

func (k Kind) String() string {
	switch k {
	case KindUint:
		return "uint"
	case KindBstr:
		return "bstr"
	case KindTstr:
		return "tstr"
	case KindArray:
		return "array"
	case KindMap:
		return "map"
	case KindBool:
		return "bool"
	}
	return "?"
}

// Pair это одна пара карты. Ключ всегда беззнаковое целое (правило C9).
type Pair struct {
	Key uint64
	Val Value
}

// Value это разобранный элемент CBOR. Bstr и Tstr указывают внутрь входного
// буфера и не копируются: проверка подписи идёт по принятым байтам, и лишняя
// копия только даёт шанс разойтись с ними.
type Value struct {
	Kind  Kind
	U     uint64
	B     []byte
	S     string
	Bool  bool
	Array []Value
	Map   []Pair // строго по возрастанию ключа, правило C10
}

// Get возвращает значение по ключу карты. Пары отсортированы по возрастанию,
// поэтому поиск линейный с ранним выходом.
func (v *Value) Get(key uint64) (*Value, bool) {
	if v.Kind != KindMap {
		return nil, false
	}
	for i := range v.Map {
		if v.Map[i].Key == key {
			return &v.Map[i].Val, true
		}
		if v.Map[i].Key > key {
			break
		}
	}
	return nil, false
}

// Has сообщает, присутствует ли ключ.
func (v *Value) Has(key uint64) bool {
	_, ok := v.Get(key)
	return ok
}

type cborReader struct {
	b []byte
	i int
}

func cborErr(format string, args ...any) *Error {
	return errf(EParseCBOR, "P9", format, args...)
}

// decodeCBORPayload разбирает полезную нагрузку кадра: ровно один элемент
// верхнего уровня, обязательно карта (C1), потребляющий ровно len(p) байт (C2).
func decodeCBORPayload(p []byte) (Value, error) {
	r := &cborReader{b: p}
	v, err := r.item(1)
	if err != nil {
		return Value{}, err
	}
	if v.Kind != KindMap {
		return Value{}, cborErr("C1: top-level item is %s, must be a map", v.Kind)
	}
	if r.i != len(p) {
		return Value{}, cborErr("C2: %d trailing byte(s) inside payload_len", len(p)-r.i)
	}
	return v, nil
}

// remaining возвращает число непрочитанных байт.
func (r *cborReader) remaining() int { return len(r.b) - r.i }

// head читает начальный байт и его аргумент, проверяя правила C3 (только
// определённые длины) и C4 (кратчайшая форма головы).
func (r *cborReader) head() (major byte, ai byte, arg uint64, err error) {
	if r.remaining() < 1 {
		return 0, 0, 0, cborErr("truncated: expected an item head")
	}
	ib := r.b[r.i]
	r.i++
	major = ib >> 5
	ai = ib & 0x1f

	switch {
	case ai < 24:
		arg = uint64(ai)
	case ai == 24:
		if r.remaining() < 1 {
			return 0, 0, 0, cborErr("truncated one-byte argument")
		}
		arg = uint64(r.b[r.i])
		r.i++
		if arg < 24 {
			return 0, 0, 0, cborErr("C4: non-minimal head, %d encoded with a one-byte argument", arg)
		}
	case ai == 25:
		if r.remaining() < 2 {
			return 0, 0, 0, cborErr("truncated two-byte argument")
		}
		arg = uint64(r.b[r.i])<<8 | uint64(r.b[r.i+1])
		r.i += 2
		if arg < 256 {
			return 0, 0, 0, cborErr("C4: non-minimal head, %d encoded with a two-byte argument", arg)
		}
	case ai == 26:
		if r.remaining() < 4 {
			return 0, 0, 0, cborErr("truncated four-byte argument")
		}
		arg = uint64(r.b[r.i])<<24 | uint64(r.b[r.i+1])<<16 | uint64(r.b[r.i+2])<<8 | uint64(r.b[r.i+3])
		r.i += 4
		if arg < 65536 {
			return 0, 0, 0, cborErr("C4: non-minimal head, %d encoded with a four-byte argument", arg)
		}
	case ai == 27:
		if r.remaining() < 8 {
			return 0, 0, 0, cborErr("truncated eight-byte argument")
		}
		for k := 0; k < 8; k++ {
			arg = arg<<8 | uint64(r.b[r.i+k])
		}
		r.i += 8
		if arg < 1<<32 {
			return 0, 0, 0, cborErr("C4: non-minimal head, %d encoded with an eight-byte argument", arg)
		}
	case ai == 31:
		return 0, 0, 0, cborErr("C3: indefinite length is forbidden (major %d)", major)
	default: // 28, 29, 30
		return 0, 0, 0, cborErr("reserved additional information %d", ai)
	}
	return major, ai, arg, nil
}

func (r *cborReader) item(depth int) (Value, error) {
	if depth > MaxDepth {
		return Value{}, cborErr("C12: nesting depth %d exceeds MAX_DEPTH %d", depth, MaxDepth)
	}
	if r.remaining() < 1 {
		return Value{}, cborErr("truncated: expected an item")
	}

	// Простые значения разбираются до общей головы: профиль допускает ровно
	// два из них, и всё остальное в major 7 (float, null, undefined) отвергается.
	if r.b[r.i]>>5 == 7 {
		ib := r.b[r.i]
		r.i++
		switch ib & 0x1f {
		case 20:
			return Value{Kind: KindBool, Bool: false}, nil
		case 21:
			return Value{Kind: KindBool, Bool: true}, nil
		case 22:
			return Value{}, cborErr("C7: null is forbidden")
		case 23:
			return Value{}, cborErr("C7: undefined is forbidden")
		case 25, 26, 27:
			return Value{}, cborErr("C6: floats are forbidden")
		default:
			return Value{}, cborErr("C7: simple value 0x%02x is forbidden", ib)
		}
	}

	major, _, arg, err := r.head()
	if err != nil {
		return Value{}, err
	}

	switch major {
	case 0: // uint
		if arg > MaxUint {
			return Value{}, cborErr("uint %d exceeds MAX_UINT %d", arg, MaxUint)
		}
		return Value{Kind: KindUint, U: arg}, nil

	case 1: // negative
		return Value{}, cborErr("C8: negative integers are forbidden")

	case 2: // bstr
		if arg > MaxBstrBytes {
			return Value{}, cborErr("bstr of %d bytes exceeds MAX_BSTR_BYTES %d", arg, MaxBstrBytes)
		}
		n := int(arg)
		if n > r.remaining() {
			return Value{}, cborErr("bstr of %d bytes overruns the payload", n)
		}
		v := Value{Kind: KindBstr, B: r.b[r.i : r.i+n]}
		r.i += n
		return v, nil

	case 3: // tstr
		if arg > MaxTstrBytes {
			return Value{}, cborErr("tstr of %d bytes exceeds MAX_TSTR_BYTES %d", arg, MaxTstrBytes)
		}
		n := int(arg)
		if n > r.remaining() {
			return Value{}, cborErr("tstr of %d bytes overruns the payload", n)
		}
		raw := r.b[r.i : r.i+n]
		if !utf8.Valid(raw) {
			return Value{}, cborErr("C11: text string is not well-formed UTF-8")
		}
		r.i += n
		return Value{Kind: KindTstr, S: string(raw)}, nil

	case 4: // array
		if arg > MaxArrayItems {
			return Value{}, cborErr("array of %d items exceeds MAX_ARRAY_ITEMS %d", arg, MaxArrayItems)
		}
		n := int(arg)
		// Отказ до выделения: каждый элемент занимает минимум один байт.
		if n > r.remaining() {
			return Value{}, cborErr("array of %d items cannot fit in %d remaining bytes", n, r.remaining())
		}
		out := Value{Kind: KindArray, Array: make([]Value, 0, n)}
		for k := 0; k < n; k++ {
			it, err := r.item(depth + 1)
			if err != nil {
				return Value{}, err
			}
			out.Array = append(out.Array, it)
		}
		return out, nil

	case 5: // map
		if arg > MaxMapPairs {
			return Value{}, cborErr("map of %d pairs exceeds MAX_MAP_PAIRS %d", arg, MaxMapPairs)
		}
		n := int(arg)
		// Отказ до выделения: каждая пара занимает минимум два байта.
		if n > r.remaining()/2 {
			return Value{}, cborErr("map of %d pairs cannot fit in %d remaining bytes", n, r.remaining())
		}
		out := Value{Kind: KindMap, Map: make([]Pair, 0, n)}
		var prev uint64
		for k := 0; k < n; k++ {
			kmajor, _, karg, err := r.head()
			if err != nil {
				return Value{}, err
			}
			if kmajor != 0 {
				return Value{}, cborErr("C9: map key is major type %d, keys MUST be unsigned integers", kmajor)
			}
			if karg < CriticalKeyMin {
				return Value{}, cborErr("3.3: map key 0 is forbidden")
			}
			if karg > NonCriticalKeyMax {
				return Value{}, cborErr("3.3: map key %d is at or above 1024", karg)
			}
			if k > 0 && karg <= prev {
				return Value{}, cborErr("C10: map key %d does not exceed the previous key %d", karg, prev)
			}
			prev = karg
			val, err := r.item(depth + 1)
			if err != nil {
				return Value{}, err
			}
			out.Map = append(out.Map, Pair{Key: karg, Val: val})
		}
		return out, nil

	case 6: // tag
		return Value{}, cborErr("C5: tags are forbidden, tag %d seen", arg)
	}

	return Value{}, cborErr("unreachable major type %d", major)
}
