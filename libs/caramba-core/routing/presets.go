package routing

import (
	"sort"
	"strings"
)

// Preset — именованный набор правил маршрутизации, готовый к применению.
//
// URL внешних списков задаётся через плейсхолдер "{BASE}", который при сборке
// заменяется на базовый адрес зеркала (обычно панель), см. Build.
type Preset struct {
	ID          string
	Name        string // человекочитаемое имя (RU)
	Emoji       string
	Country     string // ISO-код страны или "" для глобальных
	Description string
	// FinalAction — поведение для нераспознанного трафика.
	FinalAction Action
	Rules       []Rule
	Providers   []RuleProvider
}

// PoolOptions описывает, откуда пресет берёт свои внешние списки.
//
// Плейсхолдер "{BASE}" исторически подставлялся одним базовым адресом панели.
// Пул зеркал обобщает его: 02-SPEC.md 8.10 требует, чтобы подстановка шла из
// того же упорядоченного пула, которым пользуется выборка манифеста, а не из
// единственного адреса, который цензор блокирует первым.
type PoolOptions struct {
	// Bases — упорядоченный пул зеркал ("https://host"). Пусто означает, что
	// удалённые списки недоступны: ruleset-правила и провайдеры выбрасываются.
	//
	// В URL провайдера подставляется ПЕРВОЕ зеркало: у http-vehicle mihomo
	// один адрес и переключаться между ними он не умеет. Остальной пул живёт
	// в лестнице, которая и перебирает зеркала при проверенной загрузке
	// (см. Verified и transport.ResourceGuard).
	Bases []string
	// Proxy — имя исходящего для ключа proxy: у провайдеров. Обычно это
	// группа-селектор: тогда список едет по туннелю, а не в открытый интернет.
	Proxy string
	// Files — проверенные локальные файлы по имени rule-set'а. Имя, которое
	// здесь есть, всегда побеждает http-форму: байты уже сверены с подписанным
	// sha256, и повторно качать их незачем.
	Files map[string]string
	// Verified требует проверенный файл на КАЖДЫЙ rule-set. Провайдер без
	// файла и правила, которые на него ссылаются, выбрасываются: инвариант 12
	// запрещает докачивать неподписанное вместо подписанного. Отказ, а не
	// откат на непроверенную копию.
	Verified bool
}

// Build возвращает Config пресета, подставляя baseURL вместо "{BASE}" в URL
// провайдеров. group — имя группы-селектора для действия PROXY.
//
// Это тонкая обёртка над BuildWith для вызывающих без пула зеркал.
func (p Preset) Build(baseURL, group string) Config {
	base := strings.TrimSpace(baseURL)
	opt := PoolOptions{}
	if base != "" {
		opt.Bases = []string{base}
	}
	return p.BuildWith(opt, group)
}

// BuildWith возвращает Config пресета по описанию пула.
func (p Preset) BuildWith(opt PoolOptions, group string) Config {
	base := ""
	for _, b := range opt.Bases {
		if b = strings.TrimRight(strings.TrimSpace(b), "/"); b != "" {
			base = b
			break
		}
	}

	// Какие rule-set'ы вообще доступны в этой сборке конфига.
	available := func(name string) bool {
		if _, ok := opt.Files[name]; ok {
			return true
		}
		if opt.Verified {
			// Проверенного файла нет: неподписанная докачка запрещена.
			return false
		}
		return base != ""
	}

	providers := make([]RuleProvider, 0, len(p.Providers))
	for _, rp := range p.Providers {
		if !available(rp.Name) {
			continue
		}
		if path, ok := opt.Files[rp.Name]; ok {
			rp.URL = ""
			rp.Interval = 0
			rp.Proxy = ""
			rp.Path = path
		} else {
			rp.URL = strings.ReplaceAll(rp.URL, "{BASE}", base)
			rp.Proxy = opt.Proxy
		}
		providers = append(providers, rp)
	}

	// Без базового URL (клиент без панели, импортированная подписка) и без
	// проверенных файлов удалённые rule-provider'ы недоступны: mihomo падал бы
	// на Get "/rulesets/..." с пустой схемой. Оставляем только
	// geosite/geoip/домены; ruleset-правила выкидываем вместе с провайдерами.
	rules := make([]Rule, 0, len(p.Rules))
	for _, r := range p.Rules {
		if r.Type == MatchRuleSet && !available(r.Value) {
			continue
		}
		rules = append(rules, r)
	}

	if len(providers) == 0 {
		providers = nil
	}
	return Config{
		ProxyGroup:  group,
		FinalAction: p.FinalAction,
		Rules:       rules,
		Providers:   providers,
	}
}

// ─── Хелперы построения правил ─────────────────────────────────────────────

// geosite — правило по geosite-тегу (использует встроенную базу mihomo).
func geosite(tag string, a Action) Rule { return Rule{Type: MatchGeosite, Value: tag, Action: a} }

// geoip — правило по geoip-коду (с no-resolve по умолчанию).
func geoip(code string, a Action) Rule {
	return Rule{Type: MatchGeoIP, Value: code, Action: a, NoResolve: true}
}

// ruleset — правило по внешнему списку.
func ruleset(name string, a Action) Rule { return Rule{Type: MatchRuleSet, Value: name, Action: a} }

// appRules разворачивает один логический «апп» в правила PROCESS-NAME для всех
// платформ (Android-пакет + бинарники Win/macOS/Linux).
func appRules(a Action, names ...string) []Rule {
	out := make([]Rule, 0, len(names))
	for _, n := range names {
		out = append(out, Rule{Type: MatchProcessName, Value: n, Action: a})
	}
	return out
}

// Имена процессов Telegram на разных платформах.
var telegramProcesses = []string{
	"org.telegram.messenger",     // Android (Google Play)
	"org.telegram.messenger.web", // Android (Web/др. сборки)
	"Telegram.exe",               // Windows
	"Telegram",                   // macOS / Linux (Telegram Desktop)
	"telegram-desktop",           // Linux (пакетные сборки)
}

// privateAndAds — общий хвост: приватные сети напрямую, реклама в блок.
// Ставится в начало большинства пресетов.
func leadIn(blockAds bool) []Rule {
	r := []Rule{
		geoip("private", ActionDirect),
	}
	if blockAds {
		r = append(r, geosite("category-ads-all", ActionReject))
	}
	return r
}

// ─── Реестр пресетов ───────────────────────────────────────────────────────

// presetList — все встроенные пресеты в порядке отображения.
var presetList = []Preset{
	// 🇷🇺 Россия — умный режим: по умолчанию ДИРЕКТ, через прокси только
	// заблокированное. Это основной режим для РФ.
	{
		ID: "ru-smart", Name: "Россия (умный)", Emoji: "🇷🇺", Country: "RU",
		Description: "По умолчанию напрямую. Через VPN — только заблокированные сервисы (Telegram, Instagram, X, YouTube, Discord, ChatGPT и список заблокированного в РФ). Российские сайты и банки — напрямую.",
		FinalAction: ActionDirect,
		Rules: append(append(leadIn(false),
			// Заблокированные в РФ популярные сервисы → через прокси.
			geosite("telegram", ActionProxy),
			geosite("instagram", ActionProxy),
			geosite("facebook", ActionProxy),
			geosite("twitter", ActionProxy),
			geosite("youtube", ActionProxy),
			geosite("discord", ActionProxy),
			geosite("openai", ActionProxy),
			// Внешний автообновляемый список «заблокировано в РФ».
			ruleset("ru-blocked", ActionProxy),
			ruleset("ru-blocked-ip", ActionProxy),
		),
			// Российское — напрямую (чтобы не ломать банки/госуслуги и не грузить ноды).
			geosite("category-ru", ActionDirect),
			geoip("RU", ActionDirect),
		),
		Providers: []RuleProvider{
			{Name: "ru-blocked", Behavior: BehaviorDomain, Format: FormatText, URL: "{BASE}/rulesets/ru-blocked", Interval: 43200},
			{Name: "ru-blocked-ip", Behavior: BehaviorIPCIDR, Format: FormatText, URL: "{BASE}/rulesets/ru-blocked-ip", Interval: 43200},
		},
	},
	// 🇷🇺 Россия — полный обход: всё через прокси, кроме российского и LAN.
	{
		ID: "ru-full", Name: "Россия (полный обход)", Emoji: "🇷🇺", Country: "RU",
		Description: "Весь трафик через VPN, напрямую — только российские сайты, российские IP и локальная сеть.",
		FinalAction: ActionProxy,
		Rules: append(leadIn(false),
			geosite("category-ru", ActionDirect),
			geoip("RU", ActionDirect),
		),
	},
	// ✈️ Только Telegram — пример пользователя: Telegram через прокси, остальное директ.
	{
		ID: "telegram-only", Name: "Только Telegram", Emoji: "✈️", Country: "",
		Description: "Через VPN идёт только Telegram (приложение + домены + IP-диапазоны). Всё остальное — напрямую.",
		FinalAction: ActionDirect,
		Rules: append(append(append(leadIn(false),
			appRules(ActionProxy, telegramProcesses...)...),
			geosite("telegram", ActionProxy)),
			geoip("telegram", ActionProxy),
		),
	},
	// 🇮🇷 Иран — умный режим, аналогично РФ.
	{
		ID: "ir-smart", Name: "Иран (умный)", Emoji: "🇮🇷", Country: "IR",
		Description: "По умолчанию напрямую. Через VPN — заблокированные в Иране ресурсы. Иранские сайты и IP — напрямую.",
		FinalAction: ActionDirect,
		Rules: append(append(leadIn(false),
			geosite("telegram", ActionProxy),
			geosite("youtube", ActionProxy),
			geosite("twitter", ActionProxy),
			geosite("facebook", ActionProxy),
			geosite("openai", ActionProxy),
			ruleset("ir-blocked", ActionProxy),
		),
			geoip("IR", ActionDirect),
		),
		Providers: []RuleProvider{
			{Name: "ir-blocked", Behavior: BehaviorDomain, Format: FormatText, URL: "{BASE}/rulesets/ir-blocked", Interval: 43200},
		},
	},
	// 🇧🇾 Беларусь — умный режим, как РФ.
	{
		ID: "by-smart", Name: "Беларусь (умный)", Emoji: "🇧🇾", Country: "BY",
		Description: "По умолчанию напрямую. Через VPN — заблокированные сервисы. Белорусское и LAN — напрямую.",
		FinalAction: ActionDirect,
		Rules: append(append(leadIn(false),
			geosite("telegram", ActionProxy),
			geosite("instagram", ActionProxy),
			geosite("twitter", ActionProxy),
			geosite("youtube", ActionProxy),
			ruleset("by-blocked", ActionProxy),
		),
			geoip("BY", ActionDirect),
		),
		Providers: []RuleProvider{
			{Name: "by-blocked", Behavior: BehaviorDomain, Format: FormatText, URL: "{BASE}/rulesets/by-blocked", Interval: 43200},
		},
	},
	// 🇨🇳 Китай — обратная логика: всё через прокси, китайское — напрямую.
	{
		ID: "cn-smart", Name: "Китай (умный)", Emoji: "🇨🇳", Country: "CN",
		Description: "Весь зарубежный трафик через VPN, китайские сайты и IP — напрямую (классическая схема GFW).",
		FinalAction: ActionProxy,
		Rules: append(leadIn(true),
			geosite("cn", ActionDirect),
			geoip("CN", ActionDirect),
		),
	},
	// 🌍 Глобал/стриминг — директ по умолчанию, через прокси только стриминги/AI.
	{
		ID: "streaming", Name: "Стриминг и AI", Emoji: "🌍", Country: "",
		Description: "По умолчанию напрямую. Через VPN — Netflix, YouTube, Spotify, Disney+, ChatGPT (обход гео-ограничений).",
		FinalAction: ActionDirect,
		Rules: append(leadIn(false),
			geosite("netflix", ActionProxy),
			geosite("youtube", ActionProxy),
			geosite("spotify", ActionProxy),
			geosite("disney", ActionProxy),
			geosite("openai", ActionProxy),
		),
	},
	// 🛡️ Блок рекламы — ничего не проксируем, только режем рекламу/трекеры.
	{
		ID: "adblock", Name: "Только блок рекламы", Emoji: "🛡️", Country: "",
		Description: "VPN не меняет маршрут трафика — только блокирует рекламу и трекеры на уровне DNS/правил.",
		FinalAction: ActionDirect,
		Rules:       leadIn(true),
	},
	// 🌐 Глобальный обход — всё через прокси (классический «full tunnel»).
	{
		ID: "global", Name: "Полный обход", Emoji: "🌐", Country: "",
		Description: "Весь трафик через VPN, напрямую — только локальная сеть.",
		FinalAction: ActionProxy,
		Rules:       leadIn(false),
	},
}

// RecommendedUpstreams — какие апстримы панель должна зеркалить в /rulesets/NAME.
// Ключ — имя rule-set'а (и путь /rulesets/NAME), значение — апстрим-источник.
// Панель отдаёт списки текстом (behavior=domain|ipcidr, format=text): один
// элемент на строку, без бинарного .mrs-тулинга. Так список остаётся актуальным
// и доступным даже там, где GitHub заблокирован, а зеркало — это просто
// текстовый файл за обычным HTTP.
var RecommendedUpstreams = map[string]string{
	"ru-blocked":    "github.com/runetfreedom/russia-v2ray-rules-dat (geosite: russia-blocked) либо itdoginfo/allow-domains (Russia/inside)",
	"ru-blocked-ip": "github.com/runetfreedom/russia-v2ray-rules-dat (geoip: russia-blocked-ip)",
	"ir-blocked":    "github.com/bootmortis/iran-hosted-domains либо chocolate4u/Iran-clash-rules",
	"by-blocked":    "наследует РФ-список + локальные дополнения",
}

// ─── Доступ к реестру ──────────────────────────────────────────────────────

// Presets возвращает копию списка всех встроенных пресетов.
func Presets() []Preset {
	out := make([]Preset, len(presetList))
	copy(out, presetList)
	return out
}

// PresetByID находит пресет по идентификатору.
func PresetByID(id string) (Preset, bool) {
	for _, p := range presetList {
		if p.ID == id {
			return p, true
		}
	}
	return Preset{}, false
}

// PresetsForCountry возвращает пресеты, релевантные стране (по ISO-коду), а
// также глобальные пресеты. Страновые идут первыми.
func PresetsForCountry(iso string) []Preset {
	iso = strings.ToUpper(strings.TrimSpace(iso))
	var country, global []Preset
	for _, p := range presetList {
		switch {
		case p.Country == iso && iso != "":
			country = append(country, p)
		case p.Country == "":
			global = append(global, p)
		}
	}
	sort.SliceStable(country, func(i, j int) bool { return country[i].ID < country[j].ID })
	return append(country, global...)
}
