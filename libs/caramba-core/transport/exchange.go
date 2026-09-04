package transport

import (
	"context"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"golang.org/x/net/http2"
)

// ErrPinMismatch это отказ по SPKI пину: сертификат валиден по цепочке, но его
// открытый ключ не тот, который назвал подписанный документ.
var ErrPinMismatch = errors.New("transport: SPKI пин не совпал")

// NetExchange это сетевой исполнитель обмена. Один обмен это одно соединение:
// гигиена соединения 03-WIRE.md 11.2 считает бюджет на соединение, и
// переиспользование пула Go за нашей спиной сделало бы этот счёт ложью.
type NetExchange struct {
	// UserAgent уходит в каждый запрос.
	UserAgent string
}

// NewNetExchange создаёт исполнителя с User-Agent по умолчанию.
func NewNetExchange(ua string) *NetExchange {
	if ua == "" {
		ua = "caramba-core/1.0"
	}
	return &NetExchange{UserAgent: ua}
}

// Do выполняет один обмен по цели t.
func (e *NetExchange) Do(ctx context.Context, t Target, req *http.Request) (*http.Response, error) {
	if req.Header.Get("User-Agent") == "" {
		req.Header.Set("User-Agent", e.UserAgent)
	}
	if req.URL == nil {
		return nil, fmt.Errorf("%w: пустой URL запроса", ErrBadHostname)
	}
	// Прокси НЕ передаётся в http.Transport, даже когда ступень его несёт.
	// http.Transport зовёт DialTLSContext только "for non-proxied HTTPS
	// requests": при непустом Proxy он берёт TLSClientConfig, который здесь
	// пуст, и тогда на ступенях R4 и R5 молча исчезают SPKI пины, нижняя
	// граница TLS и явный ClientHello. Прокси поэтому набирается внутри
	// dialRawConn, а рукопожатие идёт тем же кодом, что и на прямой ступени.
	if t.Proxy != "" {
		if _, err := parseProxyURL(t.Proxy); err != nil {
			return nil, err
		}
	}

	// Один срок на всю попытку, 02-SPEC.md 8.5. Раньше его держал
	// http.Client.Timeout, и набор соединения в него не входил; теперь
	// соединение открывается здесь, поэтому срок считается от одной точки и
	// делится между набором и обменом.
	deadline := time.Now().Add(t.Rung.Timeout())

	// TLS соединение открывается ЗДЕСЬ, а не лениво внутри http.Transport,
	// потому что версия HTTP выбирается по результату ALPN, а он известен
	// только после рукопожатия. Ленивый набор оставлял бы выбор транспорту, а
	// транспорт его сделать не может (см. exchangeOverConn).
	var conn net.Conn
	if strings.EqualFold(req.URL.Scheme, "https") {
		dialCtx, cancel := context.WithDeadline(ctx, deadline)
		c, err := dialTLSConn(dialCtx, t, dialAddr(req.URL))
		cancel()
		if err != nil {
			return nil, err
		}
		conn = c
	}
	return e.exchangeOverConn(ctx, t, conn, req, deadline)
}

// exchangeOverConn ведёт обмен поверх уже открытого соединения conn. Пустой
// conn означает путь без TLS (только .onion, инвариант 8): там соединение
// наберёт сам http.Transport.
//
// Версия HTTP выбирается по ALPN, и выбирается ЗДЕСЬ, потому что
// http.Transport выбрать её в нашей конфигурации не может ПРИНЦИПИАЛЬНО, а не
// по недонастройке: состояние сессии он читает приведением типа возвращённого
// соединения к *crypto/tls.Conn, и только при удачном приведении заполняет
// tlsState и смотрит TLSNextProto. uTLS возвращает свой тип, приведение не
// проходит, tlsState остаётся пустым, и транспорт пишет в согласованное h2
// соединение запрос HTTP/1.1. Ответом приходит кадр SETTINGS, который парсер
// ответа читает как «malformed HTTP response».
func (e *NetExchange) exchangeOverConn(ctx context.Context, t Target, conn net.Conn,
	req *http.Request, deadline time.Time) (*http.Response, error) {

	rest := time.Until(deadline)
	if rest <= 0 {
		if conn != nil {
			_ = conn.Close()
		}
		return nil, context.DeadlineExceeded
	}
	if conn != nil {
		// Срок ставится на соединение, а не только на http.Client: у пути h2
		// клиента нет, а тело ответа читает уже вызывающий.
		_ = conn.SetDeadline(deadline)
		if negotiatedALPN(conn) == "h2" {
			return roundTripH2(conn, req)
		}
	}
	return e.roundTripH1(t, conn, req, rest)
}

// roundTripH1 ведёт обмен по HTTP/1.1. Соединение либо уже открыто (тогда оно
// отдаётся транспорту один раз), либо его наберёт сам транспорт.
func (e *NetExchange) roundTripH1(t Target, conn net.Conn, req *http.Request,
	rest time.Duration) (*http.Response, error) {

	tr := &http.Transport{
		// Переходы за нас не выполняются: правило одного перехода живёт в
		// readCSMResponse, вместе с проверкой хоста и схемы.
		DisableCompression:  true,
		DisableKeepAlives:   true,
		MaxConnsPerHost:     1,
		TLSHandshakeTimeout: TLSHandshakeTimeout,
		ForceAttemptHTTP2:   false,
	}
	tr.DialContext = func(ctx context.Context, network, addr string) (net.Conn, error) {
		return dialRawConn(ctx, t, addr)
	}
	var mu sync.Mutex
	ready := conn
	take := func() net.Conn {
		mu.Lock()
		defer mu.Unlock()
		c := ready
		ready = nil
		return c
	}
	tr.DialTLSContext = func(ctx context.Context, network, addr string) (net.Conn, error) {
		// Готовое соединение отдаётся ровно один раз. Если транспорт решит
		// набрать ещё, он наберёт по-настоящему, а не получит уже отданное:
		// молча вернуть его дважды значило бы отдать закрытый поток.
		if c := take(); c != nil {
			return c, nil
		}
		return dialTLSConn(ctx, t, addr)
	}

	cli := &http.Client{
		Transport: tr,
		Timeout:   rest,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			// followRedirects: false. Переход решает readCSMResponse.
			return http.ErrUseLastResponse
		},
	}
	resp, err := cli.Do(req)
	if c := take(); c != nil {
		// Транспорт до набора не дошёл: соединение открыли мы, закрывать его
		// тоже нам.
		_ = c.Close()
	}
	if err != nil {
		return nil, err
	}
	return resp, nil
}

// roundTripH2 ведёт обмен по HTTP/2 поверх уже установленного соединения.
//
// ClientConn создаётся напрямую, минуя пул http2.Transport: гигиена соединения
// 03-WIRE.md 11.2 считает бюджет НА СОЕДИНЕНИЕ, а пул мультиплексирует и живёт
// дольше обмена, и этот счёт стал бы ложью — ровно то, ради чего на пути
// HTTP/1.1 стоит DisableKeepAlives.
func roundTripH2(conn net.Conn, req *http.Request) (*http.Response, error) {
	tr := &http2.Transport{DisableCompression: true}
	cc, err := tr.NewClientConn(conn)
	if err != nil {
		_ = conn.Close()
		return nil, err
	}
	resp, err := cc.RoundTrip(req)
	if err != nil {
		_ = cc.Close()
		return nil, err
	}
	if resp.Body == nil {
		resp.Body = http.NoBody
	}
	// Закрытие привязано к телу: раньше него соединение закрыть нельзя, а
	// после него держать открытым нечего. Тело закрывает readCSMResponse.
	resp.Body = &h2Body{rc: resp.Body, cc: cc}
	return resp, nil
}

// h2Body закрывает соединение h2 вместе с телом ответа.
type h2Body struct {
	rc io.ReadCloser
	cc *http2.ClientConn
}

func (b *h2Body) Read(p []byte) (int, error) { return b.rc.Read(p) }

func (b *h2Body) Close() error {
	err := b.rc.Close()
	_ = b.cc.Close()
	return err
}

// negotiatedALPN отдаёт протокол, выбранный сервером в ALPN. Пустая строка
// означает «ALPN не было или не согласовалось», и тогда обмен идёт по
// HTTP/1.1: это же и есть поведение сборки без uTLS, где ClientHello ALPN
// вовсе не объявляет.
func negotiatedALPN(c net.Conn) string {
	if p, ok := utlsALPN(c); ok {
		return p
	}
	if tc, ok := c.(*tls.Conn); ok {
		return tc.ConnectionState().NegotiatedProtocol
	}
	return ""
}

// dialAddr отдаёт host:port цели набора. Порт берётся из URL, иначе
// подразумевается порт схемы.
func dialAddr(u *url.URL) string {
	port := u.Port()
	if port == "" {
		port = "443"
		if strings.EqualFold(u.Scheme, "http") {
			port = "80"
		}
	}
	return net.JoinHostPort(u.Hostname(), port)
}

// parseProxyURL принимает socks5://host:port, http://host:port и голый
// host:port (тогда подразумевается socks5).
func parseProxyURL(s string) (*url.URL, error) {
	s = strings.TrimSpace(s)
	if !strings.Contains(s, "://") {
		s = "socks5://" + s
	}
	u, err := url.Parse(s)
	if err != nil {
		return nil, fmt.Errorf("transport: адрес прокси %q: %w", s, err)
	}
	switch strings.ToLower(u.Scheme) {
	case "socks5", "socks5h", "http", "https":
		return u, nil
	default:
		return nil, fmt.Errorf("transport: схема прокси %q не поддерживается", u.Scheme)
	}
}

// serverNameFor возвращает имя, которое уходит в SNI и против которого
// проверяется сертификат. Проверка НЕ отключается и сертификат с IP-SAN не
// нужен, потому что проверяется именно это имя.
func serverNameFor(t Target) string {
	if t.SNI != "" {
		return t.SNI
	}
	return t.Host
}

// checkPins сверяет sha256 SubjectPublicKeyInfo ЛИСТОВОГО сертификата с
// подписанным набором пинов. Пустой набор означает "пинов нет", и тогда
// работает обычная проверка цепочки.
//
// Сравнивается ровно certs[0] и никогда остальная цепочка. x509.Verify
// принимает certs[1:] как ПУЛ промежуточных и молча игнорирует неиспользованные
// записи, поэтому обход цепочки означал бы вот что: обладатель любого публично
// доверенного сертификата на то же имя присылает
// [свой лист, настоящие промежуточные, легитимный запиненный сертификат
// довеском], цепочка сходится по certs[0], а совпадение находится на довеске.
// Пин, единственный контроль, который обязан пережить враждебное хранилище
// корней, не давал бы тогда ничего.
func checkPins(state tls.ConnectionState, pins [][]byte) error {
	if len(pins) == 0 {
		return nil
	}
	if len(state.PeerCertificates) == 0 {
		return fmt.Errorf("%w: цепочка пуста", ErrPinMismatch)
	}
	sum := sha256.Sum256(state.PeerCertificates[0].RawSubjectPublicKeyInfo)
	for _, p := range pins {
		if len(p) == len(sum) && equalBytes(p, sum[:]) {
			return nil
		}
	}
	return ErrPinMismatch
}

func equalBytes(a, b []byte) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// verifyChain выполняет обычную проверку цепочки против системного пула по
// имени name. Нужна там, где мы отключили встроенную проверку, чтобы добавить
// свою: отключить её и не заменить значило бы принять любой сертификат.
func verifyChain(rawCerts [][]byte, name string) (tls.ConnectionState, error) {
	certs := make([]*x509.Certificate, 0, len(rawCerts))
	for _, raw := range rawCerts {
		c, err := x509.ParseCertificate(raw)
		if err != nil {
			return tls.ConnectionState{}, err
		}
		certs = append(certs, c)
	}
	if len(certs) == 0 {
		return tls.ConnectionState{}, errors.New("transport: сервер не прислал сертификат")
	}
	inter := x509.NewCertPool()
	for _, c := range certs[1:] {
		inter.AddCert(c)
	}
	if _, err := certs[0].Verify(x509.VerifyOptions{
		DNSName:       name,
		Intermediates: inter,
		CurrentTime:   time.Now(),
	}); err != nil {
		return tls.ConnectionState{}, err
	}
	return tls.ConnectionState{PeerCertificates: certs}, nil
}
