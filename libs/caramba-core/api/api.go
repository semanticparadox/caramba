// Package api — фасад caramba-core. Это единственная точка, которую вызывают и
// тонкий CLI (caramba-cli), и Flutter-приложение (через gomobile/FFI). Бизнес-
// логика живёт в пакетах auth/subscription/profile/engine; здесь — только
// оркестрация и JSON-дружественные типы.
package api

import (
	"context"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/app"
	"github.com/semanticparadox/caramba/libs/caramba-core/auth"
	"github.com/semanticparadox/caramba/libs/caramba-core/autotune"
	"github.com/semanticparadox/caramba/libs/caramba-core/engine"
	"github.com/semanticparadox/caramba/libs/caramba-core/profile"
	"github.com/semanticparadox/caramba/libs/caramba-core/routing"
	"github.com/semanticparadox/caramba/libs/caramba-core/subimport"
	"github.com/semanticparadox/caramba/libs/caramba-core/subscription"
	"github.com/semanticparadox/caramba/libs/caramba-core/transport"
)

// Config — параметры инициализации Core.
type Config struct {
	// PanelBaseURL — корневой URL панели (auth/refresh/logout).
	PanelBaseURL string
	// SubBaseURL — корневой URL сервиса подписок. Если пусто, используется
	// PanelBaseURL.
	SubBaseURL string
	// WorkDir — каталог для собранного конфига и кэша. Если пусто, выбирается
	// <UserConfigDir>/caramba.
	WorkDir string
	// TokenStorePath — путь к файлу токенов. Если пусто — стандартный путь
	// файлового хранилища.
	TokenStorePath string
	// SubscriptionDomain — единственный хост, на который CSM/1 разрешает
	// ровно один переход. Пусто означает, что переходы запрещены полностью.
	SubscriptionDomain string
}

// Core связывает аутентификацию, подписку, сборку профиля и движок.
// Безопасен для конкурентного использования.
//
// ВАЖНО — состояние туннеля живёт только в памяти движка (engine.Engine).
// Для долгоживущих процессов (Flutter-приложение, будущий демон) один Core
// держит туннель весь свой жизненный цикл, и Up/Down/Status согласованы.
//
// Но в режиме тонкого CLI (caramba-cli) каждая команда создаёт НОВЫЙ Core, а
// сборка по умолчанию использует engine.mihomoStub без OS-уровня TUN/процесса.
// Поэтому `caramba down`/`caramba status` после `caramba up` в разных запусках
// видят свежий движок в StateStopped и НЕ могут наблюдать/остановить туннель
// предыдущего запуска. Это ожидаемо для заглушки.
//
// TODO(mihomo): когда появится реальный движок, состояние туннеля должно
// определяться по OS-уровню (TUN-интерфейс, pidfile/сокет демона в workDir),
// а не по in-memory полю движка, чтобы CLI-жизненный цикл работал между
// запусками. Альтернатива — фоновый демон/IPC, к которому подключаются команды.
type Core struct {
	cfg     Config
	workDir string

	auth    *auth.PanelClient
	sub     *subscription.Client
	subInfo *app.SubscriptionClient
	engine  engine.Engine
	// store — хранилище токенов. Держим ссылку, чтобы пересобрать auth-клиента
	// при смене адреса панели (SetPanelURL), не потеряв уже выданные токены.
	store auth.Store

	// ladder и doer — лестница транспортов CSM/1 и HTTPDoer поверх неё.
	// Именно этот HTTPDoer уходит в auth.NewPanelClient и
	// subscription.NewClient, и делается это в ОБОИХ местах их сборки:
	// в NewCore и в SetPanelURL. Забыть второе место значит молча вернуть
	// перерегистрированного арендатора к собственному ClientHello Go.
	ladder *transport.Ladder
	doer   *transport.Doer
	// csm — выборщик подписанных документов. Он же служит источником
	// ступени R0 (последние хорошие документы с диска).
	csm *transport.Fetcher
	// deviceBridge — платформенный держатель ключей устройства (ABI v3).
	// Задаётся мостом до первого обращения к CSM; nil означает программный
	// уровень 3, который называет себя своим именем.
	deviceBridge transport.DeviceKeyBridge

	mu             sync.Mutex
	subscriptionID string // кэш UUID подписки текущего пользователя
	policy         profile.Policy
	// loopbackProxyURL — адрес служебного инбаунда на петле вместе с парой
	// логин-пароль текущего подъёма. Пусто, пока движок не поднят.
	loopbackProxyURL string
	// csmProfileKey — профиль, чьё хранилище CSM сейчас выбрано (02-SPEC.md
	// 1.2). Пусто означает единственное хранилище в рабочем каталоге, как у
	// установок, заведённых до появления второго оператора.
	csmProfileKey string
	relayCountry  string // вход через страну (relay), ISO-2 или имя; пусто — прямое
	// userCountry — страна, где находится САМ пользователь, ISO-2. Пусто —
	// неизвестна, и это полноценное третье состояние, а не «Россия».
	//
	// Ядро определить её не может: геобазы оно не носит, а адрес, который оно
	// видит у себя, ничего не говорит. Единственный, кто её знает, — панель:
	// она видит исходящий адрес запроса и отдаёт результат в
	// `x-client-country` на теле подписки и в `client_country` в
	// `GET /api/v2/app/subscription`. Платформенный слой кладёт её сюда через
	// SetUserCountry.
	//
	// Отличать её от relayCountry обязательно: relayCountry это ВЫБОР входа
	// («хочу заходить через Россию»), а это ФАКТ о пользователе. Раньше на
	// месте факта стояла страна пресета — то есть выбор режима выдавал себя за
	// местоположение, и американский пользователь на умолчании `ru-smart`
	// получал российский домашний резолвер.
	userCountry string
	// importedConfig — сырой clash/mihomo YAML импортированной подписки (см.
	// SetImportedConfig). Если задан, Up поднимает туннель из него БЕЗ обращения
	// к панели и БЕЗ требования аутентификации (raw-путь, contract A/D).
	importedConfig []byte
	// presetID — идентификатор применённого пресета маршрутизации. Хранится
	// отдельно от скомпилированного Routing, потому что пересборка нужна на
	// КАЖДЫЙ подъём: пул зеркал и набор подтверждённых списков приходят из
	// каталога и живут короче, чем выбор пользователя.
	presetID string
	// lastRoute — снимок маршрутизации ПОСЛЕДНЕГО подъёма (RouteReportJSON).
	//
	// Снимок, а не пересчёт на чтении: presetID и relayCountry живут дольше
	// подъёма и к моменту вопроса «почему не резалась реклама» уже могли
	// смениться. Отчёт обязан рассказывать про то, что применено, а не про то,
	// что выбрано сейчас. nil означает, что подъёма ещё не было.
	lastRoute *routeSnapshot
	// lastPanelYAML — сырой YAML последнего успешно загруженного профиля панели.
	// Нужен CarambaProbe: замер узлов идёт по «текущей загруженной конфигурации»
	// и не должен ради этого повторно ходить в сеть.
	lastPanelYAML []byte
}

// NewCore создаёт фасад. Возвращает ошибку, если не удаётся подготовить рабочий
// каталог или хранилище токенов.
func NewCore(cfg Config) (*Core, error) {
	// Пустой PanelBaseURL допустим: ядро в режиме «только импорт» (rawSub,
	// гостевой режим клиента) не ходит в панель. Панельные операции без URL
	// отвергаются позже с понятной ошибкой (см. Up).
	subBase := cfg.SubBaseURL
	if subBase == "" {
		subBase = cfg.PanelBaseURL
	}

	workDir := cfg.WorkDir
	if workDir == "" {
		dir, err := os.UserConfigDir()
		if err != nil {
			return nil, fmt.Errorf("api: каталог конфигурации: %w", err)
		}
		workDir = filepath.Join(dir, "caramba")
	}
	if err := os.MkdirAll(workDir, 0o700); err != nil {
		return nil, fmt.Errorf("api: создание рабочего каталога: %w", err)
	}

	store, err := auth.NewFileStore(cfg.TokenStorePath)
	if err != nil {
		return nil, fmt.Errorf("api: хранилище токенов: %w", err)
	}

	// Ядру нужно знать свой домашний каталог ДО разбора конфига: правила
	// GEOIP/GEOSITE заставляют mihomo искать (и при отсутствии докачивать)
	// geo-базы в нём, а constant.Path.MMDB() отдаёт пустой путь, если каталога
	// нет, — разбор падает с «can't download MMDB: open : no such file». В
	// сборке без тега mihomo вызов пустой (см. homedir_default.go).
	setCoreHomeDir(workDir)

	// Лестница строится ОДИН раз и живёт весь жизненный цикл ядра. Оба
	// клиента получают один и тот же HTTPDoer: лестница реализована один раз,
	// в Go, и два независимых её экземпляра означали бы две истории попыток и
	// два бюджета соединений на один профиль.
	ladder := transport.NewLadder(transport.NewNetExchange(auth.DefaultUserAgent))
	doer := transport.NewDoer(ladder, cfg.PanelBaseURL, subscriptionDomainOf(cfg))

	panelClient := auth.NewPanelClient(cfg.PanelBaseURL, auth.WithStore(store), auth.WithHTTPClient(doer))
	c := &Core{
		cfg:     cfg,
		workDir: workDir,
		auth:    panelClient,
		sub:     subscription.NewClient(subBase, subscription.WithHTTPClient(doer)),
		subInfo: app.NewSubscriptionClient(panelClient),
		store:   store,
		engine:  engine.New(),
		policy:  profile.DefaultPolicy(),
		ladder:  ladder,
		doer:    doer,
	}
	// Выборщик CSM НЕ поднимается здесь. Его конструктор создаёт при первом
	// запуске пару ключей устройства и пишет её на диск, а это долгоживущий
	// идентификатор, появляющийся как побочный эффект запуска ядра у каждой
	// установки, включая тех, кто никогда не зарегистрируется в CSM. Он
	// строится лениво, на первом вызове CSM: до регистрации ступени R0 всё
	// равно нечего отдавать, а LoadCached на несозданном хранилище это no-op.
	return c, nil
}

// subscriptionDomainOf возвращает единственный хост, на который разрешён один
// переход. Явная настройка сильнее; иначе берётся хост сервиса подписок,
// который приложение и так передаёт отдельным адресом. Без этого ветка
// перехода недостижима, а панель выдаёт 308 безусловно, и легаси-путь
// перестаёт работать у любого арендатора, чей Host не совпал.
func subscriptionDomainOf(cfg Config) string {
	if d := strings.TrimSpace(cfg.SubscriptionDomain); d != "" {
		return d
	}
	sub := strings.TrimSpace(cfg.SubBaseURL)
	if sub == "" {
		return ""
	}
	u, err := url.Parse(sub)
	if err != nil {
		return ""
	}
	return u.Hostname()
}

// SetPanelURL перенаправляет ядро на другую панель: пересобирает auth-клиента,
// клиента подписок и клиента /subscription под новый базовый URL.
//
// Зачем. Мультипанельный режим (enroll в чужую панель) меняет адрес уже после
// NewClient, а раньше panelURL из Configure просто отбрасывался — ядро продолжало
// ходить в панель, заданную при создании. Токен-хранилище переиспользуется, так
// что уже инъецированный JWT не теряется; кэш UUID подписки и последний
// загруженный профиль сбрасываются, потому что относятся к прежней панели.
//
// Пустой или совпадающий с текущим URL — no-op. Применяется к следующим
// сетевым вызовам (Up/Status/AutoTune).
func (c *Core) SetPanelURL(panelURL string) error {
	panelURL = strings.TrimSpace(panelURL)
	if panelURL == "" {
		return nil
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if panelURL == c.cfg.PanelBaseURL {
		return nil
	}
	// SubBaseURL следует за панелью только тогда, когда он не был задан явно
	// отдельным адресом сервиса подписок.
	subBase := c.cfg.SubBaseURL
	if subBase == "" || subBase == c.cfg.PanelBaseURL {
		subBase = panelURL
		c.cfg.SubBaseURL = ""
	}
	c.cfg.PanelBaseURL = panelURL

	// Второе место сборки клиентов, и оно обязано отдать им ТОТ ЖЕ HTTPDoer.
	// Именно здесь ошибка не видна: клиенты пересобираются, лестница молча
	// теряется, и ядро продолжает ходить в панель собственным транспортом Go.
	if c.doer == nil {
		c.ladder = transport.NewLadder(transport.NewNetExchange(auth.DefaultUserAgent))
		c.doer = transport.NewDoer(c.ladder, panelURL, subscriptionDomainOf(c.cfg))
	} else {
		c.doer.SetOrigin(panelURL, subscriptionDomainOf(c.cfg))
	}
	panelClient := auth.NewPanelClient(panelURL, auth.WithStore(c.store), auth.WithHTTPClient(c.doer))
	c.auth = panelClient
	c.sub = subscription.NewClient(subBase, subscription.WithHTTPClient(c.doer))
	c.subInfo = app.NewSubscriptionClient(panelClient)
	c.subscriptionID = ""
	c.lastPanelYAML = nil
	return nil
}

// --- JSON-дружественные результаты ---

// AuthResult — результат входа/регистрации.
type AuthResult struct {
	OK            bool `json:"ok"`
	Authenticated bool `json:"authenticated"`
}

// StatusResult — агрегированное состояние для UI/CLI.
type StatusResult struct {
	Authenticated bool                   `json:"authenticated"`
	Engine        engine.Status          `json:"engine"`
	Subscription  *subscription.Metadata `json:"subscription,omitempty"`
	// Mode — способ захвата трафика ("tun"|"proxy"), см. SetTunnelMode.
	Mode string `json:"mode"`
	// MixedPort — порт локального mixed-инбаунда; заполняется только в
	// proxy-режиме (UI показывает по нему "Proxy on 127.0.0.1:7890").
	MixedPort int `json:"mixed_port,omitempty"`
}

// UpResult — результат поднятия туннеля.
type UpResult struct {
	OK         bool          `json:"ok"`
	ConfigPath string        `json:"config_path"`
	Engine     engine.Status `json:"engine"`
	// Ignored перечисляет выборы пользователя, которые ЭТОТ путь применить не
	// смог. Пустой список означает, что применено всё, что было задано.
	//
	// Поле появилось потому, что раньше raw-путь молча выбрасывал SetRelay:
	// туннель поднимался, ошибки не было, и единственным следом расхождения
	// оставалось то, что трафик шёл не через ту страну. Записи здесь — те же
	// Capability, что отдаёт Capabilities, с Supported=false: обвязка
	// показывает ту же причину тем же кодом, а не заводит вторую формулировку
	// для того же факта.
	Ignored []Capability `json:"ignored,omitempty"`
}

// --- Возможности пути (что клиент МОЖЕТ прямо сейчас, и почему нет) ---

// Имена возможностей. Это ключи, по которым обвязка сопоставляет запись с
// конкретным элементом управления, поэтому они стабильны.
const (
	// CapNameRelayChaining это цепочка вход→выход: выбор страны ВХОДА.
	CapNameRelayChaining = "relay_chaining"
	// CapNameNodeSelection это закрепление конкретного выходного узла.
	CapNameNodeSelection = "node_selection"
)

// Значения CapabilitiesResult.Path.
const (
	// ConfigPathRaw это импортированная подписка: узлы берутся из неё, в
	// панель не ходят (см. SetImportedConfig и Up).
	ConfigPathRaw = "raw"
	// ConfigPathPanel это панельный путь: конфиг тянется у панели по UUID.
	ConfigPathPanel = "panel"
)

// Машинные коды причин. Обвязка выбирает по ним текст; сама строка кода
// пользователю не показывается.
const (
	// CapReasonRawProfile: возможность принадлежит панели, а импортированная
	// подписка это готовый список узлов без того, кто мог бы его пересобрать.
	CapReasonRawProfile = "raw_profile"
	// CapReasonPanelNotConfigured: панельный путь выбран, но адрес панели пуст.
	CapReasonPanelNotConfigured = "panel_not_configured"
	// CapReasonOperatorNotGranted: оператор не выдал бит возможности в
	// подписанных документах (02-SPEC.md 6.3).
	CapReasonOperatorNotGranted = "operator_not_granted"
)

// Capability это одна возможность: доступна она на текущем пути или нет, и
// если нет — почему, машинным словом.
//
// Смысл типа в том, чтобы недоступный элемент управления оставался видимым и
// назывался причину. Спрятанный переключатель релея выглядит одинаково при
// «оператор не выдал бит», «панель не настроена» и «эта подписка так не
// умеет», и все три случая пользователь читает как поломку приложения. Хуже
// того, причина, зашитая в Dart, — это второе место, где живёт правило: ядро
// меняет поведение, текст остаётся прежним, и приложение уверенно врёт.
type Capability struct {
	Name      string `json:"name"`
	Supported bool   `json:"supported"`
	// Reason это машинный код (CapReason*). Пусто, когда Supported истинно.
	Reason string `json:"reason,omitempty"`
	// Detail это пояснение на английском для журнала и отчётов об ошибках.
	// Оно НЕ предназначено для показа пользователю: текст для него выбирает
	// обвязка по Reason.
	Detail string `json:"detail,omitempty"`
}

// CapabilitiesResult это ответ Capabilities.
type CapabilitiesResult struct {
	// Path это путь, по которому пойдёт ближайший Up: ConfigPathRaw либо
	// ConfigPathPanel. Возможности зависят от него, поэтому он отдаётся рядом.
	Path         string       `json:"path"`
	Capabilities []Capability `json:"capabilities"`
}

// Capabilities отвечает, что ядро умеет на ТЕКУЩЕМ пути.
//
// Ответ считается по тому же состоянию, по которому решает Up: импортированный
// конфиг, адрес панели и, если профиль CSM зарегистрирован, эффективная маска
// возможностей из подписанных документов. Выборщик CSM здесь НЕ создаётся:
// csmFetcher при первом обращении пишет на диск пару ключей устройства, то
// есть долгоживущий идентификатор, и заводить его ради вопроса «показывать ли
// переключатель» значило бы создавать идентификатор без действия пользователя.
// Отсутствующий выборщик означает «профиля CSM нет», а не «нельзя».
func (c *Core) Capabilities() CapabilitiesResult {
	c.mu.Lock()
	raw := len(c.importedConfig) > 0
	panelConfigured := strings.TrimSpace(c.cfg.PanelBaseURL) != ""
	f := c.csm
	c.mu.Unlock()

	res := CapabilitiesResult{Path: ConfigPathPanel}
	if raw {
		res.Path = ConfigPathRaw
	}
	res.Capabilities = []Capability{
		c.relayChainingCap(raw, panelConfigured, f),
		nodeSelectionCap(raw, panelConfigured),
	}
	return res
}

// CapabilitiesJSON это Capabilities строкой JSON для мостов.
func (c *Core) CapabilitiesJSON() (string, error) { return toJSONString(c.Capabilities()) }

// relayChainingCap отвечает про цепочку вход→выход.
//
// На raw-пути ответ отрицательный, и это решение, а не недоделка. Цепочку
// собирает панель: она принимает relay_country, подбирает вход и выдаёт
// конфиг, где выход уже завёрнут во вход. Импортированная подписка это
// готовый список узлов от постороннего оператора; пересобрать его в цепочку
// здесь нечем и не по чему, поэтому Up на этом пути relay не применяет.
// Раньше он его молча не применял — теперь об этом сказано до подъёма.
func (c *Core) relayChainingCap(raw, panelConfigured bool, f *transport.Fetcher) Capability {
	out := Capability{Name: CapNameRelayChaining}
	switch {
	case raw:
		out.Reason = CapReasonRawProfile
		out.Detail = "an imported subscription is a finished node list; the entry-exit chain is assembled by the panel from relay_country, and this path never contacts it"
		return out
	case !panelConfigured:
		out.Reason = CapReasonPanelNotConfigured
		out.Detail = "no panel base URL is configured, so relay_country has nowhere to be sent"
		return out
	}
	// Пустая маска означает «проверенных документов нет», а не «оператор
	// запретил»: до регистрации профиля CSM панельный путь работает по
	// обычному REST, который relay_country принимает. Запрет объявляется
	// только тогда, когда маска есть и бита в ней нет.
	if f != nil {
		if eff := f.EffectiveCap(); eff != 0 && eff&transport.CapRelayChaining == 0 {
			out.Reason = CapReasonOperatorNotGranted
			out.Detail = "the relay_chaining bit is clear in the effective capability mask of the verified documents"
			return out
		}
	}
	out.Supported = true
	return out
}

// nodeSelectionCap отвечает про закрепление конкретного выходного узла.
// На raw-пути это умеет сам клиент: serverID трактуется как имя прокси и
// закрепляется первым в селекторе CARAMBA (AssembleMihomoConfigPinned), панель
// для этого не нужна.
func nodeSelectionCap(raw, panelConfigured bool) Capability {
	if raw {
		return Capability{
			Name: CapNameNodeSelection, Supported: true,
			Detail: "the node is pinned locally as the first entry of the CARAMBA selector",
		}
	}
	if !panelConfigured {
		return Capability{
			Name: CapNameNodeSelection, Reason: CapReasonPanelNotConfigured,
			Detail: "no panel base URL is configured, so node_id has nowhere to be sent",
		}
	}
	return Capability{
		Name: CapNameNodeSelection, Supported: true,
		Detail: "the node is selected by the panel from node_id",
	}
}

// CsmFleetJSON отдаёт флот доверенного каталога: выходы, входы и то, можно ли
// сейчас построить цепочку.
//
// Это ровно то, из чего собирается экран выбора, и потому один вызов, а не
// три: без Exits нечего показать, без Relays нельзя назвать страну входа, а
// без RelayChaining переключатель входа пришлось бы либо прятать, либо
// объяснять причину его недоступности заново на стороне Dart.
func (c *Core) CsmFleetJSON() (string, error) {
	f, err := c.csmFetcher()
	if err != nil {
		return "", err
	}
	snap := f.Snapshot()

	c.mu.Lock()
	raw := len(c.importedConfig) > 0
	panelConfigured := strings.TrimSpace(c.cfg.PanelBaseURL) != ""
	c.mu.Unlock()

	return toJSONString(CsmFleetResult{
		Exits:  snap.Exits,
		Relays: snap.Relays,
		// FleetEmpty это флот, которым пользоваться нельзя, а не флот с нулём
		// выходов: он взводится ПОСЛЕ фильтрации по rev.nodes, и Exits при
		// этом может быть непуст — все его записи будут помечены
		// недоступными.
		FleetEmpty:          snap.FleetEmpty,
		RevokedNodesDropped: snap.RevokedNodesDropped,
		RelayChaining:       c.relayChainingCap(raw, panelConfigured, f),
	})
}

// CsmFleetResult это ответ CsmFleetJSON.
type CsmFleetResult struct {
	Exits               []transport.NodeRef `json:"exits"`
	Relays              []transport.NodeRef `json:"relays"`
	FleetEmpty          bool                `json:"fleet_empty"`
	RevokedNodesDropped int                 `json:"revoked_nodes_dropped"`
	RelayChaining       Capability          `json:"relay_chaining"`
}

// --- Аутентификация ---

// RegisterEmail регистрирует пользователя по email/паролю.
func (c *Core) RegisterEmail(ctx context.Context, email, password string) (AuthResult, error) {
	if _, err := c.auth.RegisterEmail(ctx, email, password); err != nil {
		return AuthResult{}, err
	}
	return AuthResult{OK: true, Authenticated: true}, nil
}

// LoginEmail выполняет вход по email/паролю.
func (c *Core) LoginEmail(ctx context.Context, email, password string) (AuthResult, error) {
	if _, err := c.auth.LoginEmail(ctx, email, password); err != nil {
		return AuthResult{}, err
	}
	// После входа сразу подтягиваем UUID подписки, чтобы Up не требовал ручного
	// SetSubscriptionID. Ошибка выборки не фатальна для самого входа.
	_ = c.fetchSubscriptionID(ctx)
	return AuthResult{OK: true, Authenticated: true}, nil
}

// LoginCode выполняет вход по одноразовому коду из Telegram-бота.
func (c *Core) LoginCode(ctx context.Context, code string) (AuthResult, error) {
	if _, err := c.auth.LoginCode(ctx, code); err != nil {
		return AuthResult{}, err
	}
	_ = c.fetchSubscriptionID(ctx)
	return AuthResult{OK: true, Authenticated: true}, nil
}

// LoginTelegram выполняет вход через данные Telegram Login.
func (c *Core) LoginTelegram(ctx context.Context, data auth.TelegramLogin) (AuthResult, error) {
	if _, err := c.auth.LoginTelegram(ctx, data); err != nil {
		return AuthResult{}, err
	}
	_ = c.fetchSubscriptionID(ctx)
	return AuthResult{OK: true, Authenticated: true}, nil
}

// InjectToken инъецирует извне выданный JWT (access + опц. refresh) в token-store
// ядра, минуя сетевой вход. Решает «шов аутентификации»: Flutter-приложение уже
// аутентифицировано собственным Dart ApiClient и передаёт сюда свой access-токен
// (и, желательно, refresh), чтобы ядру не входить повторно. После инъекции Up может
// сам сходить за clash-конфигом подписки (он несёт amnezia-wg) и UUID подписки.
//
// expiresUnix — unix-секунды истечения access-токена; <=0 означает «вызывающий
// срок не передал», и тогда он достраивается из claim exp самого JWT
// (auth.jwtExpiry), а не остаётся неизвестным.
//
// Пустой refreshToken — режим деградации, и слово «деградация» тут буквальное:
// сессия живёт ровно до истечения access (~15 минут), после чего ядро честно
// перестаёт считаться авторизованным (auth.Tokens.Valid) — вместо того чтобы
// врать «авторизован» и упираться в неустранимый 401. Все мосты обязаны
// передавать refresh; без него ядро не сможет продлиться, пока приложение
// выгружено (см. mobile.Client.Configure о том, почему обратный вызов в
// приложение не вариант). subscriptionID опционален: если задан, кэшируется, и
// Up не делает round-trip к панели за UUID.
func (c *Core) InjectToken(accessToken, refreshToken string, expiresUnix int64, subscriptionID string) error {
	var expiry time.Time
	if expiresUnix > 0 {
		expiry = time.Unix(expiresUnix, 0)
	}
	if _, err := c.auth.SetTokens(accessToken, refreshToken, expiry); err != nil {
		return fmt.Errorf("api: инъекция токена: %w", err)
	}
	if subscriptionID != "" {
		c.SetSubscriptionID(subscriptionID)
	}
	return nil
}

// fetchSubscriptionID запрашивает UUID подписки у панели
// (GET /api/v2/app/subscription, bearer) и кэширует его. Ошибку возвращает
// вызывающему; при вызове из login она намеренно игнорируется (вход состоялся).
func (c *Core) fetchSubscriptionID(ctx context.Context) error {
	uuid, err := c.subInfo.FetchUUID(ctx)
	if err != nil {
		return fmt.Errorf("api: получение UUID подписки: %w", err)
	}
	c.mu.Lock()
	c.subscriptionID = uuid
	c.mu.Unlock()
	return nil
}

// Logout завершает сеанс: останавливает туннель, отзывает токены.
func (c *Core) Logout(ctx context.Context) error {
	// Гасим туннель, чтобы не оставить активное соединение без авторизации.
	_ = c.engine.Stop()
	if err := c.auth.Logout(ctx); err != nil {
		return err
	}
	c.mu.Lock()
	c.subscriptionID = ""
	c.importedConfig = nil
	c.mu.Unlock()
	return nil
}

// --- Политика ---

// SetPolicy задаёт локальную политику (TUN/kill-switch/split-tunnel/DNS),
// применяемую при следующем Up. Уже поднятый туннель не перестраивается:
// приложение должно вызвать Down + Up, чтобы новая политика вступила в силу.
func (c *Core) SetPolicy(p profile.Policy) {
	c.mu.Lock()
	c.policy = p
	c.mu.Unlock()
}

// SetTunnelMode переключает способ захвата трафика для следующего Up.
//
//   - "tun" (или пусто) — системный TUN-инбаунд: перехватывает весь трафик, но
//     требует привилегий (root на Linux/macOS, админ на Windows) либо системного
//     расширения на Apple;
//   - "proxy" — локальный mixed-инбаунд (SOCKS5+HTTP) на 127.0.0.1:mixedPort БЕЗ
//     каких-либо привилегий; трафик в него направляет само приложение или
//     системный прокси ОС.
//
// mixedPort <= 0 оставляет ранее заданный порт (по умолчанию
// profile.DefaultMixedPort) и значим только в proxy-режиме. Неизвестный режим —
// ошибка, политика при этом не меняется.
func (c *Core) SetTunnelMode(mode string, mixedPort int) error {
	m := profile.TunnelMode(strings.ToLower(strings.TrimSpace(mode)))
	switch m {
	case "", profile.ModeTun:
		m = profile.ModeTun
	case profile.ModeProxy:
	default:
		return fmt.Errorf("api: неизвестный режим туннеля %q (ожидается \"tun\" или \"proxy\")", mode)
	}

	c.mu.Lock()
	defer c.mu.Unlock()
	c.policy.Mode = m
	if mixedPort > 0 {
		c.policy.Proxy.MixedPort = mixedPort
	}
	if c.policy.Proxy.MixedPort <= 0 {
		c.policy.Proxy.MixedPort = profile.DefaultMixedPort
	}
	if strings.TrimSpace(c.policy.Proxy.BindAddress) == "" {
		c.policy.Proxy.BindAddress = profile.DefaultBindAddress
	}
	return nil
}

// TunnelMode возвращает текущий режим ("tun"|"proxy") и порт mixed-инбаунда.
// Порт значим только в proxy-режиме; в tun-режиме возвращается 0.
func (c *Core) TunnelMode() (string, int) {
	c.mu.Lock()
	policy := c.policy
	c.mu.Unlock()
	mode := policy.EffectiveMode()
	if mode != profile.ModeProxy {
		return string(mode), 0
	}
	return string(mode), policy.EffectiveProxy().MixedPort
}

// SetRouting задаёт «умную» маршрутизацию (правила/пресет) для следующего Up.
// Передайте nil, чтобы вернуться к поведению по умолчанию (geo-CN + split).
func (c *Core) SetRouting(cfg *routing.Config) {
	c.mu.Lock()
	c.policy.Routing = cfg
	// Явно поданная конфигурация сильнее пресета: пересобирать её из пресета
	// на следующем подъёме значило бы молча затереть выбор вызывающего.
	// ApplyPreset выставляет presetID обратно уже после этого вызова.
	c.presetID = ""
	c.mu.Unlock()
}

// ApplyPreset выбирает встроенный пресет маршрутизации по ID (например
// "ru-smart", "telegram-only") и применяет его. URL внешних списков строятся
// относительно базового адреса панели (зеркало rule-set'ов). Возвращает ошибку,
// если пресет с таким ID не найден.
func (c *Core) ApplyPreset(id string) error {
	preset, ok := routing.PresetByID(id)
	if !ok {
		return fmt.Errorf("api: неизвестный пресет маршрутизации %q", id)
	}
	cfg := preset.Build(c.cfg.PanelBaseURL, profile.CarambaSelector)
	if err := cfg.Validate(); err != nil {
		return fmt.Errorf("api: пресет %q: %w", id, err)
	}
	c.SetRouting(&cfg)
	c.mu.Lock()
	c.presetID = id
	c.mu.Unlock()
	return nil
}

// PresetInfo — краткая карточка пресета для UI/CLI.
type PresetInfo struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Emoji       string `json:"emoji"`
	Country     string `json:"country,omitempty"`
	Description string `json:"description"`
}

// ListPresets возвращает встроенные пресеты. Если country задан (ISO-код),
// релевантные стране пресеты идут первыми.
func (c *Core) ListPresets(country string) []PresetInfo {
	var presets []routing.Preset
	if country != "" {
		presets = routing.PresetsForCountry(country)
	} else {
		presets = routing.Presets()
	}
	out := make([]PresetInfo, 0, len(presets))
	for _, p := range presets {
		out = append(out, PresetInfo{
			ID: p.ID, Name: p.Name, Emoji: p.Emoji, Country: p.Country, Description: p.Description,
		})
	}
	return out
}

// DefaultRelayCandidates — страны relay-входа по умолчанию для автоподбора,
// порядок = предпочтение (устойчивые к блокировкам из РФ направления).
var DefaultRelayCandidates = []string{"TR", "KZ", "FI"}

// AutoTune измеряет сеть через prober, выбирает лучший сервер/протокол/стек и
// при необходимости relay-вход, затем применяет relay, протокол и сетевой стек
// к политике и возвращает рекомендацию. Выходной сервер применяется вызовом
// Up(rec.ServerID).
func (c *Core) AutoTune(ctx context.Context, prober autotune.Prober) (autotune.Recommendation, error) {
	probes, err := prober.Probe(ctx)
	if err != nil {
		return autotune.Recommendation{}, fmt.Errorf("api: измерение сети: %w", err)
	}
	rec, err := autotune.Recommend(probes, DefaultRelayCandidates)
	if err != nil {
		return autotune.Recommendation{}, fmt.Errorf("api: автоподбор: %w", err)
	}
	c.mu.Lock()
	c.relayCountry = rec.Relay
	c.policy.Protocol = rec.Protocol
	if rec.Stack != "" {
		c.policy.Tun.Stack = profile.Stack(rec.Stack)
	}
	c.mu.Unlock()
	return rec, nil
}

// SetSplitTunnel настраивает раздельное туннелирование, применяемое при
// следующем Up.
//
//   - bypassDomains: домены, всегда идущие напрямую (DIRECT), мимо туннеля.
//   - perAppMode: режим для списка apps — "allow" (в туннель идут ТОЛЬКО эти
//     приложения, остальные напрямую) или "bypass" (эти приложения идут
//     напрямую, остальные в туннель). Пустая строка трактуется как "bypass".
//   - apps: имена процессов/приложений (для mihomo это PROCESS-NAME; на Android
//     имя пакета, на десктопе — имя исполняемого файла).
//
// Передача пустых bypassDomains и apps сбрасывает split-tunnel.
func (c *Core) SetSplitTunnel(bypassDomains []string, perAppMode string, apps []string) {
	split := profile.SplitTunnel{
		BypassDomains: append([]string(nil), bypassDomains...),
	}
	switch strings.ToLower(strings.TrimSpace(perAppMode)) {
	case "allow":
		split.AllowProcesses = append([]string(nil), apps...)
	default: // "bypass" или пусто
		split.BypassProcesses = append([]string(nil), apps...)
	}
	c.mu.Lock()
	c.policy.Split = split
	c.mu.Unlock()
}

// NewDefaultProber собирает Prober по умолчанию из серверов текущей подписки.
// Реализация зависит от сборки (см. prober_default.go и prober_mihomo.go):
//
//   - без тегов: быстрый TCP-замер достижимости (autotune.TCPProber);
//   - под -tags mihomo: честная проверка handshake протоколов через ядро
//     (autotune.MihomoProber).
//
// В обеих сборках AutoTune вызывается одинаково. UUID подписки берётся из кэша
// (после входа) либо лениво подтягивается у панели; ручной SetSubscriptionID не
// обязателен.
func (c *Core) NewDefaultProber(ctx context.Context) (autotune.Prober, error) {
	cands, raw, err := c.candidates(ctx)
	if err != nil {
		return nil, err
	}
	return newProber(cands, raw), nil
}

// candidates загружает серверы подписки и преобразует их в кандидатов
// автоподбора. Возвращает также сырой YAML подписки: он нужен mihomo-Prober'у,
// чтобы построить прокси-адаптеры для честной проверки handshake.
func (c *Core) candidates(ctx context.Context) ([]autotune.Candidate, []byte, error) {
	c.mu.Lock()
	subID := c.subscriptionID
	c.mu.Unlock()
	if subID == "" {
		// Лениво подтягиваем UUID у панели, если он ещё не закэширован.
		if err := c.fetchSubscriptionID(ctx); err != nil {
			return nil, nil, err
		}
		c.mu.Lock()
		subID = c.subscriptionID
		c.mu.Unlock()
	}

	prof, err := c.sub.FetchProfile(ctx, subID, subscription.FetchOptions{})
	if err != nil {
		return nil, nil, fmt.Errorf("api: загрузка серверов подписки: %w", err)
	}

	c.mu.Lock()
	c.lastPanelYAML = prof.RawYAML
	c.mu.Unlock()

	return aggregateCandidates(prof.Metadata.Servers), prof.RawYAML, nil
}

// aggregateCandidates сворачивает серверы подписки в кандидатов автоподбора по
// имени узла (ServerID). Узел, объявляющий несколько протоколов, приходит из
// подписки несколькими записями proxies с ОДНИМ именем; здесь они собираются в
// одного кандидата, чей Protocols содержит ВСЕ объявленные протоколы. Так
// mihomo-Prober проверит handshake каждого протокола узла за один проход и
// вернёт один ProbeResult на узел без дублей ServerID. Порядок узлов и
// протоколов сохраняется по первому появлению.
func aggregateCandidates(servers []subscription.Server) []autotune.Candidate {
	cands := make([]autotune.Candidate, 0, len(servers))
	idx := make(map[string]int, len(servers)) // ServerID -> позиция в cands
	for _, s := range servers {
		proto := friendlyProtocol(s.Type)
		if i, ok := idx[s.Name]; ok {
			// Узел уже есть — добавляем протокол, если он ещё не учтён.
			c := &cands[i]
			if !containsString(c.Protocols, proto) {
				c.Protocols = append(c.Protocols, proto)
			}
			continue
		}
		idx[s.Name] = len(cands)
		cands = append(cands, autotune.Candidate{
			ServerID:  s.Name,
			Country:   s.Country,
			Host:      s.Server,
			Port:      s.Port,
			Protocols: []string{proto},
		})
	}
	return cands
}

// containsString сообщает, есть ли s в xs.
func containsString(xs []string, s string) bool {
	for _, x := range xs {
		if x == s {
			return true
		}
	}
	return false
}

// friendlyProtocol сопоставляет clash-тип прокси (ss/vless/...) с дружелюбным
// именем протокола, которым оперируют autotune и profile. Неизвестный тип
// возвращается как есть.
func friendlyProtocol(clashType string) string {
	switch strings.ToLower(clashType) {
	case "wireguard":
		return "AmneziaWG"
	case "vless":
		return "VLESS-Reality"
	case "hysteria2":
		return "Hysteria2"
	case "tuic":
		return "TUIC"
	case "ss":
		return "Shadowsocks"
	default:
		return clashType
	}
}

// SetProtocol фиксирует протокол подключения по дружелюбному имени (см.
// profile.Policy.Protocol). Пусто или "Авто" возвращает автоматику панели.
func (c *Core) SetProtocol(name string) {
	c.mu.Lock()
	c.policy.Protocol = name
	c.mu.Unlock()
}

// SetRelay задаёт страну входа (relay) для цепочки вход→выход. Пустая строка
// возвращает прямое подключение к выходному узлу. Применяется при следующем Up.
// Значение передаётся панели как relay_country при выборке подписки.
func (c *Core) SetRelay(country string) {
	c.mu.Lock()
	c.relayCountry = country
	c.mu.Unlock()
}

// SetUserCountry сообщает ядру страну, где находится пользователь (ISO-2, как
// её увидела панель). Пустая строка или мусор возвращают состояние «не знаем».
//
// Значение НЕ угадывается и не подставляется: единственный источник — панель
// (`x-client-country` / `client_country`). Пока она не ответила, страна
// остаётся пустой, и решения, которые от неё зависят, обязаны честно падать на
// свой запасной путь, а не на Россию.
func (c *Core) SetUserCountry(iso string) {
	iso = strings.ToUpper(strings.TrimSpace(iso))
	if len(iso) != 2 {
		iso = ""
	}
	c.mu.Lock()
	c.userCountry = iso
	c.mu.Unlock()
}

// UserCountry возвращает страну пользователя, известную ядру; пусто — неизвестна.
func (c *Core) UserCountry() string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.userCountry
}

// homeCountry — страна, чей ДОМАШНИЙ резолвер получит этот подъём.
//
// Порядок именно такой: место, где человек находится, важнее режима, который
// он включил. Страна пресета остаётся запасным вариантом на случай, когда
// панель ещё не сказала своего слова, — тогда явно выбранный `ru-smart` это
// лучшее доступное свидетельство о местоположении, и хуже прежнего не станет.
// Оба пусты — раскола нет вовсе (см. profile.ApplyBootstrapDNS).
func homeCountry(userCountry, presetID string) string {
	if userCountry != "" {
		return userCountry
	}
	return presetCountry(presetID)
}

// SetSubscriptionID привязывает Core к конкретному UUID подписки. Обычно UUID
// подтягивается автоматически после входа (GET /api/v2/app/subscription); этот
// метод служит ручным переопределением.
func (c *Core) SetSubscriptionID(id string) {
	c.mu.Lock()
	c.subscriptionID = id
	c.mu.Unlock()
}

// --- Импорт сырой подписки (raw-путь) ---

// SetImportedConfig сохраняет сырой clash/mihomo YAML импортированной подписки.
// После этого Up поднимает туннель из него БЕЗ обращения к панели и БЕЗ
// требования аутентификации (см. Up). clashYAML должен иметь форму конфига
// подписки панели (секция proxies:), то есть быть результатом subimport.Import.
// Пустой ввод сбрасывает импортированный конфиг (возврат к панельному пути).
func (c *Core) SetImportedConfig(clashYAML []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(clashYAML) == 0 {
		c.importedConfig = nil
		return nil
	}
	c.importedConfig = append([]byte(nil), clashYAML...)
	return nil
}

// ImportSubscription разбирает сырую подписку формата format в mihomo-конфиг
// (subimport.Import), сохраняет его как импортированный источник (SetImportedConfig)
// и возвращает метаданные. После вызова Up поднимает туннель из этого конфига без
// панели. format — один из subimport.Format* ("auto" по умолчанию).
//
// Это client-side MULTI-PROFILE примитив rawSub (contract A): импорт по URL/QR/
// файлу, подключение без аккаунта панели.
func (c *Core) ImportSubscription(raw []byte, format string) (subimport.Metadata, error) {
	clashYAML, meta, err := subimport.Import(raw, format)
	if err != nil {
		return subimport.Metadata{}, fmt.Errorf("api: импорт подписки: %w", err)
	}
	if err := c.SetImportedConfig(clashYAML); err != nil {
		return subimport.Metadata{}, err
	}
	return meta, nil
}

// --- Туннель ---

// Up собирает mihomo-конфиг с локальной политикой, записывает его в рабочий
// каталог и запускает движок. Источник узлов зависит от режима:
//
//   - raw-путь (импортированная подписка, SetImportedConfig/ImportSubscription):
//     если importedConfig задан, узлы берутся из него БЕЗ обращения к панели и
//     БЕЗ требования аутентификации. Непустой serverID трактуется как ИМЯ прокси
//     (Metadata.Servers[].id) и закрепляет этот узел первым в селекторе CARAMBA,
//     то есть делает его выбором по умолчанию; пустой оставляет автоматику.
//     relay НЕ применяется: цепочку вход→выход собирает панель, а здесь её
//     собрать не из чего. Это видно ДО подъёма через Capabilities
//     (relay_chaining, причина CapReasonRawProfile) и, если выбор всё же был
//     задан, названо в UpResult.Ignored после него.
//   - панельный путь: требуется аутентификация; конфиг подписки тянется у панели
//     по UUID. serverID необязателен и передаётся как node_id, relay — как
//     relay_country.
//
// В обоих режимах поверх сырого YAML применяется одна и та же
// profile.AssembleMihomoConfig (TUN/DNS/kill-switch/split/routing/protocol).
func (c *Core) Up(ctx context.Context, serverID string) (UpResult, error) {
	c.mu.Lock()
	imported := c.importedConfig
	policy := c.policy
	presetID := c.presetID
	rawRelay := c.relayCountry
	userCC := c.userCountry
	c.mu.Unlock()

	// Что этот подъём применить не сможет, известно до того, как он начнётся.
	// Список собирается здесь, а не после старта движка, чтобы причина
	// уезжала наружу тем же кодом, каким её отдаёт Capabilities.
	var ignored []Capability
	if len(imported) > 0 && strings.TrimSpace(rawRelay) != "" {
		ignored = append(ignored, c.relayChainingCap(true, false, nil))
	}

	// Учётные данные служебного инбаунда на петле выпускаются на КАЖДЫЙ подъём
	// и никуда не сохраняются. Без них слушатель не собирается: инбаунд с
	// ключом proxy уводит всё, что на него пришло, прямо в группу-селектор
	// мимо правил, и на Android туда ходит любое приложение с разрешением
	// INTERNET. Пару знают ровно две половины одного процесса: конфиг движка и
	// ступень R4 лестницы.
	if policy.LoopbackPort() > 0 {
		user, pass, err := profile.NewLoopbackCredential()
		if err != nil {
			return UpResult{}, err
		}
		policy.Proxy.LoopbackUser, policy.Proxy.LoopbackPass = user, pass
	}

	// Разблокировка загрузки, 02-SPEC.md 8.10. Делается ДО обращения к панели:
	// проверенные geo-базы и списки нужны ядру уже на разборе конфига, а отказ
	// каталога должен остановить сборку прежде, чем что-то попадёт на диск.
	plan, err := c.prepareBootstrap(ctx, ruleSetNames(presetID))
	if err != nil {
		return UpResult{}, err
	}
	policy.Bootstrap = plan.boot
	policy.ApplyBootstrapDNS(plan.doh, homeCountry(userCC, presetID))
	route, routeSnap := c.routingForBuild(plan)
	if route != nil {
		policy.Routing = route
	}
	setProbeTarget(plan.boot.ProbeURL)

	var rawYAML []byte
	// pinProxy — имя узла, которое нужно закрепить в селекторе CARAMBA. Для
	// импортированной подписки панельного выбора узла нет, поэтому serverID
	// применяется здесь, локально (см. profile.AssembleMihomoConfigPinned). На
	// панельном пути узел выбирает сама панель по node_id, и пин не нужен.
	pinProxy := ""
	if len(imported) > 0 {
		// raw-путь: ни auth, ни fetch к панели.
		rawYAML = imported
		pinProxy = strings.TrimSpace(serverID)
	} else {
		// панельный путь.
		if strings.TrimSpace(c.cfg.PanelBaseURL) == "" {
			return UpResult{}, fmt.Errorf("api: панель не настроена: импортируйте подписку или вызовите Configure с panelURL")
		}
		if !c.auth.IsAuthenticated() {
			return UpResult{}, auth.ErrNotAuthenticated
		}
		c.mu.Lock()
		subID := c.subscriptionID
		relay := c.relayCountry
		c.mu.Unlock()

		if subID == "" {
			// UUID не закэширован (например, вход был выполнен в прошлом запуске
			// или сетевой запрос при входе не удался) — лениво подтягиваем его.
			// SetSubscriptionID остаётся способом ручного переопределения.
			if err := c.fetchSubscriptionID(ctx); err != nil {
				return UpResult{}, err
			}
			c.mu.Lock()
			subID = c.subscriptionID
			c.mu.Unlock()
		}

		prof, err := c.sub.FetchProfile(ctx, subID, subscription.FetchOptions{NodeID: serverID, RelayCountry: relay})
		if err != nil {
			return UpResult{}, fmt.Errorf("api: загрузка подписки: %w", err)
		}
		rawYAML = prof.RawYAML
		c.mu.Lock()
		c.lastPanelYAML = rawYAML
		c.mu.Unlock()
	}

	assembled, err := profile.AssembleMihomoConfigPinned(rawYAML, policy, pinProxy)
	if err != nil {
		return UpResult{}, fmt.Errorf("api: сборка конфигурации: %w", err)
	}

	configPath := filepath.Join(c.workDir, "config.yaml")
	if err := os.WriteFile(configPath, assembled, 0o600); err != nil {
		return UpResult{}, fmt.Errorf("api: запись конфигурации: %w", err)
	}

	if err := c.engine.Start(configPath); err != nil {
		return UpResult{}, fmt.Errorf("api: запуск движка: %w", err)
	}

	// Ступень R4 включается ровно здесь: служебный mixed-инбаунд на петле
	// существует только пока поднят движок, и объявлять её доступной раньше
	// значило бы обещать лестнице путь, которого ещё нет.
	c.mu.Lock()
	l := c.ladder
	c.mu.Unlock()
	if l != nil {
		// Лестница получает адрес ВМЕСТЕ с учётными данными: голый host:port
		// означал бы слушатель без аутентификации, а такого мы не собираем.
		if addr := policy.LoopbackProxyURL(); addr != "" {
			l.SetTunnelProxy(addr)
		} else {
			l.SetTunnelUnavailable(transport.ReasonNotConfigured)
		}
	}
	// Тот же адрес нужен ядру CSM, когда оно отдельное (Android и Apple держат
	// второе ядро под профиль CSM и Up на нём не зовут). Обвязка забирает его
	// через LoopbackProxyURL и передаёт вызовом csmSetLadder.
	c.mu.Lock()
	c.loopbackProxyURL = policy.LoopbackProxyURL()
	c.mu.Unlock()

	// Снимок маршрутизации фиксируется ПОСЛЕ старта движка и только при нём:
	// отчёт отвечает на вопрос «что применено», а конфиг, на котором движок не
	// поднялся, не применён ни к чему.
	routeSnap.relay = newRouteRelayReport(rawRelay, ignored, rawYAML)
	routeSnap.ignored = ignored
	routeSnap.raisedAt = time.Now()
	c.mu.Lock()
	c.lastRoute = &routeSnap
	c.mu.Unlock()

	st, _ := c.engine.Status()
	return UpResult{OK: true, ConfigPath: configPath, Engine: st, Ignored: ignored}, nil
}

// Down останавливает туннель.
func (c *Core) Down() error {
	if err := c.engine.Stop(); err != nil {
		return fmt.Errorf("api: остановка движка: %w", err)
	}
	// Слушателя больше нет, значит и у R4 больше нет пути. Причина именно
	// not_configured, а не platform_unsupported: сборка умеет, туннель опущен.
	c.mu.Lock()
	l := c.ladder
	c.loopbackProxyURL = ""
	c.mu.Unlock()
	if l != nil {
		l.SetTunnelProxy("")
	}
	return nil
}

// LoopbackProxyURL отдаёт адрес служебного инбаунда на петле вместе с парой
// логин-пароль, выпущенной на этот подъём. Пустая строка означает, что
// слушателя нет.
//
// Нужен обвязке, которая держит ОТДЕЛЬНОЕ ядро под профиль CSM: у того ядра
// своя лестница, Up на нём никто не зовёт, и без этой передачи его ступень R4
// навсегда остаётся not_configured, а диагностика показывает разные пути на
// разных ОС у одного арендатора в одной сети.
func (c *Core) LoopbackProxyURL() string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.loopbackProxyURL
}

// SetTunnelUnavailable объявляет ступень R4 недоступной с явной причиной.
//
// Это точка для платформенного моста: платформа, на которой локального
// слушателя не будет (например TUN без петли), обязана сказать это явно, чтобы
// диагностика не показывала успех R4 на одной ОС и молчаливый отказ на другой
// у одного арендатора в одной сети. Пустая причина трактуется как
// platform_unsupported.
func (c *Core) SetTunnelUnavailable(reason string) error {
	c.mu.Lock()
	l := c.ladder
	c.mu.Unlock()
	if l == nil {
		return fmt.Errorf("api: лестница недоступна")
	}
	l.SetTunnelUnavailable(transport.Reason(strings.TrimSpace(reason)))
	return nil
}

// SetTunFd пробрасывает в движок TUN file descriptor, созданный платформой
// (Android/iOS). Вызывать до Up.
func (c *Core) SetTunFd(fd int) error {
	return c.engine.SetTunFd(fd)
}

// Traffic возвращает мгновенную скорость и накопленные счётчики туннеля,
// снятые со статистики ядра. Лёгкий вызов без сети — нативный слой опрашивает
// его ~1 Гц для traffic-канала. Когда туннель не поднят — нули.
func (c *Core) Traffic() (engine.Traffic, error) {
	return c.engine.Traffic()
}

// EngineStatus возвращает состояние движка без обращения к сети (в отличие от
// Status, который ещё подтягивает метаданные подписки). Нативный слой опрашивает
// его между переходами для status-канала.
func (c *Core) EngineStatus() (engine.Status, error) {
	return c.engine.Status()
}

// Status возвращает агрегированное состояние. ctx используется для возможного
// обновления метаданных подписки.
func (c *Core) Status(ctx context.Context) (StatusResult, error) {
	st, err := c.engine.Status()
	if err != nil {
		return StatusResult{}, fmt.Errorf("api: состояние движка: %w", err)
	}
	mode, mixedPort := c.TunnelMode()
	res := StatusResult{
		Authenticated: c.auth.IsAuthenticated(),
		Engine:        st,
		Mode:          mode,
		MixedPort:     mixedPort,
	}

	c.mu.Lock()
	subID := c.subscriptionID
	c.mu.Unlock()
	if res.Authenticated && subID != "" {
		// Лёгкое обновление метаданных подписки с коротким таймаутом.
		mctx, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		if prof, ferr := c.sub.FetchProfile(mctx, subID, subscription.FetchOptions{}); ferr == nil {
			res.Subscription = &prof.Metadata
		}
	}
	return res, nil
}
