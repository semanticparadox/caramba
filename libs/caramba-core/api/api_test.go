package api

import (
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/subscription"
)

// TestFriendlyProtocol фиксирует отображение clash-типов прокси в дружелюбные
// имена протоколов, которыми оперируют autotune/profile. Это отображение —
// контракт между секцией proxies подписки (clash-типы) и ключами протоколов в
// кандидатах/ProxyConfigs, поэтому покрываем его явно.
func TestFriendlyProtocol(t *testing.T) {
	cases := map[string]string{
		"wireguard": "AmneziaWG",
		"vless":     "VLESS-Reality",
		"hysteria2": "Hysteria2",
		"tuic":      "TUIC",
		"ss":        "Shadowsocks",
		"VLESS":     "VLESS-Reality", // регистр не важен
		"trojan":    "trojan",        // неизвестный тип — как есть
	}
	for in, want := range cases {
		if got := friendlyProtocol(in); got != want {
			t.Errorf("friendlyProtocol(%q) = %q, ожидалось %q", in, got, want)
		}
	}
}

// TestAggregateCandidates проверяет ключевой контракт автоподбора: узел,
// объявляющий несколько протоколов, приходит из подписки несколькими записями
// proxies с одним именем и должен свернуться в ОДНОГО кандидата со всеми
// протоколами (без дублей ServerID), чтобы mihomo-Prober проверил каждый
// протокол узла за один проход. Порядок узлов и протоколов — по первому
// появлению; повтор протокола внутри узла не дублируется.
func TestAggregateCandidates(t *testing.T) {
	servers := []subscription.Server{
		{Name: "node-de", Type: "vless", Server: "de.example", Port: 443, Country: "DE"},
		{Name: "node-de", Type: "hysteria2", Server: "de.example", Port: 8443, Country: "DE"},
		{Name: "node-de", Type: "ss", Server: "de.example", Port: 9000, Country: "DE"},
		{Name: "node-nl", Type: "vless", Server: "nl.example", Port: 443, Country: "NL"},
		{Name: "node-de", Type: "vless", Server: "de.example", Port: 443, Country: "DE"}, // дубль протокола
	}

	cands := aggregateCandidates(servers)

	if len(cands) != 2 {
		t.Fatalf("ожидалось 2 кандидата (по числу уникальных имён), получено %d: %+v", len(cands), cands)
	}

	de := cands[0]
	if de.ServerID != "node-de" || de.Country != "DE" || de.Host != "de.example" {
		t.Errorf("первый кандидат: %+v", de)
	}
	wantDE := []string{"VLESS-Reality", "Hysteria2", "Shadowsocks"}
	if len(de.Protocols) != len(wantDE) {
		t.Fatalf("node-de протоколы = %v, ожидалось %v", de.Protocols, wantDE)
	}
	for i, p := range wantDE {
		if de.Protocols[i] != p {
			t.Errorf("node-de протокол[%d] = %q, ожидалось %q (порядок по первому появлению)", i, de.Protocols[i], p)
		}
	}

	nl := cands[1]
	if nl.ServerID != "node-nl" || len(nl.Protocols) != 1 || nl.Protocols[0] != "VLESS-Reality" {
		t.Errorf("node-nl кандидат: %+v", nl)
	}
}
