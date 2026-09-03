package api

import (
	"encoding/json"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/transport"
)

// TestLadderInjectedAtBothConstructionSites проверяет шов, ради которого этот
// пакет вообще правили: HTTPDoer поверх лестницы обязан уходить и в
// auth.NewPanelClient, и в subscription.NewClient, и это обязано выполняться
// в ОБОИХ местах их сборки.
//
// Проверять надо именно второе место. Первое ломается на первом же запуске и
// чинится сразу; забытый SetPanelURL молча возвращает перерегистрированного
// арендатора к собственному транспорту Go, и это находят через полгода.
func TestLadderInjectedAtBothConstructionSites(t *testing.T) {
	dir := t.TempDir()
	c, err := NewCore(Config{
		PanelBaseURL:   "https://panel.example.net",
		WorkDir:        dir,
		TokenStorePath: dir + "/tokens.json",
	})
	if err != nil {
		t.Fatalf("ядро: %v", err)
	}
	if c.doer == nil || c.ladder == nil {
		t.Fatal("лестница не собрана в NewCore")
	}
	if any(c.auth.HTTPDoer()) != any(c.doer) {
		t.Fatal("NewCore: панельный клиент собран без лестницы")
	}
	if any(c.sub.HTTPDoer()) != any(c.doer) {
		t.Fatal("NewCore: подписочный клиент собран без лестницы")
	}

	if err := c.SetPanelURL("https://other.example.net"); err != nil {
		t.Fatalf("смена панели: %v", err)
	}
	if any(c.auth.HTTPDoer()) != any(c.doer) {
		t.Fatal("SetPanelURL: панельный клиент пересобран без лестницы")
	}
	if any(c.sub.HTTPDoer()) != any(c.doer) {
		t.Fatal("SetPanelURL: подписочный клиент пересобран без лестницы")
	}
}

// TestLadderStateEnumeratesEveryCompiledRung: инвариант 17 требует, чтобы на
// одном экране были ВСЕ скомпилированные ступени, включая недоступные, с
// причиной. Скрытая ступень это ступень, которую пользователь не может
// проверить.
func TestLadderStateEnumeratesEveryCompiledRung(t *testing.T) {
	dir := t.TempDir()
	c, err := NewCore(Config{PanelBaseURL: "https://panel.example.net", WorkDir: dir, TokenStorePath: dir + "/t.json"})
	if err != nil {
		t.Fatalf("ядро: %v", err)
	}
	raw, err := c.CsmLadderJSON()
	if err != nil {
		t.Fatalf("состояние лестницы: %v", err)
	}
	var out struct {
		Rungs []transport.RungState `json:"rungs"`
	}
	if err := json.Unmarshal([]byte(raw), &out); err != nil {
		t.Fatalf("разбор: %v", err)
	}
	if len(out.Rungs) != len(transport.AllRungs()) {
		t.Fatalf("перечислено %d ступеней, скомпилировано %d", len(out.Rungs), len(transport.AllRungs()))
	}
	for _, r := range out.Rungs {
		if !r.Enabled && r.Reason == "" {
			t.Fatalf("ступень %s выключена без причины", r.Name)
		}
	}
}

// TestCsmSetLadderRefusesMandatoryOff: ступени 0 и 6 выключить нельзя, и отказ
// возвращается, а не проглатывается.
func TestCsmSetLadderRefusesMandatoryOff(t *testing.T) {
	dir := t.TempDir()
	c, err := NewCore(Config{PanelBaseURL: "https://panel.example.net", WorkDir: dir, TokenStorePath: dir + "/t.json"})
	if err != nil {
		t.Fatalf("ядро: %v", err)
	}
	if err := c.CsmSetLadderJSON(`{"enabled":{"0":false}}`); err == nil {
		t.Fatal("R0 удалось выключить через API")
	}
	if err := c.CsmSetLadderJSON(`{"enabled":{"6":false}}`); err == nil {
		t.Fatal("R6 удалось выключить через API")
	}
	if err := c.CsmSetLadderJSON(`{"enabled":{"5":false}}`); err != nil {
		t.Fatalf("R5 не удалось выключить: %v", err)
	}
}

// TestLadderRequestRefusesHTTP: инвариант 8 действует и на произвольный запрос
// через лестницу.
func TestLadderRequestRefusesHTTP(t *testing.T) {
	dir := t.TempDir()
	c, err := NewCore(Config{PanelBaseURL: "https://panel.example.net", WorkDir: dir, TokenStorePath: dir + "/t.json"})
	if err != nil {
		t.Fatalf("ядро: %v", err)
	}
	raw, err := c.LadderRequestJSON(t.Context(), `{"method":"GET","path":"/sub/k1","origin":"http://panel.example.net"}`)
	if err != nil {
		t.Fatalf("вызов: %v", err)
	}
	var out LadderHTTPResponse
	if err := json.Unmarshal([]byte(raw), &out); err != nil {
		t.Fatalf("разбор: %v", err)
	}
	if out.Error == "" {
		t.Fatal("origin по http принят")
	}
}
