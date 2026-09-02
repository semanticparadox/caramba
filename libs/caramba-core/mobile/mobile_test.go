package mobile

import (
	"encoding/json"
	"testing"
)

func TestSplitCSV(t *testing.T) {
	cases := []struct {
		in   string
		want []string
	}{
		{"", nil},
		{"   ", nil},
		{"a", []string{"a"}},
		{"a,b,c", []string{"a", "b", "c"}},
		{" a , b ,, c ", []string{"a", "b", "c"}},
		{",,", nil},
	}
	for _, tc := range cases {
		got := splitCSV(tc.in)
		if len(got) != len(tc.want) {
			t.Errorf("splitCSV(%q): длина ожидалось %d, получено %d (%v)", tc.in, len(tc.want), len(got), got)
			continue
		}
		for i := range tc.want {
			if got[i] != tc.want[i] {
				t.Errorf("splitCSV(%q)[%d]: ожидалось %q, получено %q", tc.in, i, tc.want[i], got[i])
			}
		}
	}
}

func TestToJSONRoundTrip(t *testing.T) {
	type sample struct {
		Name  string `json:"name"`
		Count int    `json:"count"`
	}
	s, err := toJSON(sample{Name: "fi", Count: 3})
	if err != nil {
		t.Fatalf("toJSON: %v", err)
	}
	var back sample
	if err := json.Unmarshal([]byte(s), &back); err != nil {
		t.Fatalf("обратный разбор JSON: %v", err)
	}
	if back.Name != "fi" || back.Count != 3 {
		t.Errorf("round-trip потерял данные: %+v", back)
	}
}

// TestNewClientAllowsEmptyPanelURL: клиент без panelURL допустим (режим «только
// импорт», гостевой режим), а панельный Up без URL даёт понятную ошибку.
func TestNewClientAllowsEmptyPanelURL(t *testing.T) {
	c, err := NewClient("", "", t.TempDir(), "")
	if err != nil {
		t.Fatalf("NewClient без panelURL: %v", err)
	}
	if _, err := c.Up(""); err == nil {
		t.Error("ожидалась ошибка панельного Up без panelURL")
	}
}
