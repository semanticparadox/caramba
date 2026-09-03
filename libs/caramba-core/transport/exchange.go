package transport

import (
	"context"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"
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
	tr := &http.Transport{
		// Переходы за нас не выполняются: правило одного перехода живёт в
		// readCSMResponse, вместе с проверкой хоста и схемы.
		DisableCompression:  true,
		DisableKeepAlives:   true,
		MaxConnsPerHost:     1,
		TLSHandshakeTimeout: TLSHandshakeTimeout,
		ForceAttemptHTTP2:   false,
	}
	// tr.Proxy НАМЕРЕННО не задаётся, даже когда ступень несёт прокси.
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
	tr.DialContext = func(ctx context.Context, network, addr string) (net.Conn, error) {
		return dialRawConn(ctx, t, addr)
	}
	tr.DialTLSContext = func(ctx context.Context, network, addr string) (net.Conn, error) {
		return dialTLSConn(ctx, t, addr)
	}

	cli := &http.Client{
		Transport: tr,
		Timeout:   t.Rung.Timeout(),
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			// followRedirects: false. Переход решает readCSMResponse.
			return http.ErrUseLastResponse
		},
	}
	resp, err := cli.Do(req)
	if err != nil {
		return nil, err
	}
	return resp, nil
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
