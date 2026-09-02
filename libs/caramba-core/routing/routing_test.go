package routing

import (
	"strings"
	"testing"
)

func TestCompiledRulesResolvesProxyAndFinal(t *testing.T) {
	cfg := Config{
		FinalAction: ActionDirect,
		Rules: []Rule{
			{Type: MatchGeosite, Value: "telegram", Action: ActionProxy},
			{Type: MatchGeoIP, Value: "RU", Action: ActionDirect, NoResolve: true},
			{Type: MatchRuleSet, Value: "ru-blocked", Action: ActionProxy},
		},
		Providers: []RuleProvider{{Name: "ru-blocked", URL: "https://x/y.mrs"}},
	}
	rules := cfg.CompiledRules("CARAMBA")

	want := []string{
		"GEOSITE,telegram,CARAMBA",
		"GEOIP,RU,DIRECT,no-resolve",
		"RULE-SET,ru-blocked,CARAMBA",
		"MATCH,DIRECT",
	}
	if len(rules) != len(want) {
		t.Fatalf("ожидалось %d правил, получено %d: %v", len(want), len(rules), rules)
	}
	for i := range want {
		if rules[i] != want[i] {
			t.Errorf("правило #%d = %q, ожидалось %q", i, rules[i], want[i])
		}
	}
}

func TestCompiledRulesDefaultFinalIsDirect(t *testing.T) {
	cfg := Config{Rules: []Rule{{Type: MatchDomain, Value: "a.com", Action: ActionReject}}}
	rules := cfg.CompiledRules("G")
	if rules[len(rules)-1] != "MATCH,DIRECT" {
		t.Errorf("финал по умолчанию = %q, ожидался MATCH,DIRECT", rules[len(rules)-1])
	}
}

func TestCompiledRulesSkipsEmpty(t *testing.T) {
	cfg := Config{Rules: []Rule{{Type: MatchDomain, Value: "", Action: ActionProxy}, {Type: "", Value: "x", Action: ActionProxy}}}
	rules := cfg.CompiledRules("G")
	if len(rules) != 1 { // только MATCH
		t.Errorf("пустые правила должны отбрасываться, получено: %v", rules)
	}
}

func TestCompiledProviders(t *testing.T) {
	cfg := Config{Providers: []RuleProvider{
		{Name: "ru-blocked", Behavior: BehaviorDomain, Format: FormatMrs, URL: "https://x/y.mrs"},
		{Name: "empty", URL: ""}, // пропускается
	}}
	got := cfg.CompiledProviders()
	if got == nil {
		t.Fatal("ожидалась непустая секция провайдеров")
	}
	if _, ok := got["empty"]; ok {
		t.Error("провайдер с пустым URL не должен попадать в вывод")
	}
	p, ok := got["ru-blocked"].(map[string]any)
	if !ok {
		t.Fatalf("ru-blocked отсутствует или неверного типа: %T", got["ru-blocked"])
	}
	if p["behavior"] != "domain" || p["format"] != "mrs" || p["type"] != "http" {
		t.Errorf("неверные поля провайдера: %+v", p)
	}
	if p["interval"].(int) != 86400 {
		t.Errorf("интервал по умолчанию должен быть 86400, получено %v", p["interval"])
	}
}

func TestValidateRejectsUnknownRuleSet(t *testing.T) {
	cfg := Config{Rules: []Rule{{Type: MatchRuleSet, Value: "missing", Action: ActionProxy}}}
	if err := cfg.Validate(); err == nil {
		t.Error("ожидалась ошибка для RULE-SET без провайдера")
	}
}

func TestMergePrependsHigherPriority(t *testing.T) {
	base := Config{
		FinalAction: ActionDirect,
		Rules:       []Rule{{Type: MatchGeosite, Value: "telegram", Action: ActionProxy}},
		Providers:   []RuleProvider{{Name: "a", URL: "u"}},
	}
	hp := Config{
		Rules:     []Rule{{Type: MatchProcessName, Value: "Telegram.exe", Action: ActionDirect}},
		Providers: []RuleProvider{{Name: "a", URL: "dup"}, {Name: "b", URL: "u2"}},
	}
	m := base.Merge(hp)
	if m.Rules[0].Value != "Telegram.exe" {
		t.Errorf("правила высокого приоритета должны идти первыми, получено: %v", m.Rules[0])
	}
	if len(m.Providers) != 2 {
		t.Errorf("дубли провайдеров по имени должны схлопываться, получено %d", len(m.Providers))
	}
}

func TestPresetBuildSubstitutesBase(t *testing.T) {
	p, ok := PresetByID("ru-smart")
	if !ok {
		t.Fatal("пресет ru-smart должен существовать")
	}
	cfg := p.Build("https://panel.example.com/", "CARAMBA")
	if err := cfg.Validate(); err != nil {
		t.Fatalf("ru-smart не проходит валидацию: %v", err)
	}
	for _, prov := range cfg.Providers {
		if strings.Contains(prov.URL, "{BASE}") {
			t.Errorf("плейсхолдер {BASE} не подставлен: %s", prov.URL)
		}
		if !strings.HasPrefix(prov.URL, "https://panel.example.com/rulesets/") {
			t.Errorf("URL провайдера не относительно базы: %s", prov.URL)
		}
		// Зеркало панели отдаёт текстовые списки без .mrs-тулинга.
		if strings.HasSuffix(prov.URL, ".mrs") {
			t.Errorf("URL провайдера не должен ссылаться на .mrs: %s", prov.URL)
		}
		if prov.Format != FormatText {
			t.Errorf("провайдер %s должен быть format=text, получено %q", prov.Name, prov.Format)
		}
		if prov.Behavior != BehaviorDomain && prov.Behavior != BehaviorIPCIDR {
			t.Errorf("провайдер %s: behavior должен быть domain или ipcidr, получено %q", prov.Name, prov.Behavior)
		}
	}
}

// TestAllPresetProvidersAreText гарантирует, что ни один встроенный пресет не
// эмитит .mrs / format=mrs провайдеров — все резолвятся против текстового
// зеркала панели /rulesets/NAME.
func TestAllPresetProvidersAreText(t *testing.T) {
	for _, p := range Presets() {
		cfg := p.Build("https://b", "CARAMBA")
		if err := cfg.Validate(); err != nil {
			t.Errorf("пресет %s не проходит валидацию: %v", p.ID, err)
		}
		for _, prov := range cfg.Providers {
			if prov.Format == FormatMrs || strings.HasSuffix(prov.URL, ".mrs") {
				t.Errorf("пресет %s: провайдер %s всё ещё .mrs (%s, %q)", p.ID, prov.Name, prov.URL, prov.Format)
			}
			if !strings.HasPrefix(prov.URL, "https://b/rulesets/") {
				t.Errorf("пресет %s: провайдер %s не указывает на зеркало панели: %s", p.ID, prov.Name, prov.URL)
			}
		}
	}
}

// TestCompiledProvidersTextFormat проверяет, что text-формат и behavior
// корректно прокидываются в секцию rule-providers mihomo.
func TestCompiledProvidersTextFormat(t *testing.T) {
	cfg := Config{Providers: []RuleProvider{
		{Name: "ru-blocked", Behavior: BehaviorDomain, Format: FormatText, URL: "https://b/rulesets/ru-blocked", Interval: 43200},
	}}
	got := cfg.CompiledProviders()
	p, ok := got["ru-blocked"].(map[string]any)
	if !ok {
		t.Fatalf("ru-blocked отсутствует: %T", got["ru-blocked"])
	}
	if p["format"] != "text" || p["behavior"] != "domain" {
		t.Errorf("неверные поля text-провайдера: %+v", p)
	}
	if p["url"] != "https://b/rulesets/ru-blocked" {
		t.Errorf("url искажён: %v", p["url"])
	}
	if p["interval"].(int) != 43200 {
		t.Errorf("интервал должен прокидываться как есть, получено %v", p["interval"])
	}
}

func TestPresetsForCountryPutsCountryFirst(t *testing.T) {
	presets := PresetsForCountry("RU")
	if len(presets) == 0 {
		t.Fatal("ожидались пресеты")
	}
	if presets[0].Country != "RU" {
		t.Errorf("страновые пресеты должны идти первыми, первый = %q", presets[0].Country)
	}
}

func TestTelegramOnlyPresetHasPerAppRules(t *testing.T) {
	p, _ := PresetByID("telegram-only")
	cfg := p.Build("https://b", "CARAMBA")
	rules := cfg.CompiledRules("CARAMBA")
	var hasProcess, hasFinalDirect bool
	for _, r := range rules {
		if strings.HasPrefix(r, "PROCESS-NAME,org.telegram.messenger,CARAMBA") {
			hasProcess = true
		}
		if r == "MATCH,DIRECT" {
			hasFinalDirect = true
		}
	}
	if !hasProcess {
		t.Error("telegram-only должен содержать per-app правило для Android-пакета Telegram")
	}
	if !hasFinalDirect {
		t.Error("telegram-only должен заканчиваться MATCH,DIRECT (остальное напрямую)")
	}
}
