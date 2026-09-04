package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"strings"
	"testing"
	"time"
)

// listenLocal поднимает TCP-слушателя на свободном порту петли и возвращает его
// порт. Слушатель принимает и сразу закрывает соединения: замеру нужен только
// успешный TCP-handshake.
func listenLocal(t *testing.T) int {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	t.Cleanup(func() { _ = ln.Close() })
	go func() {
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			_ = conn.Close()
		}
	}()
	return ln.Addr().(*net.TCPAddr).Port
}

// closedPort возвращает порт, который заведомо никто не слушает: слушателя
// открываем и тут же закрываем.
func closedPort(t *testing.T) int {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	port := ln.Addr().(*net.TCPAddr).Port
	if err := ln.Close(); err != nil {
		t.Fatalf("close: %v", err)
	}
	return port
}

// probeFixture собирает clash-YAML из трёх узлов: два живых слушателя и один
// закрытый порт.
//
// Тип узлов намеренно неизвестен ядру ("probe-fixture"): под -tags mihomo
// adapter.ParseProxy на нём падает, и замер честно деградирует до TCP-пробы —
// той же, что в сборке без ядра. Так тест меряет реальные задержки и ведёт себя
// одинаково в обеих сборках, не выходя в интернет за URL-тестом.
func probeFixture(alive1, alive2, dead int) string {
	return fmt.Sprintf(`
proxies:
  - name: "🇳🇱 NL-1"
    type: probe-fixture
    server: 127.0.0.1
    port: %d
  - name: "DE-2"
    type: probe-fixture
    server: 127.0.0.1
    port: %d
  - name: "Dead node"
    type: probe-fixture
    server: 127.0.0.1
    port: %d
proxy-groups:
  - name: CARAMBA
    type: select
    proxies: ["🇳🇱 NL-1", "DE-2", "Dead node", "DIRECT"]
`, alive1, alive2, dead)
}

// Живые узлы получают реальную задержку, мёртвый — -1; порядок и поля совпадают
// с контрактом ABI v2.
func TestProbeMeasuresLoadedConfig(t *testing.T) {
	core := newTestCore(t)
	a1, a2, dead := listenLocal(t), listenLocal(t), closedPort(t)
	if err := core.SetImportedConfig([]byte(probeFixture(a1, a2, dead))); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}

	rep, err := core.Probe(context.Background(), 2000)
	if err != nil {
		t.Fatalf("Probe: %v", err)
	}
	if len(rep.Servers) != 3 {
		t.Fatalf("узлов в отчёте %d, ожидалось 3: %+v", len(rep.Servers), rep.Servers)
	}

	first := rep.Servers[0]
	if first.ID != "🇳🇱 NL-1" || first.Name != first.ID {
		t.Fatalf("id/name первого узла: %+v", first)
	}
	if first.Country != "NL" {
		t.Fatalf("страна из флаг-эмодзи не распознана: %+v", first)
	}
	if first.Type != "probe-fixture" || first.Server != "127.0.0.1" || first.Port != a1 {
		t.Fatalf("поля узла: %+v", first)
	}
	if first.LatencyMs < 1 {
		t.Fatalf("живой узел должен дать задержку >= 1мс, получено %d", first.LatencyMs)
	}

	second := rep.Servers[1]
	if second.Country != "DE" {
		t.Fatalf("страна из двухбуквенного токена не распознана: %+v", second)
	}
	if second.LatencyMs < 1 {
		t.Fatalf("живой узел должен дать задержку >= 1мс, получено %d", second.LatencyMs)
	}

	third := rep.Servers[2]
	if third.LatencyMs != -1 {
		t.Fatalf("закрытый порт должен дать -1, получено %d", third.LatencyMs)
	}
	if third.Country != "" {
		t.Fatalf("страна не должна выводиться из %q: %q", third.ID, third.Country)
	}
}

// Без загруженной конфигурации замер возвращает пустой список, а не ошибку:
// приложение вправе спросить о серверах до импорта.
func TestProbeWithoutLoadedConfig(t *testing.T) {
	core := newTestCore(t)
	out, err := core.ProbeJSON(context.Background(), 500)
	if err != nil {
		t.Fatalf("ProbeJSON: %v", err)
	}
	if out != `{"servers":[]}` {
		t.Fatalf("ожидалось {\"servers\":[]}, получено %s", out)
	}
}

// JSON-форма отчёта — ровно та, что описана в контракте ABI v2.
func TestProbeJSONShape(t *testing.T) {
	core := newTestCore(t)
	a1, a2, dead := listenLocal(t), listenLocal(t), closedPort(t)
	if err := core.SetImportedConfig([]byte(probeFixture(a1, a2, dead))); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}
	out, err := core.ProbeJSON(context.Background(), 2000)
	if err != nil {
		t.Fatalf("ProbeJSON: %v", err)
	}
	var decoded struct {
		Servers []map[string]any `json:"servers"`
	}
	if err := json.Unmarshal([]byte(out), &decoded); err != nil {
		t.Fatalf("разбор %s: %v", out, err)
	}
	if len(decoded.Servers) != 3 {
		t.Fatalf("узлов %d: %s", len(decoded.Servers), out)
	}
	for _, key := range []string{"id", "name", "type", "server", "port", "country", "latencyMs"} {
		if _, ok := decoded.Servers[0][key]; !ok {
			t.Fatalf("в отчёте нет поля %q: %s", key, out)
		}
	}
}

// Отменённый контекст не подвешивает замер: недомеренные узлы остаются с -1.
func TestProbeRespectsCanceledContext(t *testing.T) {
	core := newTestCore(t)
	a1, a2, dead := listenLocal(t), listenLocal(t), closedPort(t)
	if err := core.SetImportedConfig([]byte(probeFixture(a1, a2, dead))); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	rep, err := core.Probe(ctx, 2000)
	if err != nil {
		t.Fatalf("Probe: %v", err)
	}
	for _, s := range rep.Servers {
		if s.LatencyMs != -1 {
			t.Fatalf("при отменённом контексте ожидались -1, получено %+v", s)
		}
	}
}

// Узлы без имени в отчёт не попадают: id — это имя прокси, а пустой id
// приложение не сможет передать обратно в Up.
func TestProbeSkipsUnnamedProxies(t *testing.T) {
	core := newTestCore(t)
	const cfg = `
proxies:
  - type: probe-fixture
    server: 127.0.0.1
    port: 1
  - name: "NL-1"
    type: probe-fixture
    server: 127.0.0.1
    port: 1
`
	if err := core.SetImportedConfig([]byte(cfg)); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}
	rep, err := core.Probe(context.Background(), 200)
	if err != nil {
		t.Fatalf("Probe: %v", err)
	}
	if len(rep.Servers) != 1 || rep.Servers[0].ID != "NL-1" {
		t.Fatalf("ожидался единственный узел NL-1, получено %+v", rep.Servers)
	}
}

// Битый YAML — честная ошибка с именем пакета, а не пустой список.
func TestProbeBrokenConfig(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetImportedConfig([]byte("proxies: [ {name: broken")); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}
	if _, err := core.Probe(context.Background(), 200); err == nil {
		t.Fatal("ожидалась ошибка разбора")
	} else if !strings.Contains(err.Error(), "api:") {
		t.Fatalf("ошибка без контекста пакета: %v", err)
	}
}

// panelCore — ядро на панельном пути: настроенная панель и инъецированный токен,
// то есть ровно то состояние, в котором приложение спрашивает про задержки ДО
// подъёма туннеля.
//
// Сеть в этих тестах не поднимается: выборка подписки идёт через лестницу
// транспортов, а та требует настоящего TLS (инвариант 8, `CheckFetchURL`), и
// самоподписанный httptest ей не подсунуть. Поэтому проверяется ГРАНИЦА
// поведения — ходит ядро за конфигом или нет, — а не содержимое ответа: сам
// замер загруженного конфига покрыт тестами выше.
func panelCore(t *testing.T, base string) *Core {
	t.Helper()
	dir := t.TempDir()
	core, err := NewCore(Config{
		PanelBaseURL:   base,
		SubBaseURL:     base,
		WorkDir:        dir,
		TokenStorePath: dir + "/tokens.json",
	})
	if err != nil {
		t.Fatalf("NewCore: %v", err)
	}
	if err := core.InjectToken("access-token", "", time.Now().Add(time.Hour).Unix(), "sub-uuid"); err != nil {
		t.Fatalf("InjectToken: %v", err)
	}
	return core
}

// ГЛАВНОЕ по панельному пути: ядро идёт за конфигом САМО, до первого Up.
//
// Конфиг там кэшировался только внутри Up, поэтому ядро, заведённое приложением
// специально под замер, отвечало `{"servers":[]}` без ошибки — «Ядро не вернуло
// ни одного узла». Пустой ответ и есть та регрессия, которую тест ловит: теперь
// ядро обязано либо померить узлы, либо назвать причину, по которой не смогло их
// взять, но не молчать.
func TestProbeFetchesPanelProfileBeforeUp(t *testing.T) {
	core := panelCore(t, "https://panel.invalid")

	rep, err := core.Probe(context.Background(), 500)
	if err == nil {
		t.Fatalf("ожидалась причина неудачи, получен отчёт %+v", rep.Servers)
	}
	if !strings.Contains(err.Error(), "загрузка узлов подписки для замера") {
		t.Fatalf("ошибка не про загрузку узлов для замера: %v", err)
	}
}

// Загруженный конфиг сильнее панели: в сеть за тем же телом ядро не идёт.
//
// Иначе каждый повторный замер стоил бы round-trip к подписке — а на панельном
// пути этот round-trip сегодня ещё и идёт через российский релэй.
func TestProbePrefersLoadedConfigOverPanel(t *testing.T) {
	core := panelCore(t, "https://panel.invalid")
	a1, a2, dead := listenLocal(t), listenLocal(t), closedPort(t)
	if err := core.SetImportedConfig([]byte(probeFixture(a1, a2, dead))); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}

	rep, err := core.Probe(context.Background(), 2000)
	if err != nil {
		t.Fatalf("Probe: %v", err)
	}
	if len(rep.Servers) != 3 {
		t.Fatalf("узлов %d, ожидалось 3: %+v", len(rep.Servers), rep.Servers)
	}
	if rep.Servers[0].LatencyMs < 1 {
		t.Fatalf("живой узел должен дать задержку >= 1мс: %+v", rep.Servers[0])
	}
}

// Панель настроена, но вход не выполнен — по-прежнему пустой список без ошибки:
// спросить про задержки до входа приложение вправе, и ходить за конфигом,
// которого ему не дадут, незачем.
func TestProbeWithoutAuthStaysEmpty(t *testing.T) {
	dir := t.TempDir()
	core, err := NewCore(Config{
		PanelBaseURL:   "https://panel.invalid",
		SubBaseURL:     "https://panel.invalid",
		WorkDir:        dir,
		TokenStorePath: dir + "/tokens.json",
	})
	if err != nil {
		t.Fatalf("NewCore: %v", err)
	}
	rep, err := core.Probe(context.Background(), 200)
	if err != nil {
		t.Fatalf("Probe: %v", err)
	}
	if len(rep.Servers) != 0 {
		t.Fatalf("без входа ожидался пустой список, получено %+v", rep.Servers)
	}
}
