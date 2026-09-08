package routing

import "testing"

// Владелец сказал: «блок рекламы или стриминг непонятно работают или нет».
//
// Для блока рекламы это было буквально так: пресет состоял из одного тега
// GEOSITE, база под тегом качается по geox-url, geox-url пишется только под
// доверенным каталогом, а без него ядро идёт на зашитое умолчание
// meta-rules-dat — недоступное ровно там, где клиент и нужен. Ни один слой не
// мог сказать про это ничего, кроме «неизвестно».
//
// Тесты ниже фиксируют ровно две вещи: с зеркалом оператора блок рекламы
// становится НАБЛЮДАЕМЫМ источником, а без зеркала он не пропадает, а честно
// откатывается на встроенную базу — и отчёт эти два случая различает.

// countRules считает правила конфига по типу и значению.
func countRules(cfg Config, typ MatchType, value string) int {
	n := 0
	for _, r := range cfg.Rules {
		if r.Type == typ && r.Value == value {
			n++
		}
	}
	return n
}

// С зеркалом реклама режется списком оператора: правило RULE-SET в конфиге,
// провайдер с подставленным адресом, источник назван в отчёте. Дублирующего
// тега GEOSITE при этом НЕТ — иначе отчёт сообщал бы про зависимость от базы,
// которой у этой сборки уже нет.
func TestAdblockResolvesThroughOperatorMirrorWhenOneIsConfigured(t *testing.T) {
	p, ok := PresetByID("adblock")
	if !ok {
		t.Fatal("нет пресета adblock")
	}

	cfg, rep := p.BuildWithReport(PoolOptions{
		Bases: []string{"https://panel.example"},
		Proxy: "CARAMBA",
	}, "CARAMBA")

	if n := countRules(cfg, MatchRuleSet, adsRuleSet); n != 1 {
		t.Fatalf("правил RULE-SET,%s в конфиге %d, ожидалось 1: %+v", adsRuleSet, n, cfg.Rules)
	}
	if n := countRules(cfg, MatchGeosite, adsGeositeTag); n != 0 {
		t.Errorf("тег GEOSITE %s остался при живом зеркале (%d правил): замена не должна дублировать источник", adsGeositeTag, n)
	}
	for _, tag := range rep.GeositeTags {
		if tag == adsGeositeTag {
			t.Errorf("отчёт объявил зависимость от базы GEOSITE (%v), хотя реклама режется списком оператора", rep.GeositeTags)
		}
	}

	if len(cfg.Providers) != 1 {
		t.Fatalf("провайдеров %d, ожидался один (%s): %+v", len(cfg.Providers), adsRuleSet, cfg.Providers)
	}
	if got := cfg.Providers[0].URL; got != "https://panel.example/rulesets/ads" {
		t.Errorf("URL провайдера = %q: плейсхолдер {BASE} не подставлен", got)
	}
	if cfg.Providers[0].Proxy != "CARAMBA" {
		t.Errorf("provider.proxy = %q: список обязан ехать по туннелю, а не в открытый интернет", cfg.Providers[0].Proxy)
	}

	// И то же самое в отчёте — именно его читает слой выше.
	src := sourceByName(t, rep, adsRuleSet)
	if src.State != RuleSourceMirror {
		t.Fatalf("state = %q, ожидалось %q", src.State, RuleSourceMirror)
	}
	if src.URL != "https://panel.example/rulesets/ads" {
		t.Errorf("отчёт назвал адрес %q", src.URL)
	}
	if src.Rules != 1 || src.KeptRules != 1 {
		t.Errorf("rules = %d, kept_rules = %d, ожидалось 1/1", src.Rules, src.KeptRules)
	}
	if len(src.Fallback) != 0 {
		t.Errorf("fallback = %v у доехавшего источника: замена в конфиг не шла и объявлять её нечестно", src.Fallback)
	}
	if rep.DroppedRules != 0 {
		t.Errorf("dropped_rules = %d: подавленная замена это не потеря правила", rep.DroppedRules)
	}
}

// Проверенный файл каталога — тот же наблюдаемый исход, только без сети.
func TestAdblockResolvesThroughVerifiedFile(t *testing.T) {
	p, ok := PresetByID("adblock")
	if !ok {
		t.Fatal("нет пресета adblock")
	}
	cfg, rep := p.BuildWithReport(PoolOptions{
		Verified: true,
		Files:    map[string]string{adsRuleSet: "rulesets/ads"},
	}, "CARAMBA")

	src := sourceByName(t, rep, adsRuleSet)
	if src.State != RuleSourceFile {
		t.Fatalf("state = %q, ожидалось %q", src.State, RuleSourceFile)
	}
	if src.Path != "rulesets/ads" {
		t.Errorf("path = %q", src.Path)
	}
	if len(src.Fallback) != 0 {
		t.Errorf("fallback = %v у доехавшего источника", src.Fallback)
	}
	if n := countRules(cfg, MatchGeosite, adsGeositeTag); n != 0 {
		t.Errorf("тег GEOSITE %s остался при проверенном файле", adsGeositeTag)
	}
	if cfg.Providers[0].URL != "" || cfg.Providers[0].Path != "rulesets/ads" {
		t.Errorf("провайдер не переведён в файловую форму: %+v", cfg.Providers[0])
	}
}

// Без зеркала блок рекламы НЕ исчезает: тег GEOSITE возвращается на своё место,
// и отчёт называет его именно заменой отказавшего списка, а не самостоятельным
// правилом пресета.
func TestAdblockFallsBackToGeositeWhenNoMirrorIsAvailable(t *testing.T) {
	p, ok := PresetByID("adblock")
	if !ok {
		t.Fatal("нет пресета adblock")
	}

	cases := []struct {
		name       string
		opt        PoolOptions
		wantReason string
	}{
		{
			// Импортированная подписка: панели нет, качать список неоткуда.
			name:       "без зеркала",
			opt:        PoolOptions{},
			wantReason: RuleSourceReasonNoMirror,
		},
		{
			// Доверенный каталог есть, но `ads` он не подписывает: инвариант 12
			// запрещает докачивать неподписанную копию.
			name:       "каталог не подписал список",
			opt:        PoolOptions{Bases: []string{"https://panel.example"}, Verified: true},
			wantReason: RuleSourceReasonNotInCatalog,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			cfg, rep := p.BuildWithReport(tc.opt, "CARAMBA")

			if n := countRules(cfg, MatchGeosite, adsGeositeTag); n != 1 {
				t.Fatalf("правил GEOSITE,%s = %d, ожидалось 1: блокировка потеряна целиком", adsGeositeTag, n)
			}
			if n := countRules(cfg, MatchRuleSet, adsRuleSet); n != 0 {
				t.Errorf("правило RULE-SET,%s осталось без провайдера (%d)", adsRuleSet, n)
			}
			if len(cfg.Providers) != 0 {
				t.Errorf("провайдер не выброшен: %+v", cfg.Providers)
			}
			if err := cfg.Validate(); err != nil {
				t.Errorf("конфиг несогласован: %v", err)
			}

			src := sourceByName(t, rep, adsRuleSet)
			if src.State != RuleSourceDropped || src.Reason != tc.wantReason {
				t.Fatalf("источник %+v, ожидался отказ %q", src, tc.wantReason)
			}
			if len(src.Fallback) != 1 || src.Fallback[0] != adsGeositeTag {
				t.Errorf("fallback = %v, ожидалось [%s]: без этого поля тег неотличим от штатного правила пресета", src.Fallback, adsGeositeTag)
			}
			if src.KeptRules != 0 {
				t.Errorf("kept_rules = %d у выброшенного источника", src.KeptRules)
			}
			// Тег вернулся в правила — значит база GeoSite.dat снова нужна, и
			// отчёт обязан это объявить.
			found := false
			for _, tag := range rep.GeositeTags {
				if tag == adsGeositeTag {
					found = true
				}
			}
			if !found {
				t.Errorf("geosite_tags = %v: замена включилась, а зависимость от базы не объявлена", rep.GeositeTags)
			}
		})
	}
}

// Отчёт обязан РАЗЛИЧАТЬ два исхода одного и того же пресета. Это и есть то,
// чего не хватало: снаружи «режу списком оператора» и «режу встроенной базой,
// про которую ничего не известно» выглядели одинаково.
func TestAdblockReportDistinguishesMirrorFromFallback(t *testing.T) {
	p, ok := PresetByID("adblock")
	if !ok {
		t.Fatal("нет пресета adblock")
	}

	_, withMirror := p.BuildWithReport(PoolOptions{Bases: []string{"https://panel.example"}}, "CARAMBA")
	_, without := p.BuildWithReport(PoolOptions{}, "CARAMBA")

	mirrored := sourceByName(t, withMirror, adsRuleSet)
	fallen := sourceByName(t, without, adsRuleSet)

	if mirrored.State == fallen.State {
		t.Fatalf("оба исхода отчитаны состоянием %q — различить их нечем", mirrored.State)
	}
	if len(withMirror.GeositeTags) == len(without.GeositeTags) {
		t.Errorf("зависимость от базы GEOSITE одинакова в обоих исходах: %v vs %v",
			withMirror.GeositeTags, without.GeositeTags)
	}
	if mirrored.Detail == "" || fallen.Detail == "" {
		t.Error("исход без пояснения для журнала")
	}
}

// Пресет, режущий рекламу, обязан объявить провайдера `ads`. Забытое объявление
// не ломает сборку заметно: замена перестаёт подавляться, зеркало не
// подключается, и пресет молча остаётся на встроенной базе — ровно то
// состояние, из которого мы уходим.
func TestAdsProviderDeclaredByEveryAdBlockingPreset(t *testing.T) {
	for _, p := range Presets() {
		blocks := false
		for _, r := range p.Rules {
			if r.Type == MatchRuleSet && r.Value == adsRuleSet {
				blocks = true
			}
		}
		if !blocks {
			continue
		}
		declared := false
		for _, rp := range p.Providers {
			if rp.Name == adsRuleSet {
				declared = true
				if rp.Behavior != BehaviorDomain || rp.Format != FormatText {
					t.Errorf("%s: провайдер ads = %s/%s, панель отдаёт domain/text", p.ID, rp.Behavior, rp.Format)
				}
				if rp.URL != "{BASE}/rulesets/"+adsRuleSet {
					t.Errorf("%s: URL провайдера ads = %q", p.ID, rp.URL)
				}
			}
		}
		if !declared {
			t.Errorf("%s: правило RULE-SET,%s есть, провайдера нет", p.ID, adsRuleSet)
		}
	}
}

// Стриминг эквивалента на зеркале НЕ имеет, и заимствовать успех блока рекламы
// не может. Отчёт обязан оставить его тем, чем он есть: пятью тегами GEOSITE
// без единого внешнего источника.
func TestStreamingStaysGeositeOnlyAndSaysSo(t *testing.T) {
	p, ok := PresetByID("streaming")
	if !ok {
		t.Fatal("нет пресета streaming")
	}
	// Даже при живом зеркале — подменять нечем.
	cfg, rep := p.BuildWithReport(PoolOptions{Bases: []string{"https://panel.example"}}, "CARAMBA")

	if len(rep.Sources) != 0 {
		t.Errorf("streaming объявил внешние источники: %+v", rep.Sources)
	}
	if len(cfg.Providers) != 0 {
		t.Errorf("streaming получил провайдеров: %+v", cfg.Providers)
	}
	want := []string{"netflix", "youtube", "spotify", "disney", "openai"}
	if len(rep.GeositeTags) != len(want) {
		t.Fatalf("теги = %v, ожидались %v", rep.GeositeTags, want)
	}
	for i := range want {
		if rep.GeositeTags[i] != want[i] {
			t.Errorf("тег #%d = %q, ожидался %q", i, rep.GeositeTags[i], want[i])
		}
	}
}

// Российские списки уже реальны и уже едут через зеркало: у них тот же
// механизм, и отдельная замена им не нужна — эквивалента «всё заблокированное в
// РФ» во встроенной базе mihomo просто нет. Тест фиксирует, что механизм для
// них РАБОТАЕТ, а не что его забыли применить.
func TestRussianListsResolveThroughTheSameMirrorMechanism(t *testing.T) {
	p, ok := PresetByID("ru-smart")
	if !ok {
		t.Fatal("нет пресета ru-smart")
	}
	_, rep := p.BuildWithReport(PoolOptions{Bases: []string{"https://panel.example"}, Proxy: "CARAMBA"}, "CARAMBA")

	for _, name := range []string{"ru-blocked", "ru-blocked-ip"} {
		src := sourceByName(t, rep, name)
		if src.State != RuleSourceMirror {
			t.Errorf("%s: state = %q, ожидалось %q", name, src.State, RuleSourceMirror)
		}
		if src.URL != "https://panel.example/rulesets/"+name {
			t.Errorf("%s: URL = %q", name, src.URL)
		}
		if src.KeptRules != 1 {
			t.Errorf("%s: kept_rules = %d", name, src.KeptRules)
		}
		if len(src.Fallback) != 0 {
			t.Errorf("%s: объявлена замена %v, которой во встроенной базе не существует", name, src.Fallback)
		}
	}
	// А теги GEOSITE ru-smart это отдельные сервисы, не подмена спискам: они
	// стоят на месте в обоих исходах.
	if len(rep.GeositeTags) != 8 {
		t.Errorf("тегов GEOSITE = %d, ожидалось 8: %v", len(rep.GeositeTags), rep.GeositeTags)
	}
}

// Замена привязана к списку, объявленному тем же пресетом. Ссылка на чужое имя
// не должна ничего подавлять: иначе пресет, забывший объявить провайдера,
// терял бы и зеркало, и подстраховку разом.
func TestFallbackForUndeclaredRuleSetIsNeverSuppressed(t *testing.T) {
	p := Preset{
		ID: "orphan", FinalAction: ActionDirect,
		Rules: []Rule{geositeFallback(adsGeositeTag, "never-declared", ActionReject)},
	}
	cfg, rep := p.BuildWithReport(PoolOptions{Bases: []string{"https://panel.example"}}, "CARAMBA")
	if n := countRules(cfg, MatchGeosite, adsGeositeTag); n != 1 {
		t.Fatalf("правило-замена выброшено (%d), хотя подавлять его нечем", n)
	}
	if len(rep.Sources) != 0 {
		t.Errorf("отчёт выдумал источник: %+v", rep.Sources)
	}
}

// Скомпилированный вид обеих форм. Это то, что реально уезжает в ядро, и
// проверять стоит именно его: отчёт описывает конфиг, а не наоборот.
func TestAdblockCompilesToMihomoRulesInBothForms(t *testing.T) {
	p, ok := PresetByID("adblock")
	if !ok {
		t.Fatal("нет пресета adblock")
	}

	withMirror := p.BuildWith(PoolOptions{Bases: []string{"https://panel.example"}, Proxy: "CARAMBA"}, "CARAMBA")
	wantMirror := []string{
		"GEOIP,private,DIRECT,no-resolve",
		"RULE-SET,ads,REJECT",
		"MATCH,DIRECT",
	}
	got := withMirror.CompiledRules("CARAMBA")
	if len(got) != len(wantMirror) {
		t.Fatalf("с зеркалом правила = %v, ожидались %v", got, wantMirror)
	}
	for i := range wantMirror {
		if got[i] != wantMirror[i] {
			t.Errorf("правило #%d = %q, ожидалось %q", i, got[i], wantMirror[i])
		}
	}
	prov, _ := withMirror.CompiledProviders()["ads"].(map[string]any)
	if prov == nil {
		t.Fatal("секция rule-providers не содержит ads")
	}
	if prov["type"] != "http" || prov["url"] != "https://panel.example/rulesets/ads" ||
		prov["behavior"] != "domain" || prov["format"] != "text" || prov["proxy"] != "CARAMBA" {
		t.Errorf("провайдер ads собран неверно: %+v", prov)
	}

	without := p.BuildWith(PoolOptions{}, "CARAMBA")
	wantFallback := []string{
		"GEOIP,private,DIRECT,no-resolve",
		"GEOSITE,category-ads-all,REJECT",
		"MATCH,DIRECT",
	}
	got = without.CompiledRules("CARAMBA")
	if len(got) != len(wantFallback) {
		t.Fatalf("без зеркала правила = %v, ожидались %v", got, wantFallback)
	}
	for i := range wantFallback {
		if got[i] != wantFallback[i] {
			t.Errorf("правило #%d = %q, ожидалось %q", i, got[i], wantFallback[i])
		}
	}
	if without.CompiledProviders() != nil {
		t.Errorf("без зеркала секция rule-providers не пуста: %+v", without.CompiledProviders())
	}
}
