// Package subscription загружает mihomo (clash.meta) конфигурацию из панели
// caramba и разбирает метаданные подписки.
//
// Панель отдаёт готовый clash/mihomo YAML по адресу /sub/{uuid}. Тип клиента
// определяется по User-Agent либо параметру ?client=clash. Заголовок
// subscription-userinfo содержит лимиты трафика и срок действия.
package subscription

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"gopkg.in/yaml.v3"
)

// ClashUserAgent — User-Agent, по которому панель определяет clash/mihomo-клиента
// и отдаёт mihomo-конфиг. Дублируем определением ?client=clash для надёжности.
const ClashUserAgent = "caramba-core/1.0 (mihomo) clash.meta"

// HTTPDoer — минимальный интерфейс HTTP-клиента.
type HTTPDoer interface {
	Do(req *http.Request) (*http.Response, error)
}

// Server — узел/прокси из mihomo-конфига, извлечённый для отображения в UI.
type Server struct {
	// ID — стабильный идентификатор узла для приложения. Совпадает с именем
	// прокси в mihomo-конфиге: именно его приложение возвращает в Up(serverID),
	// чтобы закрепить узел в селекторе CARAMBA (контракт ABI v2). Отдельное поле
	// нужно, потому что Name показывается пользователю и может быть локализован,
	// а ID — машинный ключ.
	ID     string `json:"id"`
	Name   string `json:"name"`
	Type   string `json:"type"`
	Server string `json:"server"`
	Port   int    `json:"port"`
	// Country — ISO-2 код страны выходного узла, если его удалось извлечь из
	// имени прокси (панель кодирует страну во флаг-эмодзи или префикс имени).
	// Пусто, если определить не удалось. Используется автоподбором для relay-
	// логики.
	Country string `json:"country,omitempty"`

	// Transport — способ, которым протокол уложен в провод: tcp, ws, grpc,
	// httpupgrade, h2. Пусто означает «источник не назвал», а не «tcp».
	//
	// Поле заведено потому, что владелец спрашивал про ИНБАУНДЫ, а экран
	// сводил двадцать узлов в четыре строки по одному типу протокола: отказ
	// целого транспорта (httpupgrade) прятался за строкой «VLESS 510 мс»,
	// которую вытягивал соседний транспорт того же протокола.
	Transport string `json:"transport,omitempty"`

	// Security — чем закрыт провод: reality, tls, none. Пусто — источник не
	// назвал (или у семейства нет понятия TLS, как у wireguard).
	Security string `json:"security,omitempty"`

	// Role — RoleExit или RoleRelay. Relay это узел, через который НАБИРАЮТ
	// другой узел (detour/dialer-proxy). Он не выход, и показывать его в
	// списке выходов значит предлагать человеку подключиться к промежуточной
	// машине — автоподбор на такой строке увозил трафик не туда.
	Role string `json:"role"`
}

// Роли узла в подписке.
const (
	// RoleExit — обычный выходной узел: тот, чей адрес видит интернет.
	RoleExit = "exit"
	// RoleRelay — промежуточный узел: на него кто-то ссылается как на
	// dialer-proxy, и сам по себе он выходом не является.
	RoleRelay = "relay"
)

// Traffic — статистика трафика из заголовка subscription-userinfo.
type Traffic struct {
	Upload   int64 `json:"upload"`
	Download int64 `json:"download"`
	// Total — лимит трафика в байтах; 0 означает «безлимит».
	Total int64 `json:"total"`
}

// Used возвращает суммарно использованный трафик (upload+download) в байтах.
func (t Traffic) Used() int64 { return t.Upload + t.Download }

// Metadata — разобранные метаданные подписки.
type Metadata struct {
	Title   string    `json:"title,omitempty"`
	Traffic Traffic   `json:"traffic"`
	// Expiry — момент истечения подписки; нулевое значение означает отсутствие
	// срока.
	Expiry  time.Time `json:"expiry,omitempty"`
	Servers []Server  `json:"servers"`
	// UpdateInterval — рекомендованный интервал обновления конфига.
	UpdateInterval time.Duration `json:"update_interval,omitempty"`
}

// Profile — результат загрузки подписки: сырой YAML и метаданные.
type Profile struct {
	// RawYAML — оригинальный mihomo-конфиг как отдан панелью.
	RawYAML  []byte   `json:"-"`
	Metadata Metadata `json:"metadata"`
}

// Client загружает подписки с панели/сервиса подписок.
type Client struct {
	// SubBaseURL — корневой URL сервиса подписок (например,
	// "https://exarobot.top"). Может совпадать с панелью.
	subBaseURL string
	http       HTTPDoer
	userAgent  string
}

// Option настраивает Client.
type Option func(*Client)

// WithHTTPClient задаёт HTTP-клиент.
func WithHTTPClient(d HTTPDoer) Option { return func(c *Client) { c.http = d } }

// WithUserAgent переопределяет User-Agent (по умолчанию ClashUserAgent).
func WithUserAgent(ua string) Option { return func(c *Client) { c.userAgent = ua } }

// NewClient создаёт клиент подписки. subBaseURL — корень сервиса подписок.
func NewClient(subBaseURL string, opts ...Option) *Client {
	c := &Client{
		subBaseURL: strings.TrimRight(subBaseURL, "/"),
		http:       &http.Client{Timeout: 30 * time.Second},
		userAgent:  ClashUserAgent,
	}
	for _, o := range opts {
		o(c)
	}
	return c
}

// HTTPDoer возвращает текущий HTTP-клиент. Нужен, чтобы проверить, что
// лестница транспортов действительно передана в обоих местах сборки клиента.
func (c *Client) HTTPDoer() HTTPDoer { return c.http }

// FetchOptions — необязательные параметры выборки.
type FetchOptions struct {
	// NodeID привязывает конфиг к конкретному выходному узлу (?node_id=).
	NodeID string
	// RelayCountry переопределяет фильтрацию релеев (?relay_country=, ISO-2 или
	// "none").
	RelayCountry string
	// Variant передаёт вариант конфигурации (?variant=). Нужен только когда
	// оператор выставил бит возможностей 10 (проброс variant): без проброса
	// панель детерминированно отдаёт вариант по умолчанию, и каждая проверка
	// хеша конфигурации падала бы. Пустая строка означает "не отправлять".
	Variant string
}

// MaxProfileBytes — потолок тела конфигурации подписки.
//
// Он не подписан и оператором не поднимается. Существует потому, что
// неограниченный io.ReadAll на теле, размер которого выбирает отвечающая
// сторона, это выделение памяти по её заявлению.
const MaxProfileBytes int64 = 4 << 20

// FetchProfile загружает mihomo-конфиг подписки по её UUID и возвращает сырой
// YAML вместе с разобранными метаданными.
func (c *Client) FetchProfile(ctx context.Context, subscriptionUUID string, opts FetchOptions) (*Profile, error) {
	if subscriptionUUID == "" {
		return nil, fmt.Errorf("subscription: пустой UUID подписки")
	}

	endpoint, err := url.Parse(c.subBaseURL + "/sub/" + url.PathEscape(subscriptionUUID))
	if err != nil {
		return nil, fmt.Errorf("subscription: разбор URL подписки: %w", err)
	}
	q := endpoint.Query()
	q.Set("client", "clash")
	if opts.NodeID != "" {
		q.Set("node_id", opts.NodeID)
	}
	if opts.RelayCountry != "" {
		q.Set("relay_country", opts.RelayCountry)
	}
	if opts.Variant != "" {
		q.Set("variant", opts.Variant)
	}
	endpoint.RawQuery = q.Encode()

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
	if err != nil {
		return nil, fmt.Errorf("subscription: создание запроса: %w", err)
	}
	req.Header.Set("User-Agent", c.userAgent)
	req.Header.Set("Accept", "text/yaml, application/yaml, */*")

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("subscription: запрос подписки: %w", err)
	}
	defer func() {
		_, _ = io.Copy(io.Discard, resp.Body)
		_ = resp.Body.Close()
	}()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("subscription: панель вернула статус %d", resp.StatusCode)
	}

	// Чтение под потолком: +1 байт, чтобы переполнение было видно на первом
	// лишнем байте, а не после того, как оно уже в памяти.
	raw, err := io.ReadAll(io.LimitReader(resp.Body, MaxProfileBytes+1))
	if err != nil {
		return nil, fmt.Errorf("subscription: чтение тела: %w", err)
	}
	if int64(len(raw)) > MaxProfileBytes {
		return nil, fmt.Errorf("subscription: тело выше потолка %d байт", MaxProfileBytes)
	}

	meta, err := parseMetadata(resp.Header, raw)
	if err != nil {
		return nil, fmt.Errorf("subscription: разбор метаданных: %w", err)
	}

	return &Profile{RawYAML: raw, Metadata: meta}, nil
}

// parseMetadata собирает метаданные из заголовков ответа и тела YAML.
func parseMetadata(h http.Header, raw []byte) (Metadata, error) {
	meta := Metadata{
		Title:   h.Get("profile-title"),
		Traffic: parseUserInfo(h.Get("subscription-userinfo")),
	}
	if exp := expiryFromUserInfo(h.Get("subscription-userinfo")); !exp.IsZero() {
		meta.Expiry = exp
	}
	if iv := h.Get("profile-update-interval"); iv != "" {
		if minutes, err := strconv.Atoi(strings.TrimSpace(iv)); err == nil && minutes > 0 {
			meta.UpdateInterval = time.Duration(minutes) * time.Minute
		}
	}

	servers, err := parseServers(raw)
	if err != nil {
		return meta, err
	}
	meta.Servers = servers
	return meta, nil
}

// parseUserInfo разбирает заголовок вида
// "upload=0; download=123; total=456; expire=1700000000".
func parseUserInfo(header string) Traffic {
	var t Traffic
	for _, part := range strings.Split(header, ";") {
		kv := strings.SplitN(strings.TrimSpace(part), "=", 2)
		if len(kv) != 2 {
			continue
		}
		key := strings.TrimSpace(kv[0])
		val, err := strconv.ParseInt(strings.TrimSpace(kv[1]), 10, 64)
		if err != nil {
			continue
		}
		switch key {
		case "upload":
			t.Upload = val
		case "download":
			t.Download = val
		case "total":
			t.Total = val
		}
	}
	return t
}

// expiryFromUserInfo извлекает поле expire (unix-секунды) из заголовка.
func expiryFromUserInfo(header string) time.Time {
	for _, part := range strings.Split(header, ";") {
		kv := strings.SplitN(strings.TrimSpace(part), "=", 2)
		if len(kv) != 2 || strings.TrimSpace(kv[0]) != "expire" {
			continue
		}
		if sec, err := strconv.ParseInt(strings.TrimSpace(kv[1]), 10, 64); err == nil && sec > 0 {
			return time.Unix(sec, 0).UTC()
		}
	}
	return time.Time{}
}

// clashDoc — минимальное представление секции proxies mihomo-конфига.
//
// Прокси читаются сырыми картами, а не типизированной структурой: транспорт и
// защита живут в протокол-специфичных ключах (network, ws-opts, reality-opts),
// и структура из четырёх полей их теряла.
type clashDoc struct {
	Proxies []map[string]any `yaml:"proxies"`
}

// ProxyMaps извлекает сырые clash-map'ы прокси из секции `proxies` конфига
// подписки, сгруппированные по имени узла и clash-типу прокси:
//
//	имя_прокси (ServerID) → (clash-тип, например "vless"/"ss" → сырой map прокси).
//
// Сырые map'ы передаются дальше в mihomo (adapter.ParseProxy) для честной
// проверки handshake каждого протокола без обращения к панели повторно. Функция
// чистая и не зависит от build-тега mihomo: разбор YAML доступен в любой сборке,
// поэтому её можно тестировать без нативного ядра. Прокси без поля name/type
// пропускаются. Возвращает пустую (не nil) карту, если прокси нет.
//
// ServerID = поле name, что совпадает с subscription.Server.Name и, далее,
// autotune.Candidate.ServerID — благодаря этому MihomoProber находит конфиг по
// тому же ключу, под которым кандидат пришёл из метаданных.
func ProxyMaps(rawYAML []byte) (map[string]map[string]map[string]any, error) {
	var doc struct {
		Proxies []map[string]any `yaml:"proxies"`
	}
	if err := yaml.Unmarshal(rawYAML, &doc); err != nil {
		return nil, fmt.Errorf("subscription: разбор proxies: %w", err)
	}
	out := make(map[string]map[string]map[string]any, len(doc.Proxies))
	for _, px := range doc.Proxies {
		name, _ := px["name"].(string)
		clashType, _ := px["type"].(string)
		if name == "" || clashType == "" {
			continue
		}
		byType := out[name]
		if byType == nil {
			byType = make(map[string]map[string]any, 1)
			out[name] = byType
		}
		byType[clashType] = px
	}
	return out, nil
}

// parseServers извлекает список прокси из YAML для отображения в UI.
func parseServers(raw []byte) ([]Server, error) {
	var doc clashDoc
	if err := yaml.Unmarshal(raw, &doc); err != nil {
		return nil, fmt.Errorf("разбор YAML конфигурации: %w", err)
	}
	return ServersFromProxies(doc.Proxies), nil
}

// ServersFromProxies строит список узлов для UI из сырых clash-карт прокси,
// СОХРАНЯЯ порядок источника.
//
// Живёт здесь, а не в subimport, ровно по одной причине: обе дороги (подписка
// панели и импорт пользователя) обязаны описывать узел одинаково. Пока
// транспорт выводился в одном месте, а в другом нет, экран «Тип подключения»
// зависел от того, каким путём попала подписка.
func ServersFromProxies(proxies []map[string]any) []Server {
	relays := relayNames(proxies)
	servers := make([]Server, 0, len(proxies))
	for _, px := range proxies {
		name := asStr(px["name"])
		role := RoleExit
		if _, ok := relays[name]; ok && name != "" {
			role = RoleRelay
		}
		servers = append(servers, Server{
			ID:        name,
			Name:      name,
			Type:      asStr(px["type"]),
			Server:    asStr(px["server"]),
			Port:      asPort(px["port"]),
			Country:   countryFromName(name),
			Transport: ProxyTransport(px),
			Security:  ProxySecurity(px),
			Role:      role,
		})
	}
	return servers
}

// relayNames собирает имена узлов, на которые кто-то ссылается как на
// dialer-proxy. Ссылка и есть единственное свидетельство, что узел
// промежуточный: своего признака у него нет.
func relayNames(proxies []map[string]any) map[string]struct{} {
	out := make(map[string]struct{})
	for _, px := range proxies {
		if t := asStr(px["dialer-proxy"]); t != "" {
			out[t] = struct{}{}
		}
	}
	return out
}

// udpOnlyTypes — семейства, у которых нет понятия «транспорт поверх TCP»: они
// сами себе транспорт. Пустая строка у них честнее, чем выдуманный «tcp».
var udpOnlyTypes = map[string]struct{}{
	"hysteria":  {},
	"hysteria2": {},
	"tuic":      {},
	"wireguard": {},
}

// NormalizeProxyForCore чинит формы, которые источник называет иначе, чем ядро.
//
// Сегодня это ровно один случай, и он дорогой: панель отдаёт «network:
// httpupgrade» прямо в clash-теле (снято с подписки 34: узлы 🇩🇪 HTTP и
// 🇨🇦 HTTP), а mihomo такой сети не знает. Хуже, чем отказ: у vless нераспознанная
// сеть МОЛЧА вырождается в обычный TLS-поток, узел ждёт апгрейда и отвечает
// «unexpected response version» — то есть отказ выглядит как отвергнутый ключ.
// Проверено разделением причин на живом флоте: с прежним именем — отказ, с
// перекладкой в ws + v2ray-http-upgrade — 569 мс DE и 157 мс CA.
//
// Функция вызывается на ОБОИХ путях (сборка конфига для ядра и разбор узлов для
// замера), потому что панельная подписка через импорт не проходит вовсе.
// Повторный вызов безвреден: после перекладки сеть уже ws.
//
// Правку панели это не отменяет — там та же ошибка бьёт по всем прочим
// клиентам, — но перестаёт держать наших пользователей заложниками её сроков.
func NormalizeProxyForCore(px map[string]any) {
	if !strings.EqualFold(strings.TrimSpace(asStr(px["network"])), "httpupgrade") {
		return
	}
	path := "/"
	host := ""
	if hu, ok := px["http-upgrade-opts"].(map[string]any); ok {
		if p := asStr(hu["path"]); p != "" {
			path = p
		}
		host = asStr(hu["host"])
		if host == "" {
			if hdrs, ok := hu["headers"].(map[string]any); ok {
				host = asStr(hdrs["Host"])
			}
		}
	}
	opts := map[string]any{"v2ray-http-upgrade": true, "path": path}
	if host != "" {
		// Host именно из опций апгрейда: подставлять сюда servername значило
		// бы выдумать заголовок, которого источник не называл.
		opts["headers"] = map[string]any{"Host": host}
	}
	px["network"] = "ws"
	px["ws-opts"] = opts
	delete(px, "http-upgrade-opts")
}

// IsUDPOnlyType отвечает, живёт ли семейство целиком на UDP.
//
// Ответ нужен замеру: на порту такого узла TCP никто не слушает, и справочная
// TCP-проба возвращает -1 у совершенно здорового узла. Пока это -1 считалось
// доказательством, вся ветка UDP-семейств сваливалась в «адрес не отвечает».
func IsUDPOnlyType(typ string) bool {
	_, ok := udpOnlyTypes[strings.ToLower(strings.TrimSpace(typ))]
	return ok
}

// unbuildableProxyTypes — типы, для которых ядро (mihomo) НЕ СТРОИТ outbound ни
// при каких полях.
//
// Список — общая правда для трёх мест: замер обязан сказать «ядро не умеет» без
// единого соединения, сборка профиля обязана убрать такой прокси из конфига
// (mihomo отвергает ВЕСЬ конфиг из-за одного незнакомого типа, то есть туннель
// не поднялся бы вовсе), а импорт обязан сохранить честное имя типа вместо
// подмены на похожий.
var unbuildableProxyTypes = map[string]struct{}{
	"naive": {},
}

// CoreCanBuildProxyType отвечает, есть ли смысл отдавать такой прокси ядру.
func CoreCanBuildProxyType(typ string) bool {
	_, bad := unbuildableProxyTypes[strings.ToLower(strings.TrimSpace(typ))]
	return !bad
}

// ProxyTransport называет способ укладки протокола в провод.
//
// httpupgrade распознаётся по ФЛАГУ внутри ws-opts, а не по имени сети: mihomo
// не знает network: httpupgrade, и импорт кладёт этот провод как ws с
// v2ray-http-upgrade. Без обратного распознавания httpupgrade слился бы с ws в
// одну строку — то есть спрятался бы ровно так же, как прятался раньше.
func ProxyTransport(px map[string]any) string {
	network := strings.ToLower(strings.TrimSpace(asStr(px["network"])))
	if network == "ws" {
		if opts, ok := px["ws-opts"].(map[string]any); ok {
			if b, ok := opts["v2ray-http-upgrade"].(bool); ok && b {
				return "httpupgrade"
			}
		}
	}
	if network != "" {
		return network
	}
	if _, udp := udpOnlyTypes[strings.ToLower(strings.TrimSpace(asStr(px["type"])))]; udp {
		return ""
	}
	// Отсутствие ключа network у TCP-семейств означает у mihomo именно tcp:
	// это умолчание ядра, а не незнание источника.
	return "tcp"
}

// ProxySecurity называет, чем закрыт провод.
func ProxySecurity(px map[string]any) string {
	if ro, ok := px["reality-opts"].(map[string]any); ok && len(ro) > 0 {
		return "reality"
	}
	if b, ok := px["tls"].(bool); ok && b {
		return "tls"
	}
	switch strings.ToLower(strings.TrimSpace(asStr(px["type"]))) {
	case "hysteria", "hysteria2", "tuic":
		// В clash-карте у них ключа tls нет вовсе: TLS в QUIC неотделим от
		// самого протокола. Сказать «none» здесь было бы неправдой.
		return "tls"
	case "wireguard", "ss", "ssr":
		// Своя криптография, TLS-понятия нет — и выдумывать его нечего.
		return ""
	}
	return "none"
}

// asStr / asPort — локальные приведения сырых значений YAML. Дубликат хелперов
// subimport здесь намеренный: subimport импортирует subscription, обратная
// зависимость замкнула бы цикл.
func asStr(v any) string {
	s, _ := v.(string)
	return s
}

func asPort(v any) int {
	switch n := v.(type) {
	case int:
		return n
	case int64:
		return int(n)
	case float64:
		return int(n)
	case string:
		if i, err := strconv.Atoi(n); err == nil {
			return i
		}
	}
	return 0
}

// countryFromName извлекает ISO-2 код страны из имени прокси. Панель кодирует
// страну во флаг-эмодзи (например "🇹🇷 Istanbul") либо в виде префикса-кода
// ("TR — Istanbul", "[NL] Amsterdam"). Возвращает код в верхнем регистре или
// пустую строку, если страну определить не удалось.
//
// Поддерживаемые формы (по приоритету):
//   - флаг-эмодзи из двух Regional Indicator Symbol'ов в начале имени;
//   - ведущий токен из двух латинских букв, отделённый не-буквой.
func countryFromName(name string) string {
	name = strings.TrimSpace(name)
	if name == "" {
		return ""
	}
	if iso := flagToISO(name); iso != "" {
		return iso
	}
	// Ведущий двухбуквенный латинский токен: "TR", "[NL]", "US-1" и т.п.
	var letters []rune
	for _, r := range name {
		if (r >= 'A' && r <= 'Z') || (r >= 'a' && r <= 'z') {
			letters = append(letters, r)
			if len(letters) > 2 {
				return "" // токен длиннее двух букв — это не код страны
			}
			continue
		}
		// Не-буква до накопления букв (флаг/скобка/пробел) — пропускаем.
		if len(letters) == 0 {
			continue
		}
		break // граница токена после двух букв
	}
	if len(letters) == 2 {
		return strings.ToUpper(string(letters))
	}
	return ""
}

// flagToISO декодирует ведущий флаг-эмодзи (две Regional Indicator Symbol-руны,
// U+1F1E6..U+1F1FF) в ISO-2 код. Возвращает "" если имя не начинается с флага.
func flagToISO(name string) string {
	const base = 0x1F1E6 // Regional Indicator Symbol Letter A
	runes := []rune(strings.TrimSpace(name))
	if len(runes) < 2 {
		return ""
	}
	a, b := runes[0], runes[1]
	if a < base || a > base+25 || b < base || b > base+25 {
		return ""
	}
	return string([]byte{byte('A' + (a - base)), byte('A' + (b - base))})
}
