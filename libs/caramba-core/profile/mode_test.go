package profile

import (
	"strings"
	"testing"
)

// sampleWithInbounds повторяет форму конфига подписки, которая уже несёт свои
// инбаунды и tun. Proxy-режим обязан заменить их ровно одним mixed-портом.
const sampleWithInbounds = `
port: 7891
socks-port: 7892
mixed-port: 1080
tun:
  enable: true
  stack: system
proxies:
  - name: "DE Stealth"
    type: vless
    server: 1.2.3.4
    port: 443
proxy-groups:
  - name: CARAMBA
    type: select
    proxies: ["DE Stealth"]
rules:
  - MATCH,CARAMBA
`

// proxyPolicy — политика по умолчанию, переведённая в proxy-режим.
func proxyPolicy(port int) Policy {
	p := DefaultPolicy()
	p.Mode = ModeProxy
	p.Proxy.MixedPort = port
	return p
}

func TestDefaultPolicyIsTunMode(t *testing.T) {
	p := DefaultPolicy()
	if p.EffectiveMode() != ModeTun {
		t.Fatalf("режим по умолчанию должен быть tun, получено %q", p.EffectiveMode())
	}
	if got := p.EffectiveProxy().MixedPort; got != DefaultMixedPort {
		t.Fatalf("порт mixed по умолчанию %d, ожидался %d", got, DefaultMixedPort)
	}
	if got := p.EffectiveProxy().BindAddress; got != DefaultBindAddress {
		t.Fatalf("bind-address по умолчанию %q, ожидался %q", got, DefaultBindAddress)
	}
}

// Пустой Mode должен вести себя как tun: политики, собранные до появления
// proxy-режима, не должны молча менять поведение.
func TestEmptyModeFallsBackToTun(t *testing.T) {
	p := DefaultPolicy()
	p.Mode = ""
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	if _, ok := doc["tun"]; !ok {
		t.Fatalf("при пустом Mode ожидалась секция tun:\n%s", out)
	}
}

func TestTunModeHasNoMixedInbound(t *testing.T) {
	out, err := AssembleMihomoConfig([]byte(sampleConfig), DefaultPolicy())
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	if _, ok := doc["tun"]; !ok {
		t.Fatal("ожидалась секция tun")
	}
	if _, ok := doc["bind-address"]; ok {
		t.Fatalf("в tun-режиме bind-address не выставляется:\n%s", out)
	}
	// mixed-port из подписки остаётся нетронутым (мы им не управляем).
	if got := doc["mixed-port"]; got != 7890 {
		t.Fatalf("mixed-port подписки изменён: %v", got)
	}
}

func TestProxyModeDropsTunAndSetsMixedPort(t *testing.T) {
	out, err := AssembleMihomoConfig([]byte(sampleWithInbounds), proxyPolicy(7899))
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	if _, ok := doc["tun"]; ok {
		t.Fatalf("в proxy-режиме секция tun должна быть удалена:\n%s", out)
	}
	if got := doc["mixed-port"]; got != 7899 {
		t.Fatalf("mixed-port %v, ожидался 7899", got)
	}
	if got := doc["bind-address"]; got != DefaultBindAddress {
		t.Fatalf("bind-address %v, ожидался %q", got, DefaultBindAddress)
	}
	for _, key := range []string{"port", "socks-port", "redir-port", "tproxy-port"} {
		if _, ok := doc[key]; ok {
			t.Fatalf("в proxy-режиме инбаунд %q должен быть удалён:\n%s", key, out)
		}
	}
	if got := doc["allow-lan"]; got != false {
		t.Fatalf("allow-lan %v, ожидался false", got)
	}
}

// Нулевой порт означает значение по умолчанию, а не «слушать порт 0».
func TestProxyModeZeroPortUsesDefault(t *testing.T) {
	p := DefaultPolicy()
	p.Mode = ModeProxy
	p.Proxy = ProxyConfig{}
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	if got := doc["mixed-port"]; got != DefaultMixedPort {
		t.Fatalf("mixed-port %v, ожидался %d", got, DefaultMixedPort)
	}
	if got := doc["bind-address"]; got != DefaultBindAddress {
		t.Fatalf("bind-address %v, ожидался %q", got, DefaultBindAddress)
	}
}

func TestProxyModeAllowLAN(t *testing.T) {
	p := proxyPolicy(7890)
	p.Proxy.AllowLAN = true
	p.Proxy.BindAddress = "*"
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	if got := doc["allow-lan"]; got != true {
		t.Fatalf("allow-lan %v, ожидался true", got)
	}
	if got := doc["bind-address"]; got != "*" {
		t.Fatalf("bind-address %v, ожидался \"*\"", got)
	}
}

// В proxy-режиме fake-ip понижается до redir-host: синтетические адреса некому
// разворачивать обратно без TUN, а GEOIP по ним считался бы мусором.
func TestProxyModeDNSUsesRedirHost(t *testing.T) {
	out, err := AssembleMihomoConfig([]byte(sampleConfig), proxyPolicy(7890))
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	dns, ok := doc["dns"].(map[string]any)
	if !ok {
		t.Fatalf("ожидалась секция dns:\n%s", out)
	}
	if dns["enable"] != true {
		t.Fatalf("dns.enable %v, ожидался true", dns["enable"])
	}
	if got := dns["enhanced-mode"]; got != "redir-host" {
		t.Fatalf("enhanced-mode %v, ожидался redir-host", got)
	}
	if _, ok := dns["fake-ip-range"]; ok {
		t.Fatalf("fake-ip-range не должен попадать в redir-host конфиг:\n%s", out)
	}
	if _, ok := dns["nameserver"]; !ok {
		t.Fatalf("резолверы должны сохраниться:\n%s", out)
	}
}

// В tun-режиме fake-ip остаётся fake-ip: понижение касается только proxy.
func TestTunModeKeepsFakeIP(t *testing.T) {
	out, err := AssembleMihomoConfig([]byte(sampleConfig), DefaultPolicy())
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	dns, ok := doc["dns"].(map[string]any)
	if !ok {
		t.Fatalf("ожидалась секция dns:\n%s", out)
	}
	if got := dns["enhanced-mode"]; got != "fake-ip" {
		t.Fatalf("enhanced-mode %v, ожидался fake-ip", got)
	}
	if got := dns["fake-ip-range"]; got != "198.18.0.1/16" {
		t.Fatalf("fake-ip-range %v, ожидался 198.18.0.1/16", got)
	}
}

// Kill-switch в proxy-режиме не применяется: финальный REJECT сделал бы
// локальный прокси бесполезным, а утекать без TUN нечему.
func TestProxyModeIgnoresKillSwitch(t *testing.T) {
	p := proxyPolicy(7890)
	p.KillSwitch = true
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	s := string(out)
	if strings.Contains(s, "MATCH,REJECT") {
		t.Fatalf("в proxy-режиме финальный REJECT недопустим:\n%s", s)
	}
	if !strings.Contains(s, "MATCH,"+CarambaSelector) {
		t.Fatalf("ожидалось MATCH,CARAMBA:\n%s", s)
	}
}

// Allow-list в proxy-режиме тоже не должен вырождаться в REJECT.
func TestProxyModeAllowListKeepsDirectFallback(t *testing.T) {
	p := proxyPolicy(7890)
	p.KillSwitch = true
	p.Split.AllowProcesses = []string{"firefox"}
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	s := string(out)
	if !strings.Contains(s, "PROCESS-NAME,firefox,"+CarambaSelector) {
		t.Fatalf("ожидался allow-list процесса:\n%s", s)
	}
	if !strings.Contains(s, "MATCH,DIRECT") {
		t.Fatalf("ожидался MATCH,DIRECT вместо REJECT:\n%s", s)
	}
}

// Узлы, селектор CARAMBA и правила пресета режимом не затрагиваются.
func TestProxyModePreservesProxiesAndSelector(t *testing.T) {
	out, err := AssembleMihomoConfig([]byte(sampleWithInbounds), proxyPolicy(7890))
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	proxies, ok := doc["proxies"].([]any)
	if !ok || len(proxies) != 1 {
		t.Fatalf("proxies не сохранены: %v", doc["proxies"])
	}
	groups, ok := doc["proxy-groups"].([]any)
	if !ok || len(groups) != 1 {
		t.Fatalf("proxy-groups не сохранены: %v", doc["proxy-groups"])
	}
	g, ok := groups[0].(map[string]any)
	if !ok || g["name"] != CarambaSelector {
		t.Fatalf("селектор %s не сохранён: %v", CarambaSelector, groups[0])
	}
	if got := doc["mode"]; got != "rule" {
		t.Fatalf("mode %v, ожидался rule", got)
	}
}
