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
	conn := utls.UClient(raw, cfg, hello)
	hsCtx, cancel := context.WithTimeout(ctx, TLSHandshakeTimeout)
	defer cancel()
	if err := conn.HandshakeContext(hsCtx); err != nil {
		_ = raw.Close()
		return nil, err
	}
	return conn, nil
}
