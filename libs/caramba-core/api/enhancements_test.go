//go:build !mihomo

package api

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
)

// enhFixture — импортированная подписка: raw-путь не ходит в панель и не
// требует аутентификации, поэтому Up проверяется без сети.
const enhFixture = `
proxies:
  - name: "DE-1"
    type: probe-fixture
    server: 127.0.0.1
    port: 1080
proxy-groups:
  - name: CARAMBA
    type: select
    proxies: ["DE-1", "DIRECT"]
`

func enhCore(t *testing.T, policyJSON string) *Core {
	t.Helper()
	c, err := NewCore(Config{WorkDir: t.TempDir()})
	if err != nil {
		t.Fatalf("NewCore: %v", err)
	}
	if err := c.SetImportedConfig([]byte(enhFixture)); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}
	if err := c.SetPolicyJSON(policyJSON); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	return c
}

func enhReport(t *testing.T, c *Core) RouteReport {
	t.Helper()
	if _, err := c.Up(context.Background(), ""); err != nil {
		t.Fatalf("Up: %v", err)
	}
	raw, err := c.RouteReportJSON()
	if err != nil {
		t.Fatalf("RouteReportJSON: %v", err)
	}
	var rep RouteReport
	if err := json.Unmarshal([]byte(raw), &rep); err != nil {
		t.Fatalf("разбор отчёта: %v\n%s", err, raw)
	}
	return rep
}

// Единственный канал приложения — setPolicy. Пока он не запоминал
// идентификатор пресета, отчёт называл источником «правила оператора», и
// подтвердить блок рекламы было нечем: именно это «непонятно, работает или
// нет» и чинится.
func TestPolicyJSONRemembersPresetForTheReport(t *testing.T) {
	c := enhCore(t, `{"preset":"ru-smart"}`)
	rep := enhReport(t, c)

	if rep.Source != RouteSourcePreset {
		t.Fatalf("source = %q, ожидался %q", rep.Source, RouteSourcePreset)
	}
	if rep.Preset == nil || rep.Preset.PresetID != "ru-smart" {
		t.Fatalf("отчёт не назвал применённый пресет: %+v", rep.Preset)
	}
}

// Пустой пресет обязан снимать запомненный: иначе выключение режима страны
// оставляло бы отчёт рассказывать про прошлый.
func TestPolicyJSONEmptyPresetClearsIt(t *testing.T) {
	c := enhCore(t, `{"preset":"ru-smart"}`)
	if err := c.SetPolicyJSON(`{"preset":""}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	rep := enhReport(t, c)
	if rep.Source == RouteSourcePreset {
		t.Fatalf("отчёт всё ещё считает пресет применённым: %+v", rep.Preset)
	}
}

// Переключатель блока рекламы обязан приводить к НАЗВАННОМУ источнику: без
// строки sources[ads] экран может показать только галочку «мы попросили».
func TestBlockAdsAppearsAsNamedRuleSource(t *testing.T) {
	c := enhCore(t, `{"preset":"ru-smart","adblock":true}`)
	rep := enhReport(t, c)

	if rep.Preset == nil {
		t.Fatal("отчёт без пресета: подтвердить блок рекламы нечем")
	}
	var found bool
	for _, s := range rep.Preset.Sources {
		if s.Name == "ads" {
			found = true
			if s.State == "" {
				t.Error("источник ads без состояния: экран не сможет сказать, работает он или нет")
			}
		}
	}
	if !found {
		t.Fatalf("списка ads нет среди источников: %+v", rep.Preset.Sources)
	}
}

// Выключенный переключатель не приводит источник рекламы ниоткуда.
func TestBlockAdsOffLeavesNoAdsSource(t *testing.T) {
	c := enhCore(t, `{"preset":"ru-smart","adblock":false}`)
	rep := enhReport(t, c)
	if rep.Preset == nil {
		t.Fatal("ожидался отчёт о пресете")
	}
	for _, s := range rep.Preset.Sources {
		if s.Name == "ads" {
			t.Fatalf("источник ads появился при выключенном переключателе: %+v", s)
		}
	}
}

// Сайтовый allow-список отменяет режим страны, и отчёт обязан это признать:
// назвать применённым пресет, чьи правила выброшены, было бы той же ложью,
// против которой отчёт написан.
func TestAllowSitesRemoveCountryPresetFromReport(t *testing.T) {
	c := enhCore(t, `{"preset":"ru-smart","split":{"mode":"allow","allowSites":["telegram"]}}`)
	rep := enhReport(t, c)

	if rep.Source == RouteSourcePreset {
		t.Fatalf("отчёт назвал применённым отменённый пресет: %+v", rep.Preset)
	}
}

// Блок рекламы переживает отмену режима страны: он про то, ЧТО резать, а не
// про то, что проксировать.
func TestAllowSitesKeepAdBlockConfirmable(t *testing.T) {
	c := enhCore(t, `{"preset":"ru-smart","adblock":true,"split":{"mode":"allow","allowSites":["telegram"]}}`)
	rep := enhReport(t, c)

	if rep.Preset == nil || rep.Preset.PresetID != "adblock" {
		t.Fatalf("остаток маршрутизации не опознан как блок рекламы: %+v", rep.Preset)
	}
}

// Незнакомый тег сайта отвергается ЦЕЛИКОМ: молча пропущенный тег дал бы
// строку, выглядящую включённой, за которой нет ни одного правила.
func TestUnknownSiteTagRejectedAndPolicyUntouched(t *testing.T) {
	c := enhCore(t, `{"preset":"ru-smart","split":{"mode":"allow","allowSites":["telegram"]}}`)
	err := c.SetPolicyJSON(`{"split":{"mode":"allow","allowSites":["vkontakte"]}}`)
	if err == nil {
		t.Fatal("незнакомый тег принят")
	}
	if !strings.Contains(err.Error(), "allowSites") {
		t.Errorf("ошибка не называет поле: %v", err)
	}
	c.mu.Lock()
	got := append([]string(nil), c.policy.Split.AllowSites...)
	c.mu.Unlock()
	if len(got) != 1 || got[0] != "telegram" {
		t.Fatalf("политика изменена отвергнутым патчем: %v", got)
	}
}

// Выключение раздельного туннелирования обязано снимать и сайтовый список:
// иначе «выключено» продолжало бы уводить весь остальной трафик мимо туннеля.
func TestSplitOffClearsAllowSites(t *testing.T) {
	c := enhCore(t, `{"split":{"mode":"allow","allowSites":["telegram"],"allowDomains":["a.example"]}}`)
	if err := c.SetPolicyJSON(`{"split":{"mode":"off"}}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	c.mu.Lock()
	active := c.policy.Split.AllowSitesActive()
	c.mu.Unlock()
	if active {
		t.Fatal("сайтовый список пережил выключение режима")
	}
}

// Список рекламы обязан быть заказан каталогу вместе со списками пресета,
// иначе проверенный файл до сборки не доедет.
func TestRuleSetNamesIncludeAdsOnlyWhenAsked(t *testing.T) {
	with := ruleSetNames("ru-smart", true)
	if !contains(with, "ads") {
		t.Errorf("список ads не заказан при включённом блоке рекламы: %v", with)
	}
	without := ruleSetNames("ru-smart", false)
	if contains(without, "ads") {
		t.Errorf("список ads заказан без просьбы: %v", without)
	}
	// Пресет со своим блоком рекламы не должен получить имя дважды.
	cn := ruleSetNames("cn-smart", true)
	n := 0
	for _, v := range cn {
		if v == "ads" {
			n++
		}
	}
	if n != 1 {
		t.Errorf("имя ads встречается %d раз: %v", n, cn)
	}
}

func contains(list []string, want string) bool {
	for _, v := range list {
		if v == want {
			return true
		}
	}
	return false
}
