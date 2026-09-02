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
}

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

// FetchOptions — необязательные параметры выборки.
type FetchOptions struct {
	// NodeID привязывает конфиг к конкретному выходному узлу (?node_id=).
	NodeID string
	// RelayCountry переопределяет фильтрацию релеев (?relay_country=, ISO-2 или
	// "none").
	RelayCountry string
}

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

	raw, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("subscription: чтение тела: %w", err)
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

// clashProxy — минимальное представление прокси из mihomo-конфига.
type clashProxy struct {
	Name   string `yaml:"name"`
	Type   string `yaml:"type"`
	Server string `yaml:"server"`
	Port   int    `yaml:"port"`
}

type clashDoc struct {
	Proxies []clashProxy `yaml:"proxies"`
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
	servers := make([]Server, 0, len(doc.Proxies))
	for _, p := range doc.Proxies {
		servers = append(servers, Server{
			ID:      p.Name,
			Name:    p.Name,
			Type:    p.Type,
			Server:  p.Server,
			Port:    p.Port,
			Country: countryFromName(p.Name),
		})
	}
	return servers, nil
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
