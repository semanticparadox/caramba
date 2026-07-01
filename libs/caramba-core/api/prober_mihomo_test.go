//go:build mihomo

// Тест выбора Prober'а и сборки ProxyConfigs для сборки С нативным ядром
// (-tags mihomo). Проверяет, что newProber отдаёт *autotune.MihomoProber и что
// сырые clash-map'ы из YAML подписки приходят под ДРУЖЕЛЮБНЫМИ именами протоколов
// (ключи совпадают с Candidate.Protocols), а не под clash-типами.
//
// ВНИМАНИЕ: компилируется только под -tags mihomo, поэтому в окружении без
// тулчейна/полного go.sum mihomo он не собирается (это ожидаемо).
package api

import (
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/autotune"
)

func TestNewProberMihomoIsMihomo(t *testing.T) {
	p := newProber(nil, []byte("proxies: []"))
	if _, ok := p.(*autotune.MihomoProber); !ok {
		t.Fatalf("сборка с -tags mihomo должна выбирать *autotune.MihomoProber, получено %T", p)
	}
}

func TestProxyConfigsFromYAMLFriendlyKeys(t *testing.T) {
	raw := []byte(`
proxies:
  - name: "DE Stealth"
    type: vless
    server: 1.2.3.4
    port: 443
  - name: "DE Stealth"
    type: ss
    server: 1.2.3.4
    port: 8388
`)
	cfg := proxyConfigsFromYAML(raw)
	de := cfg["DE Stealth"]
	if de == nil {
		t.Fatal(`узел "DE Stealth" отсутствует`)
	}
	if _, ok := de["VLESS-Reality"]; !ok {
		t.Errorf("ожидался ключ VLESS-Reality (дружелюбный), получено: %v keys", keys(de))
	}
	if _, ok := de["Shadowsocks"]; !ok {
		t.Errorf("ожидался ключ Shadowsocks (дружелюбный), получено: %v keys", keys(de))
	}
	// clash-типы как ключи присутствовать НЕ должны.
	if _, ok := de["vless"]; ok {
		t.Error("clash-тип vless не должен быть ключом — ожидается дружелюбное имя")
	}
}

func keys(m map[string]map[string]any) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}
