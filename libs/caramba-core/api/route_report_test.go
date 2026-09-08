//go:build !mihomo

package api

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/routing"
)

// routeFixture — импортированная подписка: raw-путь не ходит в панель и не
// требует аутентификации, поэтому Up на нём проверяем без сети.
const routeFixture = `
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

func decodeRouteReport(t *testing.T, raw string) RouteReport {
	t.Helper()
	var rep RouteReport
	if err := json.Unmarshal([]byte(raw), &rep); err != nil {
		t.Fatalf("разбор отчёта: %v\n%s", err, raw)
	}
	return rep
}

// raisedCore поднимает туннель на импортированной подписке с пресетом preset и
// отдаёт разобранный отчёт.
func raisedCore(t *testing.T, preset, relay string) (*Core, RouteReport) {
	t.Helper()
	c, err := NewCore(Config{WorkDir: t.TempDir()})
	if err != nil {
		t.Fatalf("NewCore: %v", err)
	}
	if err := c.SetImportedConfig([]byte(routeFixture)); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}
	if preset != "" {
		if err := c.ApplyPreset(preset); err != nil {
			t.Fatalf("ApplyPreset(%q): %v", preset, err)
		}
	}
	if relay != "" {
		c.SetRelay(relay)
	}
	if _, err := c.Up(context.Background(), ""); err != nil {
		t.Fatalf("Up: %v", err)
	}
	raw, err := c.RouteReportJSON()
	if err != nil {
		t.Fatalf("RouteReportJSON: %v", err)
	}
	return c, decodeRouteReport(t, raw)
}

// До подъёма отчёта нет, и это сказано словом, а не пустым здоровым ответом.
func TestRouteReportUnknownBeforeAnyRaise(t *testing.T) {
	c, err := NewCore(Config{WorkDir: t.TempDir()})
	if err != nil {
		t.Fatalf("NewCore: %v", err)
	}
	raw, err := c.RouteReportJSON()
	if err != nil {
		t.Fatalf("RouteReportJSON: %v", err)
	}
	rep := decodeRouteReport(t, raw)
	if rep.Known {
		t.Fatal("отчёт объявил себя известным до первого подъёма")
	}
	if rep.Reason != RouteUnknownNotRaised {
		t.Errorf("reason = %q, ожидалось %q", rep.Reason, RouteUnknownNotRaised)
	}
	if rep.Geosite.State != GeositeUnknown {
		t.Errorf("geosite.state = %q, ожидалось %q", rep.Geosite.State, GeositeUnknown)
	}
	if rep.Rules != nil {
		t.Errorf("rules = %d, ожидался null: число правил до подъёма неизвестно", *rep.Rules)
	}
}

// ГЛАВНОЕ свойство отчёта: пресет, чей внешний список не доехал, отчитывается
// отказом с причиной, а не выглядит целым.
//
// «Россия (умный)» без ru-blocked теряет весь список заблокированного в РФ.
// До этого отчёта такой подъём был снаружи неотличим от полноценного.
func TestRouteReportNamesFailedRuleSourceAfterRaise(t *testing.T) {
	_, rep := raisedCore(t, "ru-smart", "")

	if !rep.Known || rep.Source != RouteSourcePreset {
		t.Fatalf("known=%v source=%q, ожидался известный пресетный отчёт", rep.Known, rep.Source)
	}
	if rep.Preset == nil {
		t.Fatal("пресетный отчёт пуст")
	}
	if rep.Preset.PresetID != "ru-smart" {
		t.Errorf("preset_id = %q", rep.Preset.PresetID)
	}
	if len(rep.Preset.Sources) == 0 {
		t.Fatal("источники пресета не названы: отказ снова стал молчаливым")
	}
	var dropped int
	for _, s := range rep.Preset.Sources {
		if s.State != routing.RuleSourceDropped {
			t.Errorf("источник %q: state = %q, без панели и каталога он доехать не мог", s.Name, s.State)
			continue
		}
		dropped++
		if s.Reason != routing.RuleSourceReasonNoMirror {
			t.Errorf("источник %q: reason = %q, ожидалось %q", s.Name, s.Reason, routing.RuleSourceReasonNoMirror)
		}
		if s.Rules == 0 {
			t.Errorf("источник %q: не сосчитаны ссылавшиеся на него правила", s.Name)
		}
	}
	if dropped != 2 {
		t.Errorf("отчитано отказов: %d, у ru-smart два внешних списка", dropped)
	}
	if rep.Preset.DroppedRules == 0 {
		t.Error("выброшенные правила не сосчитаны")
	}
	if rep.Rules == nil || *rep.Rules != rep.Preset.Rules {
		t.Error("верхнеуровневый счётчик правил разошёлся с отчётом пресета")
	}
}

// Пресет «Только блок рекламы» это ОДИН тег GEOSITE. Отчёт обязан сказать, что
// база для него нужна, и честно назвать её состояние — иначе владелец так и
// остаётся с «непонятно, работает или нет».
func TestRouteReportNamesGeositeDependencyOfAdblockPreset(t *testing.T) {
	c, rep := raisedCore(t, "adblock", "")

	if !rep.Geosite.Required {
		t.Fatal("adblock объявлен не зависящим от базы GEOSITE")
	}
	if len(rep.Geosite.Tags) != 1 || rep.Geosite.Tags[0] != "category-ads-all" {
		t.Errorf("теги = %v, ожидался [category-ads-all]", rep.Geosite.Tags)
	}
	// Каталога нет, значит geox-url в конфиг не пишется (profile.applyGeoX), и
	// база либо уже лежит на диске, либо её судьба ядру неизвестна. Обещать
	// «работает» нельзя ни в том, ни в другом случае.
	if rep.Geosite.State != GeositeUnknown {
		t.Errorf("geosite.state = %q, ожидалось %q: подписанного адреса нет и файла нет", rep.Geosite.State, GeositeUnknown)
	}
	if rep.Geosite.Reason != GeositeReasonUnmanaged {
		t.Errorf("geosite.reason = %q, ожидалось %q", rep.Geosite.Reason, GeositeReasonUnmanaged)
	}
	if rep.Geosite.Path == "" {
		t.Error("отчёт не назвал путь, по которому искал базу")
	}

	// Тот же подъём, но база на диске есть: ответ обязан смениться на
	// «файл есть, подписи под ним нет», а не остаться прежним.
	if err := os.WriteFile(filepath.Join(c.workDir, geoSiteFile), []byte("not-a-real-db"), 0o600); err != nil {
		t.Fatalf("подкладываем базу: %v", err)
	}
	raw, err := c.RouteReportJSON()
	if err != nil {
		t.Fatalf("RouteReportJSON: %v", err)
	}
	again := decodeRouteReport(t, raw)
	if again.Geosite.State != GeositePresent {
		t.Errorf("geosite.state = %q, ожидалось %q", again.Geosite.State, GeositePresent)
	}
	if again.Geosite.Reason != GeositeReasonUnmanaged {
		t.Errorf("geosite.reason = %q: происхождение файла всё ещё не подписано", again.Geosite.Reason)
	}
	if again.Geosite.SizeBytes == 0 {
		t.Error("размер найденной базы не отчитан")
	}
}

// Выбранная страна входа, которую raw-путь применить не может, отчитывается
// отброшенной — тем же кодом причины, каким её отдаёт Capabilities.
func TestRouteReportNamesIgnoredRelay(t *testing.T) {
	_, rep := raisedCore(t, "global", "TR")

	if rep.Relay.Requested != "TR" {
		t.Errorf("relay.requested = %q", rep.Relay.Requested)
	}
	if rep.Relay.State != RouteRelayIgnored {
		t.Fatalf("relay.state = %q, ожидалось %q", rep.Relay.State, RouteRelayIgnored)
	}
	if rep.Relay.Capability == nil || rep.Relay.Capability.Reason != CapReasonRawProfile {
		t.Errorf("relay.capability = %+v, ожидалась причина %q", rep.Relay.Capability, CapReasonRawProfile)
	}
	if rep.Relay.DialerProxySeen {
		t.Error("в теле фикстуры нет dialer-proxy, а отчёт увидел цепочку")
	}
	if len(rep.Ignored) == 0 {
		t.Error("отброшенный выбор не попал в ignored")
	}
}

// Подъём без пресета: правила выбирает profile, и он НЕ эмитит ни одного
// матчера GEOSITE (страновые geo-правила туда намеренно не зашиты). База не
// нужна — и отчёт обязан сказать именно это, а не «неизвестно» и не «работает».
func TestRouteReportGeositeNotRequiredWithoutPreset(t *testing.T) {
	_, rep := raisedCore(t, "", "")
	if rep.Source != RouteSourceCoreDefault {
		t.Fatalf("source = %q, ожидалось %q", rep.Source, RouteSourceCoreDefault)
	}
	if rep.Rules != nil {
		t.Errorf("rules = %d, ожидался null: состав правил выбрал profile", *rep.Rules)
	}
	if rep.Geosite.Required {
		t.Error("база GEOSITE объявлена нужной там, где нет ни одного тега")
	}
	if rep.Geosite.State != GeositeNotRequired {
		t.Errorf("geosite.state = %q, ожидалось %q", rep.Geosite.State, GeositeNotRequired)
	}
	if rep.Geosite.Reason != "" {
		t.Errorf("geosite.reason = %q при not_required", rep.Geosite.Reason)
	}
}

// Без выбора входа отчёт не выдумывает ни цепочки, ни отказа.
func TestRouteReportRelayNotRequested(t *testing.T) {
	_, rep := raisedCore(t, "global", "")
	if rep.Relay.State != RouteRelayNotRequested {
		t.Errorf("relay.state = %q, ожидалось %q", rep.Relay.State, RouteRelayNotRequested)
	}
	if rep.Relay.Capability != nil {
		t.Errorf("relay.capability = %+v при отсутствии выбора", rep.Relay.Capability)
	}
}
