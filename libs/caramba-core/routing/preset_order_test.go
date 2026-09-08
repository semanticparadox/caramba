package routing

import (
	"strings"
	"testing"
)

// Порядок правил — это и есть маршрутизация. mihomo берёт ПЕРВОЕ совпавшее
// правило, поэтому «список стоит выше» и «список отменяет всё, что ниже» —
// одно и то же утверждение. Ровно так и сломался Иран: RULE-SET с действием
// PROXY стоял выше GEOIP,IR,DIRECT, и правило, существующее ради иранской
// домашней инфраструктуры, не срабатывало ни разу.
//
// Отсюда золотой снимок: он фиксирует и порядок, и действие каждого правила
// каждого пресета. Перестановка двух строк местами или смена DIRECT на PROXY
// ломает сборку — а не тихо уводит чужие банки в другую страну.
//
// Правки золотого снимка допустимы, но ТОЛЬКО осознанные: если меняется
// действие внешнего списка, вместе с ним обязана меняться карта
// RecommendedUpstreams, а её значение — сверяться с README апстрима.
var goldenCompiledRules = map[string][]string{
	"ru-smart": {
		"GEOIP,private,DIRECT,no-resolve",
		"GEOSITE,telegram,CARAMBA",
		"GEOSITE,instagram,CARAMBA",
		"GEOSITE,facebook,CARAMBA",
		"GEOSITE,twitter,CARAMBA",
		"GEOSITE,youtube,CARAMBA",
		"GEOSITE,discord,CARAMBA",
		"GEOSITE,openai,CARAMBA",
		"RULE-SET,ru-blocked,CARAMBA",
		"RULE-SET,ru-blocked-ip,CARAMBA",
		// Российское — ПОСЛЕ проксируемого и ДО финального MATCH.
		"GEOSITE,category-ru,DIRECT",
		"GEOIP,RU,DIRECT,no-resolve",
		"MATCH,DIRECT",
	},
	"ru-full": {
		"GEOIP,private,DIRECT,no-resolve",
		"GEOSITE,category-ru,DIRECT",
		"GEOIP,RU,DIRECT,no-resolve",
		"MATCH,CARAMBA",
	},
	"telegram-only": {
		"GEOIP,private,DIRECT,no-resolve",
		"PROCESS-NAME,org.telegram.messenger,CARAMBA",
		"PROCESS-NAME,org.telegram.messenger.web,CARAMBA",
		"PROCESS-NAME,Telegram.exe,CARAMBA",
		"PROCESS-NAME,Telegram,CARAMBA",
		"PROCESS-NAME,telegram-desktop,CARAMBA",
		"GEOSITE,telegram,CARAMBA",
		"GEOIP,telegram,CARAMBA,no-resolve",
		"MATCH,DIRECT",
	},
	"ir-smart": {
		"GEOIP,private,DIRECT,no-resolve",
		"GEOSITE,telegram,CARAMBA",
		"GEOSITE,youtube,CARAMBA",
		"GEOSITE,twitter,CARAMBA",
		"GEOSITE,facebook,CARAMBA",
		"GEOSITE,openai,CARAMBA",
		// Иранское — напрямую. Список доменов ОБЯЗАН стоять перед GEOIP,IR:
		// у GEOIP выставлен no-resolve, и на доменном соединении оно не
		// срабатывает. См. irDirectRuleSet.
		"RULE-SET,ir-direct,DIRECT",
		"GEOIP,IR,DIRECT,no-resolve",
		"MATCH,DIRECT",
	},
	"by-smart": {
		"GEOIP,private,DIRECT,no-resolve",
		"GEOSITE,telegram,CARAMBA",
		"GEOSITE,instagram,CARAMBA",
		"GEOSITE,twitter,CARAMBA",
		"GEOSITE,youtube,CARAMBA",
		"RULE-SET,by-blocked,CARAMBA",
		"GEOIP,BY,DIRECT,no-resolve",
		"MATCH,DIRECT",
	},
	"cn-smart": {
		"GEOIP,private,DIRECT,no-resolve",
		// Список рекламы доехал, поэтому замена GEOSITE category-ads-all в
		// правила не идёт (Rule.FallbackFor).
		"RULE-SET,ads,REJECT",
		"GEOSITE,cn,DIRECT",
		"GEOIP,CN,DIRECT,no-resolve",
		"MATCH,CARAMBA",
	},
	"streaming": {
		"GEOIP,private,DIRECT,no-resolve",
		"GEOSITE,netflix,CARAMBA",
		"GEOSITE,youtube,CARAMBA",
		"GEOSITE,spotify,CARAMBA",
		"GEOSITE,disney,CARAMBA",
		"GEOSITE,openai,CARAMBA",
		"MATCH,DIRECT",
	},
	"adblock": {
		"GEOIP,private,DIRECT,no-resolve",
		"RULE-SET,ads,REJECT",
		"MATCH,DIRECT",
	},
	"global": {
		"GEOIP,private,DIRECT,no-resolve",
		"MATCH,CARAMBA",
	},
}

// TestCompiledRuleOrderIsPinned сверяет каждый встроенный пресет с золотым
// снимком — построчно и по порядку.
func TestCompiledRuleOrderIsPinned(t *testing.T) {
	presets := Presets()
	if len(presets) != len(goldenCompiledRules) {
		t.Fatalf("пресетов %d, а в золотом снимке %d: новый пресет обязан приехать со своим порядком правил",
			len(presets), len(goldenCompiledRules))
	}
	for _, p := range presets {
		want, ok := goldenCompiledRules[p.ID]
		if !ok {
			t.Errorf("пресет %q отсутствует в золотом снимке", p.ID)
			continue
		}
		got := p.Build("https://panel.example", "CARAMBA").CompiledRules("CARAMBA")
		if len(got) != len(want) {
			t.Errorf("%s: правил %d, ожидалось %d\nполучено:\n  %s\nожидалось:\n  %s",
				p.ID, len(got), len(want),
				strings.Join(got, "\n  "), strings.Join(want, "\n  "))
			continue
		}
		for i := range want {
			if got[i] != want[i] {
				t.Errorf("%s: правило #%d = %q, ожидалось %q", p.ID, i, got[i], want[i])
			}
		}
	}
}

// TestRuleSetActionsMatchTheirDeclaredIntent — главный замок против повторения
// иранской ошибки.
//
// Смысл внешнего списка задаёт апстрим, а не пресет: список «иранские домены»
// осмыслен только с DIRECT, список «заблокировано в РФ» — только с PROXY,
// список рекламы — только с REJECT. Пресет, поставивший чужому списку не то
// действие, ломает пользователя молча. Здесь каждое RULE-SET-правило каждого
// пресета сверяется с задекларированным намерением.
func TestRuleSetActionsMatchTheirDeclaredIntent(t *testing.T) {
	for _, p := range Presets() {
		for i, r := range p.Rules {
			if r.Type != MatchRuleSet {
				continue
			}
			intent, known := RecommendedUpstreams[r.Value]
			if !known {
				t.Errorf("%s: правило #%d ссылается на список %q, у которого нет записи в RecommendedUpstreams — назначение списка нигде не зафиксировано",
					p.ID, i, r.Value)
				continue
			}
			if r.Action != intent.Action {
				t.Errorf("%s: правило #%d на список %q имеет действие %s, а список объявлен как %s (%s)",
					p.ID, i, r.Value, r.Action, intent.Action, intent.Upstream)
			}
		}
		// Объявленный провайдер без записи о назначении — это список, про
		// который панель не знает, зачем его зеркалить.
		for _, prov := range p.Providers {
			if _, known := RecommendedUpstreams[prov.Name]; !known {
				t.Errorf("%s: провайдер %q не описан в RecommendedUpstreams", p.ID, prov.Name)
			}
		}
	}
}

// TestEveryDeclaredIntentHasAnUpstreamAndAction ловит полупустую запись:
// список без апстрима панель зеркалить не сможет, список без действия
// не защищён предыдущим тестом (пустое действие сравнялось бы только с пустым).
func TestEveryDeclaredIntentHasAnUpstreamAndAction(t *testing.T) {
	for name, intent := range RecommendedUpstreams {
		if strings.TrimSpace(intent.Upstream) == "" {
			t.Errorf("список %q объявлен без апстрима", name)
		}
		switch intent.Action {
		case ActionProxy, ActionDirect, ActionReject:
		default:
			t.Errorf("список %q объявлен с недопустимым действием %q", name, intent.Action)
		}
	}
}

// TestIranDomesticTrafficStaysInIran — регрессия ровно на найденный баг.
//
// Пресет ir-smart обязан держать иранскую домашнюю инфраструктуру дома:
// ни один внешний список в нём не может уезжать в туннель, а список иранских
// доменов обязан стоять ВЫШЕ правила GEOIP,IR — иначе он не защищает ничего.
func TestIranDomesticTrafficStaysInIran(t *testing.T) {
	p, ok := PresetByID("ir-smart")
	if !ok {
		t.Fatal("нет пресета ir-smart")
	}
	rules := p.Build("https://panel.example", "CARAMBA").CompiledRules("CARAMBA")

	idxDirectList, idxGeoIP := -1, -1
	for i, line := range rules {
		if strings.HasPrefix(line, "RULE-SET,") {
			if !strings.HasSuffix(line, ",DIRECT") {
				t.Errorf("ir-smart: внешний список уезжает не напрямую: %q", line)
			}
		}
		if line == "RULE-SET,"+irDirectRuleSet+",DIRECT" {
			idxDirectList = i
		}
		if strings.HasPrefix(line, "GEOIP,IR,") {
			idxGeoIP = i
		}
	}
	if idxDirectList < 0 {
		t.Fatalf("ir-smart: нет правила на список %q\n%s", irDirectRuleSet, strings.Join(rules, "\n"))
	}
	if idxGeoIP < 0 {
		t.Fatalf("ir-smart: пропало правило GEOIP,IR\n%s", strings.Join(rules, "\n"))
	}
	if idxDirectList > idxGeoIP {
		t.Errorf("ir-smart: список иранских доменов (#%d) стоит ниже GEOIP,IR (#%d); у GEOIP выставлен no-resolve, и на доменном соединении он не сработает",
			idxDirectList, idxGeoIP)
	}
}

// TestIranPresetWithoutMirrorKeepsTheDirectAnchor: без зеркала список иранских
// доменов выбрасывается целиком (инвариант сборки), но правило GEOIP,IR и
// финальный DIRECT обязаны остаться — иначе отсутствие зеркала само по себе
// увело бы иранский трафик наружу.
func TestIranPresetWithoutMirrorKeepsTheDirectAnchor(t *testing.T) {
	p, ok := PresetByID("ir-smart")
	if !ok {
		t.Fatal("нет пресета ir-smart")
	}
	rules := p.Build("", "CARAMBA").CompiledRules("CARAMBA")
	for _, line := range rules {
		if strings.HasPrefix(line, "RULE-SET,") {
			t.Errorf("без зеркала ruleset-правило осталось: %q", line)
		}
	}
	var hasGeoIP bool
	for _, line := range rules {
		if strings.HasPrefix(line, "GEOIP,IR,DIRECT") {
			hasGeoIP = true
		}
	}
	if !hasGeoIP {
		t.Errorf("без зеркала пропал якорь GEOIP,IR,DIRECT:\n%s", strings.Join(rules, "\n"))
	}
	if last := rules[len(rules)-1]; last != "MATCH,DIRECT" {
		t.Errorf("финальное правило ir-smart = %q, ожидалось MATCH,DIRECT", last)
	}
}
