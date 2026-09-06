package subscription

import (
	"net/http"
	"testing"
	"time"
)

func TestParseUserInfo(t *testing.T) {
	tr := parseUserInfo("upload=10; download=200; total=1000; expire=1700000000")
	if tr.Upload != 10 || tr.Download != 200 || tr.Total != 1000 {
		t.Fatalf("неверный разбор трафика: %+v", tr)
	}
	if tr.Used() != 210 {
		t.Fatalf("Used() = %d, ожидалось 210", tr.Used())
	}
}

func TestParseUserInfoUnlimited(t *testing.T) {
	tr := parseUserInfo("upload=0; download=5")
	if tr.Total != 0 {
		t.Fatalf("ожидался безлимит (total=0), получено %d", tr.Total)
	}
}

func TestExpiryFromUserInfo(t *testing.T) {
	exp := expiryFromUserInfo("download=1; expire=1700000000")
	want := time.Unix(1700000000, 0).UTC()
	if !exp.Equal(want) {
		t.Fatalf("expiry = %v, ожидалось %v", exp, want)
	}
	if !expiryFromUserInfo("download=1").IsZero() {
		t.Fatal("ожидался нулевой expiry при отсутствии поля")
	}
}

func TestParseServers(t *testing.T) {
	raw := []byte(`
proxies:
  - name: "DE Stealth"
    type: vless
    server: 1.2.3.4
    port: 443
  - name: "Amnezia"
    type: wireguard
    server: 5.6.7.8
    port: 51820
`)
	servers, err := parseServers(raw)
	if err != nil {
		t.Fatalf("ошибка разбора: %v", err)
	}
	if len(servers) != 2 {
		t.Fatalf("ожидалось 2 сервера, получено %d", len(servers))
	}
	if servers[0].Type != "vless" || servers[0].Port != 443 {
		t.Fatalf("неверный первый сервер: %+v", servers[0])
	}
	if servers[1].Type != "wireguard" {
		t.Fatalf("неверный второй сервер: %+v", servers[1])
	}
}

func TestCountryFromName(t *testing.T) {
	cases := map[string]string{
		"\U0001F1F9\U0001F1F7 Istanbul": "TR", // флаг-эмодзи Турции + город
		"\U0001F1F3\U0001F1F1 Amsterdam": "NL",
		"DE Stealth":                     "DE", // ведущий двухбуквенный код
		"NL - Amsterdam":                 "NL",
		"[US] West":                      "US", // код в скобках
		"us-1":                           "US", // нижний регистр нормализуется
		"Amnezia":                        "",   // длинное слово — не код
		"Tokyo Fast":                     "",   // первый токен длиннее двух букв
		"":                               "",
	}
	for in, want := range cases {
		if got := countryFromName(in); got != want {
			t.Errorf("countryFromName(%q) = %q, ожидалось %q", in, got, want)
		}
	}
}

func TestParseServersCountry(t *testing.T) {
	raw := []byte("proxies:\n  - name: \"\U0001F1F9\U0001F1F7 Istanbul\"\n    type: vless\n    server: 1.2.3.4\n    port: 443\n")
	servers, err := parseServers(raw)
	if err != nil {
		t.Fatalf("ошибка разбора: %v", err)
	}
	if len(servers) != 1 || servers[0].Country != "TR" {
		t.Fatalf("ожидалась страна TR, получено %+v", servers)
	}
}

func TestProxyMaps(t *testing.T) {
	raw := []byte(`
proxies:
  - name: "DE Stealth"
    type: vless
    server: 1.2.3.4
    port: 443
    uuid: abc
  - name: "DE Stealth"
    type: hysteria2
    server: 1.2.3.4
    port: 8443
    password: pw
  - name: "Amnezia"
    type: wireguard
    server: 5.6.7.8
    port: 51820
  - type: ss
    server: 9.9.9.9
    port: 8388
`)
	maps, err := ProxyMaps(raw)
	if err != nil {
		t.Fatalf("ProxyMaps: %v", err)
	}
	// Прокси без name пропускается; остаются два уникальных узла.
	if len(maps) != 2 {
		t.Fatalf("ожидалось 2 узла, получено %d: %v", len(maps), maps)
	}
	de, ok := maps["DE Stealth"]
	if !ok {
		t.Fatal(`узел "DE Stealth" отсутствует`)
	}
	// Узел с двумя протоколами должен дать обе записи под clash-типами.
	if len(de) != 2 {
		t.Fatalf("ожидалось 2 протокола для DE Stealth, получено %d: %v", len(de), de)
	}
	vless, ok := de["vless"]
	if !ok {
		t.Fatal("vless-конфиг для DE Stealth отсутствует")
	}
	// Сырой map должен сохранять поля как есть для adapter.ParseProxy.
	if vless["uuid"] != "abc" || vless["port"] != 443 {
		t.Fatalf("сырой vless-конфиг искажён: %v", vless)
	}
	if _, ok := de["hysteria2"]; !ok {
		t.Fatal("hysteria2-конфиг для DE Stealth отсутствует")
	}
	if _, ok := maps["Amnezia"]["wireguard"]; !ok {
		t.Fatal("wireguard-конфиг для Amnezia отсутствует")
	}
}

func TestProxyMapsEmpty(t *testing.T) {
	maps, err := ProxyMaps([]byte("proxies: []"))
	if err != nil {
		t.Fatalf("ProxyMaps: %v", err)
	}
	if maps == nil || len(maps) != 0 {
		t.Fatalf("ожидалась пустая непустая(non-nil) карта, получено %v", maps)
	}
}

func TestProxyMapsBadYAML(t *testing.T) {
	if _, err := ProxyMaps([]byte("proxies: [unterminated")); err == nil {
		t.Fatal("ожидалась ошибка разбора битого YAML")
	}
}

func TestParseMetadataHeaders(t *testing.T) {
	h := http.Header{}
	h.Set("profile-title", "Premium 100GB")
	h.Set("subscription-userinfo", "upload=0; download=50; total=100; expire=1700000000")
	h.Set("profile-update-interval", "2")

	meta, err := parseMetadata(h, []byte("proxies: []"))
	if err != nil {
		t.Fatalf("ошибка: %v", err)
	}
	if meta.Title != "Premium 100GB" {
		t.Fatalf("title = %q", meta.Title)
	}
	if meta.UpdateInterval != 2*time.Minute {
		t.Fatalf("update interval = %v", meta.UpdateInterval)
	}
	if meta.Traffic.Total != 100 {
		t.Fatalf("total = %d", meta.Traffic.Total)
	}
}

// «network: httpupgrade» приходит прямо из clash-тела панели, а mihomo такой
// сети не знает. У vless нераспознанная сеть не даёт ошибки — она МОЛЧА
// вырождается в обычный TLS-поток, и узел, ждущий апгрейда, отвечает отказом,
// неотличимым от отвергнутого ключа. Снято на живом флоте: с прежним именем
// «unexpected response version», после перекладки 569 мс DE / 157 мс CA.
func TestNormalizeProxyForCoreRemapsHTTPUpgrade(t *testing.T) {
	px := map[string]any{
		"name":    "DE HTTP",
		"type":    "vless",
		"network": "httpupgrade",
		"http-upgrade-opts": map[string]any{
			"path": "/hu",
			"host": "essentialhome.live",
		},
	}
	NormalizeProxyForCore(px)

	if px["network"] != "ws" {
		t.Fatalf("network = %v, ядро знает только ws", px["network"])
	}
	opts, ok := px["ws-opts"].(map[string]any)
	if !ok || opts["v2ray-http-upgrade"] != true || opts["path"] != "/hu" {
		t.Fatalf("ws-opts = %v", px["ws-opts"])
	}
	hdrs, _ := opts["headers"].(map[string]any)
	if hdrs == nil || hdrs["Host"] != "essentialhome.live" {
		t.Fatalf("Host потерян: %v", opts["headers"])
	}
	if _, ok := px["http-upgrade-opts"]; ok {
		t.Errorf("остался ключ, которого ядро не читает: %v", px["http-upgrade-opts"])
	}
	// Транспорт обязан ОСТАТЬСЯ узнаваемым: иначе httpupgrade слился бы с ws
	// в одну строку экрана — то есть спрятался бы ровно так же, как раньше.
	if got := ProxyTransport(px); got != "httpupgrade" {
		t.Fatalf("после перекладки транспорт = %q, ожидался httpupgrade", got)
	}
}

// Функция вызывается на двух путях и не должна ничего значить при повторе.
func TestNormalizeProxyForCoreIsIdempotentAndNarrow(t *testing.T) {
	px := map[string]any{"type": "vless", "network": "httpupgrade"}
	NormalizeProxyForCore(px)
	first := px["ws-opts"]
	NormalizeProxyForCore(px)
	if px["network"] != "ws" || px["ws-opts"] == nil {
		t.Fatalf("повторный вызов испортил карту: %v", px)
	}
	_ = first

	// Обычный ws трогать нечего: подмена его опций стёрла бы путь и Host.
	ws := map[string]any{
		"type":    "vless",
		"network": "ws",
		"ws-opts": map[string]any{"path": "/keep"},
	}
	NormalizeProxyForCore(ws)
	opts, _ := ws["ws-opts"].(map[string]any)
	if opts["path"] != "/keep" || opts["v2ray-http-upgrade"] != nil {
		t.Fatalf("обычный ws изменён: %v", ws["ws-opts"])
	}
	if ProxyTransport(ws) != "ws" {
		t.Fatalf("обычный ws перестал быть ws: %q", ProxyTransport(ws))
	}
}

// Роль выводится из ссылок, а не из имени: relay это тот, через кого набирают.
func TestServersFromProxiesMarksRelayByReference(t *testing.T) {
	servers := ServersFromProxies([]map[string]any{
		{"name": "RU", "type": "vless", "server": "10.0.0.1", "port": 443},
		{"name": "DE", "type": "vless", "server": "10.0.0.2", "port": 443, "dialer-proxy": "RU"},
	})
	if len(servers) != 2 {
		t.Fatalf("узлов %d", len(servers))
	}
	if servers[0].Role != RoleRelay {
		t.Errorf("узел, через который набирают, обязан быть relay: %q", servers[0].Role)
	}
	if servers[1].Role != RoleExit {
		t.Errorf("выход обязан быть exit: %q", servers[1].Role)
	}
	// Порядок источника — единственный, который у подписки вообще есть.
	if servers[0].ID != "RU" || servers[1].ID != "DE" {
		t.Errorf("порядок источника потерян: %v", servers)
	}
}
