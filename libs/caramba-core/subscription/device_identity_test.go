package subscription

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// Тот самый баг: подписку качало ядро, и в этом запросе устройство не называло
// себя ничем, кроме User-Agent. Панель заводила по нему ВТОРУЮ лизу на тот же
// телефон (первая уже была заведена приложением по X-Caramba-Device-Id) и
// списывала два слота лимита устройств с одного аппарата.
func TestFetchProfileSendsDeviceHeaders(t *testing.T) {
	var got http.Header
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		got = r.Header.Clone()
		w.Header().Set("Content-Type", "text/yaml")
		_, _ = w.Write([]byte("proxies: []\n"))
	}))
	defer srv.Close()

	c := NewClient(srv.URL)
	c.SetDeviceIdentity("11111111-2222-3333-4444-555555555555", "Pixel 8", "Android")

	if _, err := c.FetchProfile(context.Background(), "sub-uuid", FetchOptions{}); err != nil {
		t.Fatalf("FetchProfile: %v", err)
	}

	if v := got.Get(HeaderDeviceID); v != "11111111-2222-3333-4444-555555555555" {
		t.Fatalf("%s = %q", HeaderDeviceID, v)
	}
	if v := got.Get(HeaderDeviceName); v != "Pixel 8" {
		t.Fatalf("%s = %q", HeaderDeviceName, v)
	}
	// Платформа — ключ колонки лизы, а не текст для человека: панель ждёт
	// нижний регистр.
	if v := got.Get(HeaderDevicePlatform); v != "android" {
		t.Fatalf("%s = %q", HeaderDevicePlatform, v)
	}
	// User-Agent никуда не делся: по нему панель по-прежнему понимает, что
	// клиент — clash/mihomo, и отдаёт нужный формат конфига.
	if got.Get("User-Agent") != ClashUserAgent {
		t.Fatalf("User-Agent = %q", got.Get("User-Agent"))
	}
}

// Идентичности нет — не отправляется НИЧЕГО. Пустой X-Caramba-Device-Id хуже
// молчания: панель завела бы одну лизу с пустым ключом на все устройства сразу.
func TestFetchProfileWithoutIdentitySendsNoDeviceHeaders(t *testing.T) {
	var got http.Header
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		got = r.Header.Clone()
		_, _ = w.Write([]byte("proxies: []\n"))
	}))
	defer srv.Close()

	c := NewClient(srv.URL)
	// Имя и платформа без идентификатора панели бесполезны: без ключа ей не к
	// чему их приложить, а на проводе это лишний отпечаток клиента.
	c.SetDeviceIdentity("", "Pixel 8", "android")

	if _, err := c.FetchProfile(context.Background(), "sub-uuid", FetchOptions{}); err != nil {
		t.Fatalf("FetchProfile: %v", err)
	}
	for _, h := range []string{HeaderDeviceID, HeaderDeviceName, HeaderDevicePlatform} {
		if v := got.Get(h); v != "" {
			t.Fatalf("%s отправлен без идентичности: %q", h, v)
		}
	}
}

// Значения приходят из приложения (имя хоста, настройки человека) и уезжают в
// HTTP-заголовок. Управляющие символы там подделывают заголовки, кириллица
// едет мусором, а длинное имя раздувает каждый запрос.
func TestDeviceIdentityIsSanitized(t *testing.T) {
	c := NewClient("https://example.org")
	c.SetDeviceIdentity(" dev-1 \r\nX-Injected: 1", "Ноутбук Артёма", "  MacOS  ")
	d := c.DeviceIdentity()
	if d.ID != "dev-1 X-Injected: 1" {
		t.Fatalf("ID = %q: перевод строки обязан быть вырезан", d.ID)
	}
	if d.Name != "" {
		t.Fatalf("Name = %q: кириллица в latin-1 заголовке смысла не имеет", d.Name)
	}
	if d.Platform != "macos" {
		t.Fatalf("Platform = %q", d.Platform)
	}

	c.SetDeviceIdentity(strings.Repeat("x", 200), strings.Repeat("y", 200), strings.Repeat("z", 200))
	d = c.DeviceIdentity()
	if len(d.ID) != maxDeviceIDLen || len(d.Name) != maxDeviceNameLen || len(d.Platform) != maxDevicePlatformLen {
		t.Fatalf("потолки не соблюдены: %d/%d/%d", len(d.ID), len(d.Name), len(d.Platform))
	}
}

// Опция сборки и сеттер обязаны давать один результат: клиент строится в двух
// местах (NewCore и SetPanelURL), и расхождение между ними видно только на
// живой панели.
func TestWithDeviceIdentityMatchesSetter(t *testing.T) {
	viaOption := NewClient("https://example.org", WithDeviceIdentity("dev-1", "Mac", "MACOS"))
	viaSetter := NewClient("https://example.org")
	viaSetter.SetDeviceIdentity("dev-1", "Mac", "MACOS")
	if viaOption.DeviceIdentity() != viaSetter.DeviceIdentity() {
		t.Fatalf("%+v != %+v", viaOption.DeviceIdentity(), viaSetter.DeviceIdentity())
	}
}
