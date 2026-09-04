package transport

import (
	"bufio"
	"context"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"
)

// Прокси набирается ЗДЕСЬ, а не в http.Transport.
//
// http.Transport документирует DialTLSContext как путь "for non-proxied HTTPS
// requests": при непустом Proxy он берёт TLSClientConfig и наш обработчик не
// зовёт вовсе. Ступени R4 и R5 задают Proxy, поэтому передача его транспорту
// молча снимала бы SPKI пины, нижнюю границу TLS и явный ClientHello uTLS
// ровно на тех двух ступенях, на которые скатывается пользователь под
// цензурой. Соединение через прокси поэтому набирается вручную, а рукопожатие
// идёт по тому же коду, что и на прямой ступени.

// ErrProxy это отказ на этапе установления соединения через прокси.
var ErrProxy = errors.New("transport: прокси не установил соединение")

// dialRawConn открывает TCP до цели: напрямую, по литеральному адресу
// подписанной записи doh, либо через прокси ступени.
func dialRawConn(ctx context.Context, t Target, addr string) (net.Conn, error) {
	host, port, err := net.SplitHostPort(addr)
	if err != nil {
		host, port = addr, "443"
	}
	if t.Addr != "" {
		// R3: подключаемся к литеральному адресу из подписанной записи doh.
		// Порт берём из исходного адреса, потому что запись адреса его не
		// несёт.
		host = t.Addr
	}
	dst := net.JoinHostPort(host, port)
	if t.Proxy == "" {
		d := &net.Dialer{Timeout: TCPConnectTimeout}
		return d.DialContext(ctx, "tcp", dst)
	}
	return dialThroughProxy(ctx, t.Proxy, dst)
}

// dialThroughProxy устанавливает туннель до dst через socks5 или http прокси и
// возвращает уже готовый к TLS поток.
func dialThroughProxy(ctx context.Context, proxy, dst string) (net.Conn, error) {
	pu, err := parseProxyURL(proxy)
	if err != nil {
		return nil, err
	}
	d := &net.Dialer{Timeout: TCPConnectTimeout}
	raw, err := d.DialContext(ctx, "tcp", pu.Host)
	if err != nil {
		return nil, err
	}
	if dl, ok := ctx.Deadline(); ok {
		_ = raw.SetDeadline(dl)
	}
	switch strings.ToLower(pu.Scheme) {
	case "socks5", "socks5h":
		err = socks5Connect(raw, pu, dst)
	default: // http, https
		err = httpConnect(raw, pu, dst)
	}
	if err != nil {
		_ = raw.Close()
		return nil, err
	}
	// Дедлайн установления снимается: дальше временем управляет http.Client.
	_ = raw.SetDeadline(time.Time{})
	return raw, nil
}

// socks5Connect выполняет рукопожатие RFC 1928 с методами 0x00 и 0x02.
func socks5Connect(c net.Conn, pu *url.URL, dst string) error {
	user := pu.User.Username()
	pass, _ := pu.User.Password()
	methods := []byte{0x00}
	if user != "" {
		methods = []byte{0x00, 0x02}
	}
	greet := append([]byte{0x05, byte(len(methods))}, methods...)
	if _, err := c.Write(greet); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	var sel [2]byte
	if _, err := readFull(c, sel[:]); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	if sel[0] != 0x05 {
		return fmt.Errorf("%w: не socks5", ErrProxy)
	}
	switch sel[1] {
	case 0x00:
	case 0x02:
		if len(user) > 255 || len(pass) > 255 {
			return fmt.Errorf("%w: учётные данные длиннее 255 байт", ErrProxy)
		}
		buf := []byte{0x01, byte(len(user))}
		buf = append(buf, user...)
		buf = append(buf, byte(len(pass)))
		buf = append(buf, pass...)
		if _, err := c.Write(buf); err != nil {
			return fmt.Errorf("%w: %v", ErrProxy, err)
		}
		var ok [2]byte
		if _, err := readFull(c, ok[:]); err != nil {
			return fmt.Errorf("%w: %v", ErrProxy, err)
		}
		if ok[1] != 0x00 {
			return fmt.Errorf("%w: пара логин и пароль отвергнута", ErrProxy)
		}
	default:
		return fmt.Errorf("%w: метод аутентификации 0x%02x не поддерживается", ErrProxy, sel[1])
	}

	host, portS, err := net.SplitHostPort(dst)
	if err != nil {
		return fmt.Errorf("%w: адрес %q", ErrProxy, dst)
	}
	port, err := strconv.ParseUint(portS, 10, 16)
	if err != nil {
		return fmt.Errorf("%w: порт %q", ErrProxy, portS)
	}
	req := []byte{0x05, 0x01, 0x00}
	if ip := net.ParseIP(host); ip != nil {
		if v4 := ip.To4(); v4 != nil {
			req = append(req, 0x01)
			req = append(req, v4...)
		} else {
			req = append(req, 0x04)
			req = append(req, ip.To16()...)
		}
	} else {
		if len(host) > 255 {
			return fmt.Errorf("%w: имя хоста длиннее 255 байт", ErrProxy)
		}
		req = append(req, 0x03, byte(len(host)))
		req = append(req, host...)
	}
	var pb [2]byte
	binary.BigEndian.PutUint16(pb[:], uint16(port))
	req = append(req, pb[:]...)
	if _, err := c.Write(req); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}

	var head [4]byte
	if _, err := readFull(c, head[:]); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	if head[1] != 0x00 {
		return fmt.Errorf("%w: код ответа 0x%02x", ErrProxy, head[1])
	}
	// Связанный адрес читается и отбрасывается, иначе он останется в потоке
	// перед ClientHello.
	switch head[3] {
	case 0x01:
		var skip [4 + 2]byte
		_, err = readFull(c, skip[:])
	case 0x04:
		var skip [16 + 2]byte
		_, err = readFull(c, skip[:])
	case 0x03:
		var l [1]byte
		if _, err = readFull(c, l[:]); err == nil {
			skip := make([]byte, int(l[0])+2)
			_, err = readFull(c, skip)
		}
	default:
		return fmt.Errorf("%w: тип адреса 0x%02x", ErrProxy, head[3])
	}
	if err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	return nil
}

// httpConnect выполняет CONNECT к http прокси.
func httpConnect(c net.Conn, pu *url.URL, dst string) error {
	req := &http.Request{
		Method: http.MethodConnect,
		URL:    &url.URL{Opaque: dst},
		Host:   dst,
		Header: http.Header{},
	}
	if u := pu.User.Username(); u != "" {
		p, _ := pu.User.Password()
		req.Header.Set("Proxy-Authorization", "Basic "+basicAuth(u, p))
	}
	if err := req.Write(c); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	br := bufio.NewReader(c)
	resp, err := http.ReadResponse(br, req)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	defer func() {
		if resp.Body != nil {
			_ = resp.Body.Close()
		}
	}()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("%w: CONNECT вернул %d", ErrProxy, resp.StatusCode)
	}
	if br.Buffered() > 0 {
		// Прокси, дописавший байты за ответом CONNECT, сдвинул бы весь
		// последующий поток TLS. Такое соединение не используется.
		return fmt.Errorf("%w: лишние байты после ответа CONNECT", ErrProxy)
	}
	return nil
}

func basicAuth(user, pass string) string {
	return base64.StdEncoding.EncodeToString([]byte(user + ":" + pass))
}

func readFull(c net.Conn, b []byte) (int, error) { return io.ReadFull(c, b) }

// -------------------------------------------------------------------------
// Резервный путь ступени R5 через локальный Tor (Orbot либо системный tor).
// -------------------------------------------------------------------------

// Что этот путь даёт и чего он НЕ даёт.
//
// Выборка подписки через локальный Tor прячет от НАБЛЮДАТЕЛЯ РЯДОМ С
// ПОЛЬЗОВАТЕЛЕМ ровно один факт: что это устройство сходило к оператору за
// подписанной конфигурацией. Всё. Сеанс VPN от этого анонимным не становится и
// через Tor не идёт: ступень R5 по построению обслуживает ТОЛЬКО выборку
// манифеста и конфигурации, а туннель поднимается тем же путём, что и всегда.
// Написать пользователю "анонимно" значило бы продать свойство, которого этот
// код не даёт, и текст на экране обязан говорить то же самое, что этот
// комментарий.
//
// Почему ступень последняя среди автоматических. Она стоит дороже всех
// остальных по задержке (три хопа Tor поверх и без того медленной выборки) и
// зависит от ЧУЖОГО приложения, которое пользователь мог не ставить, мог
// закрыть и мог настроить с аутентификацией. Ставить такой путь раньше
// собственных путей оператора значило бы платить эту цену в самом обычном
// случае, когда прямой путь работает. Порядок по умолчанию 02-SPEC.md 8.3 уже
// держит R5 последней сетевой ступенью, и этот код его НЕ меняет: он только
// подставляет адрес в ступень, которая и так последняя.
//
// Почему ничего не пробуется, пока пользователь не включил R5. Проба это
// соединение на петлю: дёшево, но не бесплатно, и она не имеет права
// происходить у человека, который резервный путь не просил. Выключенная
// пользователем ступень не пробуется никогда, и у этого правила обхода нет.

const (
	// TorSOCKSOrbot это порт SOCKS по умолчанию у Orbot на Android и у
	// системного tor на десктопе.
	TorSOCKSOrbot = "127.0.0.1:9050"
	// TorSOCKSBrowser это порт SOCKS Tor Browser. Отдельная запись, потому что
	// на десктопе запущенный браузер это самый частый способ иметь работающий
	// Tor, не поднимая службу.
	TorSOCKSBrowser = "127.0.0.1:9150"
)

// TorProbeTimeout это потолок одной пробы. Проба идёт на петлю, поэтому она
// либо отвечает за миллисекунды, либо не отвечает вовсе; секунда здесь это
// запас на холодный старт Orbot, а не рабочее время.
const TorProbeTimeout = 700 * time.Millisecond

// TorStatusTTL это срок годности результата пробы. Orbot включают и выключают
// руками, поэтому ответ "найден" стареет так же, как ответ "не найден".
const TorStatusTTL = 60 * time.Second

// TorState это то, что приложение имеет право сказать пользователю про
// локальный Tor. Словарь закрытый: "не знаем" это отдельное состояние, а не
// синоним "нет".
type TorState string

const (
	// TorUnknown: не искали. Так выглядит выключенная пользователем R5.
	TorUnknown TorState = "unknown"
	// TorReady: по адресу отвечает SOCKS5 без аутентификации, ступень R5 на
	// него настроена.
	TorReady TorState = "ready"
	// TorAbsent: искали и не нашли. Ступень R5 остаётся видимой и
	// ненастроенной, и причина отказа лежит в Detail.
	TorAbsent TorState = "absent"
	// TorSuperseded: пользователь ввёл СВОЙ прокси, и он занимает R5. Свой
	// выбор человека мы молча не подменяем.
	TorSuperseded TorState = "superseded"
	// TorUnsupported: на этой платформе чужое приложение не отдаёт SOCKS на
	// петлю, поэтому искать нечего.
	TorUnsupported TorState = "unsupported"
)

// TorStatus это последний известный ответ про локальный Tor. Он уходит в
// обвязку целиком: экран обязан уметь сказать "не нашли и вот почему", а не
// промолчать (инвариант 17).
type TorStatus struct {
	State TorState `json:"state"`
	// Addr это адрес, который реально подставлен в ступень R5. Пусто во всех
	// состояниях, кроме ready.
	Addr string `json:"addr,omitempty"`
	// Detail это наша собственная строка с причиной. Строк оператора здесь
	// нет и быть не может (INV-10).
	Detail string `json:"detail,omitempty"`
	// CheckedAt это момент последней пробы, unix-секунды. Ноль означает, что
	// пробы не было ни разу, и это не то же самое, что "не найден".
	CheckedAt int64 `json:"checked_at,omitempty"`
}

// ErrTorProbe это отказ пробы адреса SOCKS.
var ErrTorProbe = errors.New("transport: локальный SOCKS5 не отвечает")

// ProbeSOCKS5 проверяет, что по адресу отвечает именно SOCKS5 без
// аутентификации.
//
// Голого "порт открыт" мало. На 9050 может слушать что угодно, а ступень R5
// имеет ровно ОДНУ попытку за цикл (02-SPEC.md 8.4): потратить её на чужую
// службу значит остаться без последнего резерва и написать в историю сетевой
// отказ вместо честного "Tor не найден". Поэтому проверяется рукопожатие
// RFC 1928: приветствие с единственным методом 0x00 и ответ 0x05 0x00.
//
// Метод 0x02 (логин и пароль) это отдельный отказ с отдельным текстом. Tor,
// настроенный на аутентификацию, работает, но нам он не подходит: пароля у нас
// нет, а угадывать его нечем.
func ProbeSOCKS5(ctx context.Context, addr string) error {
	d := &net.Dialer{Timeout: TorProbeTimeout}
	c, err := d.DialContext(ctx, "tcp", addr)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrTorProbe, err)
	}
	defer func() { _ = c.Close() }()
	_ = c.SetDeadline(time.Now().Add(TorProbeTimeout))
	if _, err := c.Write([]byte{0x05, 0x01, 0x00}); err != nil {
		return fmt.Errorf("%w: %v", ErrTorProbe, err)
	}
	var sel [2]byte
	if _, err := readFull(c, sel[:]); err != nil {
		return fmt.Errorf("%w: %v", ErrTorProbe, err)
	}
	if sel[0] != 0x05 {
		return fmt.Errorf("%w: на порту отвечает не SOCKS5", ErrTorProbe)
	}
	switch sel[1] {
	case 0x00:
		return nil
	case 0x02:
		return fmt.Errorf("%w: SOCKS5 требует логин и пароль", ErrTorProbe)
	default:
		return fmt.Errorf("%w: SOCKS5 не принимает соединения без аутентификации (метод 0x%02x)", ErrTorProbe, sel[1])
	}
}

// TorFallback это держатель того, что мы знаем про локальный Tor, и место, где
// это знание превращается в настроенную ступень R5.
type TorFallback struct {
	mu        sync.Mutex
	endpoints []string
	probe     func(context.Context, string) error
	now       func() time.Time
	goos      string
	last      TorStatus
	// applied это адрес, который в ступень R5 поставили МЫ. Нужен, чтобы
	// снимать только своё: прокси, введённый пользователем, этот код не
	// трогает ни при каких обстоятельствах.
	applied string
}

// NewTorFallback создаёт держателя с умолчаниями.
func NewTorFallback() *TorFallback {
	return &TorFallback{
		endpoints: []string{TorSOCKSOrbot, TorSOCKSBrowser},
		probe:     ProbeSOCKS5,
		now:       time.Now,
		goos:      runtime.GOOS,
		last:      TorStatus{State: TorUnknown},
	}
}

// defaultTorFallback это единственный на процесс держатель.
//
// Наличие SOCKS на 127.0.0.1:9050 это свойство УСТРОЙСТВА, а не профиля: петля
// одна, Orbot один, и второй держатель означал бы вторую пробу того же порта и
// два разных ответа на один вопрос на двух экранах.
var defaultTorFallback = NewTorFallback()

// DefaultTorFallback возвращает этого держателя.
func DefaultTorFallback() *TorFallback { return defaultTorFallback }

// SetProbe, SetClock и SetPlatform это швы для тестов. В бою проба ходит в
// сеть, часы настоящие, платформа своя.
func (t *TorFallback) SetProbe(fn func(context.Context, string) error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.probe = fn
}

func (t *TorFallback) SetClock(fn func() time.Time) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.now = fn
}

func (t *TorFallback) SetPlatform(goos string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.goos = goos
}

// SetEndpoints задаёт список адресов, на которых ищется локальный SOCKS.
func (t *TorFallback) SetEndpoints(addrs []string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.endpoints = append([]string(nil), addrs...)
}

// Status возвращает последний известный ответ без пробы.
func (t *TorFallback) Status() TorStatus {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.last
}

// Ensure приводит ступень R5 в состояние, соответствующее фактам, и
// возвращает то, что об этом можно сказать пользователю.
//
// Порядок решений здесь и есть политика:
//  1. Прокси, введённый пользователем, главнее всего. Мы его не подменяем и
//     ради него даже не пробуем искать Tor.
//  2. Ступень, выключенная пользователем, не пробуется. Совсем.
//  3. На платформе, где чужое приложение не может отдать SOCKS на петлю,
//     ступень остаётся видимой и ненастроенной с честной причиной.
//  4. Свежий ответ пробы переиспользуется; протухший перепроверяется.
//
// userProxy это адрес из сохранённого состояния профиля (State.LadderProxy).
func (t *TorFallback) Ensure(ctx context.Context, l *Ladder, userProxy string) TorStatus {
	if l == nil {
		return t.Status()
	}
	userProxy = strings.TrimSpace(userProxy)

	t.mu.Lock()
	defer t.mu.Unlock()

	if userProxy != "" {
		// Ступень занята выбором человека. Ставим именно его адрес: сюда
		// приходят и после перезапуска, когда в лестнице ещё пусто.
		l.SetProxy(userProxy)
		t.applied = ""
		t.last = TorStatus{
			State:  TorSuperseded,
			Detail: "ступень R5 занята прокси, который ввели вы",
		}
		return t.last
	}

	if !l.userEnabled(R5Proxy) {
		t.releaseLocked(l)
		t.last = TorStatus{
			State:  TorUnknown,
			Detail: "ступень R5 выключена, локальный Tor не искали",
		}
		return t.last
	}

	if t.goos == "ios" {
		// На iOS Orbot поднимает свой VPN, а не слушателя на петле, доступного
		// другому приложению. Искать нечего, и молчать об этом нельзя.
		t.releaseLocked(l)
		t.last = TorStatus{
			State:  TorUnsupported,
			Detail: "на этой платформе стороннее приложение не отдаёт SOCKS на петлю",
		}
		return t.last
	}

	now := t.now()
	if t.last.CheckedAt > 0 && now.Sub(time.Unix(t.last.CheckedAt, 0)) < TorStatusTTL &&
		(t.last.State == TorReady || t.last.State == TorAbsent) {
		// Ответ ещё свежий. Адрес всё равно переустанавливается: лестницу мог
		// пересобрать вызов SetProxy("") с другой стороны, и тогда ступень
		// показывала бы ready, не имея адреса.
		if t.last.State == TorReady {
			l.SetProxy(t.last.Addr)
			t.applied = t.last.Addr
		}
		return t.last
	}

	addr, detail := t.probeEndpointsLocked(ctx)
	if addr == "" && ctx.Err() != nil {
		// Пробу оборвал вызывающий, а не отсутствие Tor. Записать это как
		// "не найден" значило бы запомнить чужой отказ на весь срок годности
		// и не пустить в R5 работающий Orbot следующие TorStatusTTL.
		// Ступень остаётся как была, и знание о ней тоже.
		return t.last
	}
	if addr == "" {
		t.releaseLocked(l)
		t.last = TorStatus{State: TorAbsent, Detail: detail, CheckedAt: now.Unix()}
		return t.last
	}
	l.SetProxy(addr)
	t.applied = addr
	t.last = TorStatus{
		State:     TorReady,
		Addr:      addr,
		Detail:    "локальный SOCKS5 отвечает; через него берётся только подписка",
		CheckedAt: now.Unix(),
	}
	return t.last
}

// probeEndpointsLocked обходит адреса по порядку и возвращает первый живой.
// Вторая строка это накопленная причина отказа: она нужна экрану целиком,
// потому что "порт закрыт" и "требует пароль" это разные советы пользователю.
func (t *TorFallback) probeEndpointsLocked(ctx context.Context) (string, string) {
	reasons := make([]string, 0, len(t.endpoints))
	for _, addr := range t.endpoints {
		err := t.probe(ctx, addr)
		if err == nil {
			return addr, ""
		}
		reasons = append(reasons, addr+": "+err.Error())
	}
	if len(reasons) == 0 {
		return "", "адреса для поиска не заданы"
	}
	return "", strings.Join(reasons, "; ")
}

// releaseLocked снимает со ступени R5 адрес, который поставили МЫ, и не
// трогает никакой другой.
//
// Безусловный SetProxy("") здесь стёр бы прокси пользователя в тот момент,
// когда его ещё не успели прочитать из хранилища, то есть на каждом холодном
// старте.
func (t *TorFallback) releaseLocked(l *Ladder) {
	if t.applied != "" && l.proxyAddr() == t.applied {
		l.SetProxy("")
	}
	t.applied = ""
}

// EnsureTorFallback это то, что зовут снаружи пакета: выборка перед циклом и
// обвязка перед показом экрана транспортов.
func EnsureTorFallback(ctx context.Context, l *Ladder, userProxy string) TorStatus {
	return defaultTorFallback.Ensure(ctx, l, userProxy)
}

// userEnabled сообщает, включил ли ступень ПОЛЬЗОВАТЕЛЬ.
//
// Это не то же самое, что Enabled в State(): там ступень гасится ещё и
// причиной недоступности, а ненастроенность это ровно то, что мы собираемся
// исправить. Читать State().Enabled было бы замкнутым кругом: R5 без адреса
// недоступна, значит Tor не ищем, значит адреса не будет никогда.
func (l *Ladder) userEnabled(r RungID) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.enabled[r]
}

// proxyAddr возвращает адрес, стоящий сейчас в ступени R5.
func (l *Ladder) proxyAddr() string {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.proxy
}
