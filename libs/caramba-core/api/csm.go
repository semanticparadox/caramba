package api

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/transport"
)

// Поверхность CSM/1 для клиента. Каждый метод принимает и отдаёт строку JSON,
// по образцу SetPolicyJSON: это ровно то, что проходит через все пять мостов
// (Android, Apple, Windows, Linux и gomobile) без генерации типов.
//
// Управляющий слой на Dart НЕ ИМЕЕТ ПРАВА открывать собственные сокеты к
// оператору. Иначе регистрация, вход, обновление токена и настройки обходят
// лестницу, локатор нельзя перечитать после истечения токена, и приложение
// навсегда вырождается в ступень R0, пока ядро бодро лезет по лестнице за
// конфигурацией, которую ему больше нечем изменить.

// CsmEnrollRequest это вход регистрации.
type CsmEnrollRequest struct {
	// Origin это origin регистрации. Игнорируется, когда задан BlobB64.
	Origin string `json:"origin,omitempty"`
	// Code это код регистрации, 20 символов, дефисы косметические.
	Code string `json:"code,omitempty"`
	// LinkPin это продиктованный пин корня, 20 символов base32 Crockford.
	LinkPin string `json:"link_pin,omitempty"`
	// BlobB64 это bootstrap blob: кадр 0x05 или армированный текст, base64.
	BlobB64 string `json:"blob_b64,omitempty"`
	// SubscriptionDomain это единственный хост, на который разрешён переход.
	SubscriptionDomain string `json:"subscription_domain,omitempty"`
	// AccountJWT непуст для второго и последующих устройств.
	AccountJWT string `json:"account_jwt,omitempty"`
}

// CsmSettingsRequest это запрос на изменение настроек.
type CsmSettingsRequest struct {
	// Want это карта pol: номер поля к значению. Отсутствующий ключ означает
	// "без изменений", текст "default" это сброс к умолчанию оператора.
	//
	// Значения ТИПИЗИРОВАНЫ и берутся прямо из JSON: строка, целое, булево или
	// массив строк, ровно как в карте pol директивы (03-WIRE.md 8.3). Тип
	// решает JSON вызывающего, а не таблица здесь: реестр полей живёт в
	// клиенте, а ядру нужно только не переврать то, что ему дали.
	Want map[string]json.RawMessage `json:"want,omitempty"`
	// Sel это карта sel. Все её поля текстовые.
	Sel        map[string]string `json:"sel,omitempty"`
	AccountJWT string            `json:"account_jwt,omitempty"`
}

// wantItem переводит одно значение JSON в элемент CBOR.
//
// Дробное число, объект и null отвергаются: строгий профиль разбора запрещает
// null и неопределённые длины, а дробного значения нет ни у одного поля pol.
// Молча привести их к чему-нибудь значило бы отправить оператору не то, что
// попросил пользователь.
func wantItem(raw json.RawMessage) (transport.CBORItem, wantKind, bool, error) {
	var v any
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	if err := dec.Decode(&v); err != nil {
		return transport.CBORItem{}, 0, false, err
	}
	switch t := v.(type) {
	case string:
		return transport.WantText(t), wantText, t == transport.SettingsDefaultSentinel, nil
	case bool:
		return transport.WantBool(t), wantBool, false, nil
	case json.Number:
		n, err := strconv.ParseUint(t.String(), 10, 64)
		if err != nil {
			return transport.CBORItem{}, 0, false, fmt.Errorf("api: значение %s не беззнаковое целое", t.String())
		}
		return transport.WantUint(n), wantUint, false, nil
	case []any:
		list := make([]string, 0, len(t))
		for _, e := range t {
			s, ok := e.(string)
			if !ok {
				return transport.CBORItem{}, 0, false, fmt.Errorf("api: массив значения содержит не строку")
			}
			list = append(list, s)
		}
		return transport.WantTextList(list), wantTextList, false, nil
	default:
		return transport.CBORItem{}, 0, false, fmt.Errorf("api: значение %s не строка, не целое, не булево и не массив строк", string(raw))
	}
}

// wantKind это тип значения одного поля want, 03-WIRE.md 8.3.
type wantKind int

const (
	wantText wantKind = iota
	wantUint
	wantBool
	wantTextList
)

// wantFields это ЗАКРЫТАЯ таблица полей want, 03-WIRE.md 8.3 плюс
// некритический ключ 64 (сообщение смены ключа согласования, 02-SPEC.md 10.3).
//
// Таблица живёт здесь не ради дублирования реестра клиента, а потому что
// граница ABI открыта всему процессу приложения, а инвариант 15 это запрет
// ПЕРЕДАЧИ: "split.apps MUST NOT appear in pol, MUST NOT be assigned a key in
// this table, and MUST NOT be transmitted in either direction". Проверка,
// живущая только в слое Dart, оставляет ABI шире собственного правила клиента,
// и любой другой вызывающий отправил бы оператору поле, которого в протоколе
// нет. Номера здесь не назначаются: их назначает 03-WIRE.md 8.3.
var wantFields = map[uint64]wantKind{
	1:  wantText,     // protocol
	2:  wantText,     // preset
	3:  wantText,     // relay
	4:  wantText,     // stack
	5:  wantUint,     // mtu
	6:  wantBool,     // ipv6
	7:  wantBool,     // fakeIp
	8:  wantBool,     // killSwitch
	9:  wantTextList, // dns.nameservers
	10: wantTextList, // dns.fallback
	11: wantText,     // split.mode
	64: wantText,     // некритический: смена ключа согласования, 02-SPEC.md 10.3
}

// checkWantField сверяет номер поля и тип значения с таблицей 8.3.
//
// Сброс к умолчанию оператора это текстовый сентинел "default" (01-DECISION.md
// B6), и он допустим для поля ЛЮБОГО типа: правило C7 запрещает CBOR null, и
// другого способа сказать "сбрось" в протоколе нет.
func checkWantField(key uint64, got wantKind, isDefaultSentinel bool) error {
	kind, ok := wantFields[key]
	if !ok {
		return fmt.Errorf("api: поле want %d не назначено в 03-WIRE.md 8.3", key)
	}
	if isDefaultSentinel {
		return nil
	}
	if got != kind {
		return fmt.Errorf("api: поле want %d прислано не своим типом", key)
	}
	return nil
}

// CsmLadderRequest это переключатели и порядок ступеней от пользователя.
type CsmLadderRequest struct {
	// Order это новый порядок. Ступень 0 всегда переносится в начало.
	Order []int `json:"order,omitempty"`
	// Enabled это переключатели: номер ступени к состоянию.
	Enabled map[string]bool `json:"enabled,omitempty"`
	// Proxy это адрес пользовательского прокси ступени R5. Пустая строка её
	// снимает. Прокси используется ТОЛЬКО для выборки манифеста и
	// конфигурации, никогда для трафика туннеля.
	Proxy *string `json:"proxy,omitempty"`
	// TunnelProxy это адрес локального mixed-инбаунда для ступени R4.
	TunnelProxy *string `json:"tunnel_proxy,omitempty"`
}

// LadderHTTPRequest это произвольный запрос через лестницу (ABI v3,
// CarambaLadderRequest).
type LadderHTTPRequest struct {
	Method    string            `json:"method"`
	Path      string            `json:"path"`
	Origin    string            `json:"origin,omitempty"`
	Headers   map[string]string `json:"headers,omitempty"`
	BodyB64   string            `json:"body_b64,omitempty"`
	TimeoutMs int               `json:"timeout_ms,omitempty"`
	// Rungs, если непусто, ограничивает запрос этими ступенями.
	Rungs []int `json:"rungs,omitempty"`
}

// LadderHTTPResponse это ответ лестницы.
type LadderHTTPResponse struct {
	Status  int               `json:"status"`
	Headers map[string]string `json:"headers,omitempty"`
	BodyB64 string            `json:"body_b64,omitempty"`
	Rung    int               `json:"rung"`
	Error   string            `json:"error,omitempty"`
}

// csmProfileDirName это подкаталог, в котором лежат хранилища профилей CSM.
const csmProfileDirName = "csm-profiles"

// csmDirLocked возвращает каталог хранилища CSM выбранного профиля.
//
// Пустой ключ означает рабочий каталог ядра как есть: так лежат уже
// существующие установки, и переносить их некуда.
func (c *Core) csmDirLocked() string {
	if c.csmProfileKey == "" {
		return c.workDir
	}
	return filepath.Join(c.workDir, csmProfileDirName, c.csmProfileKey)
}

// csmProfileKeyOK пропускает только безопасное имя каталога.
//
// Ключ приходит с моста, то есть из слоя Dart, и попадает в путь. Точка,
// косая черта и обратная косая здесь запрещены не из аккуратности: ключ ".."
// увёл бы хранилище личности устройства на уровень выше рабочего каталога.
func csmProfileKeyOK(k string) bool {
	if k == "" || len(k) > 64 {
		return false
	}
	for i := 0; i < len(k); i++ {
		ch := k[i]
		switch {
		case ch >= 'a' && ch <= 'z', ch >= '0' && ch <= '9', ch == '-', ch == '_':
		default:
			return false
		}
	}
	return true
}

// CsmSelectProfile переключает хранилище CSM на профиль key.
//
// 02-SPEC.md 1.2: каждое хранилище состояния профиля ОБЯЗАНО ключеваться по
// pid. До этого вызова хранилище было одно на приложение, и закреплённый
// корень, регистрация устройства, монотонные отметки и история попыток второго
// оператора ложились поверх первого: изоляция тенантов, которую CSM/1 строит
// на документах, обнулялась на слое хранения.
//
// Ключ это локальный, стабильный и неизменный идентификатор профиля (id
// профиля подключения либо pid, когда он уже известен), а не отображаемое имя.
// Пустой ключ возвращает поведение до этого вызова: хранилище в рабочем
// каталоге ядра, куда его кладут уже существующие установки.
//
// Смена ключа СБРАСЫВАЕТ текущий выборщик: следующий вызов CSM поднимет его из
// каталога нового профиля. Ключи устройства при этом тоже разные, и это верно:
// dtp у каждого оператора свой, иначе два оператора связывают одно устройство
// по общему отпечатку.
func (c *Core) CsmSelectProfile(key string) error {
	key = strings.TrimSpace(strings.ToLower(key))
	if key != "" && !csmProfileKeyOK(key) {
		return fmt.Errorf("api: ключ профиля CSM %q допускает только [a-z0-9_-] длиной до 64", key)
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.csmProfileKey == key {
		return nil
	}
	c.csmProfileKey = key
	c.csm = nil
	return nil
}

// CsmProfileKey отдаёт выбранный ключ профиля CSM.
func (c *Core) CsmProfileKey() string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.csmProfileKey
}

// csmFetcher возвращает выборщик, создавая его при первом обращении.
//
// Лениво, а не в NewCore: конструктор выборщика при первом запуске порождает и
// пишет на диск пару ключей устройства, то есть долгоживущий идентификатор.
// Создавать его как побочный эффект запуска ядра у каждой установки, включая
// тех, кто никогда не воспользуется CSM, значит завести идентификатор без
// действия пользователя и без поверхности, где его видно.
func (c *Core) csmFetcher() (*transport.Fetcher, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.csm != nil {
		return c.csm, nil
	}
	if c.ladder == nil {
		return nil, fmt.Errorf("api: CSM недоступен в этой сборке")
	}
	// Мост, если он установлен, побеждает программные ключи: аппаратное
	// хранилище это и есть точный ответ на вопрос, где лежит ключ устройства.
	var keys transport.DeviceKeys
	if c.deviceBridge != nil {
		keys = transport.NewBridgeDeviceKeys(c.deviceBridge)
	}
	f, err := transport.NewFetcher(c.csmDirLocked(), "", c.ladder, keys)
	if err != nil {
		return nil, fmt.Errorf("api: хранилище CSM: %w", err)
	}
	// Отказ загрузки не фатален: профиль без регистрации это нормальное
	// состояние, а испорченное хранилище требует перепроверки из сети.
	_ = f.LoadCached()
	c.csm = f
	return c.csm, nil
}

// CsmEnroll выполняет регистрацию и возвращает снимок состояния.
func (c *Core) CsmEnroll(ctx context.Context, jsonStr string) (string, error) {
	f, err := c.csmFetcher()
	if err != nil {
		return "", err
	}
	var req CsmEnrollRequest
	if err := json.Unmarshal([]byte(jsonStr), &req); err != nil {
		return "", fmt.Errorf("api: разбор запроса регистрации: %w", err)
	}
	opt := transport.EnrollOptions{
		Origin:             req.Origin,
		Code:               req.Code,
		LinkPin:            req.LinkPin,
		SubscriptionDomain: req.SubscriptionDomain,
		AccountJWT:         req.AccountJWT,
	}
	// Второе и последующие устройства регистрируются БЕЗ кода, под токеном
	// аккаунта (02-SPEC.md 9.6). Токен уже лежит в auth.PanelClient после
	// Configure, и заставлять управляющий слой на Dart вынимать его оттуда
	// и класть в этот запрос значило бы возить секрет через границу ради
	// значения, которое ядро и так держит. Явно переданный токен побеждает.
	if opt.AccountJWT == "" && opt.Code == "" {
		opt.AccountJWT = c.storedAccountJWT(ctx)
	}
	if req.BlobB64 != "" {
		blob, err := decodeB64(req.BlobB64)
		if err != nil {
			return "", fmt.Errorf("api: bootstrap blob: %w", err)
		}
		opt.Blob = blob
	}
	if err := f.Enroll(ctx, opt); err != nil {
		return "", err
	}
	return toJSONString(f.Snapshot())
}

// CsmRefresh выполняет один цикл выборки: директива, при необходимости
// каталог.
//
// Ошибка здесь НЕ означает потерю конфигурации: профиль остаётся на
// кешированных документах и продолжает подключать. Это инвариант 16, и он
// абсолютный.
func (c *Core) CsmRefresh(ctx context.Context) (string, error) {
	f, err := c.csmFetcher()
	if err != nil {
		return "", err
	}
	rerr := f.Refresh(ctx)
	snap, jerr := toJSONString(f.Snapshot())
	if rerr != nil {
		return snap, rerr
	}
	return snap, jerr
}

// CsmStateJSON отдаёт проверенное состояние: личность оператора, версии, срок,
// отпечаток подписавшего, битовое поле возможностей, возраст конфигурации и
// список ступеней.
func (c *Core) CsmStateJSON() (string, error) {
	f, err := c.csmFetcher()
	if err != nil {
		return "", err
	}
	return toJSONString(f.Snapshot())
}

// CsmLadderJSON отдаёт состояние всех скомпилированных ступеней и историю
// попыток. История локальная и наружу оператору не уходит.
func (c *Core) CsmLadderJSON() (string, error) {
	c.mu.Lock()
	l := c.ladder
	c.mu.Unlock()
	if l == nil {
		return "", fmt.Errorf("api: лестница недоступна")
	}
	// Выборщик здесь не обязателен: список ступеней существует и до
	// регистрации. Без него доставившая ступень неизвестна, и так и
	// сообщается.
	f, _ := c.csmFetcher()
	userProxy := ""
	delivered := ""
	if f != nil {
		st := f.Store().State()
		userProxy = st.LadderProxy
		if st.FetchedAt > 0 {
			delivered = st.LastRung.Name()
		}
	}
	// Состояние локального Tor обновляется ровно здесь, когда экран
	// транспортов его спрашивает. Внутри Ensure стоит и выключатель (R5
	// выключена — пробы нет), и срок годности ответа, поэтому опрос экрана раз
	// в три секунды не превращается в пробу раз в три секунды.
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	tor := transport.EnsureTorFallback(ctx, l, userProxy)
	return toJSONString(struct {
		Rungs      []transport.RungState `json:"rungs"`
		History    []transport.Attempt   `json:"history"`
		Thresholds transport.Thresholds  `json:"thresholds"`
		BackoffMs  int64                 `json:"backoff_ms"`
		// Tor это состояние резервного пути ступени R5 через локальный Tor.
		// Он ВСЕГДА в ответе: "не искали" и "не нашли" это разные строки на
		// экране, и обе обязаны быть произносимы (инвариант 17).
		Tor transport.TorStatus `json:"tor"`
		// Delivered это имя ступени, принесшей последнюю ПРИНЯТУЮ директиву.
		// Пусто означает, что не приносила ещё ни одна; это не то же самое,
		// что R0.
		Delivered string `json:"delivered,omitempty"`
	}{
		Rungs:      l.State(),
		History:    l.History(),
		Thresholds: l.Thresholds(),
		BackoffMs:  func() int64 { d, _ := l.Backoff(); return d.Milliseconds() }(),
		Tor:        tor,
		Delivered:  delivered,
	})
}

// CsmSetLadderJSON применяет выбор пользователя. Ступени 0 и 6 выключить
// нельзя, и попытка это сделать возвращает ошибку, а не тихо игнорируется.
func (c *Core) CsmSetLadderJSON(jsonStr string) error {
	c.mu.Lock()
	l := c.ladder
	c.mu.Unlock()
	if l == nil {
		return fmt.Errorf("api: лестница недоступна")
	}
	// Выборщик здесь не обязателен: порядок и включённость ступеней это
	// настройка транспорта, а не состояние регистрации.
	f, _ := c.csmFetcher()
	var req CsmLadderRequest
	if err := json.Unmarshal([]byte(jsonStr), &req); err != nil {
		return fmt.Errorf("api: разбор настроек лестницы: %w", err)
	}
	if len(req.Order) > 0 {
		order := make([]transport.RungID, 0, len(req.Order))
		for _, r := range req.Order {
			if r < 0 || r > int(transport.MaxRung) {
				return fmt.Errorf("api: ступень %d вне диапазона", r)
			}
			order = append(order, transport.RungID(r))
		}
		if err := l.SetOrder(order); err != nil {
			return err
		}
	}
	for k, on := range req.Enabled {
		var n int
		if _, err := fmt.Sscanf(k, "%d", &n); err != nil {
			return fmt.Errorf("api: ступень %q не число", k)
		}
		if err := l.SetEnabled(transport.RungID(n), on); err != nil {
			return err
		}
	}
	if req.Proxy != nil {
		l.SetProxy(*req.Proxy)
	}
	if req.TunnelProxy != nil {
		l.SetTunnelProxy(*req.TunnelProxy)
	}
	// Выбор пользователя переживает смену каталога, поэтому он сохраняется.
	if f != nil {
		_ = f.Store().Update(func(s *transport.State) {
			if s.LadderEnabled == nil {
				s.LadderEnabled = map[string]bool{}
			}
			for _, st := range l.State() {
				if st.UserSet {
					s.LadderEnabled[fmt.Sprintf("%d", st.Rung)] = st.Enabled
				}
			}
			order := make([]transport.RungID, 0, len(l.State()))
			for _, st := range l.State() {
				order = append(order, st.Rung)
			}
			s.LadderOrder = order
			if req.Proxy != nil {
				s.LadderProxy = *req.Proxy
			}
		})
	}
	return nil
}

// CsmAnswerCatalogChangeJSON передаёт ядру ответ пользователя на карточку
// смены набора rule-set и geo-файлов (02-SPEC.md 7.7.1, INV-22).
//
// Без этого символа кнопка "Оставить прежние" не откатывает ничего: страж
// ресурсов живёт в ядре, а карточка на Dart, и ответ, оставшийся в Dart,
// изменил бы только то, о чём приложение спросит в следующий раз.
//
// Вход: {"accept":true|false}. Ответ {"answered":bool} говорит, было ли на что
// отвечать: карточка, отвеченная дважды, это не ошибка.
func (c *Core) CsmAnswerCatalogChangeJSON(jsonStr string) (string, error) {
	f, err := c.csmFetcher()
	if err != nil {
		return "", err
	}
	var req struct {
		Accept bool `json:"accept"`
	}
	if strings.TrimSpace(jsonStr) != "" {
		if err := json.Unmarshal([]byte(jsonStr), &req); err != nil {
			return "", fmt.Errorf("api: разбор ответа на смену каталога: %w", err)
		}
	}
	return toJSONString(struct {
		Answered bool `json:"answered"`
	}{Answered: f.AnswerCatalogChange(req.Accept)})
}

// CsmRequestSettingsJSON отправляет изменение настроек как подписанный запрос
// и принимает подписанный и запечатанный ответ как новую директиву.
func (c *Core) CsmRequestSettingsJSON(ctx context.Context, jsonStr string) (string, error) {
	f, err := c.csmFetcher()
	if err != nil {
		return "", err
	}
	var req CsmSettingsRequest
	if err := json.Unmarshal([]byte(jsonStr), &req); err != nil {
		return "", fmt.Errorf("api: разбор запроса настроек: %w", err)
	}
	w := transport.SettingsWrite{
		Want:       map[uint64]transport.CBORItem{},
		Sel:        map[uint64]string{},
		AccountJWT: req.AccountJWT,
	}
	// PUT /api/v2/app/preferences живёт в защищённой группе (03-WIRE.md 13.2),
	// поэтому токен обязателен. Берём тот же, что и остальные запросы ядра.
	if w.AccountJWT == "" {
		w.AccountJWT = c.storedAccountJWT(ctx)
	}
	for k, v := range req.Want {
		var n uint64
		if _, err := fmt.Sscanf(k, "%d", &n); err != nil {
			return "", fmt.Errorf("api: ключ want %q не число", k)
		}
		item, kind, isDefault, err := wantItem(v)
		if err != nil {
			return "", fmt.Errorf("api: значение want %q: %w", k, err)
		}
		if err := checkWantField(n, kind, isDefault); err != nil {
			return "", err
		}
		w.Want[n] = item
	}
	for k, v := range req.Sel {
		var n uint64
		if _, err := fmt.Sscanf(k, "%d", &n); err != nil {
			return "", fmt.Errorf("api: ключ sel %q не число", k)
		}
		w.Sel[n] = v
	}
	if err := f.RequestSettings(ctx, w); err != nil {
		return "", err
	}
	return toJSONString(f.Snapshot())
}

// LadderRequestJSON выполняет произвольный HTTP запрос через лестницу.
// Это символ CarambaLadderRequest из ABI v3.
func (c *Core) LadderRequestJSON(ctx context.Context, jsonStr string) (string, error) {
	c.mu.Lock()
	l := c.ladder
	origin := c.cfg.PanelBaseURL
	subDomain := c.cfg.SubscriptionDomain
	c.mu.Unlock()
	if l == nil {
		return "", fmt.Errorf("api: лестница недоступна")
	}
	var req LadderHTTPRequest
	if err := json.Unmarshal([]byte(jsonStr), &req); err != nil {
		return "", fmt.Errorf("api: разбор запроса лестницы: %w", err)
	}
	if req.Origin != "" {
		origin = req.Origin
	}
	norm, err := transport.NormalizeOrigin(origin)
	if err != nil {
		return toJSONString(LadderHTTPResponse{Error: err.Error()})
	}
	if req.Path == "" || !strings.HasPrefix(req.Path, "/") {
		return toJSONString(LadderHTTPResponse{Error: "api: путь обязан начинаться с /"})
	}
	method := strings.ToUpper(strings.TrimSpace(req.Method))
	if method == "" {
		method = http.MethodGet
	}
	var body io.Reader
	var bodyLen int
	if req.BodyB64 != "" {
		b, err := decodeB64(req.BodyB64)
		if err != nil {
			return toJSONString(LadderHTTPResponse{Error: err.Error()})
		}
		body = bytes.NewReader(b)
		bodyLen = len(b)
	}
	timeout := time.Duration(req.TimeoutMs) * time.Millisecond
	if timeout <= 0 {
		timeout = transport.CycleBudget
	}
	rctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	hreq, err := http.NewRequestWithContext(rctx, method, norm+req.Path, body)
	if err != nil {
		return toJSONString(LadderHTTPResponse{Error: err.Error()})
	}
	hreq.ContentLength = int64(bodyLen)
	for k, v := range req.Headers {
		hreq.Header.Set(k, v)
	}
	allow := make([]transport.RungID, 0, len(req.Rungs))
	for _, r := range req.Rungs {
		allow = append(allow, transport.RungID(r))
	}
	resp, err := l.Do(rctx, hreq, transport.DoOptions{
		Origin: norm, SubscriptionDomain: subDomain, AllowRungs: allow, Force: true,
	})
	if err != nil {
		return toJSONString(LadderHTTPResponse{Error: err.Error()})
	}
	out := LadderHTTPResponse{
		Status:  resp.Status,
		Rung:    int(resp.Rung),
		BodyB64: base64.StdEncoding.EncodeToString(resp.Body),
		Headers: map[string]string{},
	}
	for k := range resp.Header {
		out.Headers[k] = resp.Header.Get(k)
	}
	return toJSONString(out)
}

// storedAccountJWT отдаёт действующий access-токен из auth.PanelClient,
// обновляя его при необходимости. Пустая строка означает "не залогинены", и это
// НЕ ошибка здесь: регистрация по коду токена не требует, а запись настроек
// упрётся в 401 от панели, что и есть правильный текст для пользователя.
func (c *Core) storedAccountJWT(ctx context.Context) string {
	c.mu.Lock()
	a := c.auth
	c.mu.Unlock()
	if a == nil || !a.IsAuthenticated() {
		return ""
	}
	tok, err := a.AccessToken(ctx)
	if err != nil {
		return ""
	}
	return tok
}

// decodeB64 принимает стандартный и URL-безопасный base64, с дополнением и без.
func decodeB64(s string) ([]byte, error) {
	s = strings.TrimSpace(s)
	for _, enc := range []*base64.Encoding{
		base64.StdEncoding, base64.RawStdEncoding,
		base64.URLEncoding, base64.RawURLEncoding,
	} {
		if b, err := enc.DecodeString(s); err == nil {
			return b, nil
		}
	}
	return nil, fmt.Errorf("api: значение не является base64")
}

func toJSONString(v any) (string, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return "", err
	}
	return string(b), nil
}
