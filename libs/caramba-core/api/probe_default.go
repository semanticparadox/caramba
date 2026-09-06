//go:build !mihomo

package api

import (
	"context"
	"time"
)

// probeOne в сборке без нативного ядра меряет TCP-достижимость узла: криптослоя
// нет, честный URL-тест сквозь прокси построить не из чего.
//
// Вердикт здесь ОТДЕЛЬНЫЙ — tcp_only, — а не «ok»: число настоящее, но отвечает
// на более слабый вопрос («жив ли адрес»), и приложение обязано сказать это
// словами, а не показать его наравне с задержкой сквозь узел. Иначе dev-сборка
// и боевая означали бы одним и тем же числом разные вещи.
func probeOne(ctx context.Context, node probeNode, timeout time.Duration) probeOutcome {
	ms := tcpProbe(ctx, node, timeout)
	if ms < 0 {
		return probeOutcome{
			latencyMs: -1,
			tcpMs:     -1,
			verdict:   ProbeVerdictPortClosed,
			detail:    "the address did not accept a tcp connection within the timeout",
		}
	}
	return probeOutcome{
		latencyMs: ms,
		tcpMs:     ms,
		verdict:   ProbeVerdictTCPOnly,
		detail:    "this build has no core, so only the address was checked, not the protocol handshake",
	}
}
