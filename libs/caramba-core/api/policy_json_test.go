package api

import (
	"strings"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/profile"
)

// Полная политика раскладывается по всем полям profile.Policy.
func TestSetPolicyJSONFull(t *testing.T) {
	core := newTestCore(t)
	err := core.SetPolicyJSON(`{
	  "protocol":"Hysteria2",
	  "preset":"ru-smart",
	  "relay":"tr",
	  "stack":"system",
	  "mtu":1400,
	  "ipv6":true,
	  "fakeIp":false,
	  "killSwitch":false,
	  "dns":{"nameservers":["https://9.9.9.9/dns-query"],"fallback":["tls://8.8.8.8:853"]},
	  "split":{"mode":"allow","apps":["org.mozilla.firefox"," "],"bypassDomains":["example.com"]}
	}`)
	if err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}

	p := core.policy
	if p.Protocol != "Hysteria2" {
		t.Fatalf("protocol %q", p.Protocol)
	}
	if p.Routing == nil {
		t.Fatal("preset не применён: Routing == nil")
	}
	if core.relayCountry != "TR" {
		t.Fatalf("relay %q, ожидался TR", core.relayCountry)
	}
	if p.Tun.Stack != profile.StackSystem {
		t.Fatalf("stack %q", p.Tun.Stack)
	}
	if p.Tun.MTU != 1400 {
		t.Fatalf("mtu %d", p.Tun.MTU)
	}
	if !p.IPv6 {
		t.Fatal("ipv6 не применён")
	}
	if p.DNS.EnhancedMode != "redir-host" {
		t.Fatalf("fakeIp:false должен дать redir-host, получено %q", p.DNS.EnhancedMode)
	}
	if p.KillSwitch {
		t.Fatal("killSwitch не сброшен")
	}
	if len(p.DNS.Nameservers) != 1 || p.DNS.Nameservers[0] != "https://9.9.9.9/dns-query" {
		t.Fatalf("nameservers %v", p.DNS.Nameservers)
	}
	if len(p.DNS.FallbackNameservers) != 1 || p.DNS.FallbackNameservers[0] != "tls://8.8.8.8:853" {
		t.Fatalf("fallback %v", p.DNS.FallbackNameservers)
	}
	// Пустые элементы списка приложений отбрасываются.
	if len(p.Split.AllowProcesses) != 1 || p.Split.AllowProcesses[0] != "org.mozilla.firefox" {
		t.Fatalf("allow-процессы %v", p.Split.AllowProcesses)
	}
	if len(p.Split.BypassProcesses) != 0 {
		t.Fatalf("bypass-процессы должны быть пусты при mode=allow: %v", p.Split.BypassProcesses)
	}
	if len(p.Split.BypassDomains) != 1 || p.Split.BypassDomains[0] != "example.com" {
		t.Fatalf("bypass-домены %v", p.Split.BypassDomains)
	}
}

// split.mode = bypass кладёт приложения в другой список.
func TestSetPolicyJSONSplitBypass(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetPolicyJSON(`{"split":{"mode":"bypass","apps":["git","com.bank.app"]}}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	p := core.policy
	if len(p.Split.BypassProcesses) != 2 {
		t.Fatalf("bypass-процессы %v", p.Split.BypassProcesses)
	}
	if len(p.Split.AllowProcesses) != 0 {
		t.Fatalf("allow-процессы должны быть пусты: %v", p.Split.AllowProcesses)
	}
}

// split.mode = off сбрасывает оба списка приложений, оставляя домены.
func TestSetPolicyJSONSplitOff(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetPolicyJSON(`{"split":{"mode":"allow","apps":["firefox"]}}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	if err := core.SetPolicyJSON(`{"split":{"mode":"off","apps":["firefox"],"bypassDomains":["a.com"]}}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	p := core.policy
	if len(p.Split.AllowProcesses) != 0 || len(p.Split.BypassProcesses) != 0 {
		t.Fatalf("списки приложений должны быть пусты: %+v", p.Split)
	}
	if len(p.Split.BypassDomains) != 1 {
		t.Fatalf("bypass-домены %v", p.Split.BypassDomains)
	}
}

// Отсутствующие поля не трогают текущие значения, неизвестные ключи молча
// игнорируются.
func TestSetPolicyJSONPartialAndUnknownKeys(t *testing.T) {
	core := newTestCore(t)
	before := core.policy
	if err := core.SetPolicyJSON(`{"mtu":1280,"somethingNew":{"a":1},"futureFlag":true}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	p := core.policy
	if p.Tun.MTU != 1280 {
		t.Fatalf("mtu %d", p.Tun.MTU)
	}
	if p.KillSwitch != before.KillSwitch || p.DNS.EnhancedMode != before.DNS.EnhancedMode {
		t.Fatalf("нетронутые поля изменились: %+v -> %+v", before, p)
	}
	if p.Protocol != before.Protocol || p.Tun.Stack != before.Tun.Stack {
		t.Fatalf("нетронутые поля изменились: %+v -> %+v", before, p)
	}
}

// "auto" и пустая строка в protocol означают автоматику панели.
func TestSetPolicyJSONProtocolAuto(t *testing.T) {
	core := newTestCore(t)
	core.SetProtocol("TUIC")
	if err := core.SetPolicyJSON(`{"protocol":"auto"}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	if core.policy.Protocol != "" {
		t.Fatalf("protocol %q, ожидалась пустая строка (автоматика)", core.policy.Protocol)
	}
}

// Пустой preset снимает «умную» маршрутизацию.
func TestSetPolicyJSONEmptyPresetClearsRouting(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetPolicyJSON(`{"preset":"global"}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	if core.policy.Routing == nil {
		t.Fatal("пресет не применён")
	}
	if err := core.SetPolicyJSON(`{"preset":""}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	if core.policy.Routing != nil {
		t.Fatal("пустой preset должен снимать маршрутизацию")
	}
}

// Недопустимое значение перечислимого поля: ошибка называет поле, политика не
// меняется вовсе (применение атомарно).
func TestSetPolicyJSONInvalidValues(t *testing.T) {
	cases := []struct {
		name  string
		json  string
		field string
	}{
		{"protocol", `{"protocol":"WireGuardPlus"}`, "protocol"},
		{"preset", `{"preset":"mars-smart"}`, "preset"},
		{"relay", `{"relay":"TURKEY"}`, "relay"},
		{"stack", `{"stack":"userspace"}`, "stack"},
		{"mtu", `{"mtu":-1}`, "mtu"},
		{"split.mode", `{"split":{"mode":"whitelist"}}`, "split.mode"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			core := newTestCore(t)
			// Ставим заведомо отличное от умолчания состояние, чтобы увидеть,
			// если частичное применение всё-таки просочилось.
			if err := core.SetPolicyJSON(`{"killSwitch":false,"mtu":1400}`); err != nil {
				t.Fatalf("подготовка: %v", err)
			}
			before := core.policy

			err := core.SetPolicyJSON(tc.json)
			if err == nil {
				t.Fatalf("ожидалась ошибка для %s", tc.json)
			}
			if !strings.Contains(err.Error(), tc.field) {
				t.Fatalf("в ошибке %q не названо поле %q", err, tc.field)
			}
			if core.policy.KillSwitch != before.KillSwitch || core.policy.Tun.MTU != before.Tun.MTU {
				t.Fatalf("политика изменилась при ошибке: %+v -> %+v", before, core.policy)
			}
		})
	}
}

// Пустая строка — не «пустая политика», а ошибка: молчаливое игнорирование
// скрыло бы от приложения потерянный вызов.
func TestSetPolicyJSONRejectsEmptyAndGarbage(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetPolicyJSON("  "); err == nil {
		t.Fatal("ожидалась ошибка на пустую строку")
	}
	if err := core.SetPolicyJSON("not json"); err == nil {
		t.Fatal("ожидалась ошибка на некорректный JSON")
	}
}
