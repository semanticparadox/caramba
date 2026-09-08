package transport

import (
	"bytes"
	"io"
	"net/http"
	"strings"
	"sync"
)

// LegacyBodyMax это потолок тела для НЕ CSM путей: конфигурация mihomo с
// панели это 18..25 КБ, и потолок кадра CSM к ней не применим.
//
// Он всё равно конечен: неограниченный io.ReadAll на ответе, который выбирает
// враждебная сторона, это выделение памяти по её заявлению, и именно его
// требуется убрать.
const LegacyBodyMax uint64 = 4 << 20

// Doer это HTTPDoer поверх лестницы. Он и есть тот единственный шов, который
// api.NewCore и api.SetPanelURL обязаны передать в auth.NewPanelClient и
// subscription.NewClient: лестница реализована один раз, в Go, и оба места
// её конструирования обязаны ей пользоваться.
//
// Забыть SetPanelURL особенно дорого: перерегистрированный арендатор молча
// вернулся бы к собственному ClientHello Go, а найдено это было бы через
// полгода.
type Doer struct {
	mu        sync.Mutex
	ladder    *Ladder
	origin    string
	subDomain string
}

// NewDoer создаёт HTTPDoer поверх лестницы.
func NewDoer(l *Ladder, origin, subscriptionDomain string) *Doer {
	return &Doer{ladder: l, origin: strings.TrimRight(origin, "/"), subDomain: subscriptionDomain}
}

// SetOrigin меняет закреплённый origin. Вызывается из api.SetPanelURL.
func (d *Doer) SetOrigin(origin, subscriptionDomain string) {
	d.mu.Lock()
	defer d.mu.Unlock()
	d.origin = strings.TrimRight(origin, "/")
	if subscriptionDomain != "" {
		d.subDomain = subscriptionDomain
	}
}

// Ladder отдаёт лестницу.
func (d *Doer) Ladder() *Ladder { return d.ladder }

// isCSMPath сообщает, что путь несёт кадр CSM и живёт под потолком resp_max.
func isCSMPath(p string) bool {
	switch {
	case p == "/sub/k1",
		strings.HasPrefix(p, "/sub/m1/"),
		strings.HasPrefix(p, "/sub/c1/"),
		strings.HasPrefix(p, "/sub/r1/"),
		strings.HasPrefix(p, "/sub/b1/"),
		strings.HasPrefix(p, "/api/v2/app/csm/"),
		p == PathPreferences:
		return true
	}
	return false
}

// Do реализует auth.HTTPDoer и subscription.HTTPDoer.
//
// Проверки кадра здесь нет намеренно: панельный и подписочный клиенты возят
// JSON и YAML, а не документы CSM. Что здесь есть, так это лестница, правило
// http, потолок тела и учёт бюджета соединения, то есть ровно то, чего у
// http.Client{Timeout: 30s} без транспорта нет.
func (d *Doer) Do(req *http.Request) (*http.Response, error) {
	d.mu.Lock()
	origin, sub := d.origin, d.subDomain
	d.mu.Unlock()

	if req.URL != nil && req.URL.IsAbs() {
		if err := CheckFetchURL(req.URL); err != nil {
			return nil, err
		}
	}
	maxBody := LegacyBodyMax
	if req.URL != nil && isCSMPath(req.URL.Path) {
		maxBody = 0 // ноль означает thr.resp_max лестницы
	}
	if origin == "" && req.URL != nil {
		origin = req.URL.Scheme + "://" + req.URL.Host
	}

	resp, err := d.ladder.Do(req.Context(), req, DoOptions{
		Origin:             origin,
		SubscriptionDomain: sub,
		MaxBody:            maxBody,
		// Force: вызовы панельного и подписочного клиентов инициированы
		// пользователем (вход, обновление токена, выборка конфига), а backoff
		// раздела 8.7 гасит цикл выборки МАНИФЕСТА, а не действия человека.
		// Обновление по инициативе пользователя обходит backoff ровно один раз
		// и счётчик не сбрасывает.
		Force: true,
	})
	if err != nil {
		return nil, err
	}
	hdr := resp.Header
	if hdr == nil {
		hdr = http.Header{}
	}
	return &http.Response{
		Status:        http.StatusText(resp.Status),
		StatusCode:    resp.Status,
		Proto:         "HTTP/1.1",
		ProtoMajor:    1,
		ProtoMinor:    1,
		Header:        hdr,
		Body:          io.NopCloser(bytes.NewReader(resp.Body)),
		ContentLength: int64(len(resp.Body)),
		Request:       req,
	}, nil
}
