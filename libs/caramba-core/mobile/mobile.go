// Package mobile — плоский экспортируемый фасад caramba-core для gomobile bind.
//
// gomobile bind генерирует нативные привязки (Android AAR / iOS xcframework) и
// поддерживает в экспортируемых сигнатурах только ограниченный набор типов:
// string, int (и другие целые), bool, float, []byte, error и *указатели на
// экспортируемые структуры этого пакета*. Карты, срезы произвольных структур,
// дженерики и интерфейсы из других пакетов через границу не проходят.
//
// Поэтому здесь всё богатое состояние (Status, рекомендация автоподбора,
// списки) отдаётся как JSON-строка, а на вход списки приходят разделёнными
// запятой строками. Бизнес-логика не дублируется — это тонкая обёртка над
// api.Core.
//
// Использование с платформ:
//
//	Android: `gomobile bind -target=android` → caramba.aar (классы пакета
//	         `mobile`). Kotlin/Java вызывает Mobile.newClient(...), client.up(...)
//	         и т.п. TUN fd берётся из VpnService.Builder.establish().detachFd().
//	iOS:     `gomobile bind -target=ios` → exarobot.xcframework. Swift вызывает
//	         MobileNewClient(...) и методы клиента. TUN fd берётся из
//	         NEPacketTunnelProvider (tunnelFileDescriptor).
//
// Один Client держит один api.Core на весь жизненный цикл приложения, поэтому
// состояние туннеля согласовано между up/down/status (в отличие от тонкого CLI).
package mobile

import (
	"context"
	"encoding/json"
	"strings"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/api"
	"github.com/semanticparadox/caramba/libs/caramba-core/auth"
)

// Client — gomobile-дружественная обёртка над api.Core.
//
// Экспортируется как класс/тип в нативных привязках. Создаётся через NewClient.
type Client struct {
	core *api.Core
}

// NewClient создаёт клиента. panelURL обязателен; subURL/workDir/tokenPath могут
// быть пустыми (тогда берутся значения по умолчанию). Возвращает ошибку, если не
// удалось подготовить рабочий каталог или хранилище токенов.
func NewClient(panelURL, subURL, workDir, tokenPath string) (*Client, error) {
	core, err := api.NewCore(api.Config{
		PanelBaseURL:   panelURL,
		SubBaseURL:     subURL,
		WorkDir:        workDir,
		TokenStorePath: tokenPath,
	})
	if err != nil {
		return nil, err
	}
	return &Client{core: core}, nil
}

// timeoutCtx — контекст с таймаутом для сетевых операций (gomobile не
// прокидывает context.Context через границу, поэтому создаём его внутри).
func timeoutCtx(seconds int) (context.Context, context.CancelFunc) {
	if seconds <= 0 {
		seconds = 30
	}
	return context.WithTimeout(context.Background(), time.Duration(seconds)*time.Second)
}

// --- Аутентификация ---

// Login выполняет вход по email и паролю. Возвращает ошибку при неуспехе.
func (c *Client) Login(email, password string) error {
	ctx, cancel := timeoutCtx(30)
	defer cancel()
	_, err := c.core.LoginEmail(ctx, email, password)
	return err
}

// Register регистрирует новый аккаунт по email/паролю и сразу выполняет вход.
func (c *Client) Register(email, password string) error {
	ctx, cancel := timeoutCtx(30)
	defer cancel()
	_, err := c.core.RegisterEmail(ctx, email, password)
	return err
}

// LoginCode выполняет вход по одноразовому 6-значному коду из Telegram-бота
// Caramba. Пользователь получает код командой /login в боте, вводит его в
// приложении; панель сверяет код с Redis и выдаёт токены.
func (c *Client) LoginCode(code string) error {
	ctx, cancel := timeoutCtx(30)
	defer cancel()
	_, err := c.core.LoginCode(ctx, code)
	return err
}

// LoginTelegram выполняет вход по данным виджета Telegram Login (панель
// проверяет подпись по HMAC). authDate — unix-секунды поля auth_date виджета;
// hash — поле hash. username необязателен.
//
// gomobile не пропускает структуру auth.TelegramLogin через границу, поэтому
// поля передаются по отдельности.
func (c *Client) LoginTelegram(telegramID int64, username, hash string, authDate int64) error {
	ctx, cancel := timeoutCtx(30)
	defer cancel()
	_, err := c.core.LoginTelegram(ctx, auth.TelegramLogin{
		ID:       telegramID,
		Username: username,
		AuthDate: authDate,
		Hash:     hash,
	})
	return err
}

// Logout завершает сеанс: гасит туннель и отзывает токены.
func (c *Client) Logout() error {
	ctx, cancel := timeoutCtx(15)
	defer cancel()
	return c.core.Logout(ctx)
}

// SetSubscriptionID привязывает клиента к UUID подписки (пока внутренний
// эндпоинт выдачи UUID не подключён, его задают явно).
func (c *Client) SetSubscriptionID(id string) { c.core.SetSubscriptionID(id) }

// Configure разрешает «шов аутентификации» между приложением и ядром.
//
// Flutter-приложение аутентифицируется собственным Dart ApiClient (JWT) и здесь
// передаёт ядру свой access-токен и UUID подписки, чтобы ядро не входило повторно:
// после Configure ядро авторизовано (IsAuthenticated()==true) и Up сам забирает
// clash-конфиг подписки (он несёт amnezia-wg) без round-trip за UUID.
//
//   - panelURL      — базовый URL панели; пусто оставляет URL, заданный NewClient.
//   - subscriptionID — UUID подписки текущего пользователя (передаётся в ядро,
//     чтобы Up не ходил за ним к панели). Пусто — ядро подтянет его лениво.
//   - accessToken    — JWT приложения. Обязателен.
//
// gomobile-граница принимает только примитивы, поэтому это плоская сигнатура без
// refresh-токена и срока: инъекция идёт в режиме «access-only» — сессия живёт до
// истечения access-токена и НЕ продлевается сама; на 401 авторизованный запрос
// падает, и приложение должно повторно вызвать Configure со свежим токеном. Это
// сознательная деградация ради простого, бесшовного handoff (см.
// auth.PanelClient.SetTokens / api.Core.InjectToken).
func (c *Client) Configure(panelURL, subscriptionID, accessToken string) error {
	// panelURL на стороне ядра фиксируется при NewClient (NewCore требует его там).
	// Параметр оставлен в сигнатуре как часть документированного контракта плагина
	// и для будущей пере-конфигурации; сейчас ядро использует URL из NewClient.
	_ = panelURL
	return c.core.InjectToken(accessToken, "", 0, subscriptionID)
}

// ImportSubscription импортирует сырую подписку формата format (auto/clash/
// singbox/v2ray/uri) в mihomo-конфиг и сохраняет его как источник подключения
// (api.Core.ImportSubscription). После вызова Up("") поднимает туннель из
// импортированного конфига БЕЗ панели и БЕЗ входа (raw-путь, contract D/F).
//
// raw приходит строкой (gomobile-дружественно; внутри []byte(raw)). format пуст
// или "auto" — формат определяется по содержимому. Возвращает JSON метаданных
// (subimport.Metadata: Servers и др.) либо ошибку.
func (c *Client) ImportSubscription(raw, format string) (string, error) {
	meta, err := c.core.ImportSubscription([]byte(raw), format)
	if err != nil {
		return "", err
	}
	return toJSON(meta)
}

// --- Политика подключения ---

// SetProtocol фиксирует протокол по дружелюбному имени (AmneziaWG, VLESS-Reality,
// Hysteria2, TUIC, Shadowsocks). Пусто/"auto" — автоматика панели.
func (c *Client) SetProtocol(name string) { c.core.SetProtocol(name) }

// SetRelay задаёт страну relay-входа (ISO-2, напр. "TR"). Пусто — прямой вход.
func (c *Client) SetRelay(country string) { c.core.SetRelay(country) }

// SetTunnelMode переключает способ захвата трафика (применяется при следующем Up):
//
//   - "tun" (или пусто) — системный TUN-инбаунд; перехватывает весь трафик, но
//     требует привилегий (root/админ) либо системного расширения на Apple;
//   - "proxy" — локальный mixed-инбаунд (SOCKS5+HTTP) на 127.0.0.1:mixedPort БЕЗ
//     привилегий. Трафик в него направляет приложение или системный прокси ОС.
//     Это путь «доказать соединение без root» на десктопе.
//
// mixedPort <= 0 оставляет порт по умолчанию (7890) и значим только в
// proxy-режиме. Неизвестный режим возвращает ошибку.
func (c *Client) SetTunnelMode(mode string, mixedPort int) error {
	return c.core.SetTunnelMode(mode, mixedPort)
}

// ApplyPreset применяет встроенный пресет маршрутизации по ID (напр. "ru-smart",
// "telegram-only"). Ошибка — если ID неизвестен.
func (c *Client) ApplyPreset(id string) error { return c.core.ApplyPreset(id) }

// SetSplitTunnel настраивает раздельное туннелирование. bypassDomains и apps —
// строки, разделённые запятой (gomobile не пропускает []string как аргумент-срез
// строк удобно; согласованно используем CSV). perAppMode: "allow" или "bypass".
func (c *Client) SetSplitTunnel(bypassDomains, perAppMode, apps string) {
	c.core.SetSplitTunnel(splitCSV(bypassDomains), perAppMode, splitCSV(apps))
}

// ListPresets возвращает JSON-массив пресетов ([]api.PresetInfo). country — ISO-2
// для приоритизации релевантных пресетов; пусто — без приоритизации.
func (c *Client) ListPresets(country string) (string, error) {
	return toJSON(c.core.ListPresets(country))
}

// --- Туннель ---

// SetTunFd пробрасывает в движок TUN fd, созданный платформой (Android
// VpnService / iOS NEPacketTunnelProvider). Вызывать до Up.
func (c *Client) SetTunFd(fd int) error { return c.core.SetTunFd(fd) }

// Up поднимает туннель. serverID необязателен (пусто — выбор панели/автоподбора).
// Возвращает JSON api.UpResult.
func (c *Client) Up(serverID string) (string, error) {
	ctx, cancel := timeoutCtx(60)
	defer cancel()
	res, err := c.core.Up(ctx, serverID)
	if err != nil {
		return "", err
	}
	return toJSON(res)
}

// Down останавливает туннель.
func (c *Client) Down() error { return c.core.Down() }

// Status возвращает JSON api.StatusResult (состояние движка + подписка).
func (c *Client) Status() (string, error) {
	ctx, cancel := timeoutCtx(15)
	defer cancel()
	res, err := c.core.Status(ctx)
	if err != nil {
		return "", err
	}
	return toJSON(res)
}

// statusEvent — плоская форма статуса для EventChannel com.caramba/vpn/status.
// Поля и значения stage в точности совпадают с CHANNEL CONTRACT (Dart VpnStatus).
type statusEvent struct {
	Stage            string `json:"stage"`
	Detail           string `json:"detail,omitempty"`
	ConnectedSinceMs int64  `json:"connectedSinceMs"`
	// Mode — способ захвата трафика ("tun"|"proxy"), см. SetTunnelMode.
	Mode string `json:"mode"`
	// MixedPort — порт локального mixed-инбаунда. Заполняется ТОЛЬКО в
	// proxy-режиме, чтобы UI показал «Proxy on 127.0.0.1:7890».
	MixedPort int `json:"mixedPort,omitempty"`
}

// trafficEvent — плоская форма счётчиков для EventChannel com.caramba/vpn/traffic.
// Поля совпадают с CHANNEL CONTRACT (Dart TrafficStats).
type trafficEvent struct {
	DownBps   int64 `json:"downBps"`
	UpBps     int64 `json:"upBps"`
	DownTotal int64 `json:"downTotal"`
	UpTotal   int64 `json:"upTotal"`
}

// stageFromEngineState отображает engine.State на строку stage CHANNEL CONTRACT.
// reconnecting у движка нет — нативный слой эмитит его сам во время повторного
// дозвона; здесь stopped -> disconnected, starting -> connecting и т.д.
func stageFromEngineState(s string) string {
	switch s {
	case "starting":
		return "connecting"
	case "connected":
		return "connected"
	case "error":
		return "error"
	case "stopped":
		fallthrough
	default:
		return "disconnected"
	}
}

// StatusJSON возвращает плоский статус для EventChannel com.caramba/vpn/status:
// {stage, detail?, connectedSinceMs}. Это НЕ то же, что Status() (тот отдаёт
// вложенный api.StatusResult). Нативный слой подписывается на переходы и пушит
// этот JSON в канал статуса; stage берётся из движка и маппится в имена VpnStage.
//
// Лёгкий вызов без сети: читается только состояние движка (engine.Status), без
// обновления метаданных подписки.
func (c *Client) StatusJSON() (string, error) {
	st, err := c.core.EngineStatus()
	if err != nil {
		return "", err
	}
	mode, mixedPort := c.core.TunnelMode()
	return toJSON(statusEvent{
		Stage:            stageFromEngineState(string(st.State)),
		Detail:           st.Detail,
		ConnectedSinceMs: st.ConnectedSinceMs,
		Mode:             mode,
		MixedPort:        mixedPort,
	})
}

// TrafficJSON возвращает плоские счётчики для EventChannel com.caramba/vpn/traffic:
// {downBps, upBps, downTotal, upTotal}. Нативный слой опрашивает ~1 Гц, пока
// туннель поднят, и пушит результат в traffic-канал; вне соединения — нули.
//
// Источник — статистика ядра mihomo (totals per-session, сбрасываются на Down).
func (c *Client) TrafficJSON() (string, error) {
	t, err := c.core.Traffic()
	if err != nil {
		return "", err
	}
	return toJSON(trafficEvent{
		DownBps:   t.DownBps,
		UpBps:     t.UpBps,
		DownTotal: t.DownTotal,
		UpTotal:   t.UpTotal,
	})
}

// --- Автоподбор ---

// AutoTune измеряет сеть Prober'ом по умолчанию (NewDefaultProber): без тегов
// это TCP-замер серверов подписки, под -tags mihomo — честная проверка handshake
// протоколов через ядро. Затем выбирает сервер/протокол/стек/relay и применяет
// protocol+relay+stack к политике.
// Возвращает JSON autotune.Recommendation. Выходной сервер применяется вызовом
// Up(rec.server_id) — здесь Up не вызывается автоматически.
func (c *Client) AutoTune() (string, error) {
	ctx, cancel := timeoutCtx(60)
	defer cancel()
	prober, err := c.core.NewDefaultProber(ctx)
	if err != nil {
		return "", err
	}
	rec, err := c.core.AutoTune(ctx, prober)
	if err != nil {
		return "", err
	}
	return toJSON(rec)
}

// --- helpers ---

// toJSON сериализует значение в строку (gomobile-дружественный результат).
func toJSON(v any) (string, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return "", err
	}
	return string(b), nil
}

// splitCSV разбивает строку с разделителем-запятой в срез, отбрасывая пустые
// элементы и пробелы. Пустой вход → nil.
func splitCSV(s string) []string {
	s = strings.TrimSpace(s)
	if s == "" {
		return nil
	}
	parts := strings.Split(s, ",")
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		if p = strings.TrimSpace(p); p != "" {
			out = append(out, p)
		}
	}
	return out
}
