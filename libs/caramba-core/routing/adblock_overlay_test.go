package routing

import (
	"reflect"
	"testing"
)

// Наложение блока рекламы обязано давать РОВНО ту же форму, что и пресет,
// объявивший блок сам. Иначе «включить рекламу переключателем» и «выбрать
// пресет с рекламой» дают два разных конфига, и один из них никем не проверен.
func TestWithAdBlockMatchesBuiltInAdBlockingPreset(t *testing.T) {
	global, ok := PresetByID("global")
	if !ok {
		t.Fatal("в реестре нет пресета global")
	}
	got := WithAdBlock(global)

	want := append(leadIn(true), global.Rules[1:]...)
	if !reflect.DeepEqual(got.Rules, want) {
		t.Fatalf("правила наложения разошлись с leadIn(true)\n got: %+v\nwant: %+v", got.Rules, want)
	}

	declared := false
	for _, p := range got.Providers {
		if p.Name == AdBlockRuleSet {
			declared = true
		}
	}
	if !declared {
		t.Fatal("наложение не объявило провайдер списка ads: зеркало не подключится, а отчёт не назовёт источник")
	}
}

// Пресету со своим блоком рекламы наложение не добавляет ничего: дубль правила
// и второй провайдер с тем же именем — это уже расхождение с самим собой.
func TestWithAdBlockIsIdempotent(t *testing.T) {
	for _, id := range []string{"cn-smart", "adblock"} {
		p, ok := PresetByID(id)
		if !ok {
			t.Fatalf("в реестре нет пресета %s", id)
		}
		got := WithAdBlock(p)
		if !reflect.DeepEqual(got.Rules, p.Rules) {
			t.Errorf("%s: наложение изменило правила пресета, который уже режет рекламу", id)
		}
		if len(got.Providers) != len(p.Providers) {
			t.Errorf("%s: наложение добавило лишний провайдер (%d вместо %d)", id, len(got.Providers), len(p.Providers))
		}
	}
}

// Наложение не трогает финал: «резать рекламу» не значит «поменять, что идёт
// через VPN».
func TestWithAdBlockKeepsFinalAction(t *testing.T) {
	ru, _ := PresetByID("ru-smart")
	if got := WithAdBlock(ru).FinalAction; got != ru.FinalAction {
		t.Fatalf("финал изменился: %q вместо %q", got, ru.FinalAction)
	}
}

// AdBlockOnly оставляет ровно две вещи: локальную сеть напрямую и блок рекламы.
// Всё страновое обязано исчезнуть вместе со своими списками — иначе человек,
// попросивший «через VPN только эти сайты», получит ещё и весь список
// заблокированного в РФ.
func TestAdBlockOnlyDropsCountryRulesAndProviders(t *testing.T) {
	ru, _ := PresetByID("ru-smart")
	cfg, _ := WithAdBlock(ru).BuildWithReport(PoolOptions{Bases: []string{"https://panel.example"}}, "CARAMBA")

	only := AdBlockOnly(cfg)
	for _, r := range only.Rules {
		if isPrivateGeoIP(r) || isAdBlockRule(r) {
			continue
		}
		t.Errorf("в остатке уцелело чужое правило: %+v", r)
	}
	if !hasAds(only.Rules) {
		t.Error("блок рекламы потерян: остаток бессмыслен")
	}
	for _, p := range only.Providers {
		if p.Name != AdBlockRuleSet {
			t.Errorf("уцелел чужой провайдер %q", p.Name)
		}
	}
	if only.FinalAction != "" {
		t.Errorf("финал унаследован (%q), хотя его назначает allow-список", only.FinalAction)
	}
}

// Пресет из одного блока рекламы должен опознаваться приложением: оно сверяет
// применённый пресет со своим зеркалом реестра по ID.
func TestAdBlockOnlyPresetKeepsRegistryIdentity(t *testing.T) {
	p := AdBlockOnlyPreset(ActionProxy)
	if p.ID != "adblock" {
		t.Fatalf("id = %q, ожидался adblock: чужой id означает «пресет ядру неизвестен»", p.ID)
	}
	if p.FinalAction != ActionProxy {
		t.Fatalf("финал = %q, ожидался PROXY: «пресета нет» означает весь трафик в туннель", p.FinalAction)
	}
	if !hasAds(p.Rules) {
		t.Fatal("в пресете нет правил блока рекламы")
	}
}

func hasAds(rules []Rule) bool {
	for _, r := range rules {
		if isAdBlockRule(r) {
			return true
		}
	}
	return false
}
