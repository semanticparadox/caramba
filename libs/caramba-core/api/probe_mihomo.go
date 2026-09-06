//go:build mihomo

package api

import (
	"context"
	"errors"
	"time"

	"github.com/metacubex/mihomo/adapter"
	"github.com/metacubex/mihomo/common/utils"
	"github.com/metacubex/mihomo/component/proxydialer"
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
	// Тип, который ядро не строит, отсекается ДО любых соединений: раньше
	// такой узел всё равно получал TCP-пробу, и её результат решал, назвать
	// его «неподдерживаемым» или «мёртвым адресом». Это была монетка.
	if !coreCanBuildProxyType(node.typ) {
		return unsupportedOutcome(node.typ)
	}

	// TCP-пробы не будет там, где её ответ к делу не относится: у
	// UDP-семейств на этом порту TCP никто не слушает, а у выхода за релеем
	// прямой TCP не лежит ни на одном настоящем пути. Раньше её молчание в
	// обоих случаях становилось приговором здоровому узлу.
	tcpBlind := isUDPProxyType(node.typ) || node.relayRaw != nil
	tcpCh := make(chan int, 1)
	if tcpBlind {
		tcpCh <- -1
	} else {
		go func() { tcpCh <- tcpProbe(ctx, node, timeout) }()
	}

	// Цепочка через релей собирается ЗДЕСЬ и передаётся адаптеру готовым
	// диалером. Ключ dialer-proxy сам по себе в замере не работает: mihomo
	// ищет релей по имени в живом туннеле, которого во время замера нет.
	var opts []adapter.ProxyOption
	if node.relayRaw != nil {
		relayTyp, _ := node.relayRaw["type"].(string)
		relay, rerr := adapter.ParseProxy(node.relayRaw)
		if rerr != nil || relay == nil || !coreCanBuildProxyType(relayTyp) {
			// Релей не собрался — измерять нечего: прямой набор выхода
			// ответил бы про ДРУГОЙ путь, не тот, которым пойдёт туннель.
			return probeOutcome{
				latencyMs: -1,
				tcpMs:     -1,
				verdict:   ProbeVerdictUnsupported,
				detail:    "this exit is dialled through a relay the core cannot build, so the chain could not be measured",
			}
		}
		opts = append(opts, adapter.WithDialerForAPI(proxydialer.New(relay, false)))
	}

	var (
		delay uint16
		terr  error = errors.New("proxy adapter was not built")
		built bool
	)
	if node.raw != nil {
		if px, perr := adapter.ParseProxy(node.raw, opts...); perr == nil && px != nil {
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
		// У UDP-семейств такого факта нет: молчание TCP там ничего не значит.
		if tcpMs < 0 && !tcpBlind {
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

	verdict, detail := classifyProbeFailure(terr, tcpMs, tcpBlind)
	return probeOutcome{latencyMs: -1, tcpMs: tcpMs, verdict: verdict, detail: detail}
}
