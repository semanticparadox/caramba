package transport

import (
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
)

// Отказы уровня HTTP, все до разбора кадра.
var (
	// ErrContentEncoding это 03-WIRE.md 12.4: ответ CSM не имеет права нести
	// Content-Encoding. Сжатие делает размер на проводе функцией открытого
	// текста и сводит на нет дополнение. Ответ ОТВЕРГАЕТСЯ, а не
	// распаковывается, поэтому границы распаковки, которую можно ошибиться,
	// здесь просто нет.
	ErrContentEncoding = errors.New("transport: ответ CSM несёт Content-Encoding")
	// ErrBodyTooLarge это превышение потолка тела. Чтение идёт через
	// io.LimitReader на maxBody+1 и отвергается на переполнении, а не
	// буферизуется.
	ErrBodyTooLarge = errors.New("transport: тело ответа выше потолка")
	// ErrContentLength означает несогласованный или отрицательный
	// Content-Length.
	ErrContentLength = errors.New("transport: недопустимый Content-Length")
	// ErrCredentialOnSharedHost это отказ отправить запрос, несущий
	// предъявительский секрет, на ступень, подменяющую хост. Ступень
	// пропускается, а не деградирует: снять заголовок молча значило бы
	// потратить попытку на запрос, который панель всё равно отвергнет.
	ErrCredentialOnSharedHost = errors.New("transport: запрос с учётными данными не идёт на подменённый хост")
)

// hostSubstituted сообщает, что ступень отправляет запрос НЕ на закреплённый
// origin, а на хост из подписанного пула. R4 и R5 сюда не входят: они меняют
// маршрут, но хост назначения и TLS-сессия остаются с origin, поэтому прокси
// видит только шифротекст.
func hostSubstituted(r RungID) bool { return r == R2Mirrors || r == R3DoH }

// carriesCredential сообщает, что запрос несёт предъявительский секрет:
// сессионный токен аккаунта в Authorization, cookie, либо uuid подписки в
// пути. 03-WIRE.md 14.5 говорит про uuid прямо: он одновременно является
// uuid VLESS, паролем Trojan и uuid TUIC, и "putting the uuid in one publishes
// the tunnel credential to every host in the mirror pool". Локатор существует
// ровно для того, чтобы зеркала uuid не видели.
func carriesCredential(req *http.Request) bool {
	if req == nil {
		return false
	}
	if req.Header.Get("Authorization") != "" || req.Header.Get("Cookie") != "" ||
		req.Header.Get("Proxy-Authorization") != "" {
		return true
	}
	if req.URL == nil {
		return false
	}
	p := req.URL.Path
	// /sub/{uuid} это легаси-выборка конфигурации. Все пути CSM под /sub
	// перечислены поимённо, поэтому всё остальное под /sub несёт uuid.
	return strings.HasPrefix(p, "/sub/") && !isCSMPath(p)
}

// rewriteRequest готовит запрос под конкретную цель: подставляет хост ступени,
// проверяет схему по инварианту 8 и снимает всё, чего в запросе CSM быть не
// должно.
func rewriteRequest(req *http.Request, t Target) (*http.Request, error) {
	if req == nil || req.URL == nil {
		return nil, fmt.Errorf("%w: пустой запрос", ErrBadHostname)
	}
	if hostSubstituted(t.Rung) && carriesCredential(req) {
		return nil, fmt.Errorf("%w: ступень %s", ErrCredentialOnSharedHost, t.Rung.Name())
	}
	out := req.Clone(req.Context())
	u := *req.URL
	if t.Host != "" && t.Rung != R1Direct {
		// Ступени зеркал ходят на хост из подписанного пула. Путь приходит из
		// исходного запроса и уже проверен правилом 14.2.
		u.Host = t.Host
		u.Scheme = "https"
	}
	out.URL = &u
	out.Host = ""
	if err := CheckFetchURL(out.URL); err != nil {
		return nil, err
	}
	if err := CheckHostname(strings.ToLower(out.URL.Hostname())); err != nil && !IsOnion(out.URL.Hostname()) {
		return nil, err
	}
	// Запрос CSM никогда не просит сжатие: ответ, который его несёт, будет
	// отвергнут, так что просить его значит гарантированно потратить попытку.
	out.Header.Set("Accept-Encoding", "identity")
	return out, nil
}

// estimateHeaderBytes считает байты набора заголовков в проводной форме.
func estimateHeaderBytes(h http.Header) uint64 {
	if h == nil {
		return 0
	}
	n := uint64(0)
	for k, vs := range h {
		for _, v := range vs {
			n += uint64(len(k)) + 2 + uint64(len(v)) + 2
		}
	}
	return n + 2
}

// estimateRequestBytes считает строку запроса и заголовки, как они уйдут на
// провод. Тело запроса учитывается там, где оно есть.
func estimateRequestBytes(req *http.Request) uint64 {
	if req == nil || req.URL == nil {
		return 0
	}
	path := req.URL.RequestURI()
	n := uint64(len(req.Method)) + 1 + uint64(len(path)) + 1 + uint64(len("HTTP/1.1")) + 2
	n += uint64(len("Host: ")) + uint64(len(req.URL.Host)) + 2
	n += estimateHeaderBytes(req.Header)
	if req.ContentLength > 0 {
		n += uint64(req.ContentLength)
	}
	return n
}

// RedirectAllowed это единственный разрешённый переход: 3xx на настроенный
// subscription_domain. Он возвращается как ошибка намеренно, чтобы переход
// выполнял вызывающий, который ведёт учёт попыток и бюджета соединения.
type RedirectAllowed struct {
	To *url.URL
}

func (e *RedirectAllowed) Error() string {
	return "transport: разрешён один переход на " + e.To.String()
}

// readCSMResponse читает тело ответа под потолком и применяет транспортные
// правила, которые решаются до разбора кадра.
//
// Порядок здесь важен: сначала отказ по заголовкам и по переходу, потом чтение
// тела. Враждебный ответ не должен заставить нас выделить память по его
// собственному заявлению.
//
// csmFrame отделяет правила КАДРА от правил транспорта. Отказ по
// Content-Encoding это 03-WIRE.md 12.4, и он относится к путям, возящим кадры
// CSM: применять его к панельному JSON и к 18..25 КБ конфигурации подписки
// значит превращать nginx с gzip_static в жёсткий отказ там, где вчера всё
// работало. Потолок тела, разбор Content-Length и правило одного перехода
// остаются общими: это и есть то, чего у http.Client{Timeout: 30s} нет.
func readCSMResponse(resp *http.Response, maxBody uint64, reqURL *url.URL, subscriptionDomain string, csmFrame bool) ([]byte, int, http.Header, error) {
	if resp == nil {
		return nil, 0, nil, fmt.Errorf("transport: пустой ответ")
	}
	defer func() {
		if resp.Body != nil {
			_ = resp.Body.Close()
		}
	}()
	status := resp.StatusCode
	hdr := resp.Header

	if csmFrame {
		if ce := strings.TrimSpace(hdr.Get("Content-Encoding")); ce != "" && !strings.EqualFold(ce, "identity") {
			return nil, status, hdr, fmt.Errorf("%w: %q", ErrContentEncoding, ce)
		}
	}

	// Переход. Разрешён ровно один и только на настроенный subscription_domain;
	// всё остальное отвергается, включая переход на http.
	if status >= 300 && status < 400 {
		loc := hdr.Get("Location")
		if loc == "" {
			return nil, status, hdr, fmt.Errorf("%w: 3xx без Location", ErrRedirectRefused)
		}
		to, err := url.Parse(loc)
		if err != nil {
			return nil, status, hdr, fmt.Errorf("%w: %v", ErrRedirectRefused, err)
		}
		if !to.IsAbs() && reqURL != nil {
			to = reqURL.ResolveReference(to)
		}
		if err := CheckRedirect(reqURL, to, 0, subscriptionDomain); err != nil {
			return nil, status, hdr, err
		}
		// Переход разрешён, но выполняет его вызывающий: лестница ведёт учёт
		// попыток и бюджета, и молча следующий за Location транспорт этот учёт
		// обходит. Ровно один переход, и только на этот хост.
		return nil, status, hdr, &RedirectAllowed{To: to}
	}

	if cl := hdr.Get("Content-Length"); cl != "" {
		n, err := strconv.ParseUint(strings.TrimSpace(cl), 10, 64)
		if err != nil {
			return nil, status, hdr, fmt.Errorf("%w: %q", ErrContentLength, cl)
		}
		if n > maxBody {
			return nil, status, hdr, fmt.Errorf("%w: заявлено %d при потолке %d", ErrBodyTooLarge, n, maxBody)
		}
	}
	if resp.Body == nil {
		return nil, status, hdr, nil
	}
	// Потолок читается как maxBody+1: переполнение видно на первом лишнем
	// байте, и выделения по заявлению отвечающего не происходит.
	body, err := io.ReadAll(io.LimitReader(resp.Body, int64(maxBody)+1))
	if err != nil {
		return nil, status, hdr, err
	}
	if uint64(len(body)) > maxBody {
		return nil, status, hdr, fmt.Errorf("%w: больше %d байт", ErrBodyTooLarge, maxBody)
	}
	return body, status, hdr, nil
}
