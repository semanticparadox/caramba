//go:build mihomo

package api

import (
	"context"
	"time"

	"github.com/metacubex/mihomo/adapter"
	"github.com/metacubex/mihomo/common/utils"
)

// probeURL — лёгкий эндпойнт без тела (HTTP 204), стандартный для url-test в
// clash/mihomo. Успешный ответ ЧЕРЕЗ прокси означает, что узел не просто
// отзывается на TCP, а реально пропускает трафик.
const probeURL = "https://www.gstatic.com/generate_204"

// probeExpectStatus — допустимые коды ответа пробы (любой 2xx).
var probeExpectStatus, _ = utils.NewUnsignedRanges[uint16]("200-299")

// probeOne в сборке с ядром делает настоящий URL-тест через построенный
// прокси-адаптер: handshake протокола + HTTP-запрос сквозь узел.
//
// Если адаптер собрать не удалось (незнакомый ядру набор полей) или тест не
// прошёл, замер деградирует до TCP-соединения с server:port. Это осознанный
// компромисс: узел, у которого жив адрес, но зарезан протокол, показывается с
// задержкой, а не молча пропадает из списка, — приложение и так покажет его
// пользователю как один из вариантов.
func probeOne(ctx context.Context, node probeNode, timeout time.Duration) int {
	if node.raw != nil {
		if px, err := adapter.ParseProxy(node.raw); err == nil && px != nil {
			tctx, cancel := context.WithTimeout(ctx, timeout)
			delay, terr := px.URLTest(tctx, probeURL, probeExpectStatus)
			cancel()
			if terr == nil && delay > 0 {
				return int(delay)
			}
		}
	}
	return tcpProbe(ctx, node, timeout)
}
