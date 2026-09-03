package transport

import (
	"errors"
	"fmt"
	"sort"
)

// Минимальный детерминированный кодировщик CBOR для тел запросов.
//
// Тела запросов (регистрация и запись настроек) это НЕ кадры: клиент не
// подписывает ролевым ключом, поэтому кадра тут нет. Но профиль разбора у
// панели тот же строгий, поэтому кодировать надо в той же канонической форме:
// определённая длина, кратчайшая голова, ключи карт только беззнаковые целые и
// строго по возрастанию.
//
// Отдельный кодировщик здесь, а не в csm, потому что csm это верификатор: он
// проверяет по принятым байтам и ничего не сериализует по построению.

// ErrCBOREncode это отказ кодирования.
var ErrCBOREncode = errors.New("transport: ошибка кодирования CBOR")

// CBORItem это элемент, который кодировщик умеет писать.
type CBORItem struct {
	kind  byte // 'u' uint, 'b' bstr, 't' tstr, 'a' array, 'm' map
	u     uint64
	b     []byte
	s     string
	array []CBORItem
	pairs []CBORPair
}

// CBORPair это пара карты. Ключ всегда беззнаковое целое.
type CBORPair struct {
	Key uint64
	Val CBORItem
}

// CBORUint, CBORBstr, CBORTstr, CBORArray и CBORMap строят элементы.
func CBORUint(v uint64) CBORItem { return CBORItem{kind: 'u', u: v} }
func CBORBstr(b []byte) CBORItem { return CBORItem{kind: 'b', b: b} }
func CBORTstr(s string) CBORItem { return CBORItem{kind: 't', s: s} }
func CBORArray(v ...CBORItem) CBORItem {
	return CBORItem{kind: 'a', array: v}
}
func CBORMap(pairs ...CBORPair) CBORItem {
	return CBORItem{kind: 'm', pairs: pairs}
}

// EncodeCBOR кодирует элемент. Ключи карт сортируются по возрастанию, дубликат
// ключа это ошибка кодирования, а не тихая склейка.
func EncodeCBOR(it CBORItem) ([]byte, error) {
	var out []byte
	return appendItem(out, it)
}

func appendItem(dst []byte, it CBORItem) ([]byte, error) {
	switch it.kind {
	case 'u':
		return appendHead(dst, 0, it.u), nil
	case 'b':
		dst = appendHead(dst, 2, uint64(len(it.b)))
		return append(dst, it.b...), nil
	case 't':
		dst = appendHead(dst, 3, uint64(len(it.s)))
		return append(dst, it.s...), nil
	case 'a':
		dst = appendHead(dst, 4, uint64(len(it.array)))
		var err error
		for _, v := range it.array {
			dst, err = appendItem(dst, v)
			if err != nil {
				return nil, err
			}
		}
		return dst, nil
	case 'm':
		pairs := make([]CBORPair, len(it.pairs))
		copy(pairs, it.pairs)
		sort.SliceStable(pairs, func(i, j int) bool { return pairs[i].Key < pairs[j].Key })
		for i := 1; i < len(pairs); i++ {
			if pairs[i].Key == pairs[i-1].Key {
				return nil, fmt.Errorf("%w: дубликат ключа %d", ErrCBOREncode, pairs[i].Key)
			}
		}
		dst = appendHead(dst, 5, uint64(len(pairs)))
		var err error
		for _, p := range pairs {
			dst = appendHead(dst, 0, p.Key)
			dst, err = appendItem(dst, p.Val)
			if err != nil {
				return nil, err
			}
		}
		return dst, nil
	default:
		return nil, fmt.Errorf("%w: неизвестный тип %q", ErrCBOREncode, string(it.kind))
	}
}

// appendHead пишет голову кратчайшей возможной формой. Неканоническая голова
// это отказ у любого строгого разборщика, поэтому другой формы тут нет.
func appendHead(dst []byte, major byte, v uint64) []byte {
	mt := major << 5
	switch {
	case v < 24:
		return append(dst, mt|byte(v))
	case v <= 0xff:
		return append(dst, mt|24, byte(v))
	case v <= 0xffff:
		return append(dst, mt|25, byte(v>>8), byte(v))
	case v <= 0xffffffff:
		return append(dst, mt|26, byte(v>>24), byte(v>>16), byte(v>>8), byte(v))
	default:
		return append(dst, mt|27,
			byte(v>>56), byte(v>>48), byte(v>>40), byte(v>>32),
			byte(v>>24), byte(v>>16), byte(v>>8), byte(v))
	}
}
