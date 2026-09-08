package routing

import (
	"strings"
	"testing"
)

// poolPreset это пресет с одним внешним списком и правилом на него.
func poolPreset() Preset {
	return Preset{
		ID: "t", Name: "test", FinalAction: ActionDirect,
		Rules: []Rule{
			geoip("private", ActionDirect),
			ruleset("ru-blocked", ActionProxy),
		},
		Providers: []RuleProvider{
			{Name: "ru-blocked", Behavior: BehaviorDomain, Format: FormatText, URL: "{BASE}/rulesets/ru-blocked", Interval: 43200},
		},
	}
}

// TestPoolSubstitutionUsesFirstMirror: плейсхолдер {BASE} обобщён с одного
// адреса панели до упорядоченного пула зеркал (02-SPEC.md 8.10). У http-vehicle
// mihomo один адрес, поэтому подставляется первое зеркало, а панель остаётся
// последним запасным вариантом.
func TestPoolSubstitutionUsesFirstMirror(t *testing.T) {
	cfg := poolPreset().BuildWith(PoolOptions{
		Bases: []string{"https://m1.example.net/", "https://m2.example.net", "https://panel.example.com"},
		Proxy: "CARAMBA",
	}, "CARAMBA")
	pr := cfg.CompiledProviders()
	entry, ok := pr["ru-blocked"].(map[string]any)
	if !ok {
		t.Fatalf("провайдер не собран: %#v", pr)
	}
	if got, _ := entry["url"].(string); got != "https://m1.example.net/rulesets/ru-blocked" {
		t.Fatalf("url=%q, ожидалось первое зеркало без хвостового слэша", got)
	}
	if got, _ := entry["type"].(string); got != "http" {
		t.Fatalf("type=%q", got)
	}
	if got, _ := entry["proxy"].(string); got != "CARAMBA" {
		t.Fatalf("proxy=%q: без него ядро тянет список собственным диалером мимо туннеля", got)
	}
}

// TestPoolSubstitutionSkipsEmptyBases: пустые записи пула не должны давать
// URL без схемы, на котором ядро падает с Get "/rulesets/...".
func TestPoolSubstitutionSkipsEmptyBases(t *testing.T) {
	cfg := poolPreset().BuildWith(PoolOptions{Bases: []string{"", "  ", "https://m2.example.net"}}, "CARAMBA")
	entry, ok := cfg.CompiledProviders()["ru-blocked"].(map[string]any)
	if !ok {
		t.Fatalf("провайдер не собран")
	}
	if got, _ := entry["url"].(string); !strings.HasPrefix(got, "https://m2.example.net/") {
		t.Fatalf("url=%q", got)
	}
}

// TestVerifiedFileProvider: подтверждённый файл переводит провайдера в
// vehicle file. Это единственная форма, совместимая с инвариантом 12:
// подписанный sha256 фиксирует ровно одно содержимое, поэтому ни url, ни
// interval, ни proxy у такого провайдера смысла не имеют.
func TestVerifiedFileProvider(t *testing.T) {
	cfg := poolPreset().BuildWith(PoolOptions{
		Bases:    []string{"https://m1.example.net"},
		Proxy:    "CARAMBA",
		Verified: true,
		Files:    map[string]string{"ru-blocked": "rulesets/ru-blocked"},
	}, "CARAMBA")
	entry, ok := cfg.CompiledProviders()["ru-blocked"].(map[string]any)
	if !ok {
		t.Fatalf("провайдер не собран")
	}
	if got, _ := entry["type"].(string); got != "file" {
		t.Fatalf("type=%q, ожидался file", got)
	}
	if got, _ := entry["path"].(string); got != "rulesets/ru-blocked" {
		t.Fatalf("path=%q", got)
	}
	for _, key := range []string{"url", "interval", "proxy"} {
		if _, ok := entry[key]; ok {
			t.Fatalf("у файлового провайдера остался ключ %s: %#v", key, entry)
		}
	}
	// Правило на этот список обязано остаться.
	rules := cfg.CompiledRules("CARAMBA")
	if !containsRule(rules, "RULE-SET,ru-blocked,CARAMBA") {
		t.Fatalf("правило на подтверждённый список пропало: %v", rules)
	}
}

// TestVerifiedRefusesUnsignedRuleSet проверяет отказ вместо отката. Каталог не назвал
// список, значит подписанного хеша нет, и докачивать его по http запрещено:
// провайдер и правила на него выпадают целиком.
func TestVerifiedRefusesUnsignedRuleSet(t *testing.T) {
	cfg := poolPreset().BuildWith(PoolOptions{
		Bases:    []string{"https://m1.example.net"},
		Verified: true,
	}, "CARAMBA")
	if pr := cfg.CompiledProviders(); pr != nil {
		t.Fatalf("неподписанный провайдер собран: %#v", pr)
	}
	rules := cfg.CompiledRules("CARAMBA")
	for _, r := range rules {
		if strings.HasPrefix(r, "RULE-SET,") {
			t.Fatalf("правило на неподписанный список осталось: %v", rules)
		}
	}
	// Остальные правила пресета при этом сохраняются.
	if !containsRule(rules, "GEOIP,private,DIRECT,no-resolve") {
		t.Fatalf("вместе с ruleset выпали и обычные правила: %v", rules)
	}
	if err := cfg.Validate(); err != nil {
		t.Fatalf("конфиг несогласован после отказа: %v", err)
	}
}

// TestBuildStillWorks: старая двухаргументная форма остаётся рабочей обёрткой.
func TestBuildStillWorks(t *testing.T) {
	cfg := poolPreset().Build("https://panel.example.com/", "CARAMBA")
	entry, ok := cfg.CompiledProviders()["ru-blocked"].(map[string]any)
	if !ok {
		t.Fatalf("провайдер не собран")
	}
	if got, _ := entry["url"].(string); got != "https://panel.example.com/rulesets/ru-blocked" {
		t.Fatalf("url=%q", got)
	}
	if _, ok := entry["proxy"]; ok {
		t.Fatalf("обёртка без пула не должна проставлять proxy: %#v", entry)
	}
}

func containsRule(rules []string, want string) bool {
	for _, r := range rules {
		if r == want {
			return true
		}
	}
	return false
}
