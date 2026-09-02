// Package subimport импортирует пользовательскую подписку из произвольного
// формата (clash/mihomo YAML, sing-box JSON, base64-список v2ray, одиночные
// URI вида vless://, vmess://, trojan://, ss://, hysteria2://, tuic://,
// wireguard://) и нормализует её в mihomo (clash.meta) YAML, который дальше
// потребляют движок и сборка профиля ровно так же, как конфиг подписки панели
// (та же форма secure proxies: список map'ов с name/type/server/port и
// протокол-специфичными ключами).
//
// Пакет чисто парсящий и НЕ зависит от build-тега mihomo: используются только
// gopkg.in/yaml.v3, encoding/json, encoding/base64 и net/url. Импортировать
// github.com/metacubex/mihomo здесь НЕЛЬЗЯ — это сломало бы сборку по умолчанию
// (CLI/тесты) и потянуло бы CGO. Единственный честный валидатор корректности
// proxy-map'ов — реальное ядро (engine под -tags mihomo через adapter.ParseProxy);
// subimport их только ПРОИЗВОДИТ, но никогда не проверяет против ядра. Благодаря
// этому `go test ./subimport/...` гоняется без нативного ядра, как
// subscription_test.go и profile_test.go.
package subimport

import (
	"fmt"

	"github.com/semanticparadox/caramba/libs/caramba-core/subscription"
	"gopkg.in/yaml.v3"
)

// Формат входных данных для Import.
const (
	// FormatAuto автоматически определяет формат по содержимому (см. detect.go).
	FormatAuto = "auto"
	// FormatClash — готовый clash/mihomo YAML (секция proxies:).
	FormatClash = "clash"
	// FormatSingbox — конфиг sing-box (JSON с массивом outbounds).
	FormatSingbox = "singbox"
	// FormatV2ray — base64-кодированный список URI (по строке на прокси).
	FormatV2ray = "v2ray"
	// FormatURI — одиночный URI (vless://, vmess://, trojan:// и т.д.).
	FormatURI = "uri"
)

// Metadata — алиас метаданных подписки, переиспользуем тип из пакета
// subscription, чтобы импортированная подписка и подписка панели описывались
// одинаково (Title/Traffic/Expiry/Servers). Для импорта заполняется только
// Servers (из произведённых proxies) и Title (если формат его несёт).
type Metadata = subscription.Metadata

// proxyList — внутреннее представление результата парсинга: упорядоченный список
// сырых clash-proxy map'ов. Каждый элемент обязан нести как минимум name/type/
// server/port, иначе UI-список серверов и автоподбор покажут пустые поля.
type proxyList []map[string]any

// Import разбирает сырую подписку формата format и возвращает нормализованный
// mihomo YAML (секция proxies:), метаданные и ошибку.
//
// format — один из FormatAuto/FormatClash/FormatSingbox/FormatV2ray/FormatURI.
// FormatAuto определяет формат по содержимому. Пустой format трактуется как
// FormatAuto.
//
// Выходной clashYAML имеет ту же форму, что subscription.Profile.RawYAML: верхне-
// уровневый ключ proxies: со списком прокси-map'ов И группа-селектор CARAMBA в
// proxy-groups:. Его можно передавать прямо в profile.AssembleMihomoConfig и
// api.Core.SetImportedConfig.
func Import(raw []byte, format string) (clashYAML []byte, meta Metadata, err error) {
	if len(raw) == 0 {
		return nil, Metadata{}, fmt.Errorf("subimport: пустой ввод")
	}
	if format == "" {
		format = FormatAuto
	}
	if format == FormatAuto {
		format = detectFormat(raw)
	}

	var proxies proxyList
	switch format {
	case FormatClash:
		proxies, err = parseClash(raw)
	case FormatSingbox:
		proxies, err = parseSingbox(raw)
	case FormatV2ray:
		proxies, err = parseV2ray(raw)
	case FormatURI:
		proxies, err = parseURIList(raw)
	default:
		return nil, Metadata{}, fmt.Errorf("subimport: неизвестный формат %q", format)
	}
	if err != nil {
		return nil, Metadata{}, err
	}
	if len(proxies) == 0 {
		return nil, Metadata{}, fmt.Errorf("subimport: формат %q не дал ни одного прокси", format)
	}

	clashYAML, err = marshalClash(proxies)
	if err != nil {
		return nil, Metadata{}, err
	}
	meta = buildMetadata(proxies)
	return clashYAML, meta, nil
}

// carambaSelector — имя основной группы-селектора, которую формирует панель и на
// которую ссылается финальное правило MATCH в profile.AssembleMihomoConfig
// (см. profile.CarambaSelector, контракт панель<->клиент = литерал "CARAMBA").
// Дублируем литерал здесь, а не импортируем profile, чтобы subimport остался
// чисто парсящим пакетом без лишних зависимостей и риска цикла импорта.
const carambaSelector = "CARAMBA"

// marshalClash сериализует список прокси в clash/mihomo YAML с секциями proxies:
// и proxy-groups:. Группа-селектор CARAMBA собирает все импортированные узлы и
// DIRECT, повторяя форму конфига панели (subscription.Profile.RawYAML всегда
// несёт эту группу). Без неё applyRules() в profile.AssembleMihomoConfig добавит
// `MATCH,CARAMBA`, ссылающийся на несуществующую группу, и mihomo отвергнет
// конфиг при загрузке — туннель не поднимется на реальной (-tags mihomo) сборке.
func marshalClash(proxies proxyList) ([]byte, error) {
	names := make([]any, 0, len(proxies)+1)
	for _, px := range proxies {
		if n, ok := px["name"].(string); ok && n != "" {
			names = append(names, n)
		}
	}
	// DIRECT даёт безопасный фолбэк в селекторе, как в конфиге панели.
	names = append(names, "DIRECT")

	doc := map[string]any{
		"proxies": []map[string]any(proxies),
		"proxy-groups": []map[string]any{
			{
				"name":    carambaSelector,
				"type":    "select",
				"proxies": names,
			},
		},
	}
	out, err := yaml.Marshal(doc)
	if err != nil {
		return nil, fmt.Errorf("subimport: сериализация clash YAML: %w", err)
	}
	return out, nil
}

// buildMetadata собирает Metadata.Servers из произведённых proxies. Каждый прокси
// обязан нести name/type/server/port — иначе соответствующее поле сервера будет
// пустым (см. предупреждение в Import). Страна извлекается из имени
// (CountryFromName), ID = имя прокси: именно его приложение передаёт обратно в
// Up(serverID), чтобы закрепить узел в селекторе CARAMBA (контракт ABI v2).
func buildMetadata(proxies proxyList) Metadata {
	servers := make([]subscription.Server, 0, len(proxies))
	for _, px := range proxies {
		name, _ := px["name"].(string)
		typ, _ := px["type"].(string)
		server, _ := px["server"].(string)
		servers = append(servers, subscription.Server{
			ID:      name,
			Name:    name,
			Type:    typ,
			Server:  server,
			Port:    asInt(px["port"]),
			Country: CountryFromName(name),
		})
	}
	return Metadata{Servers: servers}
}
