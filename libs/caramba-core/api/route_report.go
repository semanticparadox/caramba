package api

import (
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/engine"
	"github.com/semanticparadox/caramba/libs/caramba-core/routing"
)

// Отчёт о применённой маршрутизации.
//
// Владелец сказал про блок рекламы и стриминг: «непонятно работают или нет».
// Это не претензия к экрану — до этого файла в приложении не было НИ ОДНОГО
// канала, по которому такой ответ мог бы прийти. ApplyPreset сообщает только
// «идентификатор существует»; дальше пресет молча теряет части себя:
//
//   - routing.BuildWith выбрасывает недоступный rule-provider ВМЕСТЕ с
//     правилами, которые на него ссылаются, и «Россия (умный)» без ru-blocked
//     снаружи неотличим от «Россия (умный)» с ним;
//   - пресеты adblock и streaming это ЧИСТЫЕ теги GEOSITE, а секция geox-url
//     пишется только при Bootstrap.Managed (profile.applyGeoX), то есть лишь
//     под доверенным каталогом с подписанными хешами. В остальных сборках ядро
//     остаётся на зашитом умолчании mihomo — четырёх ссылках на
//     github.com/MetaCubeX/meta-rules-dat, недоступных ровно там, где этот
//     клиент и нужен. Тег без базы не значит ничего, и это НЕ видно;
//   - relay на raw-пути отбрасывается, а на панельном уезжает в панель, чья
//     цепочка в теле CLASH не выражается вовсе.
//
// Отчёт снимается на подъёме и отвечает на вопрос «что применено», а не «что
// выбрано». Там, где ядро не знает, здесь стоит «неизвестно» с причиной: это
// законное значение, и придумывать вместо него здоровый ответ запрещено.

// Значения RouteReport.Source.
const (
	// RouteSourcePreset: маршрутизация собрана из встроенного пресета.
	RouteSourcePreset = "preset"
	// RouteSourceCustom: конфигурация подана напрямую (SetRouting) — пресета
	// за ней нет, и разбирать её на источники здесь не по чему.
	RouteSourceCustom = "custom"
	// RouteSourceCoreDefault: ни пресета, ни своей конфигурации. Правила
	// выбирает profile по умолчанию, и их состав ядру api неизвестен.
	RouteSourceCoreDefault = "core_default"
)

// Причина, по которой отчёта нет вообще.
const (
	// RouteUnknownNotRaised: туннель ни разу не поднимался в этом экземпляре
	// ядра. Отчёт снимается на подъёме, и до него сказать нечего.
	RouteUnknownNotRaised = "not_raised"
)

// Состояние базы GeoSite.dat, от которой зависят ВСЕ правила GEOSITE.
const (
	// GeositeNotRequired: во вошедших правилах нет ни одного матчера GEOSITE.
	GeositeNotRequired = "not_required"
	// GeositeVerified: каталог назвал базу, её байты сошлись с подписанным
	// sha256 и лежат на диске. Это единственное состояние, про которое ядро
	// говорит «да».
	GeositeVerified = "verified"
	// GeositePresent: файл на диске есть, но подписи под ним нет — ядро
	// скачало его само по зашитому умолчанию. Правила скорее всего работают,
	// но происхождение базы клиент не проверял.
	GeositePresent = "present"
	// GeositeRefused: каталог доверен, но базу он не назвал, поэтому geox-url
	// получил ПУСТОЙ адрес и ядро откажется её качать (инвариант 12: отказ, а
	// не откат на GitHub). Все правила GEOSITE мертвы, и это точно известно.
	GeositeRefused = "refused"
	// GeositeUnknown: файла нет и подписанного адреса нет. Скачает ли ядро
	// базу с зашитого умолчания, отсюда не видно.
	GeositeUnknown = "unknown"
)

// Машинные коды причин по базе GEOSITE.
const (
	// GeositeReasonUnmanaged: Bootstrap.Managed снят, секция geox-url в конфиг
	// не пишется (profile.applyGeoX), ядро идёт на meta-rules-dat.
	GeositeReasonUnmanaged = "geox_unmanaged"
	// GeositeReasonNotInCatalog: каталог доверен, но GeoSite.dat в нём нет.
	GeositeReasonNotInCatalog = "not_in_catalog"
	// GeositeReasonFileMissing: адрес подписан, а файла на диске нет.
	GeositeReasonFileMissing = "file_missing"
)

// Состояние запрошенной страны входа.
const (
	// RouteRelayNotRequested: вход не выбирали.
	RouteRelayNotRequested = "not_requested"
	// RouteRelayIgnored: выбор был, и этот путь его применить не смог. Причина
	// лежит в Capability тем же кодом, каким её отдаёт Capabilities.
	RouteRelayIgnored = "ignored"
	// RouteRelaySent: страна ушла в панель параметром relay_country. Построила
	// ли панель цепочку — отдельный вопрос, см. DialerProxySeen.
	RouteRelaySent = "sent"
)

// GeositeReport — доступность базы, без которой правила GEOSITE это ничто.
type GeositeReport struct {
	// Required: во вошедших правилах есть хотя бы один матчер GEOSITE.
	Required bool `json:"required"`
	// Tags — те самые теги, ради которых база нужна.
	Tags []string `json:"tags,omitempty"`
	// State — одно из Geosite* выше.
	State string `json:"state"`
	// Reason — машинный код (GeositeReason*). Пусто при verified и
	// not_required.
	Reason string `json:"reason,omitempty"`
	// Detail — пояснение на английском для журнала, не для пользователя.
	Detail string `json:"detail,omitempty"`
	// Path — файл, который ядро проверяло. Отдаётся всегда, даже когда его
	// там нет: «искал вот здесь» это часть ответа.
	Path string `json:"path"`
	// SizeBytes — размер найденного файла, 0 если файла нет.
	SizeBytes int64 `json:"size_bytes"`
	// URL — подписанный адрес базы. Пусто означает, что подписанного адреса
	// нет, а не что адрес есть и он пустой.
	URL string `json:"url,omitempty"`
}

// RouteRelayReport — что стало с выбранной страной входа.
type RouteRelayReport struct {
	// Requested — то, что выбрал пользователь (ISO-2 либо имя). Пусто, когда
	// выбора не было.
	Requested string `json:"requested,omitempty"`
	// State — одно из RouteRelay* выше.
	State string `json:"state"`
	// Capability — та же запись, что отдаёт Capabilities, когда выбор
	// отброшен. Обвязка берёт текст по её Reason и не заводит вторую
	// формулировку того же факта.
	Capability *Capability `json:"capability,omitempty"`
	// DialerProxySeen: в теле конфига, из которого поднялся туннель, найден
	// ключ dialer-proxy — единственная форма, которой mihomo выражает цепочку
	// вход→выход.
	//
	// Это НАБЛЮДЕНИЕ над применённым телом, а не убеждение о панели. Сегодня
	// панель отдаёт ядру формат CLASH, а генератор CLASH цепочку не строит:
	// поле останется false даже при принятом relay_country. Если панель
	// когда-нибудь начнёт её отдавать, поле станет true само, без правки здесь.
	DialerProxySeen bool `json:"dialer_proxy_seen"`
	// Detail — пояснение на английском для журнала.
	Detail string `json:"detail,omitempty"`
}

// RouteReport — что подъём РЕАЛЬНО применил к маршрутизации.
type RouteReport struct {
	// Known ложно ровно в одном случае: подъёма ещё не было. Всё остальное,
	// включая «ядро решает само», это известное состояние с именем.
	Known bool `json:"known"`
	// Reason — машинный код (RouteUnknown*), когда Known ложно.
	Reason string `json:"reason,omitempty"`
	// Detail — пояснение на английском для журнала.
	Detail string `json:"detail,omitempty"`
	// RaisedAtMs — момент подъёма, unix-миллисекунды. 0 при Known=false.
	RaisedAtMs int64 `json:"raised_at_ms,omitempty"`
	// TunnelUp — поднят ли туннель ПРЯМО СЕЙЧАС. Отчёт переживает Down
	// намеренно: вопрос «почему не резалась реклама» задают уже отключившись.
	TunnelUp bool `json:"tunnel_up"`
	// Source — одно из RouteSource* выше.
	Source string `json:"source,omitempty"`
	// Preset — отчёт сборки пресета. nil, когда Source не preset.
	Preset *routing.BuildReport `json:"preset,omitempty"`
	// Rules — сколько правил применено. nil означает «неизвестно»: при
	// RouteSourceCoreDefault состав правил выбирает profile, и этот слой их не
	// считает. Ноль и «не знаю» здесь разные ответы.
	Rules *int `json:"rules"`
	// Geosite — доступность базы GEOSITE.
	Geosite GeositeReport `json:"geosite"`
	// Relay — судьба выбранной страны входа.
	Relay RouteRelayReport `json:"relay"`
	// Ignored — полный список отброшенных этим подъёмом выборов, тот же, что
	// в UpResult.Ignored.
	Ignored []Capability `json:"ignored,omitempty"`
}

// routeSnapshot — сырьё отчёта, снятое внутри Up.
//
// Отдельный тип, потому что часть полей известна ДО старта движка (план
// загрузки, сборка пресета), а часть только после (момент подъёма). Собирать
// отчёт из полей Core на чтении нельзя: к тому времени presetID и relayCountry
// уже могли смениться, и отчёт рассказывал бы про выбор, которого подъём не
// видел.
type routeSnapshot struct {
	source string
	preset *routing.BuildReport
	rules  *int
	// geositeTags — теги GEOSITE во ВОШЕДШИХ правилах, без повторов.
	//
	// Считается по применённой конфигурации, а не по пресету: правила могли
	// прийти и напрямую (SetRouting). Пустой список означает, что база
	// GeoSite.dat этому подъёму не нужна вовсе, и это тоже ответ.
	geositeTags []string
	geoManaged  bool
	geoURL      string
	relay       RouteRelayReport
	ignored     []Capability
	raisedAt    time.Time
}

// buildRouteReport достраивает снимок до отчёта: проверяет базу GEOSITE на
// диске и подставляет текущее состояние движка.
func (c *Core) buildRouteReport(s routeSnapshot, tunnelUp bool) RouteReport {
	rep := RouteReport{
		Known:      true,
		RaisedAtMs: s.raisedAt.UnixMilli(),
		TunnelUp:   tunnelUp,
		Source:     s.source,
		Preset:     s.preset,
		Rules:      s.rules,
		Relay:      s.relay,
		Ignored:    s.ignored,
	}

	g := GeositeReport{
		Path: filepath.Join(c.workDir, geoSiteFile),
		URL:  s.geoURL,
		Tags: s.geositeTags,
	}
	g.Required = len(g.Tags) > 0

	st, err := os.Stat(g.Path)
	onDisk := err == nil && !st.IsDir()
	if onDisk {
		g.SizeBytes = st.Size()
	}

	switch {
	case !g.Required:
		// Без пресета profile.applyRules эмитит только GEOIP,private и MATCH:
		// страновые geo-правила туда намеренно не зашиты, а собственные
		// правила подписки перезаписываются целиком. Матчера GEOSITE в
		// применённых правилах нет, и база не нужна.
		g.State = GeositeNotRequired
		g.Detail = "no GEOSITE matcher survived into the applied rules, so the database is never consulted"
	case s.geoManaged && g.URL == "":
		// Инвариант 12: при доверенном каталоге неназванный ресурс получает
		// пустой адрес, и ядро отказывается его качать, а не идёт на GitHub.
		g.State, g.Reason = GeositeRefused, GeositeReasonNotInCatalog
		g.Detail = "the trusted catalog is in force but does not name GeoSite.dat, so geox-url carries an empty address and the engine refuses to download it: every GEOSITE rule is dead"
	case s.geoManaged && onDisk:
		g.State = GeositeVerified
		g.Detail = "the catalog named GeoSite.dat and its bytes matched the signed sha256 before they were written here"
	case s.geoManaged:
		g.State, g.Reason = GeositeUnknown, GeositeReasonFileMissing
		g.Detail = "the catalog names GeoSite.dat but no file is present at the path the engine reads"
	case onDisk:
		g.State, g.Reason = GeositePresent, GeositeReasonUnmanaged
		g.Detail = "a GeoSite.dat is present but unsigned: geox-url is not written without a trusted catalog, so the engine fetched it from its compiled default"
	default:
		g.State, g.Reason = GeositeUnknown, GeositeReasonUnmanaged
		g.Detail = "no trusted catalog and no local GeoSite.dat: geox-url is not written, and whether the engine can reach its compiled default (github.com/MetaCubeX/meta-rules-dat) is not observable from here"
	}
	rep.Geosite = g
	return rep
}

// newRouteRelayReport описывает судьбу выбранной страны входа.
//
// rawYAML это тело, из которого собран конфиг. Ключ dialer-proxy ищется в нём
// подстрокой намеренно: это ровно то имя, которым mihomo выражает цепочку, и
// наблюдение над применённым телом честнее, чем утверждение о том, что панель
// умеет.
func newRouteRelayReport(requested string, ignored []Capability, rawYAML []byte) RouteRelayReport {
	out := RouteRelayReport{
		Requested:       strings.TrimSpace(requested),
		DialerProxySeen: strings.Contains(string(rawYAML), "dialer-proxy"),
	}
	if out.Requested == "" {
		out.State = RouteRelayNotRequested
		return out
	}
	for i := range ignored {
		if ignored[i].Name == CapNameRelayChaining {
			cap := ignored[i]
			out.State, out.Capability = RouteRelayIgnored, &cap
			out.Detail = "the entry country was dropped by this path; see capability.reason"
			return out
		}
	}
	out.State = RouteRelaySent
	if out.DialerProxySeen {
		out.Detail = "relay_country was sent to the panel and the applied body carries a dialer-proxy chain"
		return out
	}
	out.Detail = "relay_country was sent to the panel, but the applied body carries no dialer-proxy key, which is the only way mihomo expresses an entry-exit chain: the core cannot confirm a chain was built"
	return out
}

// RouteReportJSON отдаёт отчёт о маршрутизации последнего подъёма.
//
// Читающий вызов: он ничего не запрашивает по сети и ничего не применяет.
func (c *Core) RouteReportJSON() (string, error) {
	c.mu.Lock()
	snap := c.lastRoute
	c.mu.Unlock()
	if snap == nil {
		return toJSONString(RouteReport{
			Reason: RouteUnknownNotRaised,
			Detail: "no tunnel has been raised by this core instance, so there is nothing applied to report on",
			Geosite: GeositeReport{
				State:  GeositeUnknown,
				Reason: RouteUnknownNotRaised,
				Path:   filepath.Join(c.workDir, geoSiteFile),
			},
			Relay: RouteRelayReport{State: RouteRelayNotRequested},
		})
	}
	st, _ := c.engine.Status()
	return toJSONString(c.buildRouteReport(*snap, st.State == engine.StateConnected))
}
