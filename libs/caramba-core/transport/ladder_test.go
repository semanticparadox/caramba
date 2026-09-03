package transport

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net/http"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

// fakeExchange это подставной исполнитель обмена. Он записывает каждую цель,
// на которую лестница реально пошла, и это единственный способ доказать, что
// выключенная ступень не пробуется: проверять надо не результат, а факт
// попытки.
type fakeExchange struct {
	mu     sync.Mutex
	seen   []Target
	fn     func(t Target, req *http.Request) (*http.Response, error)
	bodies map[RungID][]byte
}

func newFakeExchange() *fakeExchange {
	return &fakeExchange{bodies: map[RungID][]byte{}}
}

func (f *fakeExchange) Do(ctx context.Context, t Target, req *http.Request) (*http.Response, error) {
	f.mu.Lock()
	f.seen = append(f.seen, t)
	fn := f.fn
	body, ok := f.bodies[t.Rung]
	f.mu.Unlock()
	if fn != nil {
		return fn(t, req)
	}
	if !ok {
		return nil, errors.New("подставной транспорт: ступень не отвечает")
	}
	return okResponse(body), nil
}

func (f *fakeExchange) rungs() []RungID {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]RungID, 0, len(f.seen))
	for _, t := range f.seen {
		out = append(out, t.Rung)
	}
	return out
}

func okResponse(body []byte) *http.Response {
	h := http.Header{}
	h.Set("Content-Type", "application/vnd.caramba.csm1")
	return &http.Response{
		StatusCode:    200,
		Header:        h,
		Body:          io.NopCloser(bytes.NewReader(body)),
		ContentLength: int64(len(body)),
	}
}

func newRequest(t *testing.T, url string) *http.Request {
	t.Helper()
	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		t.Fatalf("запрос: %v", err)
	}
	return req
}

func mirrorPool() []csm.Mirror {
	return []csm.Mirror{
		{H: "m1.example.net", ASN: 1, CC: "DE", Weight: 10},
		{H: "m2.example.net", ASN: 2, CC: "NL", Weight: 10, IP: []string{"198.51.100.7"}},
		{H: "m3.example.net", ASN: 3, CC: "FR", Weight: 10, IP: []string{"198.51.100.8"}},
		{H: "m4.example.net", ASN: 4, CC: "SE", Weight: 10},
	}
}

// enableNetworkRungs снимает причины недоступности, которые в бою приходят из
// каталога и настроек, чтобы тест мог говорить именно про порядок.
func enableNetworkRungs(l *Ladder) {
	l.SetReservePool(mirrorPool())
	l.mu.Lock()
	l.doh = []csm.DoHEntry{{H: "doh.example.net", Path: "/dns-query", IP: []string{"198.51.100.1"}}}
	delete(l.avail, R2Mirrors)
	delete(l.avail, R3DoH)
	l.mu.Unlock()
}

func TestLadderOrderR0First(t *testing.T) {
	ex := newFakeExchange()
	l := NewLadder(ex)
	l.SetRandSource(1)
	enableNetworkRungs(l)
	// Пользователь ставит R0 в конец. Лестница обязана всё равно поставить её
	// первой: читать проверенный документ с диска до открытия сокета это не
	// вопрос политики.
	if err := l.SetOrder([]RungID{R3DoH, R2Mirrors, R1Direct, R0Cached}); err != nil {
		t.Fatalf("порядок: %v", err)
	}
	l.SetCache(cacheFunc(func(*http.Request) ([]byte, bool) { return []byte("cached"), true }))

	resp, err := l.Do(context.Background(), newRequest(t, "https://panel.example.net/sub/k1"), DoOptions{
		Origin: "https://panel.example.net",
	})
	if err != nil {
		t.Fatalf("цикл: %v", err)
	}
	if resp.Rung != R0Cached || !resp.FromCache {
		t.Fatalf("ожидалась ступень R0, получено %v", resp.Rung)
	}
	if len(ex.rungs()) != 0 {
		t.Fatalf("сеть не должна была открываться, попытки: %v", ex.rungs())
	}
}

type cacheFunc func(*http.Request) ([]byte, bool)

func (c cacheFunc) Lookup(r *http.Request) ([]byte, bool) { return c(r) }

func TestLadderWalksInOrder(t *testing.T) {
	ex := newFakeExchange()
	l := NewLadder(ex)
	l.SetRandSource(7)
	enableNetworkRungs(l)
	// Отвечает только R3: значит лестница обязана пройти R1, потом R2 трижды,
	// потом дойти до R3.
	ex.bodies[R3DoH] = []byte("doc")

	resp, err := l.Do(context.Background(), newRequest(t, "https://panel.example.net/sub/k1"), DoOptions{
		Origin: "https://panel.example.net",
	})
	if err != nil {
		t.Fatalf("цикл: %v", err)
	}
	if resp.Rung != R3DoH {
		t.Fatalf("ожидалась R3, получено %v", resp.Rung)
	}
	got := ex.rungs()
	want := []RungID{R1Direct, R2Mirrors, R2Mirrors, R2Mirrors, R3DoH}
	if len(got) != len(want) {
		t.Fatalf("порядок попыток %v, ожидалось %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("порядок попыток %v, ожидалось %v", got, want)
		}
	}
}

// TestDisabledRungIsNeverTried это правило, у которого нет исключений.
// Выключенная ступень не пробуется, даже когда все остальные отказали и
// отвечает только она.
func TestDisabledRungIsNeverTried(t *testing.T) {
	ex := newFakeExchange()
	l := NewLadder(ex)
	l.SetRandSource(3)
	enableNetworkRungs(l)
	l.SetProxy("127.0.0.1:1080")
	// Отвечает ТОЛЬКО R5, и именно её пользователь выключил.
	ex.bodies[R5Proxy] = []byte("doc")
	if err := l.SetEnabled(R5Proxy, false); err != nil {
		t.Fatalf("выключение: %v", err)
	}

	_, err := l.Do(context.Background(), newRequest(t, "https://panel.example.net/sub/k1"), DoOptions{
		Origin: "https://panel.example.net",
	})
	if err == nil {
		t.Fatal("цикл обязан был провалиться: единственная отвечающая ступень выключена")
	}
	for _, r := range ex.rungs() {
		if r == R5Proxy {
			t.Fatal("выключенная ступень R5 была опробована")
		}
	}
	// И она обязана остаться ВИДИМОЙ в состоянии, а не исчезнуть.
	found := false
	for _, s := range l.State() {
		if s.Rung == R5Proxy {
			found = true
			if s.Enabled {
				t.Fatal("R5 показана включённой")
			}
			if s.Reason != ReasonUserDisabled {
				t.Fatalf("причина %q, ожидалось user_disabled", s.Reason)
			}
		}
	}
	if !found {
		t.Fatal("R5 пропала из состояния: скрытая ступень нарушает инвариант 17")
	}
}

// TestMandatoryRungsCannotBeDisabled: ступени 0 и 6 выключить нельзя.
func TestMandatoryRungsCannotBeDisabled(t *testing.T) {
	l := NewLadder(newFakeExchange())
	if err := l.SetEnabled(R0Cached, false); err == nil {
		t.Fatal("R0 удалось выключить")
	}
	if err := l.SetEnabled(R6OutOfBand, false); err == nil {
		t.Fatal("R6 удалось выключить")
	}
}

// TestOnionRungVisibleAndDisabled: ступени, для которой в репозитории нет
// компонента, положено быть видимой и выключенной с причиной, а не спрятанной.
func TestOnionRungVisibleAndDisabled(t *testing.T) {
	l := NewLadder(newFakeExchange())
	states := l.State()
	if len(states) != len(AllRungs()) {
		t.Fatalf("состояние перечисляет %d ступеней, скомпилировано %d", len(states), len(AllRungs()))
	}
	for _, s := range states {
		if s.Rung == R7Onion {
			if s.Enabled {
				t.Fatal("onion показана включённой")
			}
			if s.Reason != ReasonAppVersionUnsupported {
				t.Fatalf("причина onion %q", s.Reason)
			}
			return
		}
	}
	t.Fatal("ступень onion отсутствует в состоянии")
}

// TestByteBudgetForcesFreshConnection проверяет правило 03-WIRE.md 11.2 на
// умолчаниях: проекция считается по МАКСИМАЛЬНО возможному ответу, поэтому
// второй запрос на то же соединение не помещается и открывается новое.
func TestByteBudget(t *testing.T) {
	thr := DefaultThresholds()
	b := NewConnBudget(thr)
	if b.Bytes() != HandshakeDebitBytes || b.Packets() != HandshakeDebitPackets {
		t.Fatalf("соединение открылось без дебета рукопожатия: %d/%d", b.Bytes(), b.Packets())
	}
	const reqBytes = 320
	if !b.CanSend(reqBytes) {
		t.Fatalf("первый запрос не помещается: %d + %d + %d против %d", b.Bytes(), uint64(reqBytes), thr.RespMax, thr.ConnBytes)
	}
	b.ChargeRequest(reqBytes)
	b.ChargeResponse(1184)
	// 4320 + 320 + 4096 = 8736 > 8192.
	if b.CanSend(reqBytes) {
		t.Fatalf("второй запрос прошёл проекцию: %d + %d + %d против %d", b.Bytes(), uint64(reqBytes), thr.RespMax, thr.ConnBytes)
	}
}

func TestClampThresholds(t *testing.T) {
	// Враждебные значения зажимаются вниз.
	got, err := ClampThresholds(csm.Thresholds{ConnBytes: 65535, ConnPackets: 255, RespMax: 4096})
	if err != nil {
		t.Fatalf("зажим: %v", err)
	}
	if got.ConnBytes != ConnBytesCeiling {
		t.Fatalf("conn_bytes %d, ожидалось %d", got.ConnBytes, ConnBytesCeiling)
	}
	if got.ConnPackets != ConnPacketsCeiling {
		t.Fatalf("conn_packets %d, ожидалось %d", got.ConnPackets, ConnPacketsCeiling)
	}
	// Более низкое подписанное значение связывает.
	got, err = ClampThresholds(csm.Thresholds{ConnBytes: 6000, ConnPackets: 12, RespMax: 2048})
	if err != nil {
		t.Fatalf("зажим: %v", err)
	}
	if got.ConnBytes != 6000 || got.ConnPackets != 12 || got.RespMax != 2048 {
		t.Fatalf("подписанное значение ниже потолка не связало: %+v", got)
	}
	// resp_max выше 4096 это отказ КАТАЛОГА, а не зажим.
	if _, err := ClampThresholds(csm.Thresholds{RespMax: 49152}); !errors.Is(err, ErrRespMaxTooHigh) {
		t.Fatalf("resp_max 49152 не отверг каталог: %v", err)
	}
}

func TestClampExphAndTTL(t *testing.T) {
	if got := ClampExpH(0, 0); got != ExphFloor {
		t.Fatalf("exph 0 не поднялся до пола: %d", got)
	}
	if got := ClampExpH(0, 600); got != 600 {
		t.Fatalf("явный выбор пользователя перекрыт полом: %d", got)
	}
	ttl, jit := ClampTTL(300, 0)
	if ttl != TTLFloor {
		t.Fatalf("ttl 300 не поднялся до %d: %d", TTLFloor, ttl)
	}
	if jit < TTLMinJitterPercent {
		t.Fatalf("разброс %d ниже минимального %d", jit, TTLMinJitterPercent)
	}
}

// TestBudgetRefusalRecorded: при порогах, при которых запрос не помещается
// даже на свежем соединении, попытка не отправляется и записывается исход
// budget.
func TestBudgetRefusesImpossibleRequest(t *testing.T) {
	ex := newFakeExchange()
	ex.bodies[R1Direct] = []byte("doc")
	l := NewLadder(ex)
	l.SetThresholds(Thresholds{ConnBytes: HandshakeDebitBytes + 10, ConnPackets: 22, RespMax: 4096})
	_, err := l.Do(context.Background(), newRequest(t, "https://panel.example.net/sub/k1"), DoOptions{
		Origin: "https://panel.example.net",
	})
	if err == nil {
		t.Fatal("запрос вне бюджета был отправлен")
	}
	if len(ex.rungs()) != 0 {
		t.Fatalf("транспорт вызван вопреки бюджету: %v", ex.rungs())
	}
	var found bool
	for _, a := range l.History() {
		if a.Outcome == OutcomeBudget {
			found = true
		}
	}
	if !found {
		t.Fatal("отказ по бюджету не попал в историю попыток")
	}
}

func TestHTTPSchemeRefused(t *testing.T) {
	// Инвариант 8: http отвергается для любой выборки, кроме .onion.
	if err := CheckFetchURLString("http://panel.example.net/sub/k1"); !errors.Is(err, ErrSchemeNotTLS) {
		t.Fatalf("http принят: %v", err)
	}
	if err := CheckFetchURLString("https://panel.example.net/sub/k1"); err != nil {
		t.Fatalf("https отвергнут: %v", err)
	}
	// Единственное исключение это onion: адрес самоаутентифицируется.
	if err := CheckFetchURLString("http://abcdefghijklmnop.onion/sub/k1"); err != nil {
		t.Fatalf("onion по http отвергнут: %v", err)
	}
	if _, err := NormalizeOrigin("http://panel.example.net"); err == nil {
		t.Fatal("NormalizeOrigin принял http")
	}
}

func TestHTTPSchemeRefusedInsideLadder(t *testing.T) {
	ex := newFakeExchange()
	ex.bodies[R1Direct] = []byte("doc")
	l := NewLadder(ex)
	_, err := l.Do(context.Background(), newRequest(t, "http://panel.example.net/sub/k1"), DoOptions{
		Origin: "http://panel.example.net",
	})
	if err == nil {
		t.Fatal("лестница выполнила запрос по http")
	}
	if len(ex.rungs()) != 0 {
		t.Fatalf("транспорт вызван по http: %v", ex.rungs())
	}
}

func TestContentEncodingRefused(t *testing.T) {
	h := http.Header{}
	h.Set("Content-Encoding", "gzip")
	resp := &http.Response{StatusCode: 200, Header: h, Body: io.NopCloser(strings.NewReader("x"))}
	_, _, _, err := readCSMResponse(resp, 4096, nil, "", true)
	if !errors.Is(err, ErrContentEncoding) {
		t.Fatalf("Content-Encoding принят: %v", err)
	}
}

func TestBodyCeiling(t *testing.T) {
	body := bytes.Repeat([]byte{0}, 4097)
	resp := &http.Response{StatusCode: 200, Header: http.Header{}, Body: io.NopCloser(bytes.NewReader(body))}
	if _, _, _, err := readCSMResponse(resp, 4096, nil, "", true); !errors.Is(err, ErrBodyTooLarge) {
		t.Fatalf("тело 4097 байт принято при потолке 4096: %v", err)
	}
	chunk := bytes.Repeat([]byte{0}, 3585)
	resp = &http.Response{StatusCode: 200, Header: http.Header{}, Body: io.NopCloser(bytes.NewReader(chunk))}
	if _, _, _, err := readCSMResponse(resp, csm.ChunkRespMax, nil, "", true); !errors.Is(err, ErrBodyTooLarge) {
		t.Fatalf("фрагмент 3585 байт принят при потолке %d: %v", csm.ChunkRespMax, err)
	}
}

func TestRedirectPolicy(t *testing.T) {
	req := newRequest(t, "https://panel.example.net/sub/k1")
	h := http.Header{}
	h.Set("Location", "https://evil.example.org/sub/k1")
	resp := &http.Response{StatusCode: 302, Header: h, Body: io.NopCloser(strings.NewReader(""))}
	_, _, _, err := readCSMResponse(resp, 4096, req.URL, "sub.example.net", true)
	if !errors.Is(err, ErrRedirectRefused) {
		t.Fatalf("кросс-origin переход принят: %v", err)
	}
	h.Set("Location", "https://sub.example.net/sub/k1")
	resp = &http.Response{StatusCode: 302, Header: h, Body: io.NopCloser(strings.NewReader(""))}
	_, _, _, err = readCSMResponse(resp, 4096, req.URL, "sub.example.net", true)
	var hop *RedirectAllowed
	if !errors.As(err, &hop) {
		t.Fatalf("переход на subscription_domain не распознан: %v", err)
	}
	if hop.To.Host != "sub.example.net" {
		t.Fatalf("цель перехода %q", hop.To.Host)
	}
	// Переход на http отвергается даже на разрешённом хосте: инвариант 8 не
	// делает исключения для перехода.
	h.Set("Location", "http://sub.example.net/sub/k1")
	resp = &http.Response{StatusCode: 302, Header: h, Body: io.NopCloser(strings.NewReader(""))}
	if _, _, _, err = readCSMResponse(resp, 4096, req.URL, "sub.example.net", true); !errors.Is(err, ErrSchemeNotTLS) {
		t.Fatalf("переход на http принят: %v", err)
	}
}

// TestRedirectFollowedOnce: разрешённый переход выполняется ровно один раз,
// внутри той же попытки и с того же бюджета соединения.
func TestRedirectFollowedOnce(t *testing.T) {
	ex := newFakeExchange()
	hops := 0
	ex.fn = func(t2 Target, r *http.Request) (*http.Response, error) {
		hops++
		if hops == 1 {
			h := http.Header{}
			h.Set("Location", "https://sub.example.net/sub/k1")
			return &http.Response{StatusCode: 308, Header: h, Body: io.NopCloser(strings.NewReader(""))}, nil
		}
		if r.URL.Host != "sub.example.net" {
			t.Errorf("второй запрос ушёл на %q", r.URL.Host)
		}
		return okResponse([]byte("doc")), nil
	}
	l := NewLadder(ex)
	resp, err := l.Do(context.Background(), newRequest(t, "https://panel.example.net/sub/k1"), DoOptions{
		Origin: "https://panel.example.net", SubscriptionDomain: "sub.example.net",
	})
	if err != nil {
		t.Fatalf("переход не выполнен: %v", err)
	}
	if string(resp.Body) != "doc" {
		t.Fatalf("тело %q", resp.Body)
	}
	if hops != 2 {
		t.Fatalf("сделано %d обменов, ожидалось 2 (исходный плюс один переход)", hops)
	}
}

func TestCatalogLadderDefaultsRejected(t *testing.T) {
	l := NewLadder(newFakeExchange())
	// en без ступени 6 обязан отвергнуть каталог.
	err := l.ApplyCatalog(&csm.Catalog{Lad: &csm.Ladder{Ord: []uint64{0, 1, 2, 6}, En: []uint64{0, 1}}}, 0)
	if !errors.Is(err, ErrBadLadderDefaults) {
		t.Fatalf("каталог без ступени 6 в en принят: %v", err)
	}
	// Дубликат в ord тоже отвергается.
	err = l.ApplyCatalog(&csm.Catalog{Lad: &csm.Ladder{Ord: []uint64{0, 1, 1, 6}, En: []uint64{0, 6}}}, 0)
	if !errors.Is(err, ErrBadLadderDefaults) {
		t.Fatalf("дубликат в ord принят: %v", err)
	}
}

// TestUserChoiceSurvivesCatalog: подписанные умолчания не восстанавливают
// ступень, которую выключил пользователь.
func TestUserChoiceSurvivesCatalog(t *testing.T) {
	l := NewLadder(newFakeExchange())
	if err := l.SetEnabled(R2Mirrors, false); err != nil {
		t.Fatalf("выключение: %v", err)
	}
	cat := &csm.Catalog{
		Mir: mirrorPool(),
		Lad: &csm.Ladder{Ord: []uint64{0, 1, 2, 3, 6}, En: []uint64{0, 1, 2, 3, 6}},
	}
	if err := l.ApplyCatalog(cat, 1<<4); err != nil {
		t.Fatalf("каталог: %v", err)
	}
	for _, s := range l.State() {
		if s.Rung == R2Mirrors && s.Enabled {
			t.Fatal("каталог восстановил ступень, выключенную пользователем")
		}
	}
}

func TestBackoffGrowsAndResets(t *testing.T) {
	ex := newFakeExchange()
	l := NewLadder(ex)
	l.SetRandSource(11)
	now := time.Unix(1788307500, 0)
	l.SetClock(func() time.Time { return now })

	_, err := l.Do(context.Background(), newRequest(t, "https://panel.example.net/sub/k1"), DoOptions{
		Origin: "https://panel.example.net",
	})
	if err == nil {
		t.Fatal("цикл без отвечающих ступеней обязан провалиться")
	}
	d, at := l.Backoff()
	if d != 2*BackoffInitial {
		t.Fatalf("backoff %v, ожидалось %v", d, 2*BackoffInitial)
	}
	if at.IsZero() {
		t.Fatal("момент следующей попытки не выставлен")
	}
	// Второй цикл без Force обязан быть отклонён до открытия сокета.
	before := len(ex.rungs())
	if _, err := l.Do(context.Background(), newRequest(t, "https://panel.example.net/sub/k1"), DoOptions{
		Origin: "https://panel.example.net",
	}); !errors.Is(err, ErrNoRung) {
		t.Fatalf("backoff не удержал цикл: %v", err)
	}
	if len(ex.rungs()) != before {
		t.Fatal("backoff пропустил сетевую попытку")
	}
	// Обновление по инициативе пользователя обходит backoff ровно один раз.
	ex.bodies[R1Direct] = []byte("doc")
	if _, err := l.Do(context.Background(), newRequest(t, "https://panel.example.net/sub/k1"), DoOptions{
		Origin: "https://panel.example.net", Force: true,
	}); err != nil {
		t.Fatalf("принудительный цикл: %v", err)
	}
	if d, _ := l.Backoff(); d != BackoffInitial {
		t.Fatalf("успешный цикл не сбросил backoff: %v", d)
	}
}

func TestMirrorSelectionAvoidsSameASN(t *testing.T) {
	l := NewLadder(newFakeExchange())
	l.SetRandSource(42)
	pool := []csm.Mirror{
		{H: "a.example.net", ASN: 5, Weight: 1},
		{H: "b.example.net", ASN: 5, Weight: 1},
		{H: "c.example.net", ASN: 6, Weight: 1},
	}
	got := l.drawMirrors(pool, 3)
	seen := map[uint64]bool{}
	for _, m := range got {
		if seen[m.ASN] {
			t.Fatalf("две цели из одной asn %d в одном цикле", m.ASN)
		}
		seen[m.ASN] = true
	}
	if len(got) != 2 {
		t.Fatalf("выбрано %d целей, ожидалось 2 различных asn", len(got))
	}
}
