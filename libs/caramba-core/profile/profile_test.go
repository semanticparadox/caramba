package profile

import (
	"fmt"
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

// sampleWithDirectFallback повторяет форму конфига подписки/импорта, где в
// селекторе рядом с узлами есть DIRECT (безопасный фолбэк). Именно его убирает
// kill-switch.
const sampleWithDirectFallback = `
proxies:
  - name: "DE Stealth"
    type: vless
    server: 1.2.3.4
    port: 443
proxy-groups:
  - name: CARAMBA
    type: select
    proxies: ["DE Stealth", "DIRECT"]
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

// Kill-switch НЕ подменяет финальное правило: MATCH всегда ведёт в селектор
// CARAMBA, иначе включённый kill-switch отбрасывал бы вообще весь трафик,
// включая тот, что прокси прекрасно вывозит.
func TestKillSwitchKeepsSelectorFinalRule(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = true
	p.Split = SplitTunnel{}
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	s := string(out)
	if strings.Contains(s, "MATCH,REJECT") {
		t.Fatalf("финальный REJECT недопустим при kill-switch без allow-list:\n%s", s)
	}
	if !strings.Contains(s, "MATCH,"+CarambaSelector) {
		t.Fatalf("ожидалось MATCH,CARAMBA:\n%s", s)
	}
}

// Отказ «в закрытую» обеспечивается на уровне селектора: под kill-switch из
// группы CARAMBA убирается DIRECT, поэтому при недоступном прокси трафику
// некуда утечь.
func TestKillSwitchRemovesDirectFromSelector(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = true
	out, err := AssembleMihomoConfig([]byte(sampleWithDirectFallback), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	for _, name := range selectorMembers(t, out) {
		if strings.EqualFold(name, "DIRECT") {
			t.Fatalf("DIRECT остался в фолбэке селектора при kill-switch:\n%s", out)
		}
	}
}

// Без kill-switch DIRECT остаётся: пользователь сознательно разрешил утечку в
// обмен на работающий интернет при упавшем прокси.
func TestNoKillSwitchKeepsDirectInSelector(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = false
	out, err := AssembleMihomoConfig([]byte(sampleWithDirectFallback), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	found := false
	for _, name := range selectorMembers(t, out) {
		if strings.EqualFold(name, "DIRECT") {
			found = true
		}
	}
	if !found {
		t.Fatalf("без kill-switch DIRECT должен остаться в селекторе:\n%s", out)
	}
}

// Вырожденный случай: если кроме DIRECT в группе никого нет, удалять его
// нельзя — mihomo отвергает пустую select-группу и туннель не поднимется.
func TestKillSwitchKeepsDirectWhenSelectorWouldBeEmpty(t *testing.T) {
	const onlyDirect = `
proxies: []
proxy-groups:
  - name: CARAMBA
    type: select
    proxies: ["DIRECT"]
rules:
  - MATCH,CARAMBA
`
	p := DefaultPolicy()
	p.KillSwitch = true
	out, err := AssembleMihomoConfig([]byte(onlyDirect), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	if got := selectorMembers(t, out); len(got) != 1 || got[0] != "DIRECT" {
		t.Fatalf("ожидался единственный участник DIRECT, получено %v", got)
	}
}

// Allow-list split — единственное место, где REJECT уместен: трафик ВНЕ списка
// при kill-switch отбрасывается.
func TestAllowListWithKillSwitchRejectsRest(t *testing.T) {
	p := DefaultPolicy()
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
	if !strings.Contains(s, "MATCH,REJECT") {
		t.Fatalf("ожидался MATCH,REJECT для трафика вне allow-list:\n%s", s)
	}
}

// Политика по умолчанию не тянет за собой страновые geo-правила: зашитый Китай
// ломал маршрутизацию пользователям РФ/Ирана/Беларуси.
func TestDefaultRulesHaveNoCountryGeoRules(t *testing.T) {
	out, err := AssembleMihomoConfig([]byte(sampleConfig), DefaultPolicy())
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := unmarshal(t, out)
	rules, ok := doc["rules"].([]any)
	if !ok {
		t.Fatalf("секция rules отсутствует: %v", doc["rules"])
	}
	var got []string
	for _, r := range rules {
		got = append(got, fmt.Sprint(r))
	}
	want := []string{"GEOIP,private,DIRECT,no-resolve", "MATCH," + CarambaSelector}
	if len(got) != len(want) {
		t.Fatalf("правила по умолчанию %v, ожидались %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("правила по умолчанию %v, ожидались %v", got, want)
		}
	}
}

// selectorMembers возвращает список участников группы CARAMBA из собранного
// конфига.
func selectorMembers(t *testing.T, out []byte) []string {
	t.Helper()
	doc := unmarshal(t, out)
	groups, ok := doc["proxy-groups"].([]any)
	if !ok {
		t.Fatalf("секция proxy-groups отсутствует: %v", doc["proxy-groups"])
	}
	for _, g := range groups {
		gm, ok := g.(map[string]any)
		if !ok {
			continue
		}
		if name, _ := gm["name"].(string); name != CarambaSelector {
			continue
		}
		list, _ := gm["proxies"].([]any)
		out := make([]string, 0, len(list))
		for _, item := range list {
			out = append(out, fmt.Sprint(item))
		}
		return out
	}
	t.Fatalf("группа %s не найдена", CarambaSelector)
	return nil
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
