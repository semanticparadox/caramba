//go:build !mihomo

package transport

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"net"
)

// В сборке без нативного ядра uTLS недоступен: он приходит вместе с mihomo.
// ClientHello тут стандартный для Go, и это видно на экране ступеней, а не
// спрятано, потому что отличимый ClientHello это ровно то, ради чего uTLS в
// протоколе есть.

// defaultTunnelReason: ступень R4 идёт через работающий экземпляр mihomo,
// которого в этой сборке нет.
func defaultTunnelReason() Reason { return ReasonAppVersionUnsupported }

// dialTLSConn открывает TLS соединение стандартным crypto/tls и применяет
// SPKI пины поверх обычной проверки цепочки.
func dialTLSConn(ctx context.Context, t Target, addr string) (net.Conn, error) {
	// Сырое соединение открывает dialRawConn: он же и набирает прокси, поэтому
	// проверка сертификата и пины действуют на ступенях R4 и R5 так же, как на
	// прямой.
	raw, err := dialRawConn(ctx, t, addr)
	if err != nil {
		return nil, err
	}
	name := serverNameFor(t)
	cfg := &tls.Config{
		ServerName:         name,
		MinVersion:         tls.VersionTLS12,
		InsecureSkipVerify: true, // проверку делаем сами в VerifyPeerCertificate
		VerifyPeerCertificate: func(rawCerts [][]byte, _ [][]*x509.Certificate) error {
			state, err := verifyChain(rawCerts, name)
			if err != nil {
				return err
			}
			return checkPins(state, t.Pins)
		},
	}
	conn := tls.Client(raw, cfg)
	hsCtx, cancel := context.WithTimeout(ctx, TLSHandshakeTimeout)
	defer cancel()
	if err := conn.HandshakeContext(hsCtx); err != nil {
		_ = raw.Close()
		return nil, err
	}
	return conn, nil
}
