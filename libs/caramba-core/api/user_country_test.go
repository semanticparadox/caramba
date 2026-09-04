package api

import "testing"

// TestHomeCountryPrefersTheUserOverThePreset: чей домашний резолвер применяется.
//
// Это тот самый разъезд, из-за которого американский пользователь получал
// российский DNS: страна БРАЛАСЬ У ПРЕСЕТА, а пресетом по умолчанию у всех был
// `ru-smart` с Country=="RU". Пресет описывает выбранный режим, а не место, где
// человек находится, и подменять одно другим нельзя.
func TestHomeCountryPrefersTheUserOverThePreset(t *testing.T) {
	if got := homeCountry("US", "ru-smart"); got != "US" {
		t.Fatalf("страна пользователя проиграла стране пресета: %q", got)
	}
	if got := homeCountry("RU", "global"); got != "RU" {
		t.Fatalf("глобальный пресет стёр страну пользователя: %q", got)
	}
}

// TestHomeCountryFallsBackToThePresetOnlyWhileTheUserIsUnknown: пока панель не
// сказала страну, явно выбранный национальный пресет остаётся лучшим — и
// единственным — свидетельством о местоположении. Хуже прежнего не становится.
func TestHomeCountryFallsBackToThePresetOnlyWhileTheUserIsUnknown(t *testing.T) {
	if got := homeCountry("", "ru-smart"); got != "RU" {
		t.Fatalf("без страны пользователя пресет перестал работать запасным: %q", got)
	}
	// Оба пусты — раскола нет вовсе, и это не ошибка: profile.ApplyBootstrapDNS
	// на пустой стране домашние резолверы не трогает.
	if got := homeCountry("", "global"); got != "" {
		t.Fatalf("страна взялась из ниоткуда: %q", got)
	}
	if got := homeCountry("", "не-такого-пресета"); got != "" {
		t.Fatalf("неизвестный пресет породил страну: %q", got)
	}
}

// TestSetUserCountryAcceptsOnlyISO2: сюда приходит значение из сети
// (`x-client-country` / `client_country`). Всё, что не ISO-2, обязано стать
// честным «не знаем», а не осесть в поле и потом выдать себя за страну.
func TestSetUserCountryAcceptsOnlyISO2(t *testing.T) {
	c := &Core{}
	for _, in := range []string{" us ", "us", "US"} {
		c.SetUserCountry(in)
		if got := c.UserCountry(); got != "US" {
			t.Fatalf("SetUserCountry(%q) дал %q", in, got)
		}
	}
	for _, in := range []string{"", "   ", "USA", "u", "unknown", "XX-YY"} {
		c.SetUserCountry("US")
		c.SetUserCountry(in)
		if got := c.UserCountry(); got != "" {
			t.Fatalf("SetUserCountry(%q) осел как %q вместо «не знаем»", in, got)
		}
	}
}
