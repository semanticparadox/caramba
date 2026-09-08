package profile

import (
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

// sampleClash имитирует подписку панели: NL-awg несёт обфускацию AmneziaWG в
// блоке amnezia-wg-option (jc/jmin/.../s2 — числа, h1..h4 — строки), как того
// требует mihomo. Сборка профиля не должна терять этот блок.
const sampleClash = `
proxies:
  - name: "NL-awg"
    type: wireguard
    server: a
    port: 1
    ip: "10.10.0.2/32"
    private-key: priv
    public-key: pub
    udp: true
    mtu: 1280
    amnezia-wg-option:
      jc: 4
      jmin: 8
      jmax: 80
      s1: 15
      s2: 25
      h1: "1111111111"
      h2: "2222222222"
      h3: "3333333333"
      h4: "4444444444"
  - {name: "NL-vless", type: vless, server: b, port: 2}
  - {name: "DE-awg", type: wireguard, server: c, port: 3}
  - {name: "FI-hy2", type: hysteria2, server: d, port: 4}
proxy-groups:
  - {name: CARAMBA, type: select, proxies: ["Auto-All", "NL-awg", "NL-vless", "DE-awg", "FI-hy2"]}
  - {name: Auto-All, type: url-test, proxies: ["NL-awg", "NL-vless", "DE-awg", "FI-hy2"]}
rules:
  - MATCH,CARAMBA
`

func assemble(t *testing.T, proto string) map[string]any {
	t.Helper()
	p := DefaultPolicy()
	p.Tun.Enable = false // не нужен TUN для проверки групп
	p.Protocol = proto
	out, err := AssembleMihomoConfig([]byte(sampleClash), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := map[string]any{}
	if err := yaml.Unmarshal(out, &doc); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	return doc
}

func groupByName(doc map[string]any, name string) map[string]any {
	groups, _ := doc["proxy-groups"].([]any)
	for _, g := range groups {
		gm, _ := g.(map[string]any)
		if n, _ := gm["name"].(string); n == name {
			return gm
		}
	}
	return nil
}

func TestApplyProtocolPrependsProtoGroup(t *testing.T) {
	doc := assemble(t, "AmneziaWG")
	pg := groupByName(doc, protoGroupName)
	if pg == nil {
		t.Fatal("служебная группа протокола не создана")
	}
	names, _ := pg["proxies"].([]any)
	if len(names) != 2 {
		t.Errorf("ожидалось 2 wireguard-прокси, получено %v", names)
	}
	car := groupByName(doc, CarambaSelector)
	first, _ := car["proxies"].([]any)
	if len(first) == 0 || first[0] != protoGroupName {
		t.Errorf("группа протокола должна быть первой в CARAMBA, получено %v", first)
	}
}

func TestApplyProtocolAutoLeavesConfig(t *testing.T) {
	for _, p := range []string{"", "Авто", "auto"} {
		doc := assemble(t, p)
		if groupByName(doc, protoGroupName) != nil {
			t.Errorf("для %q служебная группа не должна создаваться", p)
		}
		car := groupByName(doc, CarambaSelector)
		first, _ := car["proxies"].([]any)
		if len(first) == 0 || first[0] != "Auto-All" {
			t.Errorf("для %q первым в CARAMBA должен остаться Auto-All, получено %v", p, first)
		}
	}
}

func TestApplyProtocolUnknownOrMissingType(t *testing.T) {
	// неизвестное имя -> без изменений
	doc := assemble(t, "Nonsense")
	if groupByName(doc, protoGroupName) != nil {
		t.Error("неизвестный протокол не должен менять конфиг")
	}
	// известный протокол, но нет прокси такого типа (tuic отсутствует)
	doc = assemble(t, "TUIC")
	if groupByName(doc, protoGroupName) != nil {
		t.Error("при отсутствии прокси нужного типа группа не создаётся")
	}
}

// proxyByName возвращает прокси с указанным name из секции proxies.
func proxyByName(doc map[string]any, name string) map[string]any {
	proxies, _ := doc["proxies"].([]any)
	for _, p := range proxies {
		pm, _ := p.(map[string]any)
		if n, _ := pm["name"].(string); n == name {
			return pm
		}
	}
	return nil
}

// TestAssemblePreservesAmneziaOption проверяет, что сборка профиля не трогает
// узлы: блок amnezia-wg-option на wireguard-прокси сохраняется как есть, под тем
// же ключом, с числовыми jc/jmin/.../s2 и строковыми h1..h4 (схема mihomo).
func TestAssemblePreservesAmneziaOption(t *testing.T) {
	doc := assemble(t, "AmneziaWG")

	awg := proxyByName(doc, "NL-awg")
	if awg == nil {
		t.Fatal("прокси NL-awg потерян при сборке")
	}
	opt, ok := awg["amnezia-wg-option"].(map[string]any)
	if !ok {
		t.Fatalf("amnezia-wg-option отсутствует или не map: %T", awg["amnezia-wg-option"])
	}

	// jc/jmin/jmax/s1/s2 должны остаться числами.
	for _, k := range []string{"jc", "jmin", "jmax", "s1", "s2"} {
		switch opt[k].(type) {
		case int, int64, uint64, float64:
			// ok
		default:
			t.Errorf("ключ %q должен быть числом, получено %T", k, opt[k])
		}
	}
	// h1..h4 должны остаться строками (иначе декодер mihomo отвергнет прокси).
	for _, k := range []string{"h1", "h2", "h3", "h4"} {
		if _, ok := opt[k].(string); !ok {
			t.Errorf("ключ %q должен быть строкой, получено %T (%v)", k, opt[k], opt[k])
		}
	}
	// Базовые поля wireguard тоже на месте.
	if pk, _ := awg["private-key"].(string); pk != "priv" {
		t.Errorf("private-key не сохранён: %v", awg["private-key"])
	}
	if ip, _ := awg["ip"].(string); ip != "10.10.0.2/32" {
		t.Errorf("ip не сохранён: %v", awg["ip"])
	}
}

func TestProtocolMappingCoversPriority(t *testing.T) {
	for _, name := range []string{"AmneziaWG", "VLESS-Reality", "Hysteria2", "TUIC", "Shadowsocks"} {
		if protocolClashType[name] == "" {
			t.Errorf("нет clash-типа для протокола %q", name)
		}
	}
	if !strings.EqualFold(protocolClashType["AmneziaWG"], "wireguard") {
		t.Error("AmneziaWG должен маппиться в wireguard")
	}
}

// Один незнакомый ядру тип в подписке отвергал ВЕСЬ конфиг mihomo — то есть
// оставлял человека без связи. Пока импорт подменял naive на type:http, это
// было незаметно ценой лжи про протокол; с честным типом защищать обязана
// сборка профиля.
func TestAssembleDropsProxiesTheCoreCannotBuild(t *testing.T) {
	const raw = `
proxies:
  - {name: "DE Exit", type: vless, server: 10.0.0.1, port: 443, uuid: u, dialer-proxy: "RU Naive"}
  - {name: "RU Naive", type: naive, server: 10.0.0.2, port: 443, username: u, password: p}
proxy-groups:
  - {name: CARAMBA, type: select, proxies: ["DE Exit", "RU Naive", "DIRECT"]}
`
	out, err := AssembleMihomoConfig([]byte(raw), DefaultPolicy())
	if err != nil {
		t.Fatalf("AssembleMihomoConfig: %v", err)
	}
	doc := map[string]any{}
	if err := yaml.Unmarshal(out, &doc); err != nil {
		t.Fatalf("разбор результата: %v", err)
	}
	proxies, _ := doc["proxies"].([]any)
	if len(proxies) != 1 {
		t.Fatalf("ожидался один прокси (naive выброшен), получено %d: %v", len(proxies), proxies)
	}
	kept, _ := proxies[0].(map[string]any)
	if kept["name"] != "DE Exit" {
		t.Fatalf("выброшен не тот прокси: %v", kept)
	}
	// Ссылка на выброшенный узел проваливала бы каждый набор через этот выход.
	if _, ok := kept["dialer-proxy"]; ok {
		t.Errorf("ссылка в никуда не снята: %v", kept)
	}
	groups, _ := doc["proxy-groups"].([]any)
	for _, g := range groups {
		gm, _ := g.(map[string]any)
		names, _ := gm["proxies"].([]any)
		for _, n := range names {
			if n == "RU Naive" {
				t.Errorf("имя выброшенного узла осталось в группе %v: %v", gm["name"], names)
			}
		}
	}
}

// Подписка целиком из непостроимых узлов не должна давать пустую select-группу:
// mihomo отвергает такую группу так же решительно, как незнакомый тип.
func TestAssembleKeepsGroupNonEmptyAfterDroppingEverything(t *testing.T) {
	const raw = `
proxies:
  - {name: "RU Naive", type: naive, server: 10.0.0.2, port: 443}
proxy-groups:
  - {name: CARAMBA, type: select, proxies: ["RU Naive"]}
`
	out, err := AssembleMihomoConfig([]byte(raw), DefaultPolicy())
	if err != nil {
		t.Fatalf("AssembleMihomoConfig: %v", err)
	}
	doc := map[string]any{}
	if err := yaml.Unmarshal(out, &doc); err != nil {
		t.Fatalf("разбор результата: %v", err)
	}
	groups, _ := doc["proxy-groups"].([]any)
	if len(groups) == 0 {
		t.Fatal("группы потеряны")
	}
	gm, _ := groups[0].(map[string]any)
	names, _ := gm["proxies"].([]any)
	if len(names) != 1 || names[0] != "DIRECT" {
		t.Fatalf("опустевшая группа обязана удержать DIRECT, получено %v", names)
	}
}

// Конфиг, который уходит В ЯДРО, обязан нести сеть под именем, которое ядро
// знает. Панель отдаёт «httpupgrade» прямо в clash-теле, и панельная подписка
// через импорт не проходит вовсе — значит чинить обязана сборка профиля, иначе
// узел молча вырождается в обычный TLS-поток и отвечает отказом.
func TestAssembleRemapsHTTPUpgradeForTheCore(t *testing.T) {
	const raw = `
proxies:
  - {name: "DE HTTP", type: vless, server: 10.0.0.1, port: 13400, uuid: u, tls: true,
     network: httpupgrade, http-upgrade-opts: {path: /hu, host: essentialhome.live}}
proxy-groups:
  - {name: CARAMBA, type: select, proxies: ["DE HTTP", "DIRECT"]}
`
	out, err := AssembleMihomoConfig([]byte(raw), DefaultPolicy())
	if err != nil {
		t.Fatalf("AssembleMihomoConfig: %v", err)
	}
	doc := map[string]any{}
	if err := yaml.Unmarshal(out, &doc); err != nil {
		t.Fatalf("разбор результата: %v", err)
	}
	px := proxyByName(doc, "DE HTTP")
	if px == nil {
		t.Fatal("узел потерян сборкой профиля")
	}
	if px["network"] != "ws" {
		t.Fatalf("network = %v", px["network"])
	}
	opts, _ := px["ws-opts"].(map[string]any)
	if opts == nil || opts["v2ray-http-upgrade"] != true || opts["path"] != "/hu" {
		t.Fatalf("ws-opts = %v", px["ws-opts"])
	}
}
