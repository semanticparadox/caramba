//go:build mihomo

package api

import (
	"context"
	"errors"
	"time"

	"github.com/metacubex/mihomo/adapter"
	"github.com/metacubex/mihomo/common/utils"
)

// probeExpectStatus — допустимые коды ответа пробы (любой 2xx).
var probeExpectStatus, _ = utils.NewUnsignedRanges[uint16]("200-299")

// probeOne в сборке с ядром делает настоящий URL-тест через построенный
// прокси-адаптер: handshake протокола + HTTP-запрос сквозь узел.
//
// ЗДЕСЬ БЫЛ ФОЛБЭК на TCP-пробу «чтобы узел не пропадал из списка». Он и был
// причиной, по которой мёртвый флот выглядел здоровым: узел с отозванным ключом
// принимает TCP за 118 мс, отвергает handshake — и показывался как «118 мс»,
// то есть как самый быстрый. Число задержки теперь означает ровно одно:
// «сквозь этот узел прошёл настоящий запрос». Всё прочее приходит вердиктом,
// и узел из списка не пропадает — он пропадает из ВЫБОРА, что и требовалось.
//
// TCP-проба никуда не делась, но сменила роль: она идёт параллельно и служит
// второй точкой для классификатора (отличить «узел отверг ключ» от «до узла не
// достучаться») и справочным числом для экрана.
func probeOne(ctx context.Context, node probeNode, timeout time.Duration) probeOutcome {
	tcpCh := make(chan int, 1)
	go func() { tcpCh <- tcpProbe(ctx, node, timeout) }()

	var (
		delay uint16
		terr  error = errors.New("proxy adapter was not built")
		built bool
	)
	if node.raw != nil {
		if px, perr := adapter.ParseProxy(node.raw); perr == nil && px != nil {
			built = true
			tctx, cancel := context.WithTimeout(ctx, timeout)
			delay, terr = px.URLTest(tctx, probeTargetURL(), probeExpectStatus)
			cancel()
		}
	}

	tcpMs := <-tcpCh

	if built && terr == nil && delay > 0 {
		return probeOutcome{latencyMs: int(delay), tcpMs: tcpMs, verdict: ProbeVerdictOK}
	}

	if !built {
		// Адаптер не собрался: про сам узел это не говорит ничего. Кроме
		// одного — если и адрес молчит, то это уже факт, и он полезнее.
		if tcpMs < 0 {
			return probeOutcome{
				latencyMs: -1,
				tcpMs:     tcpMs,
				verdict:   ProbeVerdictPortClosed,
				detail:    "the core could not build an adapter for this proxy, and its address does not answer either",
			}
		}
		return probeOutcome{
			latencyMs: -1,
			tcpMs:     tcpMs,
			verdict:   ProbeVerdictUnsupported,
			detail:    "the core does not know this proxy type, so no handshake could be attempted",
		}
	}

	// URL-тест «прошёл» с нулевой задержкой — это не успех: ядро отдаёт 0
	// там, где измерять было нечего.
	if terr == nil {
		return probeOutcome{
			latencyMs: -1,
			tcpMs:     tcpMs,
			verdict:   ProbeVerdictTimeout,
			detail:    "the url test returned no measurable delay",
		}
	}

	verdict, detail := classifyProbeFailure(terr, tcpMs)
	return probeOutcome{latencyMs: -1, tcpMs: tcpMs, verdict: verdict, detail: detail}
}
