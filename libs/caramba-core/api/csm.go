package api

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
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
	// "без изменений", "default" это сброс к умолчанию оператора.
	Want map[string]string `json:"want,omitempty"`
	// Sel это карта sel.
	Sel        map[string]string `json:"sel,omitempty"`
	AccountJWT string            `json:"account_jwt,omitempty"`
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
	f, err := transport.NewFetcher(c.workDir, "", c.ladder, nil)
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
	return toJSONString(struct {
		Rungs      []transport.RungState `json:"rungs"`
		History    []transport.Attempt   `json:"history"`
		Thresholds transport.Thresholds  `json:"thresholds"`
		BackoffMs  int64                 `json:"backoff_ms"`
	}{
		Rungs:      l.State(),
		History:    l.History(),
		Thresholds: l.Thresholds(),
		BackoffMs:  func() int64 { d, _ := l.Backoff(); return d.Milliseconds() }(),
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
		Want:       map[uint64]string{},
		Sel:        map[uint64]string{},
		AccountJWT: req.AccountJWT,
	}
	for k, v := range req.Want {
		var n uint64
		if _, err := fmt.Sscanf(k, "%d", &n); err != nil {
			return "", fmt.Errorf("api: ключ want %q не число", k)
		}
		w.Want[n] = v
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
