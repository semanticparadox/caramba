//go:build !mihomo

// Выбор Prober'а для сборки БЕЗ нативного ядра (CLI, тесты, gomobile-фасад без
// mihomo). Здесь доступен только быстрый TCP-замер достижимости: handshake
// протоколов без криптослоя ядра проверить нельзя. Сырой YAML подписки не
// используется (нужен только mihomo-Prober'у).
package api

import (
	"github.com/semanticparadox/caramba/libs/caramba-core/autotune"
)

// newProber возвращает Prober по умолчанию для сборки без ядра — TCPProber.
func newProber(cands []autotune.Candidate, _ []byte) autotune.Prober {
	return autotune.NewTCPProber(cands)
}
