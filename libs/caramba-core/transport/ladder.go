package transport

import (
	"context"
	"errors"
	"fmt"
	"math/rand"
	"net/http"
	"net/url"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

// Отказы лестницы.
var (
	// ErrNoRung означает, что ни одна включённая ступень не дала ответа за
	// цикл. Профиль остаётся на кешированных документах и уходит в backoff.
	ErrNoRung = errors.New("transport: ни одна включённая ступень не вернула ответ")
	// ErrCycleBudget означает, что цикл превысил CYCLE_BUDGET и обязан был
	// бросить оставшиеся ступени. Продлевать цикл нельзя.
	ErrCycleBudget = errors.New("transport: превышен бюджет цикла")
	// ErrBadLadderDefaults это отказ каталога по правилу 02-SPEC.md 8.3.
	ErrBadLadderDefaults = errors.New("transport: недопустимые умолчания lad в каталоге")
)

// Target это конкретная цель одной попытки: куда идти, какое имя проверять и
// какие пины прикладывать.
type Target struct {
	Rung RungID `json:"rung"`
	// Host это имя, против которого проверяется сертификат, и оно же уходит в
	// SNI. Для R3 это имя зеркала, а не литеральный адрес.
	Host string `json:"host"`
	// SNI переопределяет имя в ClientHello, когда зеркало его задаёт.
	SNI string `json:"sni,omitempty"`
	// Addr это литеральный адрес для R3. Пусто означает обычное разрешение
	// имени. Проверка сертификата НЕ отключается: проверяется Host из SNI.
	Addr string `json:"addr,omitempty"`
	// Pins это SPKI пины хоста.
	Pins [][]byte `json:"-"`
	ASN  uint64   `json:"asn,omitempty"`
	CC   string   `json:"cc,omitempty"`
	// Proxy это адрес пользовательского прокси для R5.
	Proxy string `json:"proxy,omitempty"`
	// Label это непрозрачная метка зеркала для истории попыток. Реальный хост
	// в истории показывается только для R1.
	Label string `json:"label,omitempty"`
}

// Exchange выполняет ровно один HTTP обмен по одной ступени и одной цели.
// Реализации живут в transport_default.go и transport_mihomo.go; тесты
// подставляют свою.
type Exchange interface {
	Do(ctx context.Context, t Target, req *http.Request) (*http.Response, error)
}

// CacheSource это ступень R0: последние хорошие проверенные документы с диска.
// Она всегда первая и сети не стоит.
type CacheSource interface {
	// Lookup возвращает сохранённый кадр для этого запроса. Второе значение
	// ложно, если для запроса ничего не сохранено.
	Lookup(req *http.Request) ([]byte, bool)
}

// Response это ответ, доставленный лестницей.
type Response struct {
	Status int         `json:"status"`
	Header http.Header `json:"-"`
	Body   []byte      `json:"-"`
	Rung   RungID      `json:"rung"`
	Host   string      `json:"host,omitempty"`
	// FromCache истинно, когда ответ пришёл со ступени R0.
	FromCache bool `json:"from_cache"`
}

// Ladder это лестница транспортов одного профиля.
//
// Правило, ради которого этот тип существует в единственном экземпляре:
// ступень, выключенная пользователем, не пробуется никогда. Аварийного обхода
// нет, эскалации "последнего шанса" нет, и поля оператора, которое её включит
// обратно, тоже нет.
type Ladder struct {
	mu sync.Mutex

	// order это порядок ступеней. R0 переносится в начало независимо от того,
	// что говорит подписанный lad.ord.
	order []RungID
	// enabled это включённый набор.
	enabled map[RungID]bool
	// userSet помечает ступени, которых касался пользователь. С этого момента
	// подписанные умолчания их не восстанавливают (02-SPEC.md 8.3).
	userSet map[RungID]bool
	// avail это причина недоступности ступени по фактам сборки, платформы и
	// данных оператора. Пустая причина означает доступность.
	avail map[RungID]Reason

	thr Thresholds

	mirrors []csm.Mirror
	reserve []csm.Mirror
	doh     []csm.DoHEntry
	pins    map[string][][]byte

	proxy       string
	tunnelProxy string

	exch    Exchange
	cache   CacheSource
	history []Attempt

	// backoff это текущая задержка между неуспешными циклами, per profile.
	backoff time.Duration
	nextAt  time.Time

	rnd *rand.Rand
	now func() time.Time
}

// NewLadder создаёт лестницу с умолчаниями 02-SPEC.md 8.3: порядок
// [0..6] с добавленной в хвост несобранной ступенью onion, включённый набор
// [0,1,2,3,6].
func NewLadder(exch Exchange) *Ladder {
	l := &Ladder{
		order:   []RungID{R0Cached, R1Direct, R2Mirrors, R3DoH, R4Tunnel, R5Proxy, R6OutOfBand, R7Onion},
		enabled: map[RungID]bool{R0Cached: true, R1Direct: true, R2Mirrors: true, R3DoH: true, R6OutOfBand: true},
		userSet: map[RungID]bool{},
		avail:   map[RungID]Reason{},
		thr:     DefaultThresholds(),
		pins:    map[string][][]byte{},
		exch:    exch,
		backoff: BackoffInitial,
		rnd:     rand.New(rand.NewSource(time.Now().UnixNano())),
		now:     time.Now,
	}
	// R7 не собрана в этом репозитории. Она видна и выключена с причиной, а не
	// спрятана и не пропущена молча: инвариант 17.
	l.avail[R7Onion] = ReasonAppVersionUnsupported
	// R4 и R5 по умолчанию выключены: R4 нужен поднятый туннель, R5 нужен ввод
	// пользователя.
	l.avail[R5Proxy] = ReasonNotConfigured
	l.avail[R4Tunnel] = defaultTunnelReason()
	l.avail[R2Mirrors] = ReasonNotOffered
	l.avail[R3DoH] = ReasonNotOffered
	return l
}

// SetClock подменяет часы. Только для тестов.
func (l *Ladder) SetClock(f func() time.Time) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.now = f
}

// SetRandSource фиксирует источник случайности. Только для тестов: выбор
// зеркал и разброс backoff обязаны быть непредсказуемы в бою.
func (l *Ladder) SetRandSource(seed int64) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.rnd = rand.New(rand.NewSource(seed))
}

// SetExchange подменяет исполнителя обмена.
func (l *Ladder) SetExchange(e Exchange) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.exch = e
}

// SetCache задаёт источник ступени R0.
func (l *Ladder) SetCache(c CacheSource) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.cache = c
}

// Thresholds возвращает действующие пороги.
func (l *Ladder) Thresholds() Thresholds {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.thr
}

// SetThresholds применяет уже зажатые пороги. Зажим делает ClampThresholds,
// и сырое подписанное значение сюда не попадает.
func (l *Ladder) SetThresholds(t Thresholds) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.thr = t
}

// SetProxy задаёт пользовательский прокси ступени R5. Пустая строка снимает
// его. Прокси используется ТОЛЬКО для выборки манифеста и конфигурации и
// никогда для трафика туннеля; экран, где он вводится, обязан это сказать.
func (l *Ladder) SetProxy(addr string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.proxy = strings.TrimSpace(addr)
	if l.proxy == "" {
		l.avail[R5Proxy] = ReasonNotConfigured
	} else {
		delete(l.avail, R5Proxy)
	}
}

// SetTunnelUnavailable объявляет ступень R4 недоступной с явной причиной.
//
// Нужно именно платформенному мосту: на Android в обычном режиме TUN у R4
// пути нет, потому что приложение исключено из собственного туннеля
// (addDisallowedApplication) и локального слушателя в этом режиме не
// существует. Пока не появится mixed-инбаунд на 127.0.0.1 в ОБОИХ режимах,
// R4 обязана рисоваться видимой и выключенной с причиной platform_unsupported,
// а диагностика не имеет права показывать успех R4 на iOS и отказ на Android
// для одного арендатора в одной сети, не сказав почему.
func (l *Ladder) SetTunnelUnavailable(reason Reason) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if reason == ReasonNone {
		reason = ReasonPlatformUnsupported
	}
	l.tunnelProxy = ""
	l.avail[R4Tunnel] = reason
}

// SetTunnelProxy задаёт адрес локального mixed-инбаунда для ступени R4.
// Пустая строка возвращает ступень в недоступное состояние с причиной сборки.
func (l *Ladder) SetTunnelProxy(addr string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.tunnelProxy = strings.TrimSpace(addr)
	if r := defaultTunnelReason(); r != ReasonNone {
		l.avail[R4Tunnel] = r
		return
	}
	if l.tunnelProxy == "" {
		l.avail[R4Tunnel] = ReasonNotConfigured
	} else {
		delete(l.avail, R4Tunnel)
	}
}

// ApplyCatalog переносит в лестницу подписанные данные каталога: пул зеркал,
// список DoH, пины и умолчания порядка. Пороги зажимаются отдельно, потому что
// зажим resp_max это отказ КАТАЛОГА и решается вызывающим.
//
// capBits это уже пересечённая битовая маска возможностей (02-SPEC.md 6.2):
// бит 4 включает R2, бит 5 включает R3.
func (l *Ladder) ApplyCatalog(cat *csm.Catalog, capBits uint32) error {
	if cat == nil {
		return nil
	}
	l.mu.Lock()
	defer l.mu.Unlock()

	l.mirrors = cat.Mir
	l.doh = cat.DoH
	l.pins = map[string][][]byte{}
	for _, p := range cat.Pin {
		l.pins[p.H] = p.SPKI
	}

	// Правило 02-SPEC.md 6.5: бит, утверждающий наличие СОДЕРЖИМОГО каталога,
	// при пустом массиве считается нулём. Это утверждение о байтах, которые у
	// клиента есть, а не переопределение политики.
	if capBits&(1<<4) != 0 && len(l.mirrors)+len(l.reserve) > 0 {
		delete(l.avail, R2Mirrors)
	} else {
		l.avail[R2Mirrors] = ReasonNotOffered
	}
	if capBits&(1<<5) != 0 && len(l.doh) > 0 {
		delete(l.avail, R3DoH)
	} else if len(l.doh) == 0 && capBits&(1<<5) != 0 {
		l.avail[R3DoH] = ReasonNotConfigured
	} else {
		l.avail[R3DoH] = ReasonNotOffered
	}

	return l.applyDefaultsLocked(cat.Lad)
}

// SetReservePool кладёт резервный пул зеркал из /sub/r1/{loc}. Он живёт рядом
// с пулом каталога, а не вместо него.
func (l *Ladder) SetReservePool(mir []csm.Mirror) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.reserve = mir
	if len(l.mirrors)+len(l.reserve) > 0 && l.avail[R2Mirrors] == ReasonNotOffered {
		delete(l.avail, R2Mirrors)
	}
}

// applyDefaultsLocked применяет lad.ord и lad.en.
//
// Умолчания оператора действуют только на ступени, которых пользователь не
// касался. Более поздний каталог НЕ восстанавливает умолчание поверх выбора
// пользователя: это изменение оператором поля, которое задал пользователь, и
// оно идёт через карточку Keep or Revert выше по стеку.
func (l *Ladder) applyDefaultsLocked(lad *csm.Ladder) error {
	if lad == nil {
		return nil
	}
	if len(lad.Ord) == 0 || len(lad.Ord) > 7 {
		return fmt.Errorf("%w: ord длины %d", ErrBadLadderDefaults, len(lad.Ord))
	}
	seen := map[uint64]bool{}
	ord := make([]RungID, 0, len(lad.Ord)+1)
	for _, v := range lad.Ord {
		if v > 6 {
			return fmt.Errorf("%w: ступень %d вне 0..6", ErrBadLadderDefaults, v)
		}
		if seen[v] {
			return fmt.Errorf("%w: дубликат ступени %d", ErrBadLadderDefaults, v)
		}
		seen[v] = true
		ord = append(ord, RungID(v))
	}
	en := map[RungID]bool{}
	for _, v := range lad.En {
		if !seen[v] {
			return fmt.Errorf("%w: en содержит ступень %d вне ord", ErrBadLadderDefaults, v)
		}
		en[RungID(v)] = true
	}
	// Ступени 0 и 6 обязаны присутствовать в en; каталог, который их опускает,
	// отвергается.
	if !en[R0Cached] || !en[R6OutOfBand] {
		return fmt.Errorf("%w: en обязан содержать ступени 0 и 6", ErrBadLadderDefaults)
	}

	// Ступени, которых нет в ord каталога, сохраняют текущее место в хвосте.
	tail := make([]RungID, 0, 2)
	for _, r := range AllRungs() {
		if !seen[uint64(r)] {
			tail = append(tail, r)
		}
	}
	newOrder := append(ord, tail...)
	// R0 первая всегда, что бы ни говорил lad.ord: читать проверенный документ
	// с диска до открытия сокета это не вопрос политики.
	newOrder = hoistR0(newOrder)

	// Порядок принимается целиком только если пользователь не переупорядочивал.
	if !l.anyUserSetLocked() {
		l.order = newOrder
	}
	for r, on := range en {
		if l.userSet[r] {
			continue
		}
		l.enabled[r] = on
	}
	for _, r := range newOrder {
		if l.userSet[r] || r.Mandatory() {
			continue
		}
		if !en[r] {
			l.enabled[r] = false
		}
	}
	l.enabled[R0Cached] = true
	l.enabled[R6OutOfBand] = true
	return nil
}

func (l *Ladder) anyUserSetLocked() bool {
	for _, v := range l.userSet {
		if v {
			return true
		}
	}
	return false
}

func hoistR0(in []RungID) []RungID {
	out := make([]RungID, 0, len(in)+1)
	out = append(out, R0Cached)
	for _, r := range in {
		if r != R0Cached {
			out = append(out, r)
		}
	}
	return out
}

// SetEnabled это переключатель пользователя. Ступени 0 и 6 выключить нельзя.
// После вызова подписанные умолчания эту ступень больше не трогают.
func (l *Ladder) SetEnabled(r RungID, on bool) error {
	if r.Mandatory() && !on {
		return fmt.Errorf("transport: ступень %s выключить нельзя", r.Name())
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	l.enabled[r] = on
	l.userSet[r] = true
	return nil
}

// SetOrder это переупорядочивание пользователем. R0 переносится в начало
// независимо от переданного порядка.
func (l *Ladder) SetOrder(order []RungID) error {
	seen := map[RungID]bool{}
	for _, r := range order {
		if r > MaxRung {
			return fmt.Errorf("transport: ступень %d вне диапазона", r)
		}
		if seen[r] {
			return fmt.Errorf("transport: дубликат ступени %d", r)
		}
		seen[r] = true
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	out := make([]RungID, 0, len(AllRungs()))
	out = append(out, order...)
	for _, r := range AllRungs() {
		if !seen[r] {
			out = append(out, r)
		}
	}
	l.order = hoistR0(out)
	for _, r := range order {
		l.userSet[r] = true
	}
	return nil
}

// State отдаёт полный список скомпилированных ступеней с порядком, включением,
// причиной и последней попыткой. Список полный по построению: недоступная
// ступень видна и выключена с причиной, а не спрятана (инвариант 17).
func (l *Ladder) State() []RungState {
	l.mu.Lock()
	defer l.mu.Unlock()
	pos := map[RungID]int{}
	for i, r := range l.order {
		pos[r] = i
	}
	last := map[RungID]Attempt{}
	for _, a := range l.history {
		last[a.Rung] = a
	}
	out := make([]RungState, 0, len(AllRungs()))
	for _, r := range AllRungs() {
		st := RungState{
			Rung:    r,
			Name:    r.Name(),
			Order:   pos[r],
			Enabled: l.enabled[r],
			UserSet: l.userSet[r],
		}
		if reason, ok := l.avail[r]; ok && reason != ReasonNone {
			st.Enabled = false
			st.Reason = reason
		} else if !st.Enabled {
			st.Reason = ReasonUserDisabled
		}
		if a, ok := last[r]; ok {
			cp := a
			st.Last = &cp
		}
		out = append(out, st)
	}
	sortRungStates(out)
	return out
}

// History возвращает копию истории попыток, новые записи в конце.
func (l *Ladder) History() []Attempt {
	l.mu.Lock()
	defer l.mu.Unlock()
	out := make([]Attempt, len(l.history))
	copy(out, l.history)
	return out
}

// RecordCacheFailure записывает отказ проверки кадра, поднятого с диска.
//
// Молча отбросить такой кадр безопасно по направлению, но 02-SPEC.md 8.8.3
// требует событие записать: иначе подделанный файл кеша неотличим в обвязке от
// отсутствующего, а это ровно та разница, которую пользователь должен видеть.
func (l *Ladder) RecordCacheFailure(what string, err error) {
	if err == nil {
		return
	}
	outcome, code, detail := classify(err)
	l.mu.Lock()
	defer l.mu.Unlock()
	l.record(Attempt{
		Rung: R0Cached, Host: "cache", Start: time.Now(),
		Outcome: outcome, Code: code, Detail: what + ": " + detail,
	})
}

// RecordCacheNote записывает событие ступени R0, которое отказом не является:
// применённый отзыв узлов, например. Отдельный метод, потому что подать
// законный отзыв как отказ проверки значило бы соврать в истории попыток.
func (l *Ladder) RecordCacheNote(what, detail string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.record(Attempt{
		Rung: R0Cached, Host: "cache", Start: time.Now(),
		Outcome: OutcomeOK, Detail: what + ": " + detail,
	})
}

func (l *Ladder) record(a Attempt) {
	l.history = append(l.history, a)
	if len(l.history) > AttemptHistoryMax {
		l.history = l.history[len(l.history)-AttemptHistoryMax:]
	}
}

// effectiveOrderLocked возвращает ступени, которые цикл имеет право пробовать,
// в порядке обхода.
//
// Это единственное место, где решается "пробовать или нет". Выключенная
// пользователем ступень сюда не попадает никогда, и обхода у этого правила нет.
func (l *Ladder) effectiveOrderLocked() []RungID {
	out := make([]RungID, 0, len(l.order))
	for _, r := range l.order {
		if !l.enabled[r] {
			continue
		}
		if reason := l.avail[r]; reason != ReasonNone {
			continue
		}
		out = append(out, r)
	}
	// R0 первая всегда, но только если она вообще прошла фильтр: выключить её
	// нельзя, так что на практике она всегда здесь, и проверка защищает от
	// порчи состояния, а не выражает политику.
	for _, r := range out {
		if r == R0Cached {
			return hoistR0(out)
		}
	}
	return out
}

// Backoff возвращает текущую задержку между циклами и момент следующей
// разрешённой попытки.
func (l *Ladder) Backoff() (time.Duration, time.Time) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.backoff, l.nextAt
}

// jitter применяет разброс в обе стороны в процентах.
func (l *Ladder) jitter(d time.Duration, percent int) time.Duration {
	if percent <= 0 {
		return d
	}
	span := float64(d) * float64(percent) / 100.0
	delta := (l.rnd.Float64()*2 - 1) * span
	out := time.Duration(float64(d) + delta)
	if out < time.Second {
		out = time.Second
	}
	return out
}

// noteCycle обновляет backoff по исходу цикла.
func (l *Ladder) noteCycle(ok bool) {
	if ok {
		l.backoff = BackoffInitial
		l.nextAt = time.Time{}
		return
	}
	next := l.jitter(l.backoff, BackoffJitterPercent)
	l.nextAt = l.now().Add(next)
	l.backoff *= 2
	if l.backoff > BackoffMax {
		l.backoff = BackoffMax
	}
}

// DoOptions настраивает один цикл выборки.
type DoOptions struct {
	// Origin это закреплённый origin регистрации, к которому идёт R1.
	Origin string
	// SubscriptionDomain разрешает ровно один переход, и только на этот хост.
	SubscriptionDomain string
	// MaxBody это потолок тела ответа. Ноль означает thr.resp_max. Для
	// фрагментов каталога вызывающий ставит CHUNK_RESP_MAX.
	MaxBody uint64
	// Force пропускает backoff ровно один раз и НЕ сбрасывает счётчик:
	// это обновление по инициативе пользователя.
	Force bool
	// Verify вызывается на теле каждой попытки. Возврат nil означает, что
	// документ проверился и цикл останавливается. Ошибка csm.Error класса
	// проверки записывается в историю как событие безопасности, и лестница
	// имеет право идти дальше: враждебное зеркало это ровно тот случай, ради
	// которого лестница существует.
	Verify func(body []byte) error
	// AllowRungs, если непусто, ограничивает цикл этими ступенями. Так
	// выражается правило 02-SPEC.md 10.1: при снятом бите 1 директиву нельзя
	// брать ни с какой ступени, кроме R1.
	AllowRungs []RungID
}

// Do проходит эффективный порядок один раз и возвращает первый ответ, который
// прошёл Verify.
//
// Цикл останавливается на первой ступени, вернувшей проверяемый документ.
// Цикл, дошедший до конца включённого порядка без такого документа, уходит в
// backoff, и профиль остаётся на кешированных документах.
func (l *Ladder) Do(ctx context.Context, req *http.Request, opt DoOptions) (*Response, error) {
	l.mu.Lock()
	order := l.effectiveOrderLocked()
	thr := l.thr
	exch := l.exch
	cache := l.cache
	nextAt := l.nextAt
	now := l.now()
	l.mu.Unlock()

	if !opt.Force && !nextAt.IsZero() && now.Before(nextAt) {
		return nil, fmt.Errorf("%w: следующая попытка не раньше %s", ErrNoRung, nextAt.Format(time.RFC3339))
	}
	if len(opt.AllowRungs) > 0 {
		allow := map[RungID]bool{}
		for _, r := range opt.AllowRungs {
			allow[r] = true
		}
		filtered := make([]RungID, 0, len(order))
		for _, r := range order {
			if allow[r] {
				filtered = append(filtered, r)
			}
		}
		order = filtered
	}
	maxBody := opt.MaxBody
	if maxBody == 0 {
		maxBody = thr.RespMax
	}

	deadline := now.Add(CycleBudget)
	cycleCtx, cancel := context.WithDeadline(ctx, deadline)
	defer cancel()

	conns := map[string]*ConnBudget{}
	var lastErr error

	for _, rung := range order {
		if l.now().After(deadline) {
			l.mu.Lock()
			l.noteCycle(false)
			l.mu.Unlock()
			return nil, ErrCycleBudget
		}

		if rung == R0Cached {
			if cache == nil {
				continue
			}
			start := l.now()
			body, ok := cache.Lookup(req)
			if !ok {
				continue
			}
			a := Attempt{Rung: rung, Host: "cache", Start: start, Outcome: OutcomeOK, Status: 200}
			if opt.Verify != nil {
				if err := opt.Verify(body); err != nil {
					a.Outcome, a.Code, a.Detail = classify(err)
					l.mu.Lock()
					l.record(a)
					l.mu.Unlock()
					lastErr = err
					continue
				}
			}
			a.Millis = l.now().Sub(start).Milliseconds()
			l.mu.Lock()
			l.record(a)
			l.noteCycle(true)
			l.mu.Unlock()
			return &Response{Status: 200, Body: body, Rung: rung, Host: "cache", FromCache: true}, nil
		}

		if !rung.Network() {
			// R6 требует действия человека по построению, R7 не собрана.
			// Автоматических попыток тут нет и быть не может.
			continue
		}

		targets := l.selectTargets(rung, opt.Origin)
		if len(targets) == 0 {
			continue
		}
		for _, t := range targets {
			if l.now().After(deadline) {
				l.mu.Lock()
				l.noteCycle(false)
				l.mu.Unlock()
				return nil, ErrCycleBudget
			}
			resp, err := l.attempt(cycleCtx, exch, t, req, thr, maxBody, conns, opt)
			if err != nil {
				lastErr = err
				continue
			}
			l.mu.Lock()
			l.noteCycle(true)
			l.mu.Unlock()
			return resp, nil
		}
	}

	l.mu.Lock()
	l.noteCycle(false)
	l.mu.Unlock()
	if lastErr != nil {
		return nil, fmt.Errorf("%w: %v", ErrNoRung, lastErr)
	}
	return nil, ErrNoRung
}

// attempt выполняет одну попытку с учётом гигиены соединения.
func (l *Ladder) attempt(ctx context.Context, exch Exchange, t Target, req *http.Request,
	thr Thresholds, maxBody uint64, conns map[string]*ConnBudget, opt DoOptions) (*Response, error) {

	start := l.now()
	a := Attempt{Rung: t.Rung, Host: t.Label, Start: start}
	if a.Host == "" {
		a.Host = t.Host
	}

	out, err := rewriteRequest(req, t)
	if err != nil {
		a.Outcome, a.Code, a.Detail = OutcomeRefused, "", err.Error()
		a.Millis = l.now().Sub(start).Milliseconds()
		l.mu.Lock()
		l.record(a)
		l.mu.Unlock()
		return nil, err
	}

	// Гигиена соединения. Правило считает проекцию ДО отправки и по
	// максимально возможному ответу, поэтому на умолчаниях каждая выборка
	// получает своё соединение без специального случая на тип документа.
	key := t.Rung.Name() + "|" + t.Host
	budget := conns[key]
	reqBytes := estimateRequestBytes(out)
	if budget == nil || !budget.CanSend(reqBytes) {
		budget = NewConnBudget(thr)
		conns[key] = budget
	}
	if !budget.CanSend(reqBytes) {
		a.Outcome, a.Detail = OutcomeBudget, fmt.Sprintf("запрос %d байт не помещается в бюджет %d/%d", reqBytes, thr.ConnBytes, thr.ConnPackets)
		a.Millis = l.now().Sub(start).Milliseconds()
		l.mu.Lock()
		l.record(a)
		l.mu.Unlock()
		return nil, ErrBudgetExceeded
	}
	budget.ChargeRequest(reqBytes)

	attemptCtx, cancel := context.WithTimeout(ctx, t.Rung.Timeout())
	defer cancel()
	resp, err := exch.Do(attemptCtx, t, out.WithContext(attemptCtx))
	if err != nil {
		a.Outcome, a.Detail = OutcomeNetwork, err.Error()
		a.Millis = l.now().Sub(start).Milliseconds()
		l.mu.Lock()
		l.record(a)
		l.mu.Unlock()
		return nil, err
	}
	csmFrame := isCSMPath(out.URL.Path)
	body, status, hdr, err := readCSMResponse(resp, maxBody, out.URL, opt.SubscriptionDomain, csmFrame)
	// Разрешённый переход: ровно один, только на настроенный
	// subscription_domain, и его цена честно списывается с того же бюджета
	// соединения. Панель выдаёт этот 308 безусловно при несовпадении Host,
	// так что без этой ветки легаси-путь просто перестал бы работать.
	var hop *RedirectAllowed
	if errors.As(err, &hop) {
		hopReq := out.Clone(attemptCtx)
		hopReq.URL = hop.To
		hopReq.Host = ""
		if cerr := CheckFetchURL(hopReq.URL); cerr != nil {
			err = cerr
		} else {
			hopBytes := estimateRequestBytes(hopReq)
			if !budget.CanSend(hopBytes) {
				budget = NewConnBudget(thr)
				conns[key] = budget
			}
			if !budget.CanSend(hopBytes) {
				err = ErrBudgetExceeded
			} else {
				budget.ChargeRequest(hopBytes)
				resp2, herr := exch.Do(attemptCtx, t, hopReq)
				if herr != nil {
					err = herr
				} else {
					body, status, hdr, err = readCSMResponse(resp2, maxBody, hopReq.URL, "", csmFrame)
				}
			}
		}
	}
	if err != nil {
		a.Outcome, a.Detail = OutcomeRefused, err.Error()
		a.Status = status
		a.Millis = l.now().Sub(start).Milliseconds()
		l.mu.Lock()
		l.record(a)
		l.mu.Unlock()
		return nil, err
	}
	budget.ChargeResponse(uint64(len(body)) + estimateHeaderBytes(hdr))
	a.Status = status
	if status != http.StatusOK && status != http.StatusNotModified {
		a.Outcome, a.Detail = OutcomeHTTP, fmt.Sprintf("код состояния %d", status)
		a.Millis = l.now().Sub(start).Milliseconds()
		l.mu.Lock()
		l.record(a)
		l.mu.Unlock()
		return nil, fmt.Errorf("transport: код состояния %d", status)
	}
	if opt.Verify != nil {
		if verr := opt.Verify(body); verr != nil {
			a.Outcome, a.Code, a.Detail = classify(verr)
			a.Millis = l.now().Sub(start).Milliseconds()
			l.mu.Lock()
			l.record(a)
			l.mu.Unlock()
			return nil, verr
		}
	}
	a.Outcome = OutcomeOK
	a.Millis = l.now().Sub(start).Milliseconds()
	l.mu.Lock()
	l.record(a)
	l.mu.Unlock()
	return &Response{Status: status, Header: hdr, Body: body, Rung: t.Rung, Host: t.Host}, nil
}

// classify превращает ошибку проверки в исход и код 03-WIRE.md 6.6.
//
// Разбор и проверка это разные исходы. Отказ разбора почти всегда означает
// captive portal, зеркало с ошибкой или обрезанный ответ, и пользователю его
// нельзя показывать как заявление о подделке. Отказ проверки это событие
// безопасности, и приравнивать его к пустому ответу нельзя.
func classify(err error) (Outcome, string, string) {
	var e *csm.Error
	if errors.As(err, &e) {
		switch {
		case e.IsParse():
			return OutcomeParse, string(e.Code), e.Error()
		default:
			return OutcomeVerify, string(e.Code), e.Error()
		}
	}
	return OutcomeVerify, "", err.Error()
}

// selectTargets выбирает цели одной ступени на один цикл, 02-SPEC.md 8.4.
func (l *Ladder) selectTargets(rung RungID, origin string) []Target {
	l.mu.Lock()
	defer l.mu.Unlock()
	switch rung {
	case R1Direct:
		u, err := url.Parse(origin)
		if err != nil || u.Hostname() == "" {
			return nil
		}
		h := u.Hostname()
		return []Target{{Rung: rung, Host: h, Label: h, Pins: l.pins[h]}}

	case R2Mirrors:
		pool := make([]csm.Mirror, 0, len(l.mirrors)+len(l.reserve))
		pool = append(pool, l.mirrors...)
		pool = append(pool, l.reserve...)
		picked := l.drawMirrors(pool, R2Mirrors.Attempts())
		out := make([]Target, 0, len(picked))
		for i, m := range picked {
			out = append(out, Target{
				Rung: rung, Host: m.H, SNI: m.SNI, Pins: mirrorPins(m, l.pins),
				ASN: m.ASN, CC: m.CC, Label: fmt.Sprintf("mirror-%d", i+1),
			})
		}
		return out

	case R3DoH:
		if len(l.doh) == 0 {
			return nil
		}
		pool := make([]csm.Mirror, 0, len(l.mirrors)+len(l.reserve))
		pool = append(pool, l.mirrors...)
		pool = append(pool, l.reserve...)
		hosts := l.drawMirrors(pool, R3DoH.Attempts())
		out := make([]Target, 0, R3DoH.Attempts())
		for i := 0; i < R3DoH.Attempts() && i < len(hosts); i++ {
			m := hosts[i]
			addr := ""
			if len(m.IP) > 0 {
				addr = m.IP[0]
			}
			out = append(out, Target{
				Rung: rung, Host: m.H, SNI: m.SNI, Addr: addr, Pins: mirrorPins(m, l.pins),
				ASN: m.ASN, CC: m.CC, Label: fmt.Sprintf("doh-%d", i+1),
			})
		}
		return out

	case R4Tunnel:
		u, err := url.Parse(origin)
		if err != nil || u.Hostname() == "" {
			return nil
		}
		return []Target{{Rung: rung, Host: u.Hostname(), Proxy: l.tunnelProxy, Label: "tunnel"}}

	case R5Proxy:
		if l.proxy == "" {
			return nil
		}
		u, err := url.Parse(origin)
		if err != nil || u.Hostname() == "" {
			return nil
		}
		return []Target{{Rung: rung, Host: u.Hostname(), Proxy: l.proxy, Label: "proxy"}}
	}
	return nil
}

// drawMirrors выбирает до n различных хостов без возврата, взвешивая по w, и
// НИКОГДА не берёт два хоста из одной asn в одном цикле.
func (l *Ladder) drawMirrors(pool []csm.Mirror, n int) []csm.Mirror {
	if n <= 0 || len(pool) == 0 {
		return nil
	}
	rest := make([]csm.Mirror, len(pool))
	copy(rest, pool)
	usedASN := map[uint64]bool{}
	usedHost := map[string]bool{}
	out := make([]csm.Mirror, 0, n)
	for len(out) < n && len(rest) > 0 {
		total := uint64(0)
		for _, m := range rest {
			total += mirrorWeight(m)
		}
		if total == 0 {
			break
		}
		pick := uint64(l.rnd.Int63n(int64(total)))
		idx := 0
		acc := uint64(0)
		for i, m := range rest {
			acc += mirrorWeight(m)
			if pick < acc {
				idx = i
				break
			}
		}
		m := rest[idx]
		rest = append(rest[:idx], rest[idx+1:]...)
		if usedHost[m.H] {
			continue
		}
		if m.ASN != 0 && usedASN[m.ASN] {
			continue
		}
		usedHost[m.H] = true
		if m.ASN != 0 {
			usedASN[m.ASN] = true
		}
		out = append(out, m)
	}
	sort.SliceStable(out, func(i, j int) bool { return mirrorWeight(out[i]) > mirrorWeight(out[j]) })
	return out
}

func mirrorWeight(m csm.Mirror) uint64 {
	if m.Weight == 0 {
		return 1
	}
	return m.Weight
}

func mirrorPins(m csm.Mirror, byHost map[string][][]byte) [][]byte {
	if len(m.Pin) > 0 {
		return m.Pin
	}
	return byHost[m.H]
}
