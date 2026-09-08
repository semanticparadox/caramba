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
//
// Это обёртка над BuildWithReport для вызывающих, которым отчёт не нужен.
func (p Preset) BuildWith(opt PoolOptions, group string) Config {
	cfg, _ := p.BuildWithReport(opt, group)
	return cfg
}

// RuleSourceState — исход ОДНОГО внешнего списка на конкретной сборке конфига.
type RuleSourceState string

const (
	// RuleSourceFile: провайдер переведён на проверенный локальный файл. Байты
	// уже сверены с подписанным sha256, ядро в сеть за ними не пойдёт.
	RuleSourceFile RuleSourceState = "file"
	// RuleSourceMirror: провайдер остался http-формой на зеркало. Это НЕ
	// «список работает»: здесь известно только то, что адрес подставлен.
	// Ответил ли зеркало и разобрало ли ядро содержимое — знает ядро, а не
	// сборка конфига.
	RuleSourceMirror RuleSourceState = "mirror"
	// RuleSourceDropped: провайдер и все ссылающиеся на него правила выброшены.
	RuleSourceDropped RuleSourceState = "dropped"
)

// Машинные коды причин выброса. Строка кода пользователю не показывается:
// текст по ней выбирает обвязка.
const (
	// RuleSourceReasonNotInCatalog: включён режим Verified, а проверенного
	// файла на этот список нет. Инвариант 12 запрещает докачивать
	// неподписанное вместо подписанного, поэтому отказ, а не откат.
	RuleSourceReasonNotInCatalog = "not_in_catalog"
	// RuleSourceReasonNoMirror: ни проверенного файла, ни базового адреса.
	// Качать список неоткуда.
	RuleSourceReasonNoMirror = "no_mirror"
)

// RuleSourceReport — что стало с одним объявленным в пресете списком.
//
// Тип существует потому, что BuildWith выбрасывает недоступный провайдер
// ВМЕСТЕ с правилами, которые на него ссылаются, и снаружи такой пресет
// неотличим от пресета, у которого списка никогда и не было. Ровно это
// приложение и показывало: «Россия (умный)» без ru-blocked выглядел ровно так
// же, как «Россия (умный)» с ним.
type RuleSourceReport struct {
	Name  string          `json:"name"`
	State RuleSourceState `json:"state"`
	// Reason — машинный код (RuleSourceReason*). Пусто, когда State не dropped.
	Reason string `json:"reason,omitempty"`
	// Detail — пояснение на английском для журнала. Пользователю НЕ
	// показывается: текст для него выбирает обвязка по Reason.
	Detail string `json:"detail,omitempty"`
	// URL непуст только у RuleSourceMirror, Path — только у RuleSourceFile.
	URL  string `json:"url,omitempty"`
	Path string `json:"path,omitempty"`
	// Rules — сколько правил пресета ссылались на этот список.
	Rules int `json:"rules"`
	// KeptRules — сколько из них дожили до конфига. Для dropped это ноль, и
	// разница с Rules и есть цена отказа.
	KeptRules int `json:"kept_rules"`
	// Fallback — значения правил, вставших НА МЕСТО этого списка (сегодня это
	// теги GEOSITE). Непусто только при State == dropped: доехавший список
	// делает замену лишней, и в конфиг она не идёт.
	//
	// Поле отвечает на вопрос, который иначе не задать: тег
	// "category-ads-all" в GeositeTags сам по себе неотличим от
	// самостоятельного правила пресета, и обвязка не могла бы сказать «список
	// оператора не доехал, режем по встроенной базе» вместо «режем по
	// встроенной базе, потому что так и задумано».
	Fallback []string `json:"fallback,omitempty"`
}

// BuildReport — что сборка пресета РЕАЛЬНО положила в конфиг.
type BuildReport struct {
	PresetID    string `json:"preset_id"`
	PresetName  string `json:"preset_name,omitempty"`
	Emoji       string `json:"emoji,omitempty"`
	Country     string `json:"country,omitempty"`
	FinalAction Action `json:"final_action"`
	// Rules — сколько правил вошло в Config.Rules. Финальное MATCH сюда не
	// входит: его добавляет CompiledRules, а не пресет.
	Rules int `json:"rules"`
	// DroppedRules — сколько правил пресета выброшено вместе с их источниками.
	DroppedRules int `json:"dropped_rules"`
	// RulesByType — распределение вошедших правил по матчеру ("GEOSITE": 7).
	RulesByType map[string]int `json:"rules_by_type,omitempty"`
	// GeositeTags — теги GEOSITE, на которых стоят вошедшие правила, без
	// повторов и в порядке появления. Без базы GeoSite.dat они не значат
	// НИЧЕГО, и снаружи это никак не видно — отсюда и список.
	//
	// Пресет streaming это ПЯТЬ таких тегов и ничего кроме: эквивалента
	// стриминговых списков на зеркале оператора нет, и подменить их нечем.
	// Про него честный ответ так и остаётся «зависит от базы, состояние базы
	// см. в GeositeReport», и заимствовать чужой успех он не может: судьба
	// списков лежит в Sources, а Sources у него пуст.
	//
	// Пресет adblock, наоборот, попадает сюда ТОЛЬКО когда список `ads` не
	// доехал (см. RuleSourceReport.Fallback). Доехавший список вытесняет тег
	// из правил, и этого списка здесь не будет вовсе.
	GeositeTags []string `json:"geosite_tags,omitempty"`
	// Sources — судьба каждого объявленного пресетом внешнего списка.
	Sources []RuleSourceReport `json:"sources,omitempty"`
}

// BuildWithReport возвращает Config пресета и отчёт о том, что в него вошло.
func (p Preset) BuildWithReport(opt PoolOptions, group string) (Config, BuildReport) {
	base := ""
	for _, b := range opt.Bases {
		if b = strings.TrimRight(strings.TrimSpace(b), "/"); b != "" {
			base = b
			break
		}
	}

	rep := BuildReport{
		PresetID: p.ID, PresetName: p.Name, Emoji: p.Emoji, Country: p.Country,
		FinalAction: p.FinalAction,
	}
	if rep.FinalAction == "" {
		rep.FinalAction = ActionDirect
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

	// Судьба источника считается ДО фильтрации правил: правило, выброшенное
	// вместе с провайдером, должно попасть в счётчик именно этого провайдера,
	// а не потеряться где-то между двумя циклами.
	srcIdx := make(map[string]int, len(p.Providers))
	providers := make([]RuleProvider, 0, len(p.Providers))
	for _, rp := range p.Providers {
		item := RuleSourceReport{Name: rp.Name}
		switch {
		case !available(rp.Name) && opt.Verified:
			item.State, item.Reason = RuleSourceDropped, RuleSourceReasonNotInCatalog
			item.Detail = "the trusted catalog does not sign this rule-set, and invariant 12 forbids fetching an unsigned copy instead"
		case !available(rp.Name):
			item.State, item.Reason = RuleSourceDropped, RuleSourceReasonNoMirror
			item.Detail = "no verified file and no mirror base URL, so there is nowhere to fetch this rule-set from"
		default:
			if path, ok := opt.Files[rp.Name]; ok {
				rp.URL = ""
				rp.Interval = 0
				rp.Proxy = ""
				rp.Path = path
				item.State, item.Path = RuleSourceFile, path
				item.Detail = "served from a local file whose sha256 matched the signed catalog entry"
			} else {
				rp.URL = strings.ReplaceAll(rp.URL, "{BASE}", base)
				rp.Proxy = opt.Proxy
				item.State, item.URL = RuleSourceMirror, rp.URL
				item.Detail = "emitted as an http rule-provider; whether the mirror serves it is known to the engine at runtime, not to config assembly"
			}
			providers = append(providers, rp)
		}
		srcIdx[rp.Name] = len(rep.Sources)
		rep.Sources = append(rep.Sources, item)
	}

	// Без базового URL (клиент без панели, импортированная подписка) и без
	// проверенных файлов удалённые rule-provider'ы недоступны: mihomo падал бы
	// на Get "/rulesets/..." с пустой схемой. Оставляем только
	// geosite/geoip/домены; ruleset-правила выкидываем вместе с провайдерами.
	rules := make([]Rule, 0, len(p.Rules))
	seenTag := make(map[string]struct{})
	for _, r := range p.Rules {
		// Правило-замена. Живой список делает его лишним, поэтому оно
		// выбрасывается — но это НЕ потеря: в DroppedRules такое не считается,
		// там лежит цена отказа, а здесь замена просто не понадобилась.
		//
		// Замена привязана к списку, ОБЪЯВЛЕННОМУ этим же пресетом. Ссылка на
		// чужое имя ничего не подавляет: пресет, забывший объявить провайдер,
		// обязан потерять зеркало, а не свою же подстраховку.
		if r.FallbackFor != "" {
			i, declared := srcIdx[r.FallbackFor]
			if declared {
				if available(r.FallbackFor) {
					continue
				}
				rep.Sources[i].Fallback = append(rep.Sources[i].Fallback, r.Value)
			}
		}
		if r.Type == MatchRuleSet {
			if i, ok := srcIdx[r.Value]; ok {
				rep.Sources[i].Rules++
			}
			if !available(r.Value) {
				rep.DroppedRules++
				continue
			}
			if i, ok := srcIdx[r.Value]; ok {
				rep.Sources[i].KeptRules++
			}
		}
		if r.Type == MatchGeosite {
			if _, dup := seenTag[r.Value]; !dup {
				seenTag[r.Value] = struct{}{}
				rep.GeositeTags = append(rep.GeositeTags, r.Value)
			}
		}
		if rep.RulesByType == nil {
			rep.RulesByType = map[string]int{}
		}
		rep.RulesByType[string(r.Type)]++
		rules = append(rules, r)
	}
	rep.Rules = len(rules)

	if len(providers) == 0 {
		providers = nil
	}
	return Config{
		ProxyGroup:  group,
		FinalAction: p.FinalAction,
		Rules:       rules,
		Providers:   providers,
	}, rep
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

// geositeFallback — правило по geosite-тегу, которое включается ТОЛЬКО когда
// внешний список ruleSet недоступен. См. Rule.FallbackFor.
func geositeFallback(tag, ruleSet string, a Action) Rule {
	return Rule{Type: MatchGeosite, Value: tag, Action: a, FallbackFor: ruleSet}
}

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

// adsRuleSet — имя списка рекламы/трекеров на зеркале оператора. Панель отдаёт
// его текстом по /rulesets/ads (см. handlers/rulesets.rs, RULESETS).
const adsRuleSet = "ads"

// adsGeositeTag — встроенная категория mihomo, которой блок рекламы жил до
// появления зеркала. Остаётся ПОДСТРАХОВКОЙ: клиент без панели (импортированная
// подписка) зеркала не имеет, и терять из-за этого блокировку он не должен.
const adsGeositeTag = "category-ads-all"

// adsProvider — описание списка рекламы для пресетов, которые режут рекламу.
func adsProvider() RuleProvider {
	return RuleProvider{
		Name: adsRuleSet, Behavior: BehaviorDomain, Format: FormatText,
		URL: "{BASE}/rulesets/" + adsRuleSet, Interval: 43200,
	}
}

// AdBlockRuleSet — имя списка рекламы наружу пакета.
//
// Нужно ровно там, где решается судьба списка ДО сборки пресета:
// api.ruleSetNames обязан заказать этот список каталогу, иначе проверенный
// файл до сборки не доедет и переключатель «блок рекламы» окажется включённым
// без источника — то есть галочкой, за которой ничего нет.
const AdBlockRuleSet = adsRuleSet

// AdBlockGeositeTag — запасная встроенная категория, наружу пакета.
const AdBlockGeositeTag = adsGeositeTag

// isAdBlockRule — правило принадлежит блоку рекламы (в любой из двух форм).
func isAdBlockRule(r Rule) bool {
	if r.Action != ActionReject {
		return false
	}
	return (r.Type == MatchRuleSet && r.Value == adsRuleSet) ||
		(r.Type == MatchGeosite && r.Value == adsGeositeTag)
}

// isPrivateGeoIP — правило «локальная сеть мимо туннеля».
func isPrivateGeoIP(r Rule) bool {
	return r.Type == MatchGeoIP && r.Value == "private" && r.Action == ActionDirect
}

// WithAdBlock возвращает копию пресета, которая дополнительно режет рекламу и
// трекеры.
//
// Идемпотентна: пресету, у которого блок рекламы уже свой (cn-smart, adblock),
// она ничего не добавляет — иначе одно и то же правило попало бы в конфиг
// дважды, а провайдер объявился бы вторым именем.
//
// Правила ставятся сразу ПОСЛЕ ведущего «локальная сеть напрямую», то есть
// ровно туда, где их держит leadIn(true) у пресетов со встроенным блоком.
// Результат для пресета, построенного через leadIn(false), совпадает с
// leadIn(true) правило в правило — это и проверяет тест.
//
// Обе формы правила добавляются вместе (список с зеркала + запасной GEOSITE
// через FallbackFor), поэтому BuildWithReport сам выберет наблюдаемую, когда
// список доступен, и подстрахуется тегом, когда нет. Именно из-за этого
// переключатель может честно сказать, работает он или нет.
func WithAdBlock(p Preset) Preset {
	for _, r := range p.Rules {
		if isAdBlockRule(r) {
			return p
		}
	}

	at := 0
	for at < len(p.Rules) && isPrivateGeoIP(p.Rules[at]) {
		at++
	}
	rules := make([]Rule, 0, len(p.Rules)+2)
	rules = append(rules, p.Rules[:at]...)
	rules = append(rules,
		ruleset(adsRuleSet, ActionReject),
		geositeFallback(adsGeositeTag, adsRuleSet, ActionReject),
	)
	rules = append(rules, p.Rules[at:]...)

	providers := make([]RuleProvider, 0, len(p.Providers)+1)
	providers = append(providers, p.Providers...)
	declared := false
	for _, rp := range providers {
		if rp.Name == adsRuleSet {
			declared = true
			break
		}
	}
	if !declared {
		providers = append(providers, adsProvider())
	}

	p.Rules = rules
	p.Providers = providers
	return p
}

// AdBlockOnlyPreset — пресет из одного блока рекламы с заданным финалом.
//
// Идентификатор намеренно тот же, что у встроенного `adblock`: приложение
// сверяет применённый пресет со своим зеркалом реестра, и выдуманный id
// означал бы «пресет ядру этой сборки неизвестен» — то есть отказ подтвердить
// как раз то, что мы подтверждаем.
//
// Финал передаётся, а не берётся из реестра, потому что случаев два и они
// противоположны: пресета нет вовсе (весь трафик в туннель, ActionProxy) и
// пресет отменён сайтовым allow-списком (финал всё равно подменит allow-список).
// Зашить сюда DIRECT реестра значило бы молча выключить туннель первому.
func AdBlockOnlyPreset(final Action) Preset {
	p, ok := PresetByID("adblock")
	if !ok {
		// Реестр без `adblock` — это расхождение сборки, а не состояние, в
		// котором можно тихо выдумать пресет.
		return Preset{}
	}
	p.FinalAction = final
	return p
}

// AdBlockOnly оставляет от собранной конфигурации только блок рекламы и
// «локальная сеть напрямую», выбрасывая правила и списки режима страны.
//
// FinalAction не переносится намеренно: единственный вызывающий — сайтовый
// allow-список, который назначает финал сам. Унаследовать DIRECT страны здесь
// значило бы получить финал, о котором никто не просил.
func AdBlockOnly(c Config) Config {
	out := Config{ProxyGroup: c.ProxyGroup}
	for _, r := range c.Rules {
		if isPrivateGeoIP(r) || isAdBlockRule(r) {
			out.Rules = append(out.Rules, r)
		}
	}
	for _, rp := range c.Providers {
		if rp.Name == adsRuleSet {
			out.Providers = append(out.Providers, rp)
		}
	}
	return out
}

// irDirectRuleSet — имя списка ИРАНСКИХ доменов на зеркале оператора.
//
// Список НЕ «заблокировано в Иране», а ровно наоборот: это иранские сервисы,
// живущие на не-.ir доменах (банки, госуслуги, местная торговля). Апстрим
// говорит это сам — README bootmortis/iran-hosted-domains, раздел Categories:
//
//	`other`: non `.ir` domains, use as `direct`.
//
// До этой правки список назывался `ir-blocked` и стоял с действием PROXY выше
// правила GEOIP,IR,DIRECT. RULE-SET совпадал первым, и 62 826 доменов
// иранской домашней инфраструктуры уезжали в немецкий или канадский выход.
// Эти сервисы отгорожены по гео на иранские IP: снаружи они не «медленнее»,
// они не работают вовсе. Правило GEOIP,IR,DIRECT, которое существует ровно
// затем, чтобы этого не случилось, до такого трафика не доходило никогда.
//
// Имя списка тоже часть ошибки, поэтому оно сменено вместе с действием:
// «blocked» с действием DIRECT — это та же ложь, просто переехавшая из
// правила в имя. Смена имени безопасна именно сейчас — /rulesets/* на живой
// панели ещё отдаёт 404, установленных клиентов у старого имени нет.
const irDirectRuleSet = "ir-direct"

// leadIn — общий хвост: приватные сети напрямую, реклама в блок.
// Ставится в начало большинства пресетов.
//
// Блок рекламы объявляется ДВАЖДЫ намеренно, и в конфиг всегда попадает ровно
// одна из двух форм:
//
//   - RULE-SET на список `ads` с зеркала оператора — ~148 тыс. доменов, которые
//     оператор обновляет сам и чью судьбу сборка конфига ВИДИТ: адрес
//     подставлен, провайдер в конфиге, отчёт называет его источником;
//   - GEOSITE category-ads-all — та же категория, но из базы GeoSite.dat,
//     которую ядро качает по geox-url. Секция geox-url пишется только под
//     доверенным каталогом (profile.applyGeoX), в остальных сборках ядро идёт
//     на зашитое умолчание meta-rules-dat, недоступное ровно там, где клиент и
//     нужен. Отсюда это не наблюдается никак — только «неизвестно».
//
// Пресет, у которого зеркало есть, получает наблюдаемую форму; пресет без
// зеркала — прежнюю. Владельцу это и было нужно: «блок рекламы непонятно
// работает или нет» перестаёт быть единственным возможным ответом.
//
// ВАЖНО: пресет, вызывающий leadIn(true), ОБЯЗАН объявить adsProvider() в своих
// Providers, иначе замена ничего не подавляет и зеркало не подключается.
// Фиксируется тестом TestAdsProviderDeclaredByEveryAdBlockingPreset.
func leadIn(blockAds bool) []Rule {
	r := []Rule{
		geoip("private", ActionDirect),
	}
	if blockAds {
		r = append(r,
			ruleset(adsRuleSet, ActionReject),
			geositeFallback(adsGeositeTag, adsRuleSet, ActionReject),
		)
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
		),
			// Иранское — напрямую, тем же блоком, что и «российское напрямую»
			// в ru-smart (GEOSITE category-ru + GEOIP RU).
			//
			// Порядок здесь несущий: домены идут ПЕРЕД GEOIP,IR и делают
			// работу, которую GEOIP сделать не может. У geoip() выставлен
			// NoResolve, и CompiledRules дописывает ",no-resolve" — то есть
			// GEOIP,IR,DIRECT намеренно НЕ резолвит домен перед сравнением.
			// В fake-ip режиме до него доезжает подменённый адрес, и на
			// доменном соединении оно не срабатывает вовсе. Так что список
			// доменов не дублирует GEOIP,IR — он единственный, кто держит
			// иранские .com дома.
			ruleset(irDirectRuleSet, ActionDirect),
			geoip("IR", ActionDirect),
		),
		Providers: []RuleProvider{
			{Name: irDirectRuleSet, Behavior: BehaviorDomain, Format: FormatText, URL: "{BASE}/rulesets/" + irDirectRuleSet, Interval: 43200},
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
		Providers: []RuleProvider{adsProvider()},
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
		Providers:   []RuleProvider{adsProvider()},
	},
	// 🌐 Глобальный обход — всё через прокси (классический «full tunnel»).
	{
		ID: "global", Name: "Полный обход", Emoji: "🌐", Country: "",
		Description: "Весь трафик через VPN, напрямую — только локальная сеть.",
		FinalAction: ActionProxy,
		Rules:       leadIn(false),
	},
}

// RuleSetIntent — НАЗНАЧЕНИЕ внешнего списка: откуда его брать и с каким
// действием он осмыслен.
//
// Действие лежит здесь, а не только в правилах пресета, потому что смысл
// списка задаёт апстрим, а не пресет. Пресет, поставивший чужому списку не то
// действие, ломает пользователя молча: правило совпадает, трафик уезжает не
// туда, и ни один счётчик об этом не сообщает. Поле делает намерение
// сверяемым — TestRuleSetActionsMatchTheirDeclaredIntent проверяет каждое
// RULE-SET-правило каждого пресета против этой карты.
type RuleSetIntent struct {
	// Upstream — апстрим-источник, который панель зеркалит в /rulesets/NAME.
	Upstream string
	// Action — единственное действие, с которым список имеет смысл.
	// Менять его можно только вместе с перечитанным README апстрима.
	Action Action
}

// RecommendedUpstreams — какие апстримы панель должна зеркалить в /rulesets/NAME.
// Ключ — имя rule-set'а (и путь /rulesets/NAME).
// Панель отдаёт списки текстом (behavior=domain|ipcidr, format=text): один
// элемент на строку, без бинарного .mrs-тулинга. Так список остаётся актуальным
// и доступным даже там, где GitHub заблокирован, а зеркало — это просто
// текстовый файл за обычным HTTP.
var RecommendedUpstreams = map[string]RuleSetIntent{
	// Заблокированные в РФ домены. Оба апстрима — списки того, что НЕ
	// открывается изнутри страны: у runetfreedom это срез
	// `geosite:ru-blocked` («заблокированные в России домены»), у itdoginfo —
	// ветка «Russia inside» («ресурсы, которые блокируются, в том числе
	// зарубежные ресурсы, которые сами блокируют российские подсети»).
	//
	// У itdoginfo есть и ЗЕРКАЛЬНЫЙ список «Russia outside» — российские
	// ресурсы, доступные только с российских подсетей. Он означает ровно
	// обратное и с действием PROXY сломал бы то же самое, что `ir-direct`
	// ломал в Иране. Брать нужно `inside`.
	"ru-blocked": {
		Upstream: "github.com/runetfreedom/russia-blocked-geosite (release: ru-blocked.txt) либо itdoginfo/allow-domains (Russia/inside-raw.lst, НЕ outside)",
		Action:   ActionProxy,
	},
	"ru-blocked-ip": {
		Upstream: "github.com/1andrevich/Re-filter-lists (release: ipsum.lst) — подсети заблокированного в РФ",
		Action:   ActionProxy,
	},
	// Иранские домены на не-.ir TLD. Направление ЗДЕСЬ, а не в имени: см.
	// комментарий у irDirectRuleSet и раздел Categories в README апстрима.
	//
	// Списка «заблокировано внутри Ирана» в текстовом виде у нас нет и взять
	// его негде: у bootmortis категория `proxy` существует, но едет только
	// внутри iran.dat / iran-geosite.db (бинарь v2ray/sing-box), а
	// text-провайдер mihomo такое не читает; у Chocolate4U/Iran-clash-rules
	// (проект живой) в каталоге такой категории нет вовсе — там ir/ir-lite/
	// ircidr (direct), ads/malware/phishing/nsfw (reject) и списки отдельных
	// зарубежных сервисов. Поэтому проксирующая половина ir-smart остаётся на
	// тегах GEOSITE, и это честное состояние, а не недоделка.
	irDirectRuleSet: {
		Upstream: "github.com/bootmortis/iran-hosted-domains (release: clash_rules_other.txt, категория `other` — «non .ir domains, use as direct») + github.com/Chocolate4U/Iran-clash-rules (release: ir-lite.txt, добавляет правило на весь .ir)",
		Action:   ActionDirect,
	},
	"by-blocked": {
		Upstream: "наследует РФ-список itdoginfo/allow-domains (Russia/inside-raw.lst) + локальные дополнения",
		Action:   ActionProxy,
	},
	// Реклама/трекеры: текстовый срез той же категории, что и встроенный тег
	// category-ads-all. Пока зеркало не отвечает, пресеты режут рекламу по
	// встроенной базе (Rule.FallbackFor), поэтому отсутствие этого списка не
	// ломает блокировку — только делает её ненаблюдаемой.
	adsRuleSet: {
		Upstream: "github.com/runetfreedom/russia-blocked-geosite (release: category-ads-all.txt, ~148 тыс. доменов)",
		Action:   ActionReject,
	},
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
