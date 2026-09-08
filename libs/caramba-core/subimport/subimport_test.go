package subimport

import (
	"encoding/base64"
	"strings"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/subscription"
	"gopkg.in/yaml.v3"
)

// proxyFromYAML разбирает результат Import обратно в первый proxy-map для
// проверки полей.
func proxyFromYAML(t *testing.T, clashYAML []byte) map[string]any {
	t.Helper()
	var doc struct {
		Proxies []map[string]any `yaml:"proxies"`
	}
	if err := yaml.Unmarshal(clashYAML, &doc); err != nil {
		t.Fatalf("разбор результата YAML: %v", err)
	}
	if len(doc.Proxies) == 0 {
		t.Fatal("в результате нет proxies")
	}
	return doc.Proxies[0]
}

// TestParseURIProtocols — таблица одиночных URI на каждый протокол contract E.
// Проверяет clash-тип и ключевые поля mapped-прокси.
func TestParseURIProtocols(t *testing.T) {
	cases := []struct {
		name      string
		uri       string
		wantType  string
		wantCheck func(t *testing.T, px map[string]any)
	}{
		{
			name:     "vless-reality",
			uri:      "vless://11111111-2222-3333-4444-555555555555@1.2.3.4:443?security=reality&pbk=PUBKEY&sid=ab12&fp=chrome&type=tcp&flow=xtls-rprx-vision&sni=example.com#TR%20Reality",
			wantType: "vless",
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["uuid"] != "11111111-2222-3333-4444-555555555555" {
					t.Errorf("uuid = %v", px["uuid"])
				}
				if px["servername"] != "example.com" {
					t.Errorf("servername = %v", px["servername"])
				}
				if px["flow"] != "xtls-rprx-vision" {
					t.Errorf("flow = %v", px["flow"])
				}
				ro, ok := px["reality-opts"].(map[string]any)
				if !ok || ro["public-key"] != "PUBKEY" || ro["short-id"] != "ab12" {
					t.Errorf("reality-opts = %v", px["reality-opts"])
				}
				if px["name"] != "TR Reality" {
					t.Errorf("name = %v", px["name"])
				}
			},
		},
		{
			name:     "vless-ws",
			uri:      "vless://uuid-ws@5.6.7.8:8443?security=tls&type=ws&path=/wspath&host=cdn.example.com&sni=cdn.example.com#WS",
			wantType: "vless",
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["network"] != "ws" {
					t.Errorf("network = %v", px["network"])
				}
				ws, ok := px["ws-opts"].(map[string]any)
				if !ok || ws["path"] != "/wspath" {
					t.Errorf("ws-opts = %v", px["ws-opts"])
				}
				hdrs, _ := ws["headers"].(map[string]any)
				if hdrs == nil || hdrs["Host"] != "cdn.example.com" {
					t.Errorf("ws headers = %v", ws["headers"])
				}
			},
		},
		{
			name:     "vless-grpc",
			uri:      "vless://uuid-grpc@host:443?security=tls&type=grpc&serviceName=mygrpc&sni=h#GRPC",
			wantType: "vless",
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["network"] != "grpc" {
					t.Errorf("network = %v", px["network"])
				}
				g, ok := px["grpc-opts"].(map[string]any)
				if !ok || g["grpc-service-name"] != "mygrpc" {
					t.Errorf("grpc-opts = %v", px["grpc-opts"])
				}
			},
		},
		{
			name:     "vless-httpupgrade",
			uri:      "vless://uuid-hu@host:443?security=tls&type=httpupgrade&path=/up&host=h.example#HU",
			wantType: "vless",
			// mihomo не знает network: httpupgrade — у него это ws с флагом
			// v2ray-http-upgrade. С прежним именем адаптер не строился
			// вовсе, и целый транспорт выпадал и из замера, и из выбора.
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["network"] != "ws" {
					t.Errorf("network = %v", px["network"])
				}
				ws, ok := px["ws-opts"].(map[string]any)
				if !ok || ws["v2ray-http-upgrade"] != true || ws["path"] != "/up" {
					t.Errorf("ws-opts = %v", px["ws-opts"])
				}
				hdrs, _ := ws["headers"].(map[string]any)
				if hdrs == nil || hdrs["Host"] != "h.example" {
					t.Errorf("ws-opts.headers = %v", ws["headers"])
				}
				if px["http-upgrade-opts"] != nil {
					t.Errorf("остался ключ, которого ядро не читает: %v", px["http-upgrade-opts"])
				}
			},
		},
		{
			name:     "trojan",
			uri:      "trojan://secretpass@9.9.9.9:443?sni=trojan.example.com&type=tcp#Trojan",
			wantType: "trojan",
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["password"] != "secretpass" {
					t.Errorf("password = %v", px["password"])
				}
				if px["sni"] != "trojan.example.com" {
					t.Errorf("sni = %v", px["sni"])
				}
			},
		},
		{
			name:     "hysteria2",
			uri:      "hysteria2://hy2pass@10.0.0.1:8443?sni=hy.example.com&obfs=salamander&obfs-password=obpw#Hy2",
			wantType: "hysteria2",
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["password"] != "hy2pass" {
					t.Errorf("password = %v", px["password"])
				}
				if px["obfs"] != "salamander" || px["obfs-password"] != "obpw" {
					t.Errorf("obfs = %v / %v", px["obfs"], px["obfs-password"])
				}
				if px["sni"] != "hy.example.com" {
					t.Errorf("sni = %v", px["sni"])
				}
			},
		},
		{
			name:     "hy2-alias",
			uri:      "hy2://pw@h:443?sni=s#Hy2Alias",
			wantType: "hysteria2",
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["password"] != "pw" {
					t.Errorf("password = %v", px["password"])
				}
			},
		},
		{
			name:     "tuic",
			uri:      "tuic://uuid-t:tpass@11.0.0.1:443?congestion_control=bbr&alpn=h3&sni=tuic.example.com#TUIC",
			wantType: "tuic",
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["uuid"] != "uuid-t" || px["password"] != "tpass" {
					t.Errorf("uuid/password = %v / %v", px["uuid"], px["password"])
				}
				if px["congestion-controller"] != "bbr" {
					t.Errorf("congestion-controller = %v", px["congestion-controller"])
				}
				alpn, ok := px["alpn"].([]any)
				if !ok || len(alpn) != 1 || alpn[0] != "h3" {
					t.Errorf("alpn = %v", px["alpn"])
				}
			},
		},
		{
			name:     "wireguard-plain",
			uri:      "wireguard://PRIVKEY@12.0.0.1:51820?publickey=PUBKEY&address=10.0.0.2/32&mtu=1420#WG",
			wantType: "wireguard",
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["private-key"] != "PRIVKEY" || px["public-key"] != "PUBKEY" {
					t.Errorf("keys = %v / %v", px["private-key"], px["public-key"])
				}
				if px["ip"] != "10.0.0.2/32" {
					t.Errorf("ip = %v", px["ip"])
				}
				if px["mtu"] != 1420 {
					t.Errorf("mtu = %v", px["mtu"])
				}
				if _, ok := px["amnezia-wg-option"]; ok {
					t.Errorf("plain WG не должен нести amnezia-wg-option: %v", px["amnezia-wg-option"])
				}
			},
		},
		{
			name:     "amneziawg",
			uri:      "wireguard://PRIVKEY@13.0.0.1:51820?publickey=PUBKEY&address=10.0.0.3/32&jc=4&jmin=40&jmax=70&s1=15&s2=20&h1=1234567&h2=7654321&h3=1111111&h4=2222222#AWG",
			wantType: "wireguard",
			wantCheck: func(t *testing.T, px map[string]any) {
				opt, ok := px["amnezia-wg-option"].(map[string]any)
				if !ok {
					t.Fatalf("amnezia-wg-option отсутствует: %v", px)
				}
				if opt["jc"] != 4 || opt["jmin"] != 40 || opt["jmax"] != 70 {
					t.Errorf("jc/jmin/jmax = %v/%v/%v (ожидались int)", opt["jc"], opt["jmin"], opt["jmax"])
				}
				if opt["s1"] != 15 || opt["s2"] != 20 {
					t.Errorf("s1/s2 = %v/%v (ожидались int)", opt["s1"], opt["s2"])
				}
				// КРИТИЧНО: h1..h4 — строки, иначе mihomo деградирует до plain WG.
				for _, h := range []string{"h1", "h2", "h3", "h4"} {
					if _, isStr := opt[h].(string); !isStr {
						t.Errorf("%s = %v (тип %T), ожидалась строка", h, opt[h], opt[h])
					}
				}
				if opt["h1"] != "1234567" {
					t.Errorf("h1 = %v", opt["h1"])
				}
			},
		},
		{
			// Тип остаётся naive: подмена на http давала узел, который ядро
			// набирало и получало отказ, а экран объяснял отказ чужой
			// причиной вместо единственной настоящей — «ядро не умеет Naive».
			name:     "naive",
			uri:      "naive+https://user:pass@naive.example.com:443?sni=naive.example.com#Naive",
			wantType: "naive",
			wantCheck: func(t *testing.T, px map[string]any) {
				if px["tls"] != true {
					t.Errorf("tls = %v", px["tls"])
				}
				if px["username"] != "user" || px["password"] != "pass" {
					t.Errorf("user/pass = %v / %v", px["username"], px["password"])
				}
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			clashYAML, meta, err := Import([]byte(tc.uri), FormatURI)
			if err != nil {
				t.Fatalf("Import(%s): %v", tc.name, err)
			}
			px := proxyFromYAML(t, clashYAML)
			if px["type"] != tc.wantType {
				t.Fatalf("type = %v, ожидался %v", px["type"], tc.wantType)
			}
			// Каждый прокси обязан нести server/port для UI-списка и автоподбора.
			if asString(px["server"]) == "" {
				t.Errorf("пустой server: %v", px)
			}
			if asInt(px["port"]) == 0 {
				t.Errorf("пустой port: %v", px)
			}
			if len(meta.Servers) != 1 {
				t.Fatalf("ожидался 1 сервер в метаданных, получено %d", len(meta.Servers))
			}
			if meta.Servers[0].Type != tc.wantType {
				t.Errorf("metadata server type = %v", meta.Servers[0].Type)
			}
			tc.wantCheck(t, px)
		})
	}
}

// TestParseShadowsocks — SIP002 и legacy формы + ShadowTLS plugin.
func TestParseShadowsocks(t *testing.T) {
	method := "aes-256-gcm"
	password := "sspassword"
	userinfo := base64.RawURLEncoding.EncodeToString([]byte(method + ":" + password))

	t.Run("sip002", func(t *testing.T) {
		uri := "ss://" + userinfo + "@14.0.0.1:8388#SS"
		clashYAML, _, err := Import([]byte(uri), FormatURI)
		if err != nil {
			t.Fatalf("Import: %v", err)
		}
		px := proxyFromYAML(t, clashYAML)
		if px["type"] != "ss" || px["cipher"] != method || px["password"] != password {
			t.Fatalf("ss mapping неверный: %v", px)
		}
		if asInt(px["port"]) != 8388 {
			t.Errorf("port = %v", px["port"])
		}
	})

	t.Run("legacy", func(t *testing.T) {
		whole := base64.StdEncoding.EncodeToString([]byte(method + ":" + password + "@15.0.0.1:8389"))
		uri := "ss://" + whole + "#SSLegacy"
		clashYAML, _, err := Import([]byte(uri), FormatURI)
		if err != nil {
			t.Fatalf("Import: %v", err)
		}
		px := proxyFromYAML(t, clashYAML)
		if px["server"] != "15.0.0.1" || asInt(px["port"]) != 8389 {
			t.Fatalf("legacy ss host/port неверны: %v", px)
		}
	})

	t.Run("shadowtls", func(t *testing.T) {
		uri := "ss://" + userinfo + "@16.0.0.1:8388?plugin=shadow-tls%3Bhost%3Dstls.example.com%3Bpassword%3Dstpw%3Bversion%3D3#STLS"
		clashYAML, _, err := Import([]byte(uri), FormatURI)
		if err != nil {
			t.Fatalf("Import: %v", err)
		}
		px := proxyFromYAML(t, clashYAML)
		if px["plugin"] != "shadow-tls" {
			t.Fatalf("plugin = %v, ожидался shadow-tls", px["plugin"])
		}
		opts, ok := px["plugin-opts"].(map[string]any)
		if !ok || opts["host"] != "stls.example.com" || opts["password"] != "stpw" || opts["version"] != 3 {
			t.Fatalf("plugin-opts неверны: %v", px["plugin-opts"])
		}
	})
}

// TestParseVmessJSON — vmess://base64(json) форма v2rayN.
func TestParseVmessJSON(t *testing.T) {
	j := `{"v":"2","ps":"VMess Node","add":"17.0.0.1","port":"443","id":"vmess-uuid","aid":"0","net":"ws","type":"none","host":"vm.example.com","path":"/vm","tls":"tls","sni":"vm.example.com"}`
	uri := "vmess://" + base64.StdEncoding.EncodeToString([]byte(j))
	clashYAML, _, err := Import([]byte(uri), FormatURI)
	if err != nil {
		t.Fatalf("Import: %v", err)
	}
	px := proxyFromYAML(t, clashYAML)
	if px["type"] != "vmess" || px["uuid"] != "vmess-uuid" {
		t.Fatalf("vmess mapping неверный: %v", px)
	}
	if px["server"] != "17.0.0.1" || asInt(px["port"]) != 443 {
		t.Errorf("server/port = %v / %v", px["server"], px["port"])
	}
	if px["network"] != "ws" {
		t.Errorf("network = %v", px["network"])
	}
	if px["tls"] != true || px["servername"] != "vm.example.com" {
		t.Errorf("tls/servername = %v / %v", px["tls"], px["servername"])
	}
	ws, ok := px["ws-opts"].(map[string]any)
	if !ok || ws["path"] != "/vm" {
		t.Errorf("ws-opts = %v", px["ws-opts"])
	}
}

// TestImportClash — clash YAML passthrough + нормализация h1..h4 в строки.
func TestImportClash(t *testing.T) {
	doc := `
proxies:
  - name: "TR VLESS"
    type: vless
    server: 18.0.0.1
    port: 443
    uuid: clash-uuid
    network: tcp
    tls: true
  - name: "AWG Node"
    type: wireguard
    server: 18.0.0.2
    port: 51820
    private-key: PK
    public-key: PUB
    amnezia-wg-option:
      jc: 4
      h1: 1234567
      h2: 7654321
  - type: ss
    server: 18.0.0.3
    port: 8388
`
	clashYAML, meta, err := Import([]byte(doc), FormatClash)
	if err != nil {
		t.Fatalf("Import: %v", err)
	}
	var out struct {
		Proxies []map[string]any `yaml:"proxies"`
	}
	if err := yaml.Unmarshal(clashYAML, &out); err != nil {
		t.Fatalf("разбор результата: %v", err)
	}
	// Прокси без name пропускается → остаётся 2.
	if len(out.Proxies) != 2 {
		t.Fatalf("ожидалось 2 прокси (без name отбрасывается), получено %d", len(out.Proxies))
	}
	if len(meta.Servers) != 2 {
		t.Fatalf("ожидалось 2 сервера в метаданных, получено %d", len(meta.Servers))
	}
	// Находим AWG-узел и проверяем строковость h1/h2.
	var awg map[string]any
	for _, p := range out.Proxies {
		if p["type"] == "wireguard" {
			awg = p
		}
	}
	if awg == nil {
		t.Fatal("wireguard-узел потерян")
	}
	opt, ok := awg["amnezia-wg-option"].(map[string]any)
	if !ok {
		t.Fatalf("amnezia-wg-option потерян: %v", awg)
	}
	if _, isStr := opt["h1"].(string); !isStr {
		t.Errorf("h1 = %v (тип %T), ожидалась строка после нормализации", opt["h1"], opt["h1"])
	}
	if opt["jc"] != 4 {
		t.Errorf("jc = %v, ожидался int 4", opt["jc"])
	}
}

// TestImportSingbox — sing-box JSON → clash, включая naive и congestion_control.
func TestImportSingbox(t *testing.T) {
	doc := `{
  "outbounds": [
    {"type":"direct","tag":"direct"},
    {"type":"vless","tag":"SB VLESS","server":"19.0.0.1","server_port":443,"uuid":"sb-uuid","flow":"xtls-rprx-vision",
     "tls":{"enabled":true,"server_name":"sb.example.com","reality":{"enabled":true,"public_key":"RPK","short_id":"sid"},"utls":{"enabled":true,"fingerprint":"chrome"}}},
    {"type":"tuic","tag":"SB TUIC","server":"19.0.0.2","server_port":8443,"uuid":"tuic-u","password":"tuic-pw","congestion_control":"bbr",
     "tls":{"enabled":true,"server_name":"tuic.example.com","alpn":["h3"]}},
    {"type":"naive","tag":"SB Naive","server":"19.0.0.3","server_port":443,"username":"nu","password":"np",
     "tls":{"enabled":true,"server_name":"naive.example.com"}},
    {"type":"wireguard","tag":"SB AWG","server":"19.0.0.4","server_port":51820,"private_key":"WPK","peer_public_key":"WPUB",
     "local_address":["10.9.0.2/32"],"jc":3,"h1":111,"h2":222,"h3":333,"h4":444}
  ]
}`
	clashYAML, meta, err := Import([]byte(doc), FormatSingbox)
	if err != nil {
		t.Fatalf("Import: %v", err)
	}
	var out struct {
		Proxies []map[string]any `yaml:"proxies"`
	}
	if err := yaml.Unmarshal(clashYAML, &out); err != nil {
		t.Fatalf("разбор результата: %v", err)
	}
	// direct отброшен → 4 прокси.
	if len(out.Proxies) != 4 {
		t.Fatalf("ожидалось 4 прокси (direct отброшен), получено %d", len(out.Proxies))
	}
	if len(meta.Servers) != 4 {
		t.Fatalf("ожидалось 4 сервера, получено %d", len(meta.Servers))
	}

	byType := map[string]map[string]any{}
	for _, p := range out.Proxies {
		byType[asString(p["type"])] = p
	}

	vless := byType["vless"]
	if vless == nil {
		t.Fatal("vless потерян")
	}
	if vless["servername"] != "sb.example.com" || vless["client-fingerprint"] != "chrome" {
		t.Errorf("vless tls = %v", vless)
	}
	ro, ok := vless["reality-opts"].(map[string]any)
	if !ok || ro["public-key"] != "RPK" || ro["short-id"] != "sid" {
		t.Errorf("reality-opts = %v", vless["reality-opts"])
	}

	tuic := byType["tuic"]
	if tuic == nil || tuic["congestion-controller"] != "bbr" || tuic["sni"] != "tuic.example.com" {
		t.Errorf("tuic mapping = %v", tuic)
	}

	// Naive сохраняет собственный тип. Ядро его не строит, и это ровно то,
	// что экран обязан сказать словами; подменённый type:http говорил вместо
	// этого про чужой протокол.
	naive := byType["naive"]
	if naive == nil || naive["tls"] != true || naive["username"] != "nu" {
		t.Errorf("naive mapping = %v", naive)
	}
	if byType["http"] != nil {
		t.Errorf("naive всё ещё подменяется на http: %v", byType["http"])
	}

	awg := byType["wireguard"]
	if awg == nil {
		t.Fatal("wireguard потерян")
	}
	if awg["ip"] != "10.9.0.2/32" {
		t.Errorf("wg ip = %v", awg["ip"])
	}
	opt, ok := awg["amnezia-wg-option"].(map[string]any)
	if !ok {
		t.Fatalf("amnezia-wg-option потерян: %v", awg)
	}
	if opt["jc"] != 3 {
		t.Errorf("jc = %v, ожидался int", opt["jc"])
	}
	for _, h := range []string{"h1", "h2", "h3", "h4"} {
		if _, isStr := opt[h].(string); !isStr {
			t.Errorf("%s = %v (тип %T), ожидалась строка", h, opt[h], opt[h])
		}
	}
}

// TestImportV2rayBase64 — base64-список URI.
func TestImportV2rayBase64(t *testing.T) {
	list := strings.Join([]string{
		"vless://u1@20.0.0.1:443?security=reality&pbk=PK&type=tcp#Node1",
		"trojan://tp@20.0.0.2:443?sni=s#Node2",
		"hysteria2://hp@20.0.0.3:8443?sni=s#Node3",
	}, "\n")
	encoded := base64.StdEncoding.EncodeToString([]byte(list))

	clashYAML, meta, err := Import([]byte(encoded), FormatV2ray)
	if err != nil {
		t.Fatalf("Import: %v", err)
	}
	var out struct {
		Proxies []map[string]any `yaml:"proxies"`
	}
	if err := yaml.Unmarshal(clashYAML, &out); err != nil {
		t.Fatalf("разбор результата: %v", err)
	}
	if len(out.Proxies) != 3 {
		t.Fatalf("ожидалось 3 прокси, получено %d", len(out.Proxies))
	}
	if len(meta.Servers) != 3 {
		t.Fatalf("ожидалось 3 сервера, получено %d", len(meta.Servers))
	}
	wantTypes := []string{"vless", "trojan", "hysteria2"}
	for i, p := range out.Proxies {
		if p["type"] != wantTypes[i] {
			t.Errorf("proxy[%d] type = %v, ожидался %v", i, p["type"], wantTypes[i])
		}
	}
}

// TestDetectFormat — таблица автоопределения формата.
func TestDetectFormat(t *testing.T) {
	clashDoc := "proxies:\n  - name: x\n    type: ss\n"
	singboxDoc := `{"outbounds":[{"type":"vless"}]}`
	v2rayDoc := base64.StdEncoding.EncodeToString([]byte("vless://u@h:443#n"))

	cases := []struct {
		name string
		in   string
		want string
	}{
		{"single-uri", "vless://u@h:443?type=tcp#n", FormatURI},
		{"singbox-json", singboxDoc, FormatSingbox},
		{"clash-yaml", clashDoc, FormatClash},
		{"v2ray-base64", v2rayDoc, FormatV2ray},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := detectFormat([]byte(tc.in)); got != tc.want {
				t.Errorf("detectFormat = %q, ожидался %q", got, tc.want)
			}
		})
	}
}

// TestImportAuto — Import с FormatAuto проходит сквозь автоопределение.
func TestImportAuto(t *testing.T) {
	uri := "vless://auto-uuid@21.0.0.1:443?security=reality&pbk=PK&type=tcp#Auto"
	clashYAML, _, err := Import([]byte(uri), FormatAuto)
	if err != nil {
		t.Fatalf("Import auto: %v", err)
	}
	px := proxyFromYAML(t, clashYAML)
	if px["type"] != "vless" || px["uuid"] != "auto-uuid" {
		t.Fatalf("auto-импорт неверный: %v", px)
	}
}

// TestImportEmpty — пустой ввод и неизвестный формат дают ошибку.
func TestImportErrors(t *testing.T) {
	if _, _, err := Import(nil, FormatAuto); err == nil {
		t.Error("ожидалась ошибка на пустом вводе")
	}
	if _, _, err := Import([]byte("x"), "bogus"); err == nil {
		t.Error("ожидалась ошибка на неизвестном формате")
	}
	if _, _, err := Import([]byte("proxies: []"), FormatClash); err == nil {
		t.Error("ожидалась ошибка: clash без прокси")
	}
}

// TestMetadataCountryFromName — страна извлекается из имени прокси для UI-списка.
func TestMetadataCountryFromName(t *testing.T) {
	uri := "vless://u@22.0.0.1:443?security=reality&pbk=PK&type=tcp#NL%20Amsterdam"
	_, meta, err := Import([]byte(uri), FormatURI)
	if err != nil {
		t.Fatalf("Import: %v", err)
	}
	if len(meta.Servers) != 1 || meta.Servers[0].Country != "NL" {
		t.Fatalf("ожидалась страна NL, получено %+v", meta.Servers)
	}
}

// detour в sing-box означает «набирать этот outbound ЧЕРЕЗ тот». Пока поле
// молча терялось, цепочка «через 🇷🇺» превращалась в прямой набор адреса выхода
// — другой маршрут под тем же именем, — а сам промежуточный узел стоял в списке
// выходов и выигрывал автоподбор своими 693 мс.
func TestSingboxDetourBecomesDialerProxyAndMarksRelay(t *testing.T) {
	doc := `{
  "outbounds": [
    {"type":"direct","tag":"direct"},
    {"type":"vless","tag":"RU Relay","server":"10.1.0.1","server_port":443,"uuid":"u1",
     "tls":{"enabled":true,"server_name":"r.example.com"}},
    {"type":"vless","tag":"DE Exit","server":"10.2.0.1","server_port":443,"uuid":"u2","detour":"RU Relay",
     "tls":{"enabled":true,"server_name":"d.example.com"}},
    {"type":"hysteria2","tag":"CA Direct","server":"10.3.0.1","server_port":443,"password":"p","detour":"direct"}
  ]
}`
	clashYAML, meta, err := Import([]byte(doc), FormatSingbox)
	if err != nil {
		t.Fatalf("Import: %v", err)
	}
	var out struct {
		Proxies []map[string]any `yaml:"proxies"`
	}
	if err := yaml.Unmarshal(clashYAML, &out); err != nil {
		t.Fatalf("разбор результата: %v", err)
	}
	byName := map[string]map[string]any{}
	for _, p := range out.Proxies {
		byName[asString(p["name"])] = p
	}

	if got := asString(byName["DE Exit"]["dialer-proxy"]); got != "RU Relay" {
		t.Fatalf("detour не стал dialer-proxy: %q", got)
	}
	// detour в служебный direct — ссылка в никуда: в clash-списке такого
	// прокси нет, и mihomo провалил бы КАЖДЫЙ набор через этот узел.
	if _, ok := byName["CA Direct"]["dialer-proxy"]; ok {
		t.Fatalf("ссылка в несуществующий прокси не снята: %v", byName["CA Direct"])
	}

	roles := map[string]string{}
	for _, s := range meta.Servers {
		roles[s.Name] = s.Role
	}
	if roles["RU Relay"] != subscription.RoleRelay {
		t.Errorf("промежуточный узел обязан быть relay, получено %q", roles["RU Relay"])
	}
	if roles["DE Exit"] != subscription.RoleExit || roles["CA Direct"] != subscription.RoleExit {
		t.Errorf("выходы обязаны быть exit: %v", roles)
	}
}

// Метаданные обязаны различать ИНБАУНДЫ, а не только протоколы: пока строка
// собиралась по одному type, отказ httpupgrade прятался за числом соседнего
// транспорта того же VLESS.
func TestSingboxMetadataCarriesTransportAndSecurity(t *testing.T) {
	doc := `{
  "outbounds": [
    {"type":"vless","tag":"V Reality","server":"10.1.0.1","server_port":443,"uuid":"u1",
     "tls":{"enabled":true,"server_name":"r.example.com","reality":{"enabled":true,"public_key":"K"}}},
    {"type":"vless","tag":"V HU","server":"10.1.0.2","server_port":443,"uuid":"u2",
     "tls":{"enabled":true,"server_name":"h.example.com"},
     "transport":{"type":"httpupgrade","path":"/up","host":"h.example.com"}},
    {"type":"vless","tag":"V WS","server":"10.1.0.3","server_port":443,"uuid":"u3",
     "tls":{"enabled":true,"server_name":"w.example.com"},
     "transport":{"type":"ws","path":"/ws"}},
    {"type":"hysteria2","tag":"H2","server":"10.1.0.4","server_port":443,"password":"p"},
    {"type":"naive","tag":"NV","server":"10.1.0.5","server_port":443,"username":"u","password":"p"}
  ]
}`
	_, meta, err := Import([]byte(doc), FormatSingbox)
	if err != nil {
		t.Fatalf("Import: %v", err)
	}
	got := map[string][2]string{}
	for _, s := range meta.Servers {
		got[s.Name] = [2]string{s.Transport, s.Security}
	}
	want := map[string][2]string{
		"V Reality": {"tcp", "reality"},
		"V HU":      {"httpupgrade", "tls"},
		"V WS":      {"ws", "tls"},
		// У QUIC-семейства понятия «транспорт поверх TCP» нет, а TLS от
		// протокола неотделим.
		"H2": {"", "tls"},
		"NV": {"tcp", "tls"},
	}
	for name, w := range want {
		if got[name] != w {
			t.Errorf("%s: транспорт/защита = %v, ожидалось %v", name, got[name], w)
		}
	}
}

// httpupgrade из sing-box обязан доехать до ядра под именем, которое ядро
// знает, вместе с Host: без него апгрейд на стороне узла не совпадёт.
func TestSingboxHTTPUpgradeMapsToWSFlag(t *testing.T) {
	doc := `{"outbounds":[{"type":"vless","tag":"HU","server":"10.0.0.9","server_port":443,"uuid":"u",
	 "tls":{"enabled":true,"server_name":"s.example.com"},
	 "transport":{"type":"httpupgrade","path":"/up","host":"hu.example.com"}}]}`
	clashYAML, _, err := Import([]byte(doc), FormatSingbox)
	if err != nil {
		t.Fatalf("Import: %v", err)
	}
	px := proxyFromYAML(t, clashYAML)
	if px["network"] != "ws" {
		t.Fatalf("network = %v (mihomo не знает httpupgrade)", px["network"])
	}
	ws, ok := px["ws-opts"].(map[string]any)
	if !ok || ws["v2ray-http-upgrade"] != true || ws["path"] != "/up" {
		t.Fatalf("ws-opts = %v", px["ws-opts"])
	}
	hdrs, _ := ws["headers"].(map[string]any)
	if hdrs == nil || hdrs["Host"] != "hu.example.com" {
		t.Fatalf("Host потерян: %v", ws["headers"])
	}
}
