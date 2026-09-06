package profile

import (
	"strings"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/routing"
)

// rulesOfPolicy собирает конфиг и отдаёт секцию rules строками.
func rulesOfPolicy(t *testing.T, p Policy) []string {
	t.Helper()
	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	var got []string
	for _, line := range strings.Split(string(out), "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "- ") {
			got = append(got, strings.TrimPrefix(line, "- "))
		}
	}
	return got
}

func hasLine(lines []string, want string) bool {
	for _, l := range lines {
		if l == want || strings.Trim(l, `"'`) == want {
			return true
		}
	}
	return false
}

// Сайтовый allow-список: перечисленное идёт В туннель, остальное мимо.
// Без kill-switch «мимо» это DIRECT.
func TestAllowSitesRouteOnlyListedThroughTunnel(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = false
	p.Split.AllowDomains = []string{"youtube.com"}
	p.Split.AllowSites = []string{"telegram"}

	rules := rulesOfPolicy(t, p)
	for _, want := range []string{
		"DOMAIN-SUFFIX,youtube.com," + CarambaSelector,
		"GEOSITE,telegram," + CarambaSelector,
		"MATCH,DIRECT",
	} {
		if !hasLine(rules, want) {
			t.Errorf("нет правила %q\n%v", want, rules)
		}
	}
}

// С kill-switch «всё остальное» не утекает мимо туннеля, а отбрасывается.
func TestAllowSitesRejectRestUnderKillSwitch(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = true
	p.Split.AllowSites = []string{"telegram"}

	rules := rulesOfPolicy(t, p)
	if !hasLine(rules, "MATCH,REJECT") {
		t.Fatalf("ожидался MATCH,REJECT при kill-switch\n%v", rules)
	}
}

// ГЛАВНОЕ свойство: сайтовый список ОТМЕНЯЕТ режим страны. Человек, назвавший
// один сайт, не должен получить ещё и весь список заблокированного в РФ.
func TestAllowSitesSuppressCountryPreset(t *testing.T) {
	ru, ok := routing.PresetByID("ru-smart")
	if !ok {
		t.Fatal("в реестре нет ru-smart")
	}
	cfg, _ := ru.BuildWithReport(routing.PoolOptions{Bases: []string{"https://panel.example"}}, CarambaSelector)

	p := DefaultPolicy()
	p.KillSwitch = false
	p.Routing = &cfg
	p.Split.AllowDomains = []string{"youtube.com"}

	rules := rulesOfPolicy(t, p)
	for _, line := range rules {
		if strings.HasPrefix(line, "RULE-SET,ru-blocked") || strings.HasPrefix(line, "GEOSITE,category-ru") {
			t.Fatalf("правило режима страны уцелело при сайтовом списке: %q\n%v", line, rules)
		}
	}
	if !hasLine(rules, "DOMAIN-SUFFIX,youtube.com,"+CarambaSelector) {
		t.Fatalf("сайт из списка не попал в туннель\n%v", rules)
	}
	if !hasLine(rules, "MATCH,DIRECT") {
		t.Fatalf("финал остался от пресета, а не от списка\n%v", rules)
	}
}

// Секции rules и rule-providers обязаны строиться из одного набора: провайдер
// без единого правила заставляет ядро качать файл впустую, а отчёт — называть
// источник, которого в сборке нет.
func TestAllowSitesDropUnusedRuleProviders(t *testing.T) {
	ru, _ := routing.PresetByID("ru-smart")
	cfg, _ := ru.BuildWithReport(routing.PoolOptions{Bases: []string{"https://panel.example"}}, CarambaSelector)

	p := DefaultPolicy()
	p.Routing = &cfg
	p.Split.AllowSites = []string{"telegram"}

	out, err := AssembleMihomoConfig([]byte(sampleConfig), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	if strings.Contains(string(out), "ru-blocked") {
		t.Fatalf("провайдер отменённого пресета остался в конфиге:\n%s", out)
	}
}

// Блок рекламы поверх пресета: правило есть, а страновые правила остаются.
func TestBlockAdsOverlaysPresetWithoutReplacingIt(t *testing.T) {
	ru, _ := routing.PresetByID("ru-smart")
	withAds := routing.WithAdBlock(ru)
	cfg, _ := withAds.BuildWithReport(routing.PoolOptions{Bases: []string{"https://panel.example"}}, CarambaSelector)

	p := DefaultPolicy()
	p.BlockAds = true
	p.Routing = &cfg

	rules := rulesOfPolicy(t, p)
	if !hasLine(rules, "RULE-SET,"+routing.AdBlockRuleSet+",REJECT") {
		t.Errorf("нет правила блока рекламы\n%v", rules)
	}
	if !hasLine(rules, "RULE-SET,ru-blocked,"+CarambaSelector) {
		t.Errorf("страновые правила потеряны вместе с включением блока рекламы\n%v", rules)
	}
}

// Прямой вызов сборки без Routing (CLI, тесты) не должен молча терять флаг:
// провайдера здесь взять неоткуда, но запасное правило по встроенной базе есть.
func TestBlockAdsFallsBackToBuiltInGeositeWithoutRouting(t *testing.T) {
	p := DefaultPolicy()
	p.BlockAds = true
	p.Routing = nil

	rules := rulesOfPolicy(t, p)
	if !hasLine(rules, "GEOSITE,"+routing.AdBlockGeositeTag+",REJECT") {
		t.Fatalf("флаг блока рекламы не дал ни одного правила\n%v", rules)
	}
}

// Выключенный флаг не добавляет ничего: правило без просьбы это тот же обман,
// что и просьба без правила.
func TestBlockAdsOffAddsNothing(t *testing.T) {
	p := DefaultPolicy()
	p.Routing = nil
	for _, line := range rulesOfPolicy(t, p) {
		if strings.Contains(line, routing.AdBlockGeositeTag) {
			t.Fatalf("правило блока рекламы появилось при выключенном флаге: %q", line)
		}
	}
}

// Kill-switch не должен резать трафик, который человек сам отправил напрямую.
//
// Проверено на устройстве и было сломано: при включённой галочке «через VPN
// только выбранные сайты» и kill-switch по умолчанию финальным правилом
// оказывалось MATCH,REJECT, и весь интернет вне списка переставал работать —
// притом что экран обещал «всё остальное напрямую».
func TestAllowListKeepsTheRestDirectEvenWithKillSwitch(t *testing.T) {
	for _, killSwitch := range []bool{false, true} {
		p := Policy{KillSwitch: killSwitch, Routing: &routing.Config{}}
		p.Split.AllowDomains = []string{"example.com"}

		rules := compileSmartRules(p)
		if len(rules) == 0 {
			t.Fatalf("killSwitch=%v: правил нет вовсе", killSwitch)
		}
		final := rules[len(rules)-1]

		if strings.Contains(final, "REJECT") {
			t.Fatalf("killSwitch=%v: финальное правило %q режет весь трафик вне "+
				"списка, а он идёт напрямую по выбору человека, а не из-за поломки",
				killSwitch, final)
		}
		if !strings.Contains(final, "DIRECT") {
			t.Fatalf("killSwitch=%v: финальное правило %q, ожидался DIRECT",
				killSwitch, final)
		}
	}
}
