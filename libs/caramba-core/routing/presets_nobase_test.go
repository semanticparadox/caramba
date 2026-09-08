package routing

import "testing"

// TestBuildWithoutBaseURLDropsRuleSets: без базового URL пресет не должен
// ссылаться на удалённые rule-provider'ы.
func TestBuildWithoutBaseURLDropsRuleSets(t *testing.T) {
	p, ok := PresetByID("ru-smart")
	if !ok {
		t.Fatal("нет пресета ru-smart")
	}
	cfg := p.Build("", "CARAMBA")
	if len(cfg.Providers) != 0 {
		t.Errorf("ожидалось 0 провайдеров, получено %d", len(cfg.Providers))
	}
	for _, r := range cfg.Rules {
		if r.Type == MatchRuleSet {
			t.Errorf("ruleset-правило осталось: %+v", r)
		}
	}
	if len(cfg.Rules) == 0 {
		t.Error("правила пресета пропали целиком")
	}
}
