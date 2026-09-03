package transport

import (
	"errors"
	"testing"
)

// TestCBORCanonicalHeads: голова пишется кратчайшей возможной формой.
// Неканоническая голова отвергается любым строгим разборщиком, поэтому другой
// формы у кодировщика нет.
func TestCBORCanonicalHeads(t *testing.T) {
	cases := []struct {
		in   CBORItem
		want string
	}{
		{CBORUint(0), "00"},
		{CBORUint(1), "01"},
		{CBORUint(23), "17"},
		{CBORUint(24), "1818"},
		{CBORUint(255), "18ff"},
		{CBORUint(256), "190100"},
		{CBORUint(65536), "1a00010000"},
		{CBORBstr([]byte{0x01, 0x02}), "42" + "0102"},
		{CBORTstr("a"), "6161"},
		{CBORArray(CBORUint(1), CBORUint(2)), "820102"},
		{CBORMap(CBORPair{Key: 1, Val: CBORUint(1)}), "a10101"},
	}
	for _, c := range cases {
		got, err := EncodeCBOR(c.in)
		if err != nil {
			t.Fatalf("кодирование: %v", err)
		}
		if hexOf(got) != c.want {
			t.Fatalf("получено %s, ожидалось %s", hexOf(got), c.want)
		}
	}
}

// TestCBORMapKeysSortedAndUnique: ключи карт строго по возрастанию, дубликат
// это ошибка кодирования, а не тихая склейка.
func TestCBORMapKeysSortedAndUnique(t *testing.T) {
	got, err := EncodeCBOR(CBORMap(
		CBORPair{Key: 5, Val: CBORUint(5)},
		CBORPair{Key: 1, Val: CBORUint(1)},
		CBORPair{Key: 3, Val: CBORUint(3)},
	))
	if err != nil {
		t.Fatalf("кодирование: %v", err)
	}
	if hexOf(got) != "a3010103030505" {
		t.Fatalf("ключи не отсортированы: %s", hexOf(got))
	}
	_, err = EncodeCBOR(CBORMap(
		CBORPair{Key: 1, Val: CBORUint(1)},
		CBORPair{Key: 1, Val: CBORUint(2)},
	))
	if !errors.Is(err, ErrCBOREncode) {
		t.Fatalf("дубликат ключа принят: %v", err)
	}
}
