package transport

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"testing"
	"time"
)

// dialALPN открывает TLS соединение к тестовому серверу, предлагая protos, и
// возвращает уже установленное соединение: обмен ниже получает ровно то, что
// получает dialTLSConn в бою — соединение с УЖЕ выбранным протоколом.
func dialALPN(t *testing.T, srv *httptest.Server, protos []string) net.Conn {
	t.Helper()
	pool := x509.NewCertPool()
	pool.AddCert(srv.Certificate())
	raw, err := net.Dial("tcp", srv.Listener.Addr().String())
	if err != nil {
		t.Fatalf("tcp: %v", err)
	}
	c := tls.Client(raw, &tls.Config{
		ServerName: "example.com", // имя из сертификата httptest
		RootCAs:    pool,
		NextProtos: protos,
		MinVersion: tls.VersionTLS12,
	})
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := c.HandshakeContext(ctx); err != nil {
		_ = raw.Close()
		t.Fatalf("рукопожатие: %v", err)
	}
	return c
}

// Обмен обязан говорить на том протоколе, который выбрал сервер в ALPN.
//
// Подслучай h2 это регрессия на живой отказ: браузерный ClientHello предлагает
// h2 первым, сервер его выбирает, а обмен, умеющий только HTTP/1.1, пишет в
// такое соединение запрос HTTP/1.1 и получает в ответ кадр SETTINGS, который
// парсер ответа читает как «malformed HTTP response». Ступень R1 падала на
// этом за доли секунды на КАЖДОМ запросе к любому серверу с HTTP/2.
func TestExchangeSpeaksNegotiatedProtocol(t *testing.T) {
	h := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Seen-Proto", r.Proto)
		_, _ = io.WriteString(w, "ok")
	})
	cases := []struct {
		name      string
		http2     bool
		protos    []string
		wantMajor int
	}{
		{name: "h2", http2: true, protos: []string{"h2", "http/1.1"}, wantMajor: 2},
		{name: "http1.1", http2: false, protos: []string{"http/1.1"}, wantMajor: 1},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			srv := httptest.NewUnstartedServer(h)
			srv.EnableHTTP2 = tc.http2
			srv.StartTLS()
			defer srv.Close()

			conn := dialALPN(t, srv, tc.protos)
			if got := negotiatedALPN(conn); got != tc.protos[0] {
				t.Fatalf("сервер выбрал ALPN %q вместо %q: фикстура проверяет не то, ради чего написана", got, tc.protos[0])
			}
			req, err := http.NewRequest(http.MethodGet, srv.URL+"/sub/k1", nil)
			if err != nil {
				t.Fatalf("запрос: %v", err)
			}
			e := NewNetExchange("")
			resp, err := e.exchangeOverConn(context.Background(),
				Target{Rung: R1Direct, Host: "example.com"}, conn, req, time.Now().Add(10*time.Second))
			if err != nil {
				t.Fatalf("обмен по %s: %v", tc.protos[0], err)
			}
			defer func() { _ = resp.Body.Close() }()
			body, err := io.ReadAll(resp.Body)
			if err != nil {
				t.Fatalf("тело: %v", err)
			}
			if resp.ProtoMajor != tc.wantMajor {
				t.Fatalf("ответ по %s, ожидался HTTP/%d.x", resp.Proto, tc.wantMajor)
			}
			if seen := resp.Header.Get("X-Seen-Proto"); seen == "" {
				t.Fatal("сервер не увидел запрос")
			}
			if string(body) != "ok" {
				t.Fatalf("тело %q", body)
			}
		})
	}
}

// Соединение h2 закрывается вместе с телом ответа: гигиена 03-WIRE.md 11.2
// считает бюджет на соединение, и переживший обмен мультиплексор сделал бы
// этот счёт ложью.
func TestH2ConnClosedWithBody(t *testing.T) {
	srv := httptest.NewUnstartedServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.WriteString(w, "ok")
	}))
	srv.EnableHTTP2 = true
	srv.StartTLS()
	defer srv.Close()

	conn := dialALPN(t, srv, []string{"h2", "http/1.1"})
	req, err := http.NewRequest(http.MethodGet, srv.URL+"/sub/k1", nil)
	if err != nil {
		t.Fatalf("запрос: %v", err)
	}
	e := NewNetExchange("")
	resp, err := e.exchangeOverConn(context.Background(),
		Target{Rung: R1Direct, Host: "example.com"}, conn, req, time.Now().Add(10*time.Second))
	if err != nil {
		t.Fatalf("обмен: %v", err)
	}
	if _, err := io.ReadAll(resp.Body); err != nil {
		t.Fatalf("тело: %v", err)
	}
	if err := resp.Body.Close(); err != nil {
		t.Fatalf("закрытие тела: %v", err)
	}
	// Соединение закрыто, значит запись в него больше не проходит.
	_ = conn.SetWriteDeadline(time.Now().Add(2 * time.Second))
	if _, err := conn.Write([]byte{0}); err == nil {
		t.Fatal("соединение h2 пережило закрытие тела")
	}
}

// Живая проверка против настоящего сервера с HTTP/2. По умолчанию пропускается:
// в обычном прогоне сети нет. Инвариант проверяется в ОБЕИХ сборках, потому
// что ALPN спрашивается у соединения, а не предполагается: без uTLS ClientHello
// ALPN не объявляет, сервер выбирает http/1.1, и ответ обязан быть HTTP/1.1.
//
//	CARAMBA_LIVE=1 go test -tags mihomo -run TestLivePanelALPN ./transport
func TestLivePanelALPN(t *testing.T) {
	host := os.Getenv("CARAMBA_LIVE_HOST")
	if os.Getenv("CARAMBA_LIVE") == "" {
		t.Skip("живая сеть: CARAMBA_LIVE=1")
	}
	if host == "" {
		host = "panel.exarobot.top"
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	tgt := Target{Rung: R1Direct, Host: host}
	conn, err := dialTLSConn(ctx, tgt, host+":443")
	if err != nil {
		t.Fatalf("рукопожатие с %s: %v", host, err)
	}
	alpn := negotiatedALPN(conn)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://"+host+"/", nil)
	if err != nil {
		t.Fatalf("запрос: %v", err)
	}
	e := NewNetExchange("")
	resp, err := e.exchangeOverConn(ctx, tgt, conn, req, time.Now().Add(15*time.Second))
	if err != nil {
		t.Fatalf("обмен при ALPN %q: %v", alpn, err)
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		t.Fatalf("тело: %v", err)
	}
	want := 1
	if alpn == "h2" {
		want = 2
	}
	if resp.ProtoMajor != want {
		t.Fatalf("ALPN %q, а ответ %s", alpn, resp.Proto)
	}
	t.Logf("%s: ALPN %q, ответ %s, код %d, тело %d байт", host, alpn, resp.Proto, resp.StatusCode, len(body))
}

// Живая выборка подписки целиком через лестницу: R1 к панели, её безусловный
// 308 на subscription_domain, переход под именем ЦЕЛИ перехода и тело
// конфигурации. Оба живых отказа лежали на этом пути, и по отдельности ни один
// из них здесь не виден: h2 роняет первый запрос, а имя панели в SNI роняет
// переход. Адрес подписки берётся из окружения: это предъявительский секрет,
// и в файле ему не место.
//
//	CARAMBA_LIVE=1 CARAMBA_LIVE_SUB=https://panel.../sub/<uuid> \
//	  go test -tags mihomo -run TestLiveSubscriptionThroughLadder ./transport
func TestLiveSubscriptionThroughLadder(t *testing.T) {
	sub := os.Getenv("CARAMBA_LIVE_SUB")
	if os.Getenv("CARAMBA_LIVE") == "" || sub == "" {
		t.Skip("живая сеть: CARAMBA_LIVE=1 и CARAMBA_LIVE_SUB=<адрес подписки>")
	}
	subDomain := os.Getenv("CARAMBA_LIVE_SUB_DOMAIN")
	if subDomain == "" {
		subDomain = "app.exarobot.top"
	}
	u, err := url.Parse(sub)
	if err != nil {
		t.Fatalf("адрес подписки: %v", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, sub, nil)
	if err != nil {
		t.Fatalf("запрос: %v", err)
	}
	l := NewLadder(NewNetExchange("caramba-core/1.0"))
	resp, err := l.Do(ctx, req, DoOptions{
		Origin:             u.Scheme + "://" + u.Host,
		SubscriptionDomain: subDomain,
		MaxBody:            LegacyBodyMax,
		Force:              true,
	})
	if err != nil {
		t.Fatalf("выборка подписки: %v", err)
	}
	if resp.Status != http.StatusOK {
		t.Fatalf("код состояния %d", resp.Status)
	}
	if len(resp.Body) == 0 {
		t.Fatal("тело конфигурации пусто")
	}
	t.Logf("подписка: код %d, тело %d байт", resp.Status, len(resp.Body))
}
