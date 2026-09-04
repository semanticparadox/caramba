package transport

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"strings"
	"sync"
	"testing"
	"time"
)

// Собственные подставки, а не общие из ladder_test.go: эти тесты доказывают
// утверждение про ПОРЯДОК ступеней, и они не должны ломаться от того, что
// соседний файл переименовал свой помощник.

type torTestExchange struct {
	mu   sync.Mutex
	seen []Target
	// answer это ступень, которая отвечает документом. Остальные отказывают,
	// как отказала бы сеть.
	answer RungID
}

func (e *torTestExchange) Do(_ context.Context, t Target, _ *http.Request) (*http.Response, error) {
	e.mu.Lock()
	e.seen = append(e.seen, t)
	answer := e.answer
	e.mu.Unlock()
	if t.Rung != answer {
		return nil, errors.New("подставной транспорт: ступень не отвечает")
	}
	h := http.Header{}
	h.Set("Content-Type", "application/vnd.caramba.csm1")
	body := []byte("doc")
	return &http.Response{
		StatusCode:    200,
		Header:        h,
		Body:          io.NopCloser(bytes.NewReader(body)),
		ContentLength: int64(len(body)),
	}, nil
}

func (e *torTestExchange) targets() []Target {
	e.mu.Lock()
	defer e.mu.Unlock()
	return append([]Target(nil), e.seen...)
}

func (e *torTestExchange) rungs() []RungID {
	out := []RungID{}
	for _, t := range e.targets() {
		out = append(out, t.Rung)
	}
	return out
}

func torRequest(t *testing.T) *http.Request {
	t.Helper()
	req, err := http.NewRequest(http.MethodGet, "https://panel.example.net/sub/k1", nil)
	if err != nil {
		t.Fatalf("запрос: %v", err)
	}
	return req
}

// torFallbackForTest даёт держателя, который никуда не ходит: проба
// подставная, часы настоящие, платформа обычная.
func torFallbackForTest(probe func(context.Context, string) error) *TorFallback {
	tf := NewTorFallback()
	tf.SetEndpoints([]string{TorSOCKSOrbot})
	tf.SetProbe(probe)
	tf.SetPlatform("android")
	return tf
}

func torProbeOK(context.Context, string) error { return nil }

func torProbeAbsent(_ context.Context, addr string) error {
	return errors.New("connect " + addr + ": connection refused")
}

// TestTorFallbackIsTheLastAutomaticRung это главное утверждение задачи.
//
// Резервный путь через чужое приложение стоит дороже всех остальных, поэтому
// он обязан пробоваться ПОСЛЕ каждого пути оператора и никогда раньше.
func TestTorFallbackIsTheLastAutomaticRung(t *testing.T) {
	ex := &torTestExchange{answer: R5Proxy}
	l := NewLadder(ex)
	if err := l.SetEnabled(R5Proxy, true); err != nil {
		t.Fatalf("включение R5: %v", err)
	}
	tf := torFallbackForTest(torProbeOK)
	st := tf.Ensure(context.Background(), l, "")
	if st.State != TorReady || st.Addr != TorSOCKSOrbot {
		t.Fatalf("состояние Tor %+v, ожидалось ready на %s", st, TorSOCKSOrbot)
	}

	resp, err := l.Do(context.Background(), torRequest(t), DoOptions{Origin: "https://panel.example.net"})
	if err != nil {
		t.Fatalf("цикл: %v", err)
	}
	if resp.Rung != R5Proxy {
		t.Fatalf("документ принесла ступень %s, ожидалась proxy", resp.Rung.Name())
	}
	order := ex.rungs()
	if len(order) < 2 {
		t.Fatalf("ступеней опробовано %v, ожидалась как минимум прямая перед резервной", order)
	}
	if order[0] != R1Direct {
		t.Fatalf("первой опробована %s, ожидалась direct", order[0].Name())
	}
	if order[len(order)-1] != R5Proxy {
		t.Fatalf("последней опробована %s, ожидалась proxy", order[len(order)-1].Name())
	}
	for _, r := range order[:len(order)-1] {
		if r == R5Proxy {
			t.Fatal("резервный путь опробован раньше, чем кончились обычные ступени")
		}
	}
	// И адрес в цели именно тот, который нашла проба: без этого "ступень
	// сработала" ничего не говорит о том, куда она пошла.
	last := ex.targets()[len(order)-1]
	if last.Proxy != TorSOCKSOrbot {
		t.Fatalf("R5 пошла через %q, ожидался %q", last.Proxy, TorSOCKSOrbot)
	}
}

// TestTorFallbackNotTriedWhenNormalPathWorks: обычный путь не платит за
// резервный ничего.
func TestTorFallbackNotTriedWhenNormalPathWorks(t *testing.T) {
	ex := &torTestExchange{answer: R1Direct}
	l := NewLadder(ex)
	if err := l.SetEnabled(R5Proxy, true); err != nil {
		t.Fatalf("включение R5: %v", err)
	}
	tf := torFallbackForTest(torProbeOK)
	tf.Ensure(context.Background(), l, "")

	if _, err := l.Do(context.Background(), torRequest(t), DoOptions{Origin: "https://panel.example.net"}); err != nil {
		t.Fatalf("цикл: %v", err)
	}
	for _, r := range ex.rungs() {
		if r == R5Proxy {
			t.Fatal("резервный путь опробован, хотя прямая ступень ответила")
		}
	}
}

// TestTorFallbackAbsentIsSaidOutLoud: когда Tor не найден, ступень остаётся
// ВИДИМОЙ и ненастроенной, причина названа, и попытки через прокси не было.
func TestTorFallbackAbsentIsSaidOutLoud(t *testing.T) {
	ex := &torTestExchange{answer: R5Proxy}
	l := NewLadder(ex)
	if err := l.SetEnabled(R5Proxy, true); err != nil {
		t.Fatalf("включение R5: %v", err)
	}
	tf := torFallbackForTest(torProbeAbsent)
	st := tf.Ensure(context.Background(), l, "")
	if st.State != TorAbsent {
		t.Fatalf("состояние Tor %q, ожидалось absent", st.State)
	}
	if st.Addr != "" {
		t.Fatalf("не найдя Tor, держатель назвал адрес %q", st.Addr)
	}
	if !strings.Contains(st.Detail, TorSOCKSOrbot) {
		t.Fatalf("причина %q не называет адрес, который проверяли", st.Detail)
	}
	if st.CheckedAt == 0 {
		t.Fatal("момент пробы не записан: экран не сможет отличить «не нашли» от «не искали»")
	}

	if _, err := l.Do(context.Background(), torRequest(t), DoOptions{Origin: "https://panel.example.net"}); err == nil {
		t.Fatal("цикл обязан был провалиться: отвечает только ненастроенная ступень")
	}
	for _, r := range ex.rungs() {
		if r == R5Proxy {
			t.Fatal("ступень без адреса всё-таки пошла в сеть")
		}
	}
	found := false
	for _, s := range l.State() {
		if s.Rung != R5Proxy {
			continue
		}
		found = true
		if s.Enabled {
			t.Fatal("R5 показана включённой, хотя адреса у неё нет")
		}
		if s.Reason != ReasonNotConfigured {
			t.Fatalf("причина %q, ожидалось not_configured", s.Reason)
		}
	}
	if !found {
		t.Fatal("R5 пропала из состояния: скрытая ступень нарушает инвариант 17")
	}
}

// TestTorFallbackNeverProbesDisabledRung: выключенная ступень не пробуется, и
// это относится и к пробе локального порта.
func TestTorFallbackNeverProbesDisabledRung(t *testing.T) {
	l := NewLadder(&torTestExchange{answer: R1Direct})
	probed := false
	tf := torFallbackForTest(func(context.Context, string) error {
		probed = true
		return nil
	})
	st := tf.Ensure(context.Background(), l, "")
	if probed {
		t.Fatal("проба ушла на устройство пользователя, который резервный путь не включал")
	}
	if st.State != TorUnknown {
		t.Fatalf("состояние %q, ожидалось unknown", st.State)
	}
}

// TestTorFallbackYieldsToUserProxy: свой прокси человека не подменяется, и
// ради него даже не ищется Tor.
func TestTorFallbackYieldsToUserProxy(t *testing.T) {
	l := NewLadder(&torTestExchange{answer: R1Direct})
	if err := l.SetEnabled(R5Proxy, true); err != nil {
		t.Fatalf("включение R5: %v", err)
	}
	probed := false
	tf := torFallbackForTest(func(context.Context, string) error {
		probed = true
		return nil
	})
	st := tf.Ensure(context.Background(), l, "socks5://127.0.0.1:1080")
	if probed {
		t.Fatal("Tor искали, хотя ступень занята прокси пользователя")
	}
	if st.State != TorSuperseded {
		t.Fatalf("состояние %q, ожидалось superseded", st.State)
	}
	if got := l.proxyAddr(); got != "socks5://127.0.0.1:1080" {
		t.Fatalf("в ступени стоит %q, ожидался прокси пользователя", got)
	}
}

// TestTorFallbackDoesNotClearUserProxy: сняв свой адрес, держатель не имеет
// права снять чужой.
func TestTorFallbackDoesNotClearUserProxy(t *testing.T) {
	l := NewLadder(&torTestExchange{answer: R1Direct})
	l.SetProxy("socks5://127.0.0.1:1080")
	tf := torFallbackForTest(torProbeOK)
	// R5 выключена, значит держатель уходит по ветке release.
	tf.Ensure(context.Background(), l, "")
	if got := l.proxyAddr(); got != "socks5://127.0.0.1:1080" {
		t.Fatalf("прокси пользователя стёрт, в ступени %q", got)
	}
}

// TestTorFallbackReleasesItsOwnAddress: выключив R5, пользователь снимает и
// найденный нами адрес, иначе ступень осталась бы настроенной невидимо для
// него.
func TestTorFallbackReleasesItsOwnAddress(t *testing.T) {
	l := NewLadder(&torTestExchange{answer: R1Direct})
	if err := l.SetEnabled(R5Proxy, true); err != nil {
		t.Fatalf("включение R5: %v", err)
	}
	tf := torFallbackForTest(torProbeOK)
	tf.Ensure(context.Background(), l, "")
	if l.proxyAddr() != TorSOCKSOrbot {
		t.Fatalf("адрес не подставлен, в ступени %q", l.proxyAddr())
	}
	if err := l.SetEnabled(R5Proxy, false); err != nil {
		t.Fatalf("выключение R5: %v", err)
	}
	tf.Ensure(context.Background(), l, "")
	if got := l.proxyAddr(); got != "" {
		t.Fatalf("адрес остался в выключенной ступени: %q", got)
	}
}

// TestTorFallbackUnsupportedPlatform: то, чего сборка не умеет, называется
// своим именем, а не молчит.
func TestTorFallbackUnsupportedPlatform(t *testing.T) {
	l := NewLadder(&torTestExchange{answer: R1Direct})
	if err := l.SetEnabled(R5Proxy, true); err != nil {
		t.Fatalf("включение R5: %v", err)
	}
	probed := false
	tf := torFallbackForTest(func(context.Context, string) error {
		probed = true
		return nil
	})
	tf.SetPlatform("ios")
	st := tf.Ensure(context.Background(), l, "")
	if probed {
		t.Fatal("проба ушла на платформе, где слушателя на петле быть не может")
	}
	if st.State != TorUnsupported {
		t.Fatalf("состояние %q, ожидалось unsupported", st.State)
	}
	if st.Detail == "" {
		t.Fatal("причина не названа")
	}
}

// TestTorFallbackReusesFreshAnswer: экран транспортов опрашивает ядро раз в три
// секунды, и проба не имеет права уходить на каждый опрос.
func TestTorFallbackReusesFreshAnswer(t *testing.T) {
	l := NewLadder(&torTestExchange{answer: R1Direct})
	if err := l.SetEnabled(R5Proxy, true); err != nil {
		t.Fatalf("включение R5: %v", err)
	}
	probes := 0
	tf := torFallbackForTest(func(context.Context, string) error {
		probes++
		return nil
	})
	now := time.Unix(1_800_000_000, 0)
	tf.SetClock(func() time.Time { return now })
	tf.Ensure(context.Background(), l, "")
	tf.Ensure(context.Background(), l, "")
	if probes != 1 {
		t.Fatalf("проб %d, ожидалась одна на срок годности ответа", probes)
	}
	now = now.Add(TorStatusTTL + time.Second)
	tf.Ensure(context.Background(), l, "")
	if probes != 2 {
		t.Fatalf("проб %d: протухший ответ не перепроверен", probes)
	}
}

// TestProbeSOCKS5 разбирает три ответа, которые реально встречаются на 9050.
func TestProbeSOCKS5(t *testing.T) {
	cases := []struct {
		name  string
		reply []byte
		ok    bool
		want  string
	}{
		{name: "без аутентификации", reply: []byte{0x05, 0x00}, ok: true},
		{name: "требует пароль", reply: []byte{0x05, 0x02}, want: "логин и пароль"},
		{name: "нет подходящего метода", reply: []byte{0x05, 0xff}, want: "без аутентификации"},
		{name: "не socks5", reply: []byte{'H', 'T'}, want: "не SOCKS5"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			ln, err := net.Listen("tcp", "127.0.0.1:0")
			if err != nil {
				t.Fatalf("слушатель: %v", err)
			}
			defer func() { _ = ln.Close() }()
			go func() {
				c, aerr := ln.Accept()
				if aerr != nil {
					return
				}
				defer func() { _ = c.Close() }()
				var greet [3]byte
				if _, rerr := io.ReadFull(c, greet[:]); rerr != nil {
					return
				}
				_, _ = c.Write(tc.reply)
			}()
			err = ProbeSOCKS5(context.Background(), ln.Addr().String())
			if tc.ok {
				if err != nil {
					t.Fatalf("проба отвергла живой SOCKS5: %v", err)
				}
				return
			}
			if err == nil {
				t.Fatal("проба приняла порт, который SOCKS5 без пароля не отдаёт")
			}
			if !errors.Is(err, ErrTorProbe) {
				t.Fatalf("ошибка %v не относится к пробе", err)
			}
			if !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("причина %q не содержит %q", err.Error(), tc.want)
			}
		})
	}
}

// TestProbeSOCKS5DeadPort: закрытый порт это отказ, а не зависание.
func TestProbeSOCKS5DeadPort(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("слушатель: %v", err)
	}
	addr := ln.Addr().String()
	_ = ln.Close()
	if err := ProbeSOCKS5(context.Background(), addr); err == nil {
		t.Fatal("проба сочла закрытый порт живым")
	}
}

// TestTorFallbackDoesNotCacheInterruptedProbe: оборванная вызывающим проба это
// не ответ про Tor, и запоминать её как "не найден" нельзя.
func TestTorFallbackDoesNotCacheInterruptedProbe(t *testing.T) {
	l := NewLadder(&torTestExchange{answer: R1Direct})
	if err := l.SetEnabled(R5Proxy, true); err != nil {
		t.Fatalf("включение R5: %v", err)
	}
	tf := torFallbackForTest(func(ctx context.Context, _ string) error {
		return ctx.Err()
	})
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	st := tf.Ensure(ctx, l, "")
	if st.State == TorAbsent {
		t.Fatal("оборванная проба записана как «Tor не найден»")
	}
	if st.CheckedAt != 0 {
		t.Fatal("момент пробы записан, хотя пробы не было")
	}

	// Следующая попытка обязана сходить заново, а не читать протухший вывод.
	probes := 0
	tf.SetProbe(func(context.Context, string) error {
		probes++
		return nil
	})
	if got := tf.Ensure(context.Background(), l, ""); got.State != TorReady {
		t.Fatalf("состояние %q, ожидалось ready", got.State)
	}
	if probes != 1 {
		t.Fatalf("проб %d, ожидалась одна", probes)
	}
}
