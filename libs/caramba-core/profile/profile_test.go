package profile

import (
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

const sampleConfig = `
mixed-port: 7890
mode: rule
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

func unmarshal(t *testing.T, data []byte) map[string]any {
	t.Helper()
	m := map[string]any{}
	if err := yaml.Unmarshal(data, &m); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	return m
}

func TestAssemblePreservesProxies(t *testing.T) {
	out, err := AssembleMihomoConfig([]byte(sampleConfig), DefaultPolicy())
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	proxies, ok := doc["proxies"].([]any)
	if !ok || len(proxies) != 1 {
		t.Fatalf("proxies не сохранены: %v", doc["proxies"])
	}
	if _, ok := doc["tun"]; !ok {
		t.Fatal("ожидалась секция tun")
	}
	if _, ok := doc["dns"]; !ok {
		t.Fatal("ожидалась секция dns")
	}
}

func TestKillSwitchRejectFinalRule(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = true
	p.Split = SplitTunnel{}
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	if !strings.Contains(string(out), "MATCH,REJECT") {
		t.Fatalf("ожидалось финальное правило MATCH,REJECT:\n%s", out)
	}
}

func TestNoKillSwitchUsesSelector(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = false
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	s := string(out)
	if !strings.Contains(s, "MATCH,"+CarambaSelector) {
		t.Fatalf("ожидалось MATCH,CARAMBA:\n%s", s)
	}
}

func TestSplitTunnelBypass(t *testing.T) {
	p := DefaultPolicy()
	p.Split.BypassDomains = []string{"example.com"}
	p.Split.BypassProcesses = []string{"git"}
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	s := string(out)
	if !strings.Contains(s, "DOMAIN-SUFFIX,example.com,DIRECT") {
		t.Fatalf("ожидался байпас домена:\n%s", s)
	}
	if !strings.Contains(s, "PROCESS-NAME,git,DIRECT") {
		t.Fatalf("ожидался байпас процесса:\n%s", s)
	}
}

func TestSplitTunnelAllowList(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = false
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
		t.Fatalf("ожидался MATCH,DIRECT для не-allow трафика:\n%s", s)
	}
}
