package routing

import "testing"

// TestPresetByIDMiss проверяет, что неизвестный идентификатор возвращает
// ok=false и нулевой пресет, не паникуя.
func TestPresetByIDMiss(t *testing.T) {
	p, ok := PresetByID("does-not-exist")
	if ok {
		t.Fatalf("PresetByID(unknown) ok = true, ожидалось false")
	}
	if p.ID != "" {
		t.Errorf("PresetByID(unknown) вернул непустой пресет: %+v", p)
	}
}

// TestPresetsForCountryUnknownReturnsGlobalsOnly проверяет, что для пустого и
// неизвестного ISO-кода возвращаются только глобальные пресеты (Country == "")
// и среди них нет ни одного странового.
func TestPresetsForCountryUnknownReturnsGlobalsOnly(t *testing.T) {
	for _, iso := range []string{"", "  ", "ZZ"} {
		got := PresetsForCountry(iso)
		if len(got) == 0 {
			t.Fatalf("PresetsForCountry(%q) пуст, ожидались глобальные пресеты", iso)
		}
		for _, p := range got {
			if p.Country != "" {
				t.Errorf("PresetsForCountry(%q) содержит страновой пресет %q (Country=%q)", iso, p.ID, p.Country)
			}
		}
	}
}

// TestPresetsForCountryIsCaseInsensitive проверяет нормализацию регистра ISO:
// строчный и заглавный коды дают одинаковый набор пресетов.
func TestPresetsForCountryIsCaseInsensitive(t *testing.T) {
	upper := PresetsForCountry("RU")
	lower := PresetsForCountry("ru")
	if len(upper) != len(lower) {
		t.Fatalf("PresetsForCountry регистрозависим: RU=%d ru=%d", len(upper), len(lower))
	}
	for i := range upper {
		if upper[i].ID != lower[i].ID {
			t.Errorf("позиция %d: RU=%q ru=%q", i, upper[i].ID, lower[i].ID)
		}
	}
	// Первый пресет для RU должен быть страновым.
	if upper[0].Country != "RU" {
		t.Errorf("первый пресет для RU не страновой: %+v", upper[0])
	}
}
