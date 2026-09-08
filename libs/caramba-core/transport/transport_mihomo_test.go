//go:build mihomo

package transport

import (
	"context"
	"crypto/tls"
	"net"
	"testing"
	"time"
)

// ClientHello обязан предлагать ALPN так, как его предлагает браузер, за
// который мы себя выдаём: h2 первым, http/1.1 запасным.
//
// Это регрессия на «починку», которая обрезала ALPN до одного http/1.1, чтобы
// обмен не спотыкался о выбранный сервером h2. Она работала и стоила ровно
// того, ради чего uTLS в протоколе есть: ни один браузер не предлагает один
// http/1.1, и такое рукопожатие выделяется из потока по одному этому признаку.
// Правильное место для разбора h2 это обмен, а не ClientHello.
//
// ClientHello читается сервером ДО выбора сертификата, поэтому фикстуре
// сертификат не нужен: рукопожатие обязано провалиться, а список ALPN к этому
// моменту уже прочитан.
func TestClientHelloOffersBrowserALPN(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("слушатель: %v", err)
	}
	defer func() { _ = ln.Close() }()

	seen := make(chan []string, 64)
	go func() {
		for {
			c, err := ln.Accept()
			if err != nil {
				return
			}
			go func(c net.Conn) {
				defer func() { _ = c.Close() }()
				srv := tls.Server(c, &tls.Config{
					GetConfigForClient: func(hi *tls.ClientHelloInfo) (*tls.Config, error) {
						protos := append([]string(nil), hi.SupportedProtos...)
						select {
						case seen <- protos:
						default:
						}
						return nil, nil
					},
				})
				_ = srv.Handshake()
			}(c)
		}
	}()

	// Пресет выбирается на соединение случайно, поэтому дозвонов заведомо
	// больше, чем записей в наборе: обрезанный ALPN у одного из них не должен
	// пройти незамеченным.
	for i := 0; i < len(helloRoster)*4; i++ {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		conn, derr := dialTLSConn(ctx, Target{Rung: R1Direct, Host: "panel.example.net"}, ln.Addr().String())
		cancel()
		if derr == nil {
			_ = conn.Close()
			t.Fatal("сервер без сертификата не мог пройти проверку цепочки: фикстура сломана")
		}
		select {
		case protos := <-seen:
			if len(protos) < 2 || protos[0] != "h2" {
				t.Fatalf("ClientHello предложил ALPN %v: браузер так не делает, и такое рукопожатие выделяется само по себе", protos)
			}
		case <-time.After(10 * time.Second):
			t.Fatal("сервер не получил ClientHello")
		}
	}
}
