//go:build !mihomo

package api

import (
	"context"
	"os"
	"testing"

	"gopkg.in/yaml.v3"
)

// pinConfig — импортированная подписка с тремя узлами и DIRECT в селекторе.
const pinConfig = `
proxies:
  - name: "NL-1"
    type: probe-fixture
    server: 127.0.0.1
    port: 1080
  - name: "DE-2"
    type: probe-fixture
    server: 127.0.0.1
    port: 1081
proxy-groups:
  - name: CARAMBA
    type: select
    proxies: ["NL-1", "DE-2", "DIRECT"]
`

// selectorFromFile читает собранный конфиг и возвращает участников CARAMBA.
func selectorFromFile(t *testing.T, path string) []string {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("чтение конфига: %v", err)
	}
	var doc struct {
		Groups []struct {
			Name    string   `yaml:"name"`
			Proxies []string `yaml:"proxies"`
		} `yaml:"proxy-groups"`
	}
	if err := yaml.Unmarshal(raw, &doc); err != nil {
		t.Fatalf("разбор конфига: %v", err)
	}
	for _, g := range doc.Groups {
		if g.Name == "CARAMBA" {
			return g.Proxies
		}
	}
	t.Fatal("группа CARAMBA не найдена в собранном конфиге")
	return nil
}

// Up(serverID) для импортированной подписки закрепляет узел первым в селекторе:
// первый участник select-группы и есть выбор mihomo по умолчанию.
//
// Тест собран только без тега mihomo: со stub-движком Up не поднимает настоящий
// туннель, и проверять можно именно собранный конфиг. Что ядро действительно
// уважает этот порядок, проверяет profile/pin_mihomo_test.go.
func TestUpPinsImportedServer(t *testing.T) {
	core := newTestCore(t)
	// Kill-switch убрал бы DIRECT и смазал проверку порядка.
	if err := core.SetPolicyJSON(`{"killSwitch":false}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	if err := core.SetImportedConfig([]byte(pinConfig)); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}

	res, err := core.Up(context.Background(), "DE-2")
	if err != nil {
		t.Fatalf("Up: %v", err)
	}
	got := selectorFromFile(t, res.ConfigPath)
	want := []string{"DE-2", "NL-1", "DIRECT"}
	if len(got) != len(want) {
		t.Fatalf("селектор %v, ожидался %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("селектор %v, ожидался %v", got, want)
		}
	}
	_ = core.Down()
}

// Пустой serverID оставляет автоматический выбор.
func TestUpWithoutServerKeepsOrder(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetPolicyJSON(`{"killSwitch":false}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	if err := core.SetImportedConfig([]byte(pinConfig)); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}
	res, err := core.Up(context.Background(), "")
	if err != nil {
		t.Fatalf("Up: %v", err)
	}
	if got := selectorFromFile(t, res.ConfigPath); got[0] != "NL-1" {
		t.Fatalf("порядок изменился без serverID: %v", got)
	}
	_ = core.Down()
}

// После Up импортированная конфигурация доступна замеру: приложение показывает
// пинги по тому же списку, что подняло туннель.
func TestProbeUsesImportedConfigAfterUp(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetImportedConfig([]byte(pinConfig)); err != nil {
		t.Fatalf("SetImportedConfig: %v", err)
	}
	rep, err := core.Probe(context.Background(), 200)
	if err != nil {
		t.Fatalf("Probe: %v", err)
	}
	if len(rep.Servers) != 2 {
		t.Fatalf("узлов %d: %+v", len(rep.Servers), rep.Servers)
	}
}
