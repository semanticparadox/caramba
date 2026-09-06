package api

import (
	"encoding/json"
	"fmt"
	"sort"
	"strings"

	"github.com/semanticparadox/caramba/libs/caramba-core/profile"
	"github.com/semanticparadox/caramba/libs/caramba-core/routing"
)

// policyPatch — разбор JSON-политики приложения (CoreConfig на стороне Flutter).
//
// Все поля опциональны, поэтому это указатели: nil означает «приложение поле не
// прислало, текущее значение не трогаем». Неизвестные ключи игнорируются молча
// (контракт ABI v2): приложение может присылать поля, которых ядро этой версии
// ещё не знает.
type policyPatch struct {
	Protocol   *string `json:"protocol"`
	Preset     *string `json:"preset"`
	Relay      *string `json:"relay"`
	Stack      *string `json:"stack"`
	MTU        *int    `json:"mtu"`
	IPv6       *bool   `json:"ipv6"`
	FakeIP     *bool   `json:"fakeIp"`
	KillSwitch *bool   `json:"killSwitch"`
	// Adblock — блок рекламы и трекеров ПОВЕРХ выбранного пресета.
	// Отдельно от preset намеренно: до него «резать рекламу» означало сменить
	// режим страны, и в интерфейсе это была не галочка, а выбор из списка.
	Adblock *bool `json:"adblock"`
	DNS     *struct {
		Nameservers []string `json:"nameservers"`
		Fallback    []string `json:"fallback"`
	} `json:"dns"`
	Split *struct {
		Mode          *string  `json:"mode"`
		Apps          []string `json:"apps"`
		BypassDomains []string `json:"bypassDomains"`
		AllowDomains  []string `json:"allowDomains"`
		AllowSites    []string `json:"allowSites"`
	} `json:"split"`
}

// allowedSiteTags — закрытый словарь тегов GEOSITE, которые приложение может
// назвать в split.allowSites.
//
// Словарь закрыт, а не «любой тег»: незнакомый базе GeoSite.dat тег не даёт ни
// ошибки, ни правила — mihomo молча его не сопоставит, и человек, добавивший
// «vk», получил бы выключенную строку, выглядящую включённой. Здесь перечислены
// ровно те теги, которыми уже пользуются встроенные пресеты, то есть
// проверенные на живом флоте.
var allowedSiteTags = map[string]struct{}{
	"telegram":  {},
	"youtube":   {},
	"instagram": {},
	"twitter":   {},
	"facebook":  {},
	"discord":   {},
	"openai":    {},
	"netflix":   {},
	"spotify":   {},
	"disney":    {},
}

// AllowedSiteTags возвращает словарь тегов сайтов в стабильном порядке.
// Зеркало на стороне приложения фиксируется тестом Dart; расхождение обязано
// проявиться отказом ядра, а не молчаливо не сработавшим правилом.
func AllowedSiteTags() []string {
	out := make([]string, 0, len(allowedSiteTags))
	for tag := range allowedSiteTags {
		out = append(out, tag)
	}
	sort.Strings(out)
	return out
}

// SetPolicyJSON применяет политику приложения, переданную одной JSON-строкой
// (контракт ABI v2, CarambaSetPolicy). Форма:
//
//	{"protocol":"auto|AmneziaWG|VLESS-Reality|Hysteria2|TUIC|Shadowsocks",
//	 "preset":"ru-smart|ru-full|telegram-only|ir-smart|by-smart|cn-smart|streaming|adblock|global|",
//	 "relay":"TR|KZ|FI|",
//	 "stack":"gvisor|system|mixed",
//	 "mtu":1280, "ipv6":false, "fakeIp":true, "killSwitch":true,
//	 "dns":{"nameservers":[...],"fallback":[...]},
//	 "split":{"mode":"off|bypass|allow","apps":[...],"bypassDomains":[...]}}
//
// Все поля опциональны; отсутствующее поле оставляет текущее значение.
// Неизвестные ключи игнорируются. Недопустимое значение перечислимого поля —
// ошибка, в тексте которой названо конкретное поле; в этом случае политика НЕ
// меняется вовсе (применение атомарно: сперва собираем новую политику целиком,
// и только потом подменяем).
//
// Политика применяется при СЛЕДУЮЩЕМ Up. Если туннель уже поднят, изменения не
// вступают в силу до Down + Up — приложение обязано переподключиться само.
func (c *Core) SetPolicyJSON(jsonStr string) error {
	jsonStr = strings.TrimSpace(jsonStr)
	if jsonStr == "" {
		return fmt.Errorf("api: пустая политика")
	}
	var patch policyPatch
	if err := json.Unmarshal([]byte(jsonStr), &patch); err != nil {
		return fmt.Errorf("api: разбор политики: %w", err)
	}

	c.mu.Lock()
	defer c.mu.Unlock()

	// Работаем с копией: любая ошибка валидации оставит текущую политику целой.
	p := c.policy
	relay := c.relayCountry
	presetID := c.presetID

	if patch.Protocol != nil {
		canonical, ok := profile.CanonicalProtocol(*patch.Protocol)
		if !ok {
			return fmt.Errorf("api: недопустимое значение поля protocol: %q", *patch.Protocol)
		}
		p.Protocol = canonical
	}

	if patch.Preset != nil {
		id := strings.TrimSpace(*patch.Preset)
		if id == "" {
			// Пустой пресет — возврат к базовым правилам без страновой логики.
			p.Routing = nil
			presetID = ""
		} else {
			preset, ok := routing.PresetByID(id)
			if !ok {
				return fmt.Errorf("api: недопустимое значение поля preset: %q", id)
			}
			cfg := preset.Build(c.cfg.PanelBaseURL, profile.CarambaSelector)
			if err := cfg.Validate(); err != nil {
				return fmt.Errorf("api: поле preset %q: %w", id, err)
			}
			p.Routing = &cfg
			// Идентификатор запоминается наравне с правилами.
			//
			// Без этого путь приложения (единственный его канал — setPolicy)
			// оставлял presetID пустым, и всё, что стоит на нём, работало
			// вхолостую: routingForBuild не пересобирал пресет под каталог,
			// ruleSetNames не заказывал его списки, а отчёт о применённом
			// маршруте называл источником «правила оператора» и не мог
			// подтвердить ни блок рекламы, ни стриминг. Ровно то «непонятно,
			// работает или нет», против которого отчёт и заводился.
			presetID = id
		}
	}

	if patch.Relay != nil {
		v, err := normalizeRelay(*patch.Relay)
		if err != nil {
			return err
		}
		relay = v
	}

	if patch.Stack != nil {
		stack, ok := profile.CanonicalStack(*patch.Stack)
		if !ok {
			return fmt.Errorf("api: недопустимое значение поля stack: %q (ожидается gvisor|system|mixed)", *patch.Stack)
		}
		if stack != "" {
			p.Tun.Stack = stack
		}
	}

	if patch.MTU != nil {
		mtu := *patch.MTU
		// 0 — «оставить умолчание ядра»; отрицательное или заведомо
		// нежизнеспособное значение отвергаем, иначе ядро молча соберёт
		// неработающий TUN.
		if mtu < 0 || (mtu > 0 && (mtu < 576 || mtu > 9000)) {
			return fmt.Errorf("api: недопустимое значение поля mtu: %d (ожидается 0 либо 576..9000)", mtu)
		}
		if mtu > 0 {
			p.Tun.MTU = mtu
		}
	}

	if patch.IPv6 != nil {
		p.IPv6 = *patch.IPv6
	}

	if patch.FakeIP != nil {
		// fake-ip имеет смысл только с TUN; в proxy-режиме applyDNS сам понизит
		// его до redir-host (см. profile.applyDNS).
		if *patch.FakeIP {
			p.DNS.EnhancedMode = "fake-ip"
		} else {
			p.DNS.EnhancedMode = "redir-host"
		}
	}

	if patch.KillSwitch != nil {
		p.KillSwitch = *patch.KillSwitch
	}

	if patch.Adblock != nil {
		p.BlockAds = *patch.Adblock
	}

	if patch.DNS != nil {
		if patch.DNS.Nameservers != nil {
			p.DNS.Nameservers = cleanList(patch.DNS.Nameservers)
		}
		if patch.DNS.Fallback != nil {
			p.DNS.FallbackNameservers = cleanList(patch.DNS.Fallback)
		}
	}

	if patch.Split != nil {
		split := profile.SplitTunnel{}
		if patch.Split.BypassDomains != nil {
			split.BypassDomains = cleanList(patch.Split.BypassDomains)
		} else {
			split.BypassDomains = p.Split.BypassDomains
		}
		mode := "off"
		if patch.Split.Mode != nil {
			mode = strings.ToLower(strings.TrimSpace(*patch.Split.Mode))
			if mode == "" {
				mode = "off"
			}
		}
		allowDomains := cleanList(patch.Split.AllowDomains)
		allowSites := cleanList(patch.Split.AllowSites)
		for _, tag := range allowSites {
			if _, ok := allowedSiteTags[tag]; !ok {
				return fmt.Errorf("api: недопустимое значение поля split.allowSites: %q (ожидается один из %v)", tag, AllowedSiteTags())
			}
		}
		apps := cleanList(patch.Split.Apps)
		switch mode {
		case "off":
			// Списки сбрасываются: режим выключен. Сайтовый allow-список тоже —
			// иначе «выключено» продолжало бы уводить весь остальной трафик
			// мимо туннеля, и выключатель ничего бы не выключал.
		case "bypass":
			split.BypassProcesses = apps
		case "allow":
			split.AllowProcesses = apps
			split.AllowDomains = allowDomains
			split.AllowSites = allowSites
		default:
			return fmt.Errorf("api: недопустимое значение поля split.mode: %q (ожидается off|bypass|allow)", mode)
		}
		p.Split = split
	}

	c.policy = p
	c.relayCountry = relay
	c.presetID = presetID
	return nil
}

// normalizeRelay приводит страну relay-входа к ISO-2 в верхнем регистре.
// Пустая строка допустима и означает прямой вход (без relay).
func normalizeRelay(v string) (string, error) {
	v = strings.TrimSpace(v)
	if v == "" {
		return "", nil
	}
	if len(v) != 2 {
		return "", fmt.Errorf("api: недопустимое значение поля relay: %q (ожидается ISO-2 код страны либо пусто)", v)
	}
	for _, r := range v {
		if (r < 'A' || r > 'Z') && (r < 'a' || r > 'z') {
			return "", fmt.Errorf("api: недопустимое значение поля relay: %q (ожидается ISO-2 код страны либо пусто)", v)
		}
	}
	return strings.ToUpper(v), nil
}

// cleanList выбрасывает пустые элементы и лишние пробелы. Пустой результат —
// nil, чтобы в YAML не появлялись пустые секции.
func cleanList(in []string) []string {
	out := make([]string, 0, len(in))
	for _, s := range in {
		if s = strings.TrimSpace(s); s != "" {
			out = append(out, s)
		}
	}
	if len(out) == 0 {
		return nil
	}
	return out
}
