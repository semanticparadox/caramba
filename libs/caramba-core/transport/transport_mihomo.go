//go:build mihomo

package transport

import (
	"context"
	"crypto/x509"
	"math/rand"
	"net"

	utls "github.com/metacubex/utls"
)

// В сборке с нативным ядром доступен uTLS, вендоренный вместе с mihomo.
// ClientHello выбирается ЯВНЫМ современным идентификатором, а не псевдонимом
// Auto: псевдонимы разрешаются в сборку Chrome, которая больше не выпускается,
// и это само по себе отличительный признак.

// helloRoster это набор явных современных ClientHello. Выбор делается НА
// СОЕДИНЕНИЕ, а не на эпоху: постоянный отпечаток на всё время жизни профиля
// это тот же самый признак, только устойчивее.
var helloRoster = []utls.ClientHelloID{
	utls.HelloChrome_133,
	utls.HelloChrome_131,
	utls.HelloChrome_120,
	utls.HelloFirefox_120,
	utls.HelloSafari_16_0,
	utls.HelloEdge_106,
}

// defaultTunnelReason: в этой сборке R4 умеет ходить через локальный
// mixed-инбаунд, поэтому доступность решает наличие адреса, а не сборка.
func defaultTunnelReason() Reason { return ReasonNone }

// utlsALPN отдаёт согласованный протокол соединения uTLS. Отдельная функция
// нужна потому, что uTLS возвращает СВОЙ тип ConnectionState: он совпадает с
// crypto/tls по полям, но не по типу, и ни одно приведение к *tls.Conn на нём
// не проходит. Ровно на этом приведении http.Transport и теряет h2.
func utlsALPN(c net.Conn) (string, bool) {
	u, ok := c.(*utls.UConn)
	if !ok {
		return "", false
	}
	return u.ConnectionState().NegotiatedProtocol, true
}

// dialTLSConn открывает соединение uTLS с явным ClientHello и применяет SPKI
// пины поверх обычной проверки цепочки.
func dialTLSConn(ctx context.Context, t Target, addr string) (net.Conn, error) {
	// Сырое соединение открывает dialRawConn: он же и набирает прокси, поэтому
	// uTLS с явным ClientHello и пины действуют на ступенях R4 и R5 так же,
	// как на прямой.
	raw, err := dialRawConn(ctx, t, addr)
	if err != nil {
		return nil, err
	}
	name := serverNameFor(t)
	cfg := &utls.Config{
		ServerName:         name,
		MinVersion:         utls.VersionTLS12,
		InsecureSkipVerify: true, // своя проверка ниже; отключить и не заменить нельзя
		VerifyPeerCertificate: func(rawCerts [][]byte, _ [][]*x509.Certificate) error {
			state, err := verifyChain(rawCerts, name)
			if err != nil {
				return err
			}
			return checkPins(state, t.Pins)
		},
	}
	hello := helloRoster[rand.Intn(len(helloRoster))]
	// Пресет применяется КАК ЕСТЬ, включая список ALPN. Он объявляет h2 первым,
	// потому что так делает браузер, за который мы себя выдаём; ни один браузер
	// не предлагает один http/1.1, и предложение из одного http/1.1 выделяло бы
	// наше рукопожатие само по себе — то есть отменяло бы весь смысл uTLS.
	// Разбирать выбранный сервером протокол умеет обмен: exchangeOverConn
	// смотрит на ALPN и ведёт h2 через golang.org/x/net/http2, а http/1.1 через
	// http.Transport.
	conn := utls.UClient(raw, cfg, hello)
	hsCtx, cancel := context.WithTimeout(ctx, TLSHandshakeTimeout)
	defer cancel()
	if err := conn.HandshakeContext(hsCtx); err != nil {
		_ = raw.Close()
		return nil, err
	}
	return conn, nil
}
