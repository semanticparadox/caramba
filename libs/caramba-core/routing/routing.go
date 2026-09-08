// Package routing описывает модель «умной маршрутизации» трафика клиента
// (как в Koala Clash): упорядоченные правила «матчер → действие», внешние
// автообновляемые списки (rule-providers) и компиляция всего этого в секции
// mihomo-конфига (`rules` + `rule-providers`).
//
// Модель платформо-независима: одни и те же правила использует CLI и Flutter.
// Действие PROXY всегда резолвится в имя группы-селектора (по умолчанию
// "CARAMBA"), которую формирует панель.
package routing

import (
	"fmt"
	"strings"
)

// Action — действие правила (куда направить совпавший трафик).
type Action string

const (
	// ActionProxy направляет трафик в группу-селектор (через VPN).
	ActionProxy Action = "PROXY"
	// ActionDirect пускает трафик напрямую, мимо туннеля.
	ActionDirect Action = "DIRECT"
	// ActionReject блокирует трафик (чёрная дыра).
	ActionReject Action = "REJECT"
)

// MatchType — тип матчера правила mihomo.
type MatchType string

const (
	MatchDomain        MatchType = "DOMAIN"
	MatchDomainSuffix  MatchType = "DOMAIN-SUFFIX"
	MatchDomainKeyword MatchType = "DOMAIN-KEYWORD"
	MatchGeosite       MatchType = "GEOSITE"
	MatchGeoIP         MatchType = "GEOIP"
	MatchIPCIDR        MatchType = "IP-CIDR"
	MatchIPCIDR6       MatchType = "IP-CIDR6"
	MatchProcessName   MatchType = "PROCESS-NAME"
	MatchProcessPath   MatchType = "PROCESS-PATH"
	MatchRuleSet       MatchType = "RULE-SET"
	MatchDstPort       MatchType = "DST-PORT"
)

// ipBased — матчеры, работающие по IP. Для них при необходимости добавляется
// модификатор no-resolve, чтобы не форсировать DNS-резолв доменов.
func (m MatchType) ipBased() bool {
	switch m {
	case MatchGeoIP, MatchIPCIDR, MatchIPCIDR6:
		return true
	default:
		return false
	}
}

// Rule — одно правило маршрутизации.
type Rule struct {
	Type   MatchType
	Value  string // домен / geosite-тег / CIDR / имя процесса / имя rule-set
	Action Action
	// NoResolve запрещает DNS-резолв перед сопоставлением (для IP-правил —
	// строго рекомендуется, иначе ломается fake-ip и растёт задержка).
	NoResolve bool
	// FallbackFor — имя rule-set'а, ЗАМЕНОЙ которому служит это правило.
	//
	// Правило с непустым FallbackFor попадает в конфиг только тогда, когда
	// названный список в этой сборке недоступен (нет ни проверенного файла,
	// ни зеркала). Доехавший список делает замену лишней, и дублировать её
	// в правилах незачем.
	//
	// Поле существует ради ОТЧЁТА, а не ради экономии правил. Блок рекламы
	// работал через geosite("category-ads-all") — встроенную базу mihomo,
	// про которую сборка конфига не знает ничего и честно обязана говорить
	// «неизвестно». Список `ads` с зеркала оператора — наблюдаемый источник,
	// и BuildWithReport называет, какой из двух включился. Без этого поля тег
	// в GeositeTags неотличим от самостоятельного правила пресета.
	//
	// На компиляцию правило поле не влияет: CompiledRules видит уже
	// отобранный набор. Решение принимает BuildWithReport — там же, где
	// известна судьба самого списка.
	FallbackFor string
}

// ProviderBehavior — тип содержимого внешнего списка.
type ProviderBehavior string

const (
	BehaviorDomain    ProviderBehavior = "domain"
	BehaviorIPCIDR    ProviderBehavior = "ipcidr"
	BehaviorClassical ProviderBehavior = "classical"
)

// ProviderFormat — формат файла списка.
type ProviderFormat string

const (
	FormatMrs  ProviderFormat = "mrs" // бинарный формат mihomo (компактный, быстрый)
	FormatYaml ProviderFormat = "yaml"
	FormatText ProviderFormat = "text"
)

// RuleProvider — внешний автообновляемый список (RULE-SET).
//
// URL по умолчанию должен указывать на ЗЕРКАЛО панели, а не на GitHub:
// в ряде стран (РФ) прямой доступ к GitHub raw нестабилен/заблокирован, а
// панель доступна клиенту по определению. Панель централизованно обновляет
// списки из апстримов (см. presets.go: RecommendedUpstreams).
type RuleProvider struct {
	Name     string
	Behavior ProviderBehavior
	Format   ProviderFormat
	URL      string
	// Interval — период автообновления в секундах (0 → 86400).
	Interval int
	// Proxy — имя исходящего, через который ядро тянет список (ключ `proxy:`
	// в rule-providers). Пусто — ядро идёт в открытый интернет собственным
	// диалером мимо туннеля и мимо лестницы. Для зеркал каталога сюда
	// подставляется группа-селектор, чтобы загрузка ехала по туннелю.
	Proxy string
	// Path — локальный файл со списком. Непустой Path переводит провайдера в
	// vehicle `file`: ядро читает готовые байты и НЕ ходит в сеть.
	//
	// Это единственная форма, совместимая с инвариантом 12. Подписанный
	// sha256 фиксирует ровно одно содержимое; http-провайдер с interval либо
	// перекачивал бы те же байты, либо принимал бы неподписанные новые. Файл
	// сюда кладёт transport.ResourceGuard уже после сверки хеша, а обновление
	// приходит следующим каталогом с новым подписанным хешем.
	Path string
}

// Config — полная конфигурация маршрутизации.
type Config struct {
	// ProxyGroup — имя группы-селектора, в которую резолвится ActionProxy.
	ProxyGroup string
	Rules      []Rule
	Providers  []RuleProvider
	// FinalAction — действие для нераспознанного трафика (правило MATCH).
	// Для «умных» пресетов РФ/Ирана это DIRECT (проксируем только нужное),
	// для «полного обхода»/Китая — PROXY.
	FinalAction Action
}

// proxyGroupOr возвращает имя группы или дефолт.
func (c Config) proxyGroupOr(def string) string {
	if strings.TrimSpace(c.ProxyGroup) != "" {
		return c.ProxyGroup
	}
	return def
}

// resolveTarget переводит Action в целевой объект правила mihomo:
// PROXY → имя группы-селектора, DIRECT/REJECT → как есть.
func resolveTarget(a Action, group string) string {
	switch a {
	case ActionProxy:
		return group
	case ActionDirect:
		return string(ActionDirect)
	case ActionReject:
		return string(ActionReject)
	default:
		// Неизвестное действие трактуем как имя кастомной группы/прокси.
		return string(a)
	}
}

// CompiledRules собирает упорядоченный список строковых правил mihomo.
//
// defaultGroup — имя селектора, если в Config.ProxyGroup пусто.
// Финальное правило MATCH добавляется всегда (FinalAction, по умолчанию DIRECT).
func (c Config) CompiledRules(defaultGroup string) []string {
	group := c.proxyGroupOr(defaultGroup)
	out := make([]string, 0, len(c.Rules)+1)

	for _, r := range c.Rules {
		if r.Type == "" || strings.TrimSpace(r.Value) == "" {
			continue
		}
		target := resolveTarget(r.Action, group)
		line := fmt.Sprintf("%s,%s,%s", r.Type, r.Value, target)
		if r.NoResolve && r.Type.ipBased() {
			line += ",no-resolve"
		}
		out = append(out, line)
	}

	final := c.FinalAction
	if final == "" {
		final = ActionDirect
	}
	out = append(out, "MATCH,"+resolveTarget(final, group))
	return out
}

// CompiledProviders возвращает секцию rule-providers как map для YAML.
// Возвращает nil, если провайдеров нет (чтобы не плодить пустые секции).
func (c Config) CompiledProviders() map[string]any {
	if len(c.Providers) == 0 {
		return nil
	}
	providers := make(map[string]any, len(c.Providers))
	for _, p := range c.Providers {
		if strings.TrimSpace(p.Name) == "" {
			continue
		}
		if strings.TrimSpace(p.URL) == "" && strings.TrimSpace(p.Path) == "" {
			continue
		}
		interval := p.Interval
		if interval <= 0 {
			interval = 86400
		}
		behavior := p.Behavior
		if behavior == "" {
			behavior = BehaviorClassical
		}
		format := p.Format
		if format == "" {
			format = FormatYaml
		}
		entry := map[string]any{
			"behavior": string(behavior),
			"format":   string(format),
		}
		if strings.TrimSpace(p.Path) != "" {
			// Проверенный файл: ядро не ходит в сеть, поэтому ни url, ни
			// interval, ни proxy здесь не имеют смысла.
			entry["type"] = "file"
			entry["path"] = p.Path
		} else {
			entry["type"] = "http"
			entry["url"] = p.URL
			entry["interval"] = interval
			if px := strings.TrimSpace(p.Proxy); px != "" {
				entry["proxy"] = px
			}
		}
		providers[p.Name] = entry
	}
	if len(providers) == 0 {
		return nil
	}
	return providers
}

// Validate проверяет внутреннюю согласованность: каждое RULE-SET-правило
// ссылается на существующий провайдер, у IP-правил корректные модификаторы.
func (c Config) Validate() error {
	known := make(map[string]struct{}, len(c.Providers))
	for _, p := range c.Providers {
		known[p.Name] = struct{}{}
	}
	for i, r := range c.Rules {
		if r.Type == MatchRuleSet {
			if _, ok := known[r.Value]; !ok {
				return fmt.Errorf("routing: правило #%d ссылается на неизвестный rule-set %q", i, r.Value)
			}
		}
	}
	return nil
}

// Merge возвращает новый Config, в котором правила other добавлены В НАЧАЛО
// (более высокий приоритет), а провайдеры объединены без дублей по имени.
// Используется, чтобы пользовательские правила перекрывали пресет.
func (c Config) Merge(higherPriority Config) Config {
	res := Config{
		ProxyGroup:  c.proxyGroupOr(higherPriority.ProxyGroup),
		FinalAction: c.FinalAction,
		Rules:       append(append([]Rule{}, higherPriority.Rules...), c.Rules...),
	}
	if higherPriority.FinalAction != "" {
		res.FinalAction = higherPriority.FinalAction
	}
	seen := make(map[string]struct{})
	for _, p := range append(append([]RuleProvider{}, c.Providers...), higherPriority.Providers...) {
		if _, ok := seen[p.Name]; ok {
			continue
		}
		seen[p.Name] = struct{}{}
		res.Providers = append(res.Providers, p)
	}
	return res
}
