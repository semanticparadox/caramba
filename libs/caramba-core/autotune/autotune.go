// Package autotune выбирает лучшие параметры подключения (выходной сервер,
// протокол, сетевой стек и при необходимости relay-вход) по результатам
// измерений. Логика выбора чистая и тестируемая; само измерение (probing)
// зависит от движка mihomo и подаётся снаружи через интерфейс Prober.
//
// Идея для России и подобных сетей: если прямой вход к серверам не проходит
// (DPI режет handshake), автоподбор рекомендует relay-вход через устойчивую
// страну, а выходной узел оставляет лучшим по задержке.
package autotune

import (
	"context"
	"errors"
	"sort"
)

// ProtocolPriority — порядок предпочтения протоколов: первыми идут лучшие для
// обхода DPI. При равных условиях autotune выбирает более приоритетный.
var ProtocolPriority = []string{
	"AmneziaWG",
	"VLESS-Reality",
	"Hysteria2",
	"TUIC",
	"Shadowsocks",
}

// DefaultStack — рекомендованный сетевой стек TUN по умолчанию.
const DefaultStack = "gvisor"

// ProbeResult — результат проверки одного выходного сервера с клиента.
type ProbeResult struct {
	ServerID string
	Country  string // ISO-2 страны выходного узла
	// LatencyMs — измеренная задержка. <= 0 означает, что узел недостижим
	// напрямую (handshake не прошёл / таймаут).
	LatencyMs int
	// OKProtocols — протоколы, чей handshake реально прошёл с этого клиента.
	OKProtocols []string
}

func (p ProbeResult) reachable() bool { return p.LatencyMs > 0 && len(p.OKProtocols) > 0 }

// Recommendation — итог автоподбора.
type Recommendation struct {
	ServerID string
	Protocol string
	Stack    string
	Relay    string // страна relay-входа; пусто — прямое подключение
	Reason   string
}

// Prober измеряет доступность и задержку выходных серверов с текущего клиента.
// Реализуется на стороне движка (mihomo): пробует handshake по протоколам и
// замеряет RTT. ctx позволяет отменить долгие замеры.
type Prober interface {
	Probe(ctx context.Context) ([]ProbeResult, error)
}

// ErrNoProbes возвращается, когда измерений нет вовсе (нечего выбирать).
var ErrNoProbes = errors.New("autotune: нет результатов измерений")

// bestProtocol выбирает наиболее приоритетный из доступных протоколов.
// Возвращает "" если список пуст.
func bestProtocol(ok []string) string {
	set := make(map[string]struct{}, len(ok))
	for _, p := range ok {
		set[p] = struct{}{}
	}
	for _, p := range ProtocolPriority {
		if _, has := set[p]; has {
			return p
		}
	}
	// Протокол вне списка приоритетов — берём первый как есть.
	if len(ok) > 0 {
		return ok[0]
	}
	return ""
}

// Recommend выбирает лучшие параметры по результатам измерений.
//
//   - Если есть достижимые напрямую серверы — берём с наименьшей задержкой и
//     самый приоритетный из прошедших на нём протоколов, relay не нужен.
//   - Если прямого пути нет (всё недостижимо), но задан relayCandidates —
//     рекомендуем relay-вход через первую страну и лучший по приоритету
//     протокол; выходной сервер — наименее «плохой» по задержке.
//   - Если измерений нет совсем — ErrNoProbes.
func Recommend(probes []ProbeResult, relayCandidates []string) (Recommendation, error) {
	if len(probes) == 0 {
		return Recommendation{}, ErrNoProbes
	}

	// Сортируем копию по возрастанию задержки (недостижимые в конец).
	sorted := append([]ProbeResult(nil), probes...)
	sort.SliceStable(sorted, func(i, j int) bool {
		a, b := sorted[i], sorted[j]
		ai, bi := a.LatencyMs, b.LatencyMs
		if ai <= 0 {
			ai = 1 << 30
		}
		if bi <= 0 {
			bi = 1 << 30
		}
		return ai < bi
	})

	// Лучший прямой путь.
	for _, p := range sorted {
		if p.reachable() {
			return Recommendation{
				ServerID: p.ServerID,
				Protocol: bestProtocol(p.OKProtocols),
				Stack:    DefaultStack,
				Relay:    "",
				Reason:   "прямой путь доступен, выбран сервер с наименьшей задержкой",
			}, nil
		}
	}

	// Прямого пути нет — пробуем relay.
	if len(relayCandidates) > 0 {
		return Recommendation{
			ServerID: sorted[0].ServerID,
			Protocol: ProtocolPriority[0],
			Stack:    DefaultStack,
			Relay:    relayCandidates[0],
			Reason:   "прямой вход заблокирован, выбран relay-вход",
		}, nil
	}

	// Ни прямого пути, ни relay — отдаём лучший по задержке с приоритетным
	// протоколом и честным reason, чтобы UI показал предупреждение.
	return Recommendation{
		ServerID: sorted[0].ServerID,
		Protocol: ProtocolPriority[0],
		Stack:    DefaultStack,
		Relay:    "",
		Reason:   "стабильный путь не найден, выбран лучший доступный вариант",
	}, nil
}
