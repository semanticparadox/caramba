package routing

import "testing"

// sourceByName находит запись отчёта по имени rule-set'а.
func sourceByName(t *testing.T, rep BuildReport, name string) RuleSourceReport {
	t.Helper()
	for _, s := range rep.Sources {
		if s.Name == name {
			return s
		}
	}
	t.Fatalf("источник %q не назван в отчёте: %+v", name, rep.Sources)
	return RuleSourceReport{}
}

// Пресет, чей внешний список недоступен, ОБЯЗАН быть отчитан как отказавший, а
// не молча лишиться правил.
//
// Это и была дыра: BuildWith выбрасывает провайдер вместе со ссылающимися на
// него правилами, и «Россия (умный)» без ru-blocked снаружи выглядит ровно так
// же, как «Россия (умный)» с ним. Пользователь видит включённый пресет и не
// видит, что половина его смысла не доехала.
func TestBuildReportNamesFailedRuleSourceInsteadOfDroppingItSilently(t *testing.T) {
	p, ok := PresetByID("ru-smart")
	if !ok {
		t.Fatal("нет пресета ru-smart")
	}

	cases := []struct {
		name       string
		opt        PoolOptions
		wantReason string
	}{
		{
			// Ни зеркала, ни проверенного файла: качать неоткуда.
			name:       "без зеркала",
			opt:        PoolOptions{},
			wantReason: RuleSourceReasonNoMirror,
		},
		{
			// Доверенный каталог есть, но этот список он не подписывает.
			// Инвариант 12 запрещает подставить неподписанную копию.
			name:       "каталог не подписал список",
			opt:        PoolOptions{Bases: []string{"https://mirror.example"}, Verified: true},
			wantReason: RuleSourceReasonNotInCatalog,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			cfg, rep := p.BuildWithReport(tc.opt, "CARAMBA")

			// Правила и провайдер действительно выпали — иначе проверять нечего.
			if len(cfg.Providers) != 0 {
				t.Fatalf("провайдеры не выброшены: %+v", cfg.Providers)
			}
			for _, r := range cfg.Rules {
				if r.Type == MatchRuleSet {
					t.Fatalf("ruleset-правило осталось в конфиге: %+v", r)
				}
			}

			// И ровно это названо в отчёте, а не пропущено.
			if len(rep.Sources) != len(p.Providers) {
				t.Fatalf("отчёт назвал %d источников, объявлено %d", len(rep.Sources), len(p.Providers))
			}
			src := sourceByName(t, rep, "ru-blocked")
			if src.State != RuleSourceDropped {
				t.Errorf("ru-blocked: state = %q, ожидалось %q", src.State, RuleSourceDropped)
			}
			if src.Reason != tc.wantReason {
				t.Errorf("ru-blocked: reason = %q, ожидалось %q", src.Reason, tc.wantReason)
			}
			if src.Detail == "" {
				t.Error("ru-blocked: отказ без пояснения")
			}
			if src.Rules == 0 {
				t.Error("ru-blocked: отчёт не сосчитал правила, которые на него ссылались")
			}
			if src.KeptRules != 0 {
				t.Errorf("ru-blocked: kept_rules = %d у выброшенного источника", src.KeptRules)
			}
			if rep.DroppedRules == 0 {
				t.Error("отчёт не сосчитал ни одного выброшенного правила")
			}
			if rep.Rules != len(cfg.Rules) {
				t.Errorf("отчёт насчитал %d правил, в конфиге %d", rep.Rules, len(cfg.Rules))
			}
		})
	}
}

// Доступный список отчитывается как доехавший, с адресом или путём, — иначе
// «отказало» и «сработало» неразличимы в другую сторону.
func TestBuildReportNamesResolvedRuleSources(t *testing.T) {
	p, ok := PresetByID("ru-smart")
	if !ok {
		t.Fatal("нет пресета ru-smart")
	}

	t.Run("зеркало", func(t *testing.T) {
		_, rep := p.BuildWithReport(PoolOptions{Bases: []string{"https://mirror.example"}}, "CARAMBA")
		src := sourceByName(t, rep, "ru-blocked")
		if src.State != RuleSourceMirror {
			t.Fatalf("state = %q, ожидалось %q", src.State, RuleSourceMirror)
		}
		if src.URL != "https://mirror.example/rulesets/ru-blocked" {
			t.Errorf("URL = %q: плейсхолдер {BASE} не подставлен в отчёт", src.URL)
		}
		if src.KeptRules == 0 {
			t.Error("kept_rules = 0 у доехавшего источника")
		}
	})

	t.Run("проверенный файл", func(t *testing.T) {
		_, rep := p.BuildWithReport(PoolOptions{
			Verified: true,
			Files:    map[string]string{"ru-blocked": "rulesets/ru-blocked"},
		}, "CARAMBA")
		src := sourceByName(t, rep, "ru-blocked")
		if src.State != RuleSourceFile {
			t.Fatalf("state = %q, ожидалось %q", src.State, RuleSourceFile)
		}
		if src.Path != "rulesets/ru-blocked" {
			t.Errorf("Path = %q", src.Path)
		}
		if src.URL != "" {
			t.Errorf("у файлового провайдера остался URL %q", src.URL)
		}
		// Второй список каталог не подписал — он обязан быть отчитан отказом.
		ip := sourceByName(t, rep, "ru-blocked-ip")
		if ip.State != RuleSourceDropped || ip.Reason != RuleSourceReasonNotInCatalog {
			t.Errorf("ru-blocked-ip: %+v, ожидался отказ not_in_catalog", ip)
		}
	})
}

// Пресеты adblock и streaming это ЧИСТЫЕ теги GEOSITE: без базы GeoSite.dat они
// не значат ничего. Отчёт обязан их называть, иначе слою выше нечем объяснить
// пользователю, почему «блок рекламы» включён и не работает.
func TestBuildReportNamesGeositeTagsPresetsDependOn(t *testing.T) {
	cases := map[string][]string{
		"adblock":   {"category-ads-all"},
		"streaming": {"netflix", "youtube", "spotify", "disney", "openai"},
	}
	for id, want := range cases {
		p, ok := PresetByID(id)
		if !ok {
			t.Fatalf("нет пресета %q", id)
		}
		_, rep := p.BuildWithReport(PoolOptions{}, "CARAMBA")
		if len(rep.GeositeTags) != len(want) {
			t.Fatalf("%s: теги = %v, ожидались %v", id, rep.GeositeTags, want)
		}
		for i := range want {
			if rep.GeositeTags[i] != want[i] {
				t.Errorf("%s: тег #%d = %q, ожидался %q", id, i, rep.GeositeTags[i], want[i])
			}
		}
		if rep.RulesByType[string(MatchGeosite)] != len(want) {
			t.Errorf("%s: правил GEOSITE = %d, тегов %d", id, rep.RulesByType[string(MatchGeosite)], len(want))
		}
	}
}

// BuildWith обязан остаться байт-в-байт тем же, что BuildWithReport: отчёт это
// наблюдение над сборкой, а не вторая сборка.
func TestBuildWithMatchesBuildWithReport(t *testing.T) {
	opt := PoolOptions{Bases: []string{"https://mirror.example"}, Proxy: "CARAMBA"}
	for _, p := range Presets() {
		a := p.BuildWith(opt, "CARAMBA")
		b, _ := p.BuildWithReport(opt, "CARAMBA")
		if len(a.Rules) != len(b.Rules) || len(a.Providers) != len(b.Providers) {
			t.Errorf("%s: BuildWith и BuildWithReport разошлись", p.ID)
		}
	}
}
