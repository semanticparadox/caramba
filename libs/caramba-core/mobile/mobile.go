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
//   - panelURL      — базовый URL панели. Непустой и отличающийся от текущего
//     перенаправляет ядро на эту панель; пусто оставляет URL из NewClient.
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
	// Непустой и отличающийся panelURL перенаправляет ядро на другую панель:
	// пересобираются auth-клиент, клиент подписок и клиент /subscription
	// (api.Core.SetPanelURL). Это нужно мультипанельному режиму (enroll в чужую
	// панель), где адрес становится известен уже после NewClient. Пусто —
	// остаётся URL из NewClient.
	if err := c.core.SetPanelURL(panelURL); err != nil {
		return err
	}
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

// SetPolicyJSON применяет политику подключения одной JSON-строкой (контракт ABI
// v2, CarambaSetPolicy). Все поля опциональны, неизвестные ключи игнорируются:
//
//	{"protocol":"auto|AmneziaWG|VLESS-Reality|Hysteria2|TUIC|Shadowsocks",
//	 "preset":"ru-smart|ru-full|telegram-only|ir-smart|by-smart|cn-smart|streaming|adblock|global|",
//	 "relay":"TR|KZ|FI|",
//	 "stack":"gvisor|system|mixed",
//	 "mtu":1280, "ipv6":false, "fakeIp":true, "killSwitch":true,
//	 "dns":{"nameservers":[...],"fallback":[...]},
//	 "split":{"mode":"off|bypass|allow","apps":[...],"bypassDomains":[...]}}
//
// Недопустимое значение перечислимого поля возвращает ошибку с именем этого поля
// и НЕ меняет политику. Политика применяется при следующем Up: если туннель уже
// поднят, приложение обязано переподключиться (Down + Up), иначе изменения
// останутся невидимыми.
func (c *Client) SetPolicyJSON(jsonStr string) error { return c.core.SetPolicyJSON(jsonStr) }

// ProbeJSON меряет задержку до каждого узла ТЕКУЩЕЙ загруженной конфигурации
// (импортированной подписки либо последнего загруженного профиля панели), не
// поднимая туннель. Возвращает JSON вида
//
//	{"servers":[{"id":"...","name":"...","type":"vless","server":"host",
//	             "port":443,"country":"NL","latencyMs":42}]}
//
// latencyMs = -1 означает, что узел не ответил за timeoutMs. timeoutMs <= 0 —
// таймаут по умолчанию (3с). Если ничего не загружено, вернётся {"servers":[]}.
func (c *Client) ProbeJSON(timeoutMs int) (string, error) {
	// Общий бюджет: таймаут одного узла плюс запас на очередь (замеры идут
	// пачками по 8). Минимум — минута, чтобы длинная подписка успела промериться.
	budget := 60
	if timeoutMs > 0 {
		budget = timeoutMs/1000 + 60
	}
	ctx, cancel := timeoutCtx(budget)
	defer cancel()
	return c.core.ProbeJSON(ctx, timeoutMs)
}

// --- Туннель ---

// SetTunFd пробрасывает в движок TUN fd, созданный платформой (Android
// VpnService / iOS NEPacketTunnelProvider). Вызывать до Up.
func (c *Client) SetTunFd(fd int) error { return c.core.SetTunFd(fd) }

// Up поднимает туннель. serverID необязателен (пусто — выбор панели/автоподбора).
// Для импортированной подписки непустой serverID — это ИМЯ узла (поле id из
// метаданных ImportSubscription), и он закрепляется как выбор по умолчанию в
// селекторе CARAMBA. Возвращает JSON api.UpResult.
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
	// ActiveProxy — имя узла, выбранного в селекторе CARAMBA. Заполняется только
	// когда туннель поднят (контракт ABI v2): UI показывает по нему «через какой
	// сервер идёт трафик».
	ActiveProxy string `json:"activeProxy,omitempty"`
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
// {stage, detail?, connectedSinceMs, activeProxy?}. Это НЕ то же, что Status() (тот отдаёт
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
		ActiveProxy:      st.ActiveProxy,
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

// --- CSM/1: подписанный манифест и лестница транспортов ---
//
// Всё богатое состояние уходит строкой JSON, как SetPolicyJSON: gomobile не
// пропускает через границу карты и срезы чужих структур, а пять мостов должны
// читать одинаковые формы. Это символы ABI v3; клиент, собранный против старой
// библиотеки, не находит их и вырождается в "CSM недоступен в этой сборке",
// а не падает.

// CsmEnroll выполняет регистрацию из bootstrap blob либо из origin, кода и
// пина. Возвращает снимок проверенного состояния.
func (c *Client) CsmEnroll(jsonStr string) (string, error) {
	ctx, cancel := timeoutCtx(60)
	defer cancel()
	return c.core.CsmEnroll(ctx, jsonStr)
}

// CsmRefresh выполняет один цикл выборки документов.
//
// Ошибка не означает потерю конфигурации: профиль остаётся на кешированных
// документах и продолжает подключать (инвариант 16).
func (c *Client) CsmRefresh(timeoutSec int) (string, error) {
	ctx, cancel := timeoutCtx(timeoutSec)
	defer cancel()
	return c.core.CsmRefresh(ctx)
}

// CsmState отдаёт личность оператора, состояние проверки документов, битовое
// поле возможностей, возраст конфигурации и её источник.
func (c *Client) CsmState() (string, error) { return c.core.CsmStateJSON() }

// CsmLadder отдаёт все скомпилированные ступени с порядком, переключателем,
// причиной недоступности и историей попыток.
func (c *Client) CsmLadder() (string, error) { return c.core.CsmLadderJSON() }

// CsmFleet отдаёт флот доверенного каталога: выходы (exits), входы (relays) и
// запись relay_chaining о том, можно ли построить цепочку на текущем пути.
//
// Это источник экрана выбора страны. Узлы приходят проекцией: идентификатор,
// ярлык, страна, форма протокола и ребро rl на вход — и НИ ОДНОГО поля, чем
// подключаются. Соединение собирает ядро; слою, который рисует, host, ключ и
// short id не нужны, а их появление там сделало бы подписанный каталог
// бессмысленным.
//
// Узел, отозванный оператором, приходит помеченным available=false с
// reason="revoked", а не пропадает из списка: пропавшую страну пользователь
// ищет, названную — понимает.
func (c *Client) CsmFleet() (string, error) { return c.core.CsmFleetJSON() }

// Capabilities отдаёт то, что ядро умеет на ТЕКУЩЕМ пути: путь ("raw" для
// импортированной подписки, "panel" для панельного) и записи возможностей с
// машинной причиной недоступности.
//
// Нужно, чтобы недоступный элемент управления оставался видимым и назывался
// причину. Прятать его нельзя: спрятанный переключатель выглядит одинаково при
// «оператор не выдал бит», «панель не настроена» и «эта подписка так не
// умеет». Держать причину в Dart тоже нельзя: правило живёт в ядре, и вторая
// его копия рано или поздно начнёт врать.
func (c *Client) Capabilities() (string, error) { return c.core.CapabilitiesJSON() }

// CsmSetLadder применяет переключатели и порядок от пользователя.
func (c *Client) CsmSetLadder(jsonStr string) error { return c.core.CsmSetLadderJSON(jsonStr) }

// CsmSelectProfile переключает хранилище CSM на профиль key (02-SPEC.md 1.2).
// Пустой ключ означает единственное хранилище в рабочем каталоге.
func (c *Client) CsmSelectProfile(key string) error { return c.core.CsmSelectProfile(key) }

// CsmAnswerCatalogChange передаёт ответ пользователя на карточку смены набора
// rule-set и geo-файлов. Вход {"accept":bool}, выход {"answered":bool}.
func (c *Client) CsmAnswerCatalogChange(jsonStr string) (string, error) {
	return c.core.CsmAnswerCatalogChangeJSON(jsonStr)
}

// LoopbackProxyURL отдаёт адрес служебного инбаунда на петле вместе с парой
// логин-пароль текущего подъёма. Пусто, когда движок не поднят.
//
// Обвязка на Android и Apple держит ОТДЕЛЬНОЕ ядро под профиль CSM, и Up на
// нём никто не зовёт. Без этой передачи ступень R4 того ядра навсегда
// not_configured, и лестница деградирует до R1 и R5 на тех самых платформах,
// ради которых служебный инбаунд и появился.
func (c *Client) LoopbackProxyURL() string { return c.core.LoopbackProxyURL() }

// CsmRequestSettings отправляет изменение настроек как подписанный запрос.
func (c *Client) CsmRequestSettings(jsonStr string) (string, error) {
	ctx, cancel := timeoutCtx(30)
	defer cancel()
	return c.core.CsmRequestSettingsJSON(ctx, jsonStr)
}

// LadderRequest выполняет произвольный HTTP запрос через лестницу. Это то, чем
// управляющий слой на Dart заменяет собственные сокеты к оператору.
func (c *Client) LadderRequest(jsonStr string) (string, error) {
	ctx, cancel := timeoutCtx(120)
	defer cancel()
	return c.core.LadderRequestJSON(ctx, jsonStr)
}

// --- CSM/1: ключи устройства (ABI v3) ---

// DeviceKeyBridge это платформенный держатель ключей устройства.
//
// Интерфейс объявлен ЗДЕСЬ, а не только в transport, потому что gomobile
// связывает обратные вызовы лишь для интерфейсов связываемого пакета. Kotlin и
// Swift реализуют его, и ядро зовёт их через границу: Secure Enclave и
// StrongBox достижимы только оттуда, а реализация на Go по построению кладёт
// ключ в файл, то есть в программный уровень.
//
// Формы JSON совпадают с символами ABI v3 (02-SPEC.md 12.2):
//
//	Keygen {"purpose":"sign"|"agree","require_hardware":bool}
//	       -> {"spki_b64","agree_pub_b64","tier":1|2|3,"generation":n}
//	Sign   {"message_b64"} -> {"sig_b64"}   64 байта r || s, низкий s
//	Agree  {"rkv":n,"peer_pub_b64"} -> {"shared_b64","own_pub_b64"}
type DeviceKeyBridge interface {
	Keygen(reqJSON string) (string, error)
	Sign(reqJSON string) (string, error)
	Agree(reqJSON string) (string, error)
}

// SetDeviceKeyBridge устанавливает держателя ключей устройства. Вызывать до
// первого обращения к CSM: подмена держателя после того, как личность
// устройства заведена и отправлена оператору, это ошибка, а не тихая замена.
func (c *Client) SetDeviceKeyBridge(b DeviceKeyBridge) error {
	if b == nil {
		return c.core.SetDeviceKeyBridge(nil)
	}
	return c.core.SetDeviceKeyBridge(b)
}

// DeviceKeygen заводит или отдаёт уже заведённую личность устройства.
// Идемпотентен: повторный вызов отдаёт тот же dtp.
func (c *Client) DeviceKeygen(jsonStr string) (string, error) {
	return c.core.DeviceKeygenJSON(jsonStr)
}

// DeviceSign подписывает СООБЩЕНИЕ ключом подписи устройства и возвращает
// 64 байта r || s с низким s.
func (c *Client) DeviceSign(jsonStr string) (string, error) {
	return c.core.DeviceSignJSON(jsonStr)
}

// DeviceAgree выполняет ECDH ключом согласования устройства.
func (c *Client) DeviceAgree(jsonStr string) (string, error) {
	return c.core.DeviceAgreeJSON(jsonStr)
}
