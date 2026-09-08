//go:build !mihomo

// Тест выбора Prober'а для сборки БЕЗ нативного ядра: newProber должен отдавать
// TCPProber (быстрый TCP-замер), игнорируя сырой YAML подписки. Под -tags mihomo
// действует другая реализация (см. prober_mihomo.go), которая в этой сборке не
// компилируется.
package api

import (
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/autotune"
)

func TestNewProberDefaultIsTCP(t *testing.T) {
	cands := []autotune.Candidate{
		{ServerID: "n1", Host: "1.2.3.4", Port: 443, Protocols: []string{"VLESS-Reality"}},
	}
	// Сырой YAML в default-сборке не используется — передаём заведомо «лишний».
	p := newProber(cands, []byte("proxies: [whatever]"))
	if _, ok := p.(*autotune.TCPProber); !ok {
		t.Fatalf("default-сборка должна выбирать *autotune.TCPProber, получено %T", p)
	}
}
