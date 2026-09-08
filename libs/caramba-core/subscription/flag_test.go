package subscription

import "testing"

// TestFlagToISO проверяет извлечение ISO-кода страны из ведущего флаг-эмодзи
// (пара Regional Indicator Symbol Letter). Это чистая функция без сети и I/O.
func TestFlagToISO(t *testing.T) {
	cases := []struct {
		name string
		in   string
		want string
	}{
		{"ru flag", "\U0001F1F7\U0001F1FA Moscow", "RU"},
		{"de flag", "\U0001F1E9\U0001F1EA Frankfurt", "DE"},
		{"us flag", "\U0001F1FA\U0001F1F8", "US"},
		{"leading spaces", "  \U0001F1F3\U0001F1F1 Amsterdam", "NL"},
		{"no flag plain ascii", "Netherlands", ""},
		{"single indicator only", "\U0001F1F7", ""},
		{"empty", "", ""},
		{"non-indicator runes", "ab Moscow", ""},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := flagToISO(c.in); got != c.want {
				t.Errorf("flagToISO(%q) = %q, ожидалось %q", c.in, got, c.want)
			}
		})
	}
}

// TestTrafficUsed проверяет, что Used суммирует upload и download и не зависит
// от поля Total (лимита).
func TestTrafficUsed(t *testing.T) {
	cases := []struct {
		name string
		tr   Traffic
		want int64
	}{
		{"zero", Traffic{}, 0},
		{"upload only", Traffic{Upload: 100}, 100},
		{"download only", Traffic{Download: 250}, 250},
		{"both", Traffic{Upload: 100, Download: 250}, 350},
		{"ignores total", Traffic{Upload: 1, Download: 2, Total: 999}, 3},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := c.tr.Used(); got != c.want {
				t.Errorf("Traffic%+v.Used() = %d, ожидалось %d", c.tr, got, c.want)
			}
		})
	}
}
