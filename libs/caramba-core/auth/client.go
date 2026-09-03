// Package auth реализует клиент аутентификации к панели caramba.
//
// PanelClient умеет регистрироваться и логиниться по email/паролю, логиниться
// через Telegram, обновлять и отзывать токены. Токены хранятся через интерфейс
// Store (по умолчанию — файловое хранилище в каталоге конфигурации
// пользователя). Запросы, требующие авторизации, автоматически повторяются
// после обновления access-токена при ответе 401.
package auth

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"
)

// DefaultUserAgent — User-Agent по умолчанию для запросов к API панели.
const DefaultUserAgent = "caramba-core/1.0"

// expirySkew — запас, с которым access-токен считается истёкшим, чтобы успеть
// обновиться до фактического протухания.
const expirySkew = 30 * time.Second

// ErrNotAuthenticated возвращается, когда операция требует авторизации, но
// токенов нет.
var ErrNotAuthenticated = errors.New("auth: не выполнен вход")

// ErrUnauthorized возвращается, когда панель отвергла учётные данные или токен.
var ErrUnauthorized = errors.New("auth: неавторизован")

// HTTPDoer — минимальный интерфейс HTTP-клиента (для тестируемости).
type HTTPDoer interface {
	Do(req *http.Request) (*http.Response, error)
}

// PanelClient — клиент API аутентификации панели caramba.
type PanelClient struct {
	baseURL   string
	http      HTTPDoer
	store     Store
	userAgent string

	mu     sync.Mutex // защищает tokens
	tokens Tokens
	loaded bool
}

// Option настраивает PanelClient.
type Option func(*PanelClient)

// WithHTTPClient задаёт пользовательский HTTP-клиент.
func WithHTTPClient(d HTTPDoer) Option {
	return func(c *PanelClient) { c.http = d }
}

// WithStore задаёт хранилище токенов.
func WithStore(s Store) Option {
	return func(c *PanelClient) { c.store = s }
}

// WithUserAgent задаёт User-Agent.
func WithUserAgent(ua string) Option {
	return func(c *PanelClient) { c.userAgent = ua }
}

// NewPanelClient создаёт клиент. baseURL — корневой URL панели
// (например, "https://exarobot.top"). Если хранилище не задано, токены
// держатся только в памяти.
func NewPanelClient(baseURL string, opts ...Option) *PanelClient {
	c := &PanelClient{
		baseURL:   strings.TrimRight(baseURL, "/"),
		http:      &http.Client{Timeout: 30 * time.Second},
		store:     NewMemoryStore(),
		userAgent: DefaultUserAgent,
	}
	for _, o := range opts {
		o(c)
	}
	return c
}

// HTTPDoer возвращает текущий HTTP-клиент.
//
// Существует ради одной проверки, которую иначе невозможно написать: что
// api.NewCore и api.SetPanelURL ОБА передали сюда лестницу транспортов.
// Забыть второе место означает молча вернуть перерегистрированного арендатора
// к собственному ClientHello Go, и находится это через полгода.
func (c *PanelClient) HTTPDoer() HTTPDoer { return c.http }

// --- DTO запросов/ответов панели ---

type registerEmailRequest struct {
	Email    string `json:"email"`
	Password string `json:"password"`
}

type loginEmailRequest struct {
	Email    string `json:"email"`
	Password string `json:"password"`
}

// TelegramLogin — данные виджета Telegram Login, проверяемые панелью по HMAC.
type TelegramLogin struct {
	ID        int64  `json:"id"`
	FirstName string `json:"first_name,omitempty"`
	LastName  string `json:"last_name,omitempty"`
	Username  string `json:"username,omitempty"`
	PhotoURL  string `json:"photo_url,omitempty"`
	AuthDate  int64  `json:"auth_date"`
	Hash      string `json:"hash"`
}

type loginCodeRequest struct {
	Code string `json:"code"`
}

type refreshRequest struct {
	RefreshToken string `json:"refresh_token"`
}

type logoutRequest struct {
	RefreshToken string `json:"refresh_token"`
}

// tokenResponse — стандартный ответ с парой токенов. Поле expires_in (секунды)
// необязательно.
type tokenResponse struct {
	AccessToken  string `json:"access_token"`
	RefreshToken string `json:"refresh_token"`
	ExpiresIn    int64  `json:"expires_in,omitempty"`
}

func (r tokenResponse) toTokens() Tokens {
	t := Tokens{AccessToken: r.AccessToken, RefreshToken: r.RefreshToken}
	if r.ExpiresIn > 0 {
		t.AccessExpiry = time.Now().Add(time.Duration(r.ExpiresIn) * time.Second)
	}
	return t
}

type apiError struct {
	Error   string `json:"error,omitempty"`
	Message string `json:"message,omitempty"`
}

// --- Публичные методы ---

// RegisterEmail регистрирует нового пользователя по email и паролю, сохраняет
// выданные токены и возвращает их.
func (c *PanelClient) RegisterEmail(ctx context.Context, email, password string) (Tokens, error) {
	body := registerEmailRequest{Email: email, Password: password}
	tr, err := c.postToken(ctx, "/api/v2/app/register", body)
	if err != nil {
		return Tokens{}, fmt.Errorf("auth: регистрация по email: %w", err)
	}
	return c.persist(tr.toTokens())
}

// LoginEmail выполняет вход по email и паролю.
func (c *PanelClient) LoginEmail(ctx context.Context, email, password string) (Tokens, error) {
	body := loginEmailRequest{Email: email, Password: password}
	tr, err := c.postToken(ctx, "/api/v2/app/login/email", body)
	if err != nil {
		return Tokens{}, fmt.Errorf("auth: вход по email: %w", err)
	}
	return c.persist(tr.toTokens())
}

// LoginCode выполняет вход по одноразовому коду, выданному ботом Telegram.
// Пользователь получает 6-значный код командой /login в боте; панель сверяет
// код с Redis, привязывает его к пользователю и выдаёт пару токенов так же,
// как при входе по email/Telegram.
func (c *PanelClient) LoginCode(ctx context.Context, code string) (Tokens, error) {
	body := loginCodeRequest{Code: code}
	tr, err := c.postToken(ctx, "/api/v2/app/login/code", body)
	if err != nil {
		return Tokens{}, fmt.Errorf("auth: вход по коду: %w", err)
	}
	return c.persist(tr.toTokens())
}

// LoginTelegram выполняет вход по данным виджета Telegram Login.
func (c *PanelClient) LoginTelegram(ctx context.Context, data TelegramLogin) (Tokens, error) {
	tr, err := c.postToken(ctx, "/api/v2/app/login/telegram", data)
	if err != nil {
		return Tokens{}, fmt.Errorf("auth: вход через Telegram: %w", err)
	}
	return c.persist(tr.toTokens())
}

// Refresh обновляет пару токенов по refresh-токену.
func (c *PanelClient) Refresh(ctx context.Context) (Tokens, error) {
	if err := c.ensureLoaded(); err != nil {
		return Tokens{}, err
	}
	c.mu.Lock()
	refresh := c.tokens.RefreshToken
	c.mu.Unlock()
	if refresh == "" {
		return Tokens{}, ErrNotAuthenticated
	}
	tr, err := c.postToken(ctx, "/api/v2/app/refresh", refreshRequest{RefreshToken: refresh})
	if err != nil {
		return Tokens{}, fmt.Errorf("auth: обновление токена: %w", err)
	}
	return c.persist(tr.toTokens())
}

// Logout отзывает refresh-токен на панели и очищает локальное хранилище.
func (c *PanelClient) Logout(ctx context.Context) error {
	if err := c.ensureLoaded(); err != nil {
		return err
	}
	c.mu.Lock()
	refresh := c.tokens.RefreshToken
	c.mu.Unlock()

	if refresh != "" {
		// Ошибку сети при отзыве не считаем фатальной: локально всё равно чистим.
		req, err := c.newRequest(ctx, http.MethodPost, "/api/v2/app/logout", logoutRequest{RefreshToken: refresh})
		if err == nil {
			if resp, derr := c.http.Do(req); derr == nil {
				_ = drainAndClose(resp.Body)
			}
		}
	}

	c.mu.Lock()
	c.tokens = Tokens{}
	c.mu.Unlock()
	if err := c.store.Clear(); err != nil {
		return fmt.Errorf("auth: очистка хранилища: %w", err)
	}
	return nil
}

// SetTokens инъецирует извне выданную пару токенов (минуя сетевой вход) в память
// и в хранилище — тем же путём persist(), что и сетевые Login*/Refresh.
//
// Назначение — «бесшовный» handoff JWT из приложения: Flutter-клиент уже
// аутентифицирован собственным Dart ApiClient, и вместо повторного входа ядра он
// передаёт сюда access-токен (и, желательно, refresh-токен). После инъекции
// IsAuthenticated()==true, AccessToken(ctx) отдаёт переданный токен, а DoAuthorized
// подставляет его в авторизованные запросы (подписка, subscription UUID).
//
// ВАЖНО про деградацию: если refresh пуст, авто-обновление по истечении access не
// сработает (Refresh вернёт ErrNotAuthenticated при пустом refresh), и при ответе
// 401 авторизованный запрос упадёт — приложение должно заново вызвать SetTokens с
// новым access-токеном (re-Configure). С непустым refresh поведение совпадает с
// сетевым входом: токен обновляется автоматически.
//
// accessExpiry — момент истечения access-токена (UTC); нулевое значение означает
// «срок неизвестен» (авто-обновление тогда только по 401, как и в toTokens()).
func (c *PanelClient) SetTokens(accessToken, refreshToken string, accessExpiry time.Time) (Tokens, error) {
	if accessToken == "" {
		return Tokens{}, ErrNotAuthenticated
	}
	return c.persist(Tokens{
		AccessToken:  accessToken,
		RefreshToken: refreshToken,
		AccessExpiry: accessExpiry,
	})
}

// IsAuthenticated сообщает, есть ли сохранённые токены.
func (c *PanelClient) IsAuthenticated() bool {
	if err := c.ensureLoaded(); err != nil {
		return false
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.tokens.Valid()
}

// AccessToken возвращает действующий access-токен, при необходимости обновляя
// его. Используется другими пакетами (subscription) для авторизованных вызовов.
func (c *PanelClient) AccessToken(ctx context.Context) (string, error) {
	if err := c.ensureLoaded(); err != nil {
		return "", err
	}
	c.mu.Lock()
	t := c.tokens
	c.mu.Unlock()
	if !t.Valid() {
		return "", ErrNotAuthenticated
	}
	if t.expired(expirySkew) {
		nt, err := c.Refresh(ctx)
		if err != nil {
			return "", err
		}
		return nt.AccessToken, nil
	}
	return t.AccessToken, nil
}

// BaseURL возвращает корневой URL панели.
func (c *PanelClient) BaseURL() string { return c.baseURL }

// UserAgent возвращает User-Agent клиента.
func (c *PanelClient) UserAgent() string { return c.userAgent }

// DoAuthorized выполняет произвольный авторизованный запрос с подстановкой
// Bearer-токена и авто-обновлением при 401. Тело ответа возвращается вызывающему
// (он обязан его закрыть). Это общий механизм, на котором строится клиент
// подписки.
func (c *PanelClient) DoAuthorized(req *http.Request) (*http.Response, error) {
	token, err := c.AccessToken(req.Context())
	if err != nil {
		return nil, err
	}
	// Клонируем запрос, чтобы можно было безопасно повторить.
	first, body, err := cloneRequest(req)
	if err != nil {
		return nil, err
	}
	first.Header.Set("Authorization", "Bearer "+token)
	if first.Header.Get("User-Agent") == "" {
		first.Header.Set("User-Agent", c.userAgent)
	}

	resp, err := c.http.Do(first)
	if err != nil {
		return nil, fmt.Errorf("auth: авторизованный запрос: %w", err)
	}
	if resp.StatusCode != http.StatusUnauthorized {
		return resp, nil
	}

	// 401 → пробуем обновить токен и повторить один раз.
	_ = drainAndClose(resp.Body)
	if _, rerr := c.Refresh(req.Context()); rerr != nil {
		return nil, fmt.Errorf("auth: повторная авторизация: %w", rerr)
	}
	token, err = c.AccessToken(req.Context())
	if err != nil {
		return nil, err
	}
	retry, _, err := cloneRequestWithBody(req, body)
	if err != nil {
		return nil, err
	}
	retry.Header.Set("Authorization", "Bearer "+token)
	if retry.Header.Get("User-Agent") == "" {
		retry.Header.Set("User-Agent", c.userAgent)
	}
	resp, err = c.http.Do(retry)
	if err != nil {
		return nil, fmt.Errorf("auth: повтор авторизованного запроса: %w", err)
	}
	return resp, nil
}

// --- Внутренние помощники ---

// ensureLoaded лениво подгружает токены из хранилища один раз.
func (c *PanelClient) ensureLoaded() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.loaded {
		return nil
	}
	t, err := c.store.Load()
	if err != nil {
		return fmt.Errorf("auth: загрузка токенов: %w", err)
	}
	c.tokens = t
	c.loaded = true
	return nil
}

// persist сохраняет токены в память и в хранилище.
func (c *PanelClient) persist(t Tokens) (Tokens, error) {
	c.mu.Lock()
	c.tokens = t
	c.loaded = true
	c.mu.Unlock()
	if err := c.store.Save(t); err != nil {
		return t, fmt.Errorf("auth: сохранение токенов: %w", err)
	}
	return t, nil
}

// postToken выполняет POST с JSON-телом и разбирает ответ с парой токенов.
func (c *PanelClient) postToken(ctx context.Context, path string, body any) (tokenResponse, error) {
	req, err := c.newRequest(ctx, http.MethodPost, path, body)
	if err != nil {
		return tokenResponse{}, err
	}
	resp, err := c.http.Do(req)
	if err != nil {
		return tokenResponse{}, fmt.Errorf("выполнение запроса: %w", err)
	}
	defer drainAndClose(resp.Body)

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return tokenResponse{}, fmt.Errorf("чтение ответа: %w", err)
	}
	if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden {
		return tokenResponse{}, fmt.Errorf("%w: %s", ErrUnauthorized, apiMessage(data))
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return tokenResponse{}, fmt.Errorf("панель вернула %d: %s", resp.StatusCode, apiMessage(data))
	}
	var tr tokenResponse
	if err := json.Unmarshal(data, &tr); err != nil {
		return tokenResponse{}, fmt.Errorf("разбор токенов: %w", err)
	}
	if tr.AccessToken == "" || tr.RefreshToken == "" {
		return tokenResponse{}, errors.New("панель вернула пустые токены")
	}
	return tr, nil
}

// newRequest собирает JSON-запрос с базовыми заголовками.
func (c *PanelClient) newRequest(ctx context.Context, method, path string, body any) (*http.Request, error) {
	var reader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return nil, fmt.Errorf("сериализация тела: %w", err)
		}
		reader = bytes.NewReader(data)
	}
	req, err := http.NewRequestWithContext(ctx, method, c.baseURL+path, reader)
	if err != nil {
		return nil, fmt.Errorf("создание запроса: %w", err)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", c.userAgent)
	return req, nil
}

// apiMessage пытается извлечь человекочитаемое сообщение об ошибке из тела.
func apiMessage(data []byte) string {
	var e apiError
	if json.Unmarshal(data, &e) == nil {
		if e.Message != "" {
			return e.Message
		}
		if e.Error != "" {
			return e.Error
		}
	}
	s := strings.TrimSpace(string(data))
	if len(s) > 256 {
		s = s[:256]
	}
	return s
}

// drainAndClose дочитывает и закрывает тело ответа (для переиспользования
// keep-alive соединений).
func drainAndClose(body io.ReadCloser) error {
	if body == nil {
		return nil
	}
	_, _ = io.Copy(io.Discard, body)
	return body.Close()
}

// cloneRequest клонирует запрос вместе с буферизованным телом, чтобы запрос
// можно было повторить.
func cloneRequest(req *http.Request) (*http.Request, []byte, error) {
	var body []byte
	if req.Body != nil {
		b, err := io.ReadAll(req.Body)
		if err != nil {
			return nil, nil, fmt.Errorf("auth: чтение тела запроса: %w", err)
		}
		_ = req.Body.Close()
		body = b
	}
	return cloneRequestWithBody(req, body)
}

// cloneRequestWithBody создаёт копию запроса с заданным телом.
func cloneRequestWithBody(req *http.Request, body []byte) (*http.Request, []byte, error) {
	cloned := req.Clone(req.Context())
	if body != nil {
		cloned.Body = io.NopCloser(bytes.NewReader(body))
		cloned.ContentLength = int64(len(body))
		cloned.GetBody = func() (io.ReadCloser, error) {
			return io.NopCloser(bytes.NewReader(body)), nil
		}
	}
	return cloned, body, nil
}
