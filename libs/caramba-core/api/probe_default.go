//go:build !mihomo

package api

import (
	"context"
	"time"
)

// probeOne в сборке без нативного ядра меряет TCP-достижимость узла: криптослоя
// нет, честный URL-тест сквозь прокси построить не из чего.
func probeOne(ctx context.Context, node probeNode, timeout time.Duration) int {
	return tcpProbe(ctx, node, timeout)
}
