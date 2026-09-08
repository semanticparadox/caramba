// Package profile собирает готовый mihomo-конфиг из подписки панели, применяя
// локальную политику клиента: TUN-режим, kill-switch, split-tunnel (списки
// разрешённых/запрещённых приложений и доменов) и настройки DNS.
//
// На вход подаётся сырой clash/mihomo YAML, отданный панелью (он уже содержит
// proxies/proxy-groups, включая amnezia-wg). Мы дополняем/переопределяем
// верхнеуровневые секции и список rules, не трогая узлы.
package profile

import (
	"crypto/rand"
	"encoding/base64"
	"fmt"
	"net/url"
	"strings"

	"github.com/semanticparadox/caramba/libs/caramba-core/routing"
	"github.com/semanticparadox/caramba/libs/caramba-core/subscription"
	"gopkg.in/yaml.v3"
)

// CarambaSelector — имя основной группы-селектора, которую формирует панель.
// На неё ссылается финальное правило MATCH.
const CarambaSelector = "CARAMBA"

// TunnelMode — способ захвата трафика на клиенте.
//
// Режимы принципиально различаются требованиями к правам:
//   - ModeTun поднимает системный TUN-интерфейс и перехватывает ВЕСЬ трафик, но
//     требует привилегий (root на Linux/macOS, админ на Windows) либо системного
//     расширения (Network Extension на Apple);
//   - ModeProxy поднимает только локальный mixed-инбаунд (SOCKS5+HTTP) на петле
//     и НЕ требует никаких привилегий. Трафик в него направляет само приложение
//     или системный прокси ОС.
type TunnelMode string

const (
	// ModeTun — системный TUN-инбаунд. Режим по умолчанию.
	ModeTun TunnelMode = "tun"
	// ModeProxy — локальный mixed-инбаунд без привилегий.
	ModeProxy TunnelMode = "proxy"
)

// Значения по умолчанию для proxy-режима.
const (
	// DefaultMixedPort — порт mixed-инбаунда (SOCKS5 и HTTP на одном порту).
	DefaultMixedPort = 7890
	// DefaultBindAddress — адрес привязки инбаундов. Только петля: без AllowLAN
	// ядро всё равно слушает 127.0.0.1 (см. listener.genAddr в mihomo).
	DefaultBindAddress = "127.0.0.1"
)

// Служебный mixed-инбаунд на петле, ступень R4 лестницы CSM/1.
const (
	// LoopbackListenerName — имя именованного инбаунда в секции listeners.
	// Оно же служит признаком «этот слушатель наш» при слиянии с конфигом
	// подписки.
	LoopbackListenerName = "caramba-csm"
	// DefaultLoopbackPort — порт служебного mixed-инбаунда. Выбран
	// детерминированно и намеренно НЕ рядом с 7890: 7891 у большинства сборок
	// занят socks-инбаундом клиентов clash, а совпадение порта с уже
	// работающим слушателем ядро трактует как ошибку конфигурации.
	DefaultLoopbackPort = 7893
	// LoopbackHost — адрес привязки служебного инбаунда. Только петля, всегда:
	// это канал выборки манифеста, а не пользовательский прокси, и AllowLAN на
	// него не распространяется.
	LoopbackHost = "127.0.0.1"
)

// ProxyConfig описывает inbound в режиме ModeProxy.
type ProxyConfig struct {
	// MixedPort — порт mixed-инбаунда. 0 → DefaultMixedPort.
	MixedPort int
	// BindAddress — адрес привязки ("127.0.0.1" или "*"). Пусто →
	// DefaultBindAddress. Ядро учитывает его ТОЛЬКО при AllowLAN.
	BindAddress string
	// AllowLAN открывает mixed-инбаунд для локальной сети.
	AllowLAN bool
	// LoopbackPort — порт служебного mixed-инбаунда на петле, через который
	// ходит ступень R4. 0 → DefaultLoopbackPort (со сдвигом при совпадении с
	// MixedPort). Отрицательное значение выключает служебный инбаунд: тогда
	// у R4 нет пути и обвязка обязана объявить ступень недоступной явной
	// причиной (transport.Ladder.SetTunnelUnavailable).
	LoopbackPort int
	// LoopbackUser и LoopbackPass — учётные данные служебного инбаунда на
	// петле. Они ОБЯЗАТЕЛЬНЫ: слушатель без них не собирается вовсе.
	//
	// Инбаунд с ключом proxy уводит всё, что на него пришло, прямо в
	// группу-селектор мимо правил маршрутизации. Без пары логин-пароль это
	// открытый релей на 127.0.0.1: на Android в него ходит ЛЮБОЕ приложение с
	// разрешением INTERNET, на десктопе любой локальный процесс, и весь их
	// трафик уходит через оплаченный пользователем узел мимо пресета и мимо
	// раздельного туннелирования. Пользователь соглашался на системный
	// туннель, а не на хостинг открытого релея.
	//
	// Пара выпускается на запуск (NewLoopbackCredential) и живёт ровно
	// столько, сколько поднят движок: она нигде не сохраняется и не уходит
	// наружу ни в снимке, ни в истории попыток.
	LoopbackUser string
	LoopbackPass string
}

// NewLoopbackCredential выпускает случайную пару логин-пароль для служебного
// инбаунда на петле. 96 бит на каждое значение: это не пароль пользователя, а
// разделяемый секрет двух половин одного процесса, и подбирать его негде.
func NewLoopbackCredential() (user string, pass string, err error) {
	one := func() (string, error) {
		var b [12]byte
		if _, err := rand.Read(b[:]); err != nil {
			return "", err
		}
		return base64.RawURLEncoding.EncodeToString(b[:]), nil
	}
	if user, err = one(); err != nil {
		return "", "", fmt.Errorf("profile: выпуск логина служебного инбаунда: %w", err)
	}
	if pass, err = one(); err != nil {
		return "", "", fmt.Errorf("profile: выпуск пароля служебного инбаунда: %w", err)
	}
	return user, pass, nil
}

// Stack — сетевой стек TUN.
type Stack string

const (
	StackGVisor Stack = "gvisor"
	StackSystem Stack = "system"
	StackMixed  Stack = "mixed"
)

// TunConfig описывает inbound в режиме TUN.
type TunConfig struct {
	// Enable включает TUN-инбаунд. На десктопе (Win/macOS/Linux) mihomo сам
	// поднимает TUN; на мобильных платформах fd передаётся отдельно через engine.
	Enable bool
	Stack  Stack
	// DeviceName — имя TUN-устройства (например, "caramba-tun"). Пусто — авто.
	DeviceName string
	// MTU. 0 → значение mihomo по умолчанию.
	MTU int
	// AutoRoute добавляет системные маршруты на TUN.
	AutoRoute bool
	// DNSHijack перехватывает указанные DNS-адреса в TUN (например,
	// "any:53"). Пусто — отключено.
	DNSHijack []string
}

// DNSConfig описывает встроенный DNS mihomo.
type DNSConfig struct {
	Enable bool
	// Listen — адрес встроенного DNS, например "127.0.0.1:1053".
	Listen string
	// Nameservers — резолверы для проксируемого трафика (DoH/DoT/UDP).
	Nameservers []string
	// FallbackNameservers — резолверы ветки fallback ядра (по fallback-filter).
	FallbackNameservers []string
	// DirectNameservers — резолверы для имён, которые идут напрямую
	// (direct-nameserver). Это национальная половина раскола: домашние сайты и
	// банки обязан резолвить домашний резолвер, иначе ответы приходят с чужой
	// геопривязкой, а половина .ru просто не открывается.
	DirectNameservers []string
	// ProxyServerNameservers — резолверы имён самих узлов
	// (proxy-server-nameserver). Их приходится резолвить ДО того, как туннель
	// поднят, поэтому здесь тоже домашний резолвер: чужой в этот момент
	// недоступен. Поток запросов пользователя сюда не попадает, он идёт в
	// Nameservers уже сквозь туннель.
	ProxyServerNameservers []string
	// EnhancedMode — "fake-ip" или "redir-host".
	EnhancedMode string
	// FakeIPRange — диапазон fake-ip, например "198.18.0.1/16".
	FakeIPRange string
}

// SnifferPolicy — восстановление имени домена из первых байтов соединения
// (SNI из TLS ClientHello, ClientHello внутри QUIC Initial, заголовок Host у
// открытого HTTP).
//
// Зачем оно вообще нужно. Доменное правило (GEOSITE, DOMAIN-SUFFIX) ядро
// сопоставляет с именем, которое оно ЗНАЕТ про соединение, а знает оно его
// ровно из двух мест: клиент назвал имя сам (SOCKS5/CONNECT) либо имя пришло
// из таблицы fake-ip, которую заполняет НАШ резолвер. Браузер, ходящий в
// DNS-over-HTTPS — Chrome делает это по умолчанию, в журнале ядра это
// «[UDP] ... 8.8.8.8:443 match Match using CARAMBA», — получает адрес мимо
// туннельного DNS и открывает соединение ПО IP. Имени в таком соединении нет,
// и ни одно доменное правило по нему не срабатывает.
//
// Ровно на этом ломался блок рекламы: галочка включена, правило
// GEOSITE,category-ads-all,REJECT в конфиге, список из 911 записей загружен, а
// googleads.g.doubleclick.net и www.googletagservices.com скачиваются. Правило
// было живым — сопоставлять его было не с чем.
//
// Что снифер стоит, честно:
//   - соединение без имени платит чтением первых байтов с крайним сроком
//     (1 с у TCP, до 3 с ожидания пакетов у QUIC) и разбором ClientHello.
//     Протокол, в котором первым говорит СЕРВЕР, простоит этот срок впустую,
//     поэтому порты держатся узкими (443 и 80), а не «все»;
//   - имя читается, только пока оно открыто. Encrypted ClientHello прячет
//     настоящее имя за публичным именем провайдера, и такая реклама не
//     режется. Ни снифер, ни что-либо ещё в конфиге этого не меняет;
//   - соединение по голому IP, не несущее ни TLS, ни QUIC, ни HTTP, имени не
//     содержит вовсе — резать его по домену нечем.
//
// Границы описаны текстом в AdBlockLimitNote: интерфейс обязан назвать их сам,
// а не обещать «вся реклама заблокирована».
type SnifferPolicy struct {
	// Enable включает секцию sniffer в конфиге.
	//
	// По умолчанию поднят (DefaultPolicy): дыра «браузер сходил в чужой DoH и
	// соединился по IP» одинаково обесценивает ВСЕ доменные правила, а не
	// только рекламные — GEOSITE,youtube,PROXY у пресета РФ промахивается по
	// той же причине и так же незаметно.
	Enable bool
	// SkipDomains — имена, которым подсмотренное имя НЕ подставляется
	// (skip-domain у ядра).
	//
	// Нужны там, где имя из ClientHello заведомо врёт или вредит: клиент,
	// специально ходящий на конкретный адрес чужого хоста. Список пуст по
	// умолчанию — при выключенной подмене назначения (см. applySniffer) цена
	// ошибки здесь мала, и выдумывать записи заранее незачем.
	SkipDomains []string
}

// AdBlockLimitNote — текст для экрана про то, чего блок рекламы НЕ ловит.
//
// Существует затем, чтобы интерфейс не обещал больше сделанного. Рядом уже
// живёт «включён, подтвердить нечем» — та же порода ответа: сказать ровно то,
// что известно, и назвать остаток вслух.
const AdBlockLimitNote = "Режется по имени домена. Имя берётся из первого пакета соединения, " +
	"поэтому реклама, загруженная по голому IP или спрятанная шифрованием имени (ECH), проходит мимо."

// SplitTunnel описывает раздельное туннелирование.
//
// Семантика:
//   - BypassDomains/BypassProcesses всегда идут напрямую (DIRECT) мимо туннеля.
//   - Если задан allow-список (AllowProcesses/AllowDomains/AllowSites), то в
//     туннель идёт ТОЛЬКО перечисленное, остальное — напрямую (или REJECT при
//     kill-switch).
type SplitTunnel struct {
	BypassDomains   []string
	BypassProcesses []string
	AllowProcesses  []string
	// AllowDomains — домены, которые идут В ТУННЕЛЬ в режиме «через VPN только
	// выбранные сайты». Правило доменное (DOMAIN-SUFFIX), поэтому «youtube.com»
	// покрывает и поддомены, а соединение, открытое по голому IP, под него не
	// попадает вовсе — это ограничение обязано быть названо в интерфейсе.
	AllowDomains []string
	// AllowSites — теги GEOSITE того же allow-списка (готовые наборы доменов
	// сервисов: telegram, youtube, ...). Живут отдельно от AllowDomains, потому
	// что зависят от базы GeoSite.dat: без неё это заведомо мёртвые правила, и
	// отчёт о применённом маршруте обязан их различать.
	AllowSites []string
}

// AllowListActive сообщает, задан ли allow-список хоть чем-нибудь.
//
// Три источника проверяются вместе: пользователь, включивший «только эти
// сайты», и пользователь, оставивший allow-список процессов, оказываются в
// одном режиме — весь остальной трафик мимо туннеля. Разделять эти случаи в
// финальном правиле нечем и незачем.
func (s SplitTunnel) AllowListActive() bool {
	return len(s.AllowProcesses) > 0 || s.AllowSitesActive()
}

// AllowSitesActive — задан ли ИМЕННО сайтовый allow-список.
//
// Он сильнее режима страны: человек, попросивший «через VPN только эти сайты»,
// не должен обнаружить, что ru-smart продолжает гнать в туннель Telegram и
// YouTube. Поэтому при непустом сайтовом списке страновой пресет не
// применяется вовсе (см. routingForBuild и compileSmartRules).
func (s SplitTunnel) AllowSitesActive() bool {
	return len(s.AllowDomains) > 0 || len(s.AllowSites) > 0
}

// Policy — локальная политика клиента, накладываемая на конфиг панели.
type Policy struct {
	// Mode выбирает способ захвата трафика: ModeTun (по умолчанию) или
	// ModeProxy. Пустая строка трактуется как ModeTun.
	Mode TunnelMode
	Tun  TunConfig
	// Proxy — параметры mixed-инбаунда; используются только при Mode == ModeProxy.
	Proxy ProxyConfig
	DNS   DNSConfig
	// Sniffer — восстановление имени домена из первых байтов соединения. Без
	// него доменные правила промахиваются по любому соединению, открытому по
	// IP (браузер с собственным DoH), см. SnifferPolicy.
	Sniffer SnifferPolicy
	// KillSwitch: при включении весь нераспознанный трафик в туннеле, который
	// не может выйти через прокси, отбрасывается (REJECT) вместо утечки в
	// DIRECT. Достигается заменой финального fallback-правила.
	KillSwitch bool
	Split      SplitTunnel
	// BlockAds включает блок рекламы и трекеров ПОВЕРХ выбранного режима
	// маршрутизации.
	//
	// Отдельным флагом, а не пресетом: до него блок рекламы существовал только
	// внутри двух пресетов (cn-smart и adblock), то есть «резать рекламу»
	// означало «сменить страну маршрутизации» или «отказаться от маршрутизации
	// вовсе». Это и есть переключатель, которого владелец не нашёл.
	//
	// Правила и провайдер списка подмешиваются там, где известна судьба
	// самого списка (api.routingForBuild → routing.WithAdBlock): только там
	// отчёт может назвать источник, а интерфейс — не показывать галочку,
	// за которой ничего нет. profile здесь лишь страхует прямые вызовы
	// AssembleMihomoConfig (CLI, тесты) запасным правилом GEOSITE.
	BlockAds bool
	// IPv6 разрешает IPv6 в ядре (верхнеуровневый ipv6 и dns.ipv6). По
	// умолчанию выключен: на мобильных операторах полурабочий IPv6 чаще ломает
	// соединение, чем помогает.
	IPv6 bool
	// Routing — «умная» маршрутизация (пресет/правила, как в Koala Clash).
	// Если nil — используется прежнее поведение (geo-CN + split-tunnel).
	// Если задан — правила пресета компонуются со split-tunnel (байпасы и
	// allow-list имеют приоритет над пресетом), а финал берётся из пресета.
	Routing *routing.Config
	// Protocol фиксирует протокол подключения по дружелюбному имени:
	// "AmneziaWG", "VLESS-Reality", "Hysteria2", "TUIC", "Shadowsocks".
	// Пусто или "Авто"/"auto" оставляет выбор автоматике панели (Auto-All).
	// Иначе селектор CARAMBA по умолчанию берёт лучший сервер этого протокола.
	Protocol string
	// AllowLAN открывает доступ к инбаундам из локальной сети.
	AllowLAN bool
	// LogLevel переопределяет log-level (silent|error|warning|info|debug).
	LogLevel string
	// Bootstrap — то, что подписанный каталог диктует про загрузку служебных
	// данных: адреса geo-баз и цель пробы. Нулевое значение оставляет
	// умолчания ядра (см. BootstrapConfig).
	Bootstrap BootstrapConfig
}

// BootstrapConfig — загрузочные адреса, продиктованные доверенным каталогом.
//
// 02-SPEC.md 8.10: geo-базы сегодня качаются прямо с GitHub, а url-test каждой
// группы стучится в gstatic.com. В РФ заблокировано и то и другое, поэтому оба
// адреса переезжают на зеркала каталога, а проба — на хост из подписанного
// пула.
type BootstrapConfig struct {
	// Managed поднят, когда каталог доверен И оператор предложил хеши
	// ресурсов (бит 6). Только тогда секция geox-url вообще пишется: без
	// подписанных хешей клиент откатывается на встроенные данные mihomo, а не
	// качает по адресам каталога без подписи (transport.ErrResourcesDisabled).
	Managed bool
	// GeoIPURL, GeoSiteURL, MmdbURL, ASNURL — абсолютные адреса geo-баз на
	// зеркалах каталога.
	//
	// ПУСТАЯ строка при Managed — это осознанный отказ, а не «оставить как
	// было»: каталог не назвал такой ресурс, значит подписанного хеша для него
	// нет, и докачка запрещена. Ядро в этом случае падает на понятной ошибке
	// вместо тихой загрузки неподписанного файла с GitHub.
	GeoIPURL   string
	GeoSiteURL string
	MmdbURL    string
	ASNURL     string
	// ProbeURL — цель url-test для служебных групп. Пусто → DefaultProbeURL.
	ProbeURL string
}

// Резолверы по умолчанию.
//
// Прежние значения (https://1.1.1.1/dns-query, https://8.8.8.8/dns-query,
// tls://1.1.1.1:853) убраны намеренно: с 2026-08-26 оба адреса блокируются в
// нескольких регионах РФ, а порт 853 закрыт целиком, поэтому клиент со
// «здоровыми» умолчаниями оставался без DNS ровно там, где он нужен.
// Полевое указание — только DoH по https и раскол, при котором домашний
// трафик резолвит домашний резолвер (00-DESIGN-BRIEF.md 161).
//
// Это компилируемые умолчания, а не удалённо включаемая функция: доверенный
// каталог их ЗАМЕНЯЕТ (поле doh), а пользователь переопределяет через
// настройки pol[9]/pol[10]. До регистрации работают именно они.
var (
	// DefaultNameservers — резолверы общего назначения (ветка nameserver).
	DefaultNameservers = []string{"https://dns.quad9.net/dns-query"}
	// DefaultDomesticNameservers — домашняя половина раскола для стран, где
	// зарубежный DoH недоступен. Используется для direct-nameserver и
	// proxy-server-nameserver.
	//
	// Адрес именно ЧИСЛОВОЙ, и это не стилистика. Здесь стояло
	// `https://common.dns.yandex.net/dns-query` — хост, которого не существует
	// (NXDOMAIN). Внутри туннеля резолвер по имени сам нуждается в резолвинге,
	// и mihomo не мог разрешить ни одного имени, идущего напрямую: туннель
	// поднимался, значок ключа загорался, а сайты не открывались. Снаружи это
	// выглядит как «не удаётся подключиться».
	//
	// IP-форма убирает саму зависимость от начального резолвинга.
	DefaultDomesticNameservers = []string{"https://77.88.8.8/dns-query"}
	// NeutralDomesticNameservers — прямая половина раскола для страны, которая
	// ИЗВЕСТНА и в domesticResolvers не значится: там нет цензуры, ради которой
	// раскол задумывался, и нет причины отдавать прямой трафик национальному
	// резолверу другой страны.
	//
	// Это тот же Quad9, что и в DefaultNameservers, но записанный адресом, а не
	// именем, — по той же причине, что и строкой выше: резолвер, заданный
	// именем, внутри туннеля сам нуждается в разрешении имени.
	NeutralDomesticNameservers = []string{"https://9.9.9.9/dns-query"}
)

// domesticResolvers — домашний резолвер по ISO-коду страны. Список намеренно
// короткий: сюда попадает только то, что подтверждено полевыми данными.
var domesticResolvers = map[string][]string{
	"RU": {"https://77.88.8.8/dns-query"},
	"BY": {"https://77.88.8.8/dns-query"},
	"CN": {"https://doh.pub/dns-query"},
}

// DomesticResolvers возвращает домашний резолвер для страны пользователя.
//
// Три ветки, и различать их обязательно:
//
//   - страна есть в таблице (RU/BY/CN) — её резолвер. Это страны, где
//     зарубежный DoH недоступен, ради них раскол и существует.
//   - страна ИЗВЕСТНА, но в таблице её нет (US, DE, …) — общий резолвер
//     (DefaultNameservers). Здесь раскола нет и быть не должно: раньше эта
//     ветка возвращала DefaultDomesticNameservers, то есть 77.88.8.8 — Яндекс
//     резолвил прямой трафик американскому пользователю просто потому, что
//     российское умолчание стояло на месте «всех остальных».
//   - страна ПУСТА (не знаем) — DefaultDomesticNameservers, прежнее
//     компилируемое умолчание. Менять его нельзя: оно выбрано полевыми
//     данными для случая, когда клиент поднимается там, где общий DoH не
//     работает, и «неизвестно» надёжнее трактовать в эту сторону.
func DomesticResolvers(iso string) []string {
	iso = strings.ToUpper(strings.TrimSpace(iso))
	if r, ok := domesticResolvers[iso]; ok {
		out := make([]string, len(r))
		copy(out, r)
		return out
	}
	if iso != "" {
		out := make([]string, len(NeutralDomesticNameservers))
		copy(out, NeutralDomesticNameservers)
		return out
	}
	out := make([]string, len(DefaultDomesticNameservers))
	copy(out, DefaultDomesticNameservers)
	return out
}

// ApplyBootstrapDNS накладывает на политику резолверы, продиктованные
// подписанным каталогом, и национальный раскол по стране пресета.
//
// Правило одно и оно важнее удобства: заменяется ТОЛЬКО компилируемое
// умолчание. Значение, которое пользователь выставил сам, каталог здесь не
// трогает — операторское изменение пользовательского поля проходит через
// карточку «оставить или вернуть» (02-SPEC.md 7.7), а она живёт в приложении,
// а не в сборщике конфига.
//
// catalogDoH — список из caталога (поле doh), country — ISO-код страны
// активного пресета маршрутизации, пусто допустимо.
func (p *Policy) ApplyBootstrapDNS(catalogDoH []string, country string) {
	if len(catalogDoH) > 0 && sameList(p.DNS.Nameservers, DefaultNameservers) {
		p.DNS.Nameservers = append([]string(nil), catalogDoH...)
	}
	if country == "" {
		return
	}
	domestic := DomesticResolvers(country)
	if len(domestic) == 0 {
		return
	}
	if sameList(p.DNS.DirectNameservers, DefaultDomesticNameservers) {
		p.DNS.DirectNameservers = append([]string(nil), domestic...)
	}
	if sameList(p.DNS.ProxyServerNameservers, DefaultDomesticNameservers) {
		p.DNS.ProxyServerNameservers = append([]string(nil), domestic...)
	}
}

// sameList сообщает, совпадает ли список с умолчанием поэлементно. Пустой
// список тоже считается нетронутым: пользователь, стёрший всё, не выбирал
// конкретные адреса.
func sameList(got, def []string) bool {
	if len(got) == 0 {
		return true
	}
	if len(got) != len(def) {
		return false
	}
	for i := range got {
		if got[i] != def[i] {
			return false
		}
	}
	return true
}

// DefaultProbeURL — цель url-test, когда каталог не назвал свою.
//
// Она остаётся зашитой только как последнее средство для клиента без каталога:
// при доверенном каталоге BootstrapConfig.ProbeURL указывает на хост из
// подписанного пула зеркал, и gstatic здесь не участвует.
const DefaultProbeURL = "https://www.gstatic.com/generate_204"

// DefaultPolicy возвращает разумную политику по умолчанию: TUN включён,
// kill-switch включён, DNS с fake-ip.
func DefaultPolicy() Policy {
	return Policy{
		Mode: ModeTun,
		Tun: TunConfig{
			Enable:     true,
			Stack:      StackGVisor,
			DeviceName: "caramba-tun",
			MTU:        1280,
			AutoRoute:  true,
			DNSHijack:  []string{"any:53"},
		},
		Proxy: ProxyConfig{
			MixedPort:   DefaultMixedPort,
			BindAddress: DefaultBindAddress,
		},
		DNS: DNSConfig{
			Enable:                 true,
			Listen:                 "127.0.0.1:1053",
			Nameservers:            append([]string(nil), DefaultNameservers...),
			DirectNameservers:      append([]string(nil), DefaultDomesticNameservers...),
			ProxyServerNameservers: append([]string(nil), DefaultDomesticNameservers...),
			EnhancedMode:           "fake-ip",
			FakeIPRange:            "198.18.0.1/16",
		},
		// Снифер поднят по умолчанию: fake-ip даёт имя только тем соединениям,
		// чей DNS-запрос прошёл через нас, а браузер со своим DoH такого
		// запроса не задаёт. Без снифера доменные правила молча промахиваются
		// в самом частом сценарии, и «неизвестно, работает ли» становится
		// единственным возможным ответом про любое из них.
		Sniffer:    SnifferPolicy{Enable: true},
		KillSwitch: true,
		LogLevel:   "info",
	}
}

// EffectiveMode возвращает режим захвата трафика с подставленным умолчанием:
// пустая строка означает ModeTun (обратная совместимость с политиками, которые
// собирались до появления proxy-режима).
func (p Policy) EffectiveMode() TunnelMode {
	if p.Mode == ModeProxy {
		return ModeProxy
	}
	return ModeTun
}

// EffectiveProxy возвращает параметры mixed-инбаунда с подставленными
// значениями по умолчанию (порт 7890, привязка к петле).
func (p Policy) EffectiveProxy() ProxyConfig {
	pc := p.Proxy
	if pc.MixedPort <= 0 {
		pc.MixedPort = DefaultMixedPort
	}
	pc.BindAddress = strings.TrimSpace(pc.BindAddress)
	if pc.BindAddress == "" {
		pc.BindAddress = DefaultBindAddress
	}
	return pc
}

// LoopbackPort возвращает порт служебного mixed-инбаунда на петле.
//
// Выбор детерминированный: либо явно заданный порт, либо DefaultLoopbackPort.
// Единственное, что его сдвигает, — совпадение с mixed-портом пользователя:
// два слушателя на одном порту ядро не поднимет, а mixed-порт принадлежит
// пользователю, поэтому уступает служебный. Возврат 0 означает «служебный
// инбаунд выключен».
func (p Policy) LoopbackPort() int {
	want := p.Proxy.LoopbackPort
	if want < 0 {
		return 0
	}
	if want == 0 {
		want = DefaultLoopbackPort
	}
	if p.EffectiveMode() == ModeProxy && want == p.EffectiveProxy().MixedPort {
		want++
	}
	return want
}

// LoopbackAddr возвращает адрес служебного инбаунда для transport.Ladder
// (SetTunnelProxy). Пустая строка означает, что ступени R4 ходить некуда.
func (p Policy) LoopbackAddr() string {
	port := p.LoopbackPort()
	if port == 0 {
		return ""
	}
	return fmt.Sprintf("%s:%d", LoopbackHost, port)
}

// LoopbackProxyURL возвращает адрес служебного инбаунда в том виде, в каком его
// принимает лестница: socks5 с парой логин-пароль в userinfo.
//
// Пустая строка означает "инбаунда нет", и ровно то же самое означает
// отсутствие учётных данных: слушатель без них не собирается, поэтому обещать
// лестнице путь к нему нельзя.
func (p Policy) LoopbackProxyURL() string {
	addr := p.LoopbackAddr()
	if addr == "" {
		return ""
	}
	pc := p.EffectiveProxy()
	if pc.LoopbackUser == "" || pc.LoopbackPass == "" {
		return ""
	}
	return "socks5://" + url.UserPassword(pc.LoopbackUser, pc.LoopbackPass).String() + "@" + addr
}

// effectiveKillSwitch сообщает, применять ли kill-switch к финальному правилу.
//
// Kill-switch — понятие TUN-режима: там туннель является маршрутом по умолчанию,
// и без REJECT нераспознанный трафик утёк бы в DIRECT мимо прокси. В
// proxy-режиме утекать нечему: ОС ничего не заворачивает, в mixed-инбаунд
// попадает только то, что приложение отправило туда явно. Финальный REJECT там
// означал бы «прокси поднят, но отбрасывает всё», поэтому в proxy-режиме
// kill-switch не применяется.
func (p Policy) effectiveKillSwitch() bool {
	if p.EffectiveMode() == ModeProxy {
		return false
	}
	return p.KillSwitch
}

// AssembleMihomoConfig принимает сырой mihomo YAML из подписки и возвращает
// новый YAML с применённой локальной политикой. Узлы (proxies/proxy-groups)
// сохраняются как есть.
func AssembleMihomoConfig(rawYAML []byte, policy Policy) ([]byte, error) {
	return AssembleMihomoConfigPinned(rawYAML, policy, "")
}

// AssembleMihomoConfigPinned делает то же, что AssembleMihomoConfig, но
// дополнительно фиксирует конкретный узел как выбор по умолчанию в группе
// CARAMBA (pinProxy — имя прокси, как оно записано в секции proxies).
//
// Это путь Up(serverID) для импортированных подписок: панельного выбора
// выходного узла там нет, поэтому узел выбирается перестановкой участников
// селектора. Пустой pinProxy оставляет автоматический выбор. Пин применяется
// ПОСЛЕ applyProtocol: явный выбор пользователя важнее служебной группы
// протокола.
func AssembleMihomoConfigPinned(rawYAML []byte, policy Policy, pinProxy string) ([]byte, error) {
	// Разбираем в общую карту, чтобы не терять неизвестные ключи панели.
	doc := map[string]any{}
	if err := yaml.Unmarshal(rawYAML, &doc); err != nil {
		return nil, fmt.Errorf("profile: разбор конфигурации подписки: %w", err)
	}

	prepareProxiesForCore(doc)
	applyGeneral(doc, policy)
	if policy.EffectiveMode() == ModeProxy {
		applyProxyInbound(doc, policy)
	} else {
		applyTun(doc, policy.Tun)
	}
	applyLoopbackInbound(doc, policy)
	applyGeoX(doc, policy)
	applyDNS(doc, policy)
	applySniffer(doc, policy)
	applyRuleProviders(doc, policy)
	applyRules(doc, policy)
	applyProtocol(doc, policy)
	applyPin(doc, pinProxy)
	applyKillSwitch(doc, policy)

	out, err := yaml.Marshal(doc)
	if err != nil {
		return nil, fmt.Errorf("profile: сериализация конфигурации: %w", err)
	}
	return out, nil
}

// prepareProxiesForCore приводит секцию proxies к тому, что ядро действительно
// умеет: переводит формы, названные источником иначе (см.
// subscription.NormalizeProxyForCore), и убирает прокси, для которых ядро не
// строит outbound вовсе (см. subscription.CoreCanBuildProxyType), вместе со
// ссылками на них.
//
// Это не косметика, а условие того, чтобы туннель вообще поднимался. mihomo
// разбирает секцию proxies целиком и на первом незнакомом типе отвергает ВЕСЬ
// конфиг («proxy N: unsupport proxy type»). Одного naive-узла в подписке хватило
// бы, чтобы человек остался без связи, — и раньше этого не случалось лишь
// потому, что импорт подменял naive на type:http, то есть лгал про протокол.
// Теперь тип честный, а конфиг для ядра чистый.
//
// Заодно снимается dialer-proxy, указывающий на ВЫБРОШЕННЫЙ НАМИ узел: ссылка в
// никуда молча проваливает КАЖДЫЙ набор через этот выход уже в бою. Ссылки на
// прочие имена не трогаются: mihomo разрешает указать в dialer-proxy имя
// группы, и «не нашли среди прокси» там не означает «ошибка».
func prepareProxiesForCore(doc map[string]any) {
	list, ok := doc["proxies"].([]any)
	if !ok {
		return
	}
	dropped := make(map[string]struct{})
	kept := make([]any, 0, len(list))
	for _, item := range list {
		pm, ok := item.(map[string]any)
		if !ok {
			kept = append(kept, item)
			continue
		}
		typ, _ := pm["type"].(string)
		if subscription.CoreCanBuildProxyType(typ) {
			subscription.NormalizeProxyForCore(pm)
			kept = append(kept, item)
			continue
		}
		if name, _ := pm["name"].(string); name != "" {
			dropped[name] = struct{}{}
		}
	}
	if len(dropped) == 0 {
		// Нормализация уже применена по месту; переставлять список незачем.
		return
	}
	doc["proxies"] = kept

	for _, item := range kept {
		pm, ok := item.(map[string]any)
		if !ok {
			continue
		}
		if target, _ := pm["dialer-proxy"].(string); target != "" {
			if _, gone := dropped[target]; gone {
				delete(pm, "dialer-proxy")
			}
		}
	}
	dropProxyNamesFromGroups(doc, dropped)
}

// dropProxyNamesFromGroups вычёркивает имена из списков участников групп.
//
// Группа, ссылающаяся на несуществующего участника, — тоже отказ загрузки
// целиком. Опустевшая группа отказ тоже: mihomo не принимает select без
// участников, поэтому в неё возвращается DIRECT — та же мягкая деградация, что
// и в applyKillSwitch.
func dropProxyNamesFromGroups(doc map[string]any, dropped map[string]struct{}) {
	groups, ok := doc["proxy-groups"].([]any)
	if !ok {
		return
	}
	for _, g := range groups {
		gm, ok := g.(map[string]any)
		if !ok {
			continue
		}
		list, ok := gm["proxies"].([]any)
		if !ok {
			continue
		}
		filtered := make([]any, 0, len(list))
		for _, item := range list {
			if s, ok := item.(string); ok {
				if _, gone := dropped[s]; gone {
					continue
				}
			}
			filtered = append(filtered, item)
		}
		if len(filtered) == 0 {
			filtered = append(filtered, "DIRECT")
		}
		gm["proxies"] = filtered
	}
}

// applyGeneral переопределяет верхнеуровневые флаги.
func applyGeneral(doc map[string]any, p Policy) {
	allowLAN := p.AllowLAN
	if p.EffectiveMode() == ModeProxy {
		// В proxy-режиме доступ из локальной сети открывает сам инбаунд.
		allowLAN = allowLAN || p.Proxy.AllowLAN
	}
	doc["allow-lan"] = allowLAN
	doc["mode"] = "rule"
	doc["ipv6"] = p.IPv6
	if p.LogLevel != "" {
		doc["log-level"] = p.LogLevel
	}
}

// applyTun формирует секцию tun.
func applyTun(doc map[string]any, t TunConfig) {
	if !t.Enable {
		delete(doc, "tun")
		return
	}
	tun := map[string]any{
		"enable":     true,
		"stack":      string(stackOrDefault(t.Stack)),
		"auto-route": t.AutoRoute,
	}
	if t.DeviceName != "" {
		tun["device"] = t.DeviceName
	}
	if t.MTU > 0 {
		tun["mtu"] = t.MTU
	}
	if len(t.DNSHijack) > 0 {
		tun["dns-hijack"] = t.DNSHijack
	}
	doc["tun"] = tun
}

// applyProxyInbound формирует локальный mixed-инбаунд (SOCKS5 и HTTP на одном
// порту) и убирает TUN. Это режим без привилегий: ядро лишь слушает порт на
// петле, а трафик в него направляет приложение либо системный прокси ОС.
//
// Прочие инбаунды из конфига подписки (port/socks-port/redir-port/tproxy-port)
// удаляются намеренно: в proxy-режиме порт должен быть ровно один и известный
// UI, иначе ядро займёт лишние порты, а подсказка «Proxy on 127.0.0.1:7890»
// перестанет соответствовать действительности.
func applyProxyInbound(doc map[string]any, p Policy) {
	delete(doc, "tun")
	for _, key := range []string{"port", "socks-port", "redir-port", "tproxy-port"} {
		delete(doc, key)
	}
	pc := p.EffectiveProxy()
	doc["mixed-port"] = pc.MixedPort
	doc["bind-address"] = pc.BindAddress
}

// applyLoopbackInbound поднимает служебный mixed-инбаунд на петле — тот самый,
// которого 02-SPEC.md 8.2 требует в ОБОИХ режимах туннеля.
//
// Зачем он нужен в TUN-режиме. Ступень R4 ходит за манифестом через
// собственный туннель приложения. На Android приложение исключено из своего же
// туннеля (addDisallowedApplication в CarambaVpnService), а локального
// слушателя в TUN-режиме до сих пор не существовало — и у R4 просто не было
// пути, из-за чего диагностика показывала not_configured на Android и успех на
// iOS в одной и той же сети. Слушатель на 127.0.0.1 снимает эту разницу: он
// живёт внутри процесса приложения, и соединение до него не пересекает TUN.
//
// Инбаунд всегда привязан к петле, независимо от AllowLAN: это канал выборки
// конфигурации, а не пользовательский прокси, и открывать его в локальную сеть
// незачем. Порт выбирается детерминированно (см. Policy.LoopbackPort).
//
// Чужие именованные слушатели из конфига подписки сохраняются; вычищается
// только запись с нашим именем, чтобы повторная сборка не плодила дубликаты.
func applyLoopbackInbound(doc map[string]any, p Policy) {
	kept := make([]any, 0, 2)
	if existing, ok := doc["listeners"].([]any); ok {
		for _, it := range existing {
			m, ok := it.(map[string]any)
			if !ok {
				continue
			}
			if name, _ := m["name"].(string); name == LoopbackListenerName {
				continue
			}
			kept = append(kept, it)
		}
	}
	pc := p.EffectiveProxy()
	// Слушатель без пары логин-пароль не собирается ВООБЩЕ. Это не degradation
	// к прежнему поведению: прежнее поведение и было открытым релеем. Лестница
	// в этом случае объявляет R4 недоступной (LoopbackProxyURL пуст), а не
	// ходит в инбаунд, которого нет.
	if port := p.LoopbackPort(); port > 0 && pc.LoopbackUser != "" && pc.LoopbackPass != "" {
		kept = append(kept, map[string]any{
			"name":   LoopbackListenerName,
			"type":   "mixed",
			"listen": LoopbackHost,
			"port":   port,
			// users это единственное, что отделяет служебный канал выборки от
			// открытого релея: mihomo раздаёт этот список в auth-store и
			// требует метод 0x02 у socks5 и Proxy-Authorization у http.
			"users": []any{
				map[string]any{"username": pc.LoopbackUser, "password": pc.LoopbackPass},
			},
			// udp не нужен: по этому инбаунду ходит только выборка манифеста
			// по HTTP, и открывать лишний сокет незачем.
			"udp": false,
			// Ключ proxy у инбаунда (BaseOption.SpecialProxy) уводит ВСЁ, что
			// пришло на этот слушатель, прямо в группу-селектор мимо правил.
			// Без него ступень R4 попадала бы под пресет маршрутизации: у
			// «умного» пресета РФ финал DIRECT, и запрос за манифестом ушёл бы
			// напрямую, то есть R4 стала бы неотличима от R1 и врала бы в
			// диагностике, что путь через туннель работает.
			"proxy": CarambaSelector,
		})
	}
	if len(kept) == 0 {
		delete(doc, "listeners")
		return
	}
	doc["listeners"] = kept
}

// applyGeoX переводит загрузку geo-баз на зеркала каталога.
//
// Ключ geox-url ядро читает в executor.ApplyConfig и раздаёт в
// geodata.SetGeoIpUrl / SetGeoSiteUrl / SetMmdbUrl / SetASNUrl, поэтому вызвать
// их напрямую не нужно — достаточно записать секцию в конфиг, и путь остаётся
// проверяемым в сборке без нативного ядра.
//
// Умолчание mihomo — четыре ссылки на github.com/MetaCubeX/meta-rules-dat, то
// есть на хост, который в РФ недоступен. При доверенном каталоге секция
// перезаписывается ЦЕЛИКОМ: ресурс, которого каталог не назвал, получает
// пустой адрес и ядро отказывается его качать, вместо того чтобы тихо сходить
// за неподписанным файлом на GitHub. Отказ, а не откат (инвариант 12).
func applyGeoX(doc map[string]any, p Policy) {
	b := p.Bootstrap
	if !b.Managed {
		return
	}
	doc["geox-url"] = map[string]any{
		"geoip":   b.GeoIPURL,
		"geosite": b.GeoSiteURL,
		"mmdb":    b.MmdbURL,
		"asn":     b.ASNURL,
	}
}

func stackOrDefault(s Stack) Stack {
	if s == "" {
		return StackGVisor
	}
	return s
}

// applyDNS формирует секцию dns.
//
// В proxy-режиме fake-ip намеренно понижается до redir-host. Причина: fake-ip
// имеет смысл только вместе с TUN, который отдаёт клиенту синтетический адрес из
// пула и сам разворачивает его обратно в домен. При mixed-инбаунде домен и так
// приходит в ядро как есть (HTTP CONNECT и SOCKS5 адресуют по имени), зато
// синтетические адреса ломают правила, которым нужен настоящий IP (GEOIP без
// no-resolve), и любого клиента, который спросил бы встроенный резолвер напрямую
// и полез бы по полученному адресу мимо прокси. redir-host отдаёт честные ответы
// и оставляет GEOIP работоспособным.
func applyDNS(doc map[string]any, p Policy) {
	d := p.DNS
	if !d.Enable {
		return
	}
	enhanced := strings.TrimSpace(d.EnhancedMode)
	if p.EffectiveMode() == ModeProxy && strings.EqualFold(enhanced, "fake-ip") {
		enhanced = "redir-host"
	}
	dns := map[string]any{
		"enable": true,
		"ipv6":   p.IPv6,
	}
	if d.Listen != "" {
		dns["listen"] = d.Listen
	}
	if enhanced != "" {
		dns["enhanced-mode"] = enhanced
	}
	// fake-ip-range имеет смысл только в fake-ip режиме; в redir-host ядро его
	// игнорирует, а в конфиге он лишь путает читателя.
	if d.FakeIPRange != "" && strings.EqualFold(enhanced, "fake-ip") {
		dns["fake-ip-range"] = d.FakeIPRange
	}
	if len(d.Nameservers) > 0 {
		dns["nameserver"] = d.Nameservers
	}
	if len(d.FallbackNameservers) > 0 {
		dns["fallback"] = d.FallbackNameservers
	}
	// Начальные резолверы — ТОЛЬКО числовые адреса.
	//
	// DoH, заданный именем (например dns.quad9.net), сам требует разрешения
	// имени, и разрешать его нечем: без этого списка mihomo берёт собственные
	// умолчания, среди которых китайские резолверы, недостижимые оттуда, где
	// работает продукт. Каталог оператора может заменить резолверы своими, но
	// пока его нет, начальная точка обязана быть самодостаточной.
	dns["default-nameserver"] = []string{"77.88.8.8", "9.9.9.9", "1.1.1.1"}
	// Национальный раскол резолверов. direct-nameserver обслуживает имена,
	// которые идут напрямую (домашние сайты, банки, госуслуги), а
	// proxy-server-nameserver — имена самих узлов, которые приходится
	// резолвить до поднятия туннеля. В обе ветки ставится домашний резолвер:
	// зарубежный в этот момент либо недоступен, либо отдаёт чужую геопривязку.
	// Поток запросов пользователя сюда не попадает — он идёт в nameserver уже
	// сквозь туннель (04-THREAT-MODEL.md 4.5 про то, почему это разные вещи).
	if len(d.DirectNameservers) > 0 {
		dns["direct-nameserver"] = d.DirectNameservers
	}
	if len(d.ProxyServerNameservers) > 0 {
		dns["proxy-server-nameserver"] = d.ProxyServerNameservers
	}
	doc["dns"] = dns
}

// SniffDomains сообщает, попадёт ли секция sniffer в собранный конфиг.
//
// Блок рекламы включает снифер САМ, даже когда политика про снифер молчит.
// Иначе получается галочка, которая честно кладёт правило в конфиг и столь же
// честно ничего не режет: правило доменное, а имени в соединении нет.
// Переключатель обязан отвечать за результат, а не за наличие строки в YAML.
func (p Policy) SniffDomains() bool {
	return p.Sniffer.Enable || p.BlockAds
}

// snifferTLSPorts, snifferHTTPPorts, snifferQUICPorts — порты, на которых ядро
// читает первые байты.
//
// Список узкий намеренно. Разбор запускается только для соединений БЕЗ имени
// (parse-pure-ip), но соединение, где первым говорит сервер, простоит на
// крайнем сроке чтения впустую — а на 443 и 80 таких протоколов практически не
// бывает. Каждый добавленный сюда порт это новая горсть протоколов, которым мы
// вслепую вставляем паузу; реклама живёт на этих двух.
var (
	snifferTLSPorts  = []string{"443"}
	snifferQUICPorts = []string{"443"}
	snifferHTTPPorts = []string{"80"}
)

// applySniffer формирует секцию sniffer — восстановление имени домена из
// первых байтов соединения. См. SnifferPolicy про то, зачем и какой ценой.
//
// Три решения, каждое из которых меняет поведение и потому названо здесь.
//
// override-destination ВЫКЛЮЧЕН. Имя нужно ядру, чтобы ВЫБРАТЬ правило, а не
// чтобы набрать адрес: доменные матчеры сопоставляются с Metadata.RuleHost(),
// а он предпочитает подсмотренное имя и без подмены назначения (mihomo,
// constant/metadata.go). Включённая подмена выбросила бы адрес, который выбрал
// сам клиент, и заставила бы ядро разрешить имя заново своим резолвером — то
// есть сломала бы всё, что от конкретного адреса зависит: привязку к узлу CDN,
// раздельный горизонт DNS в корпоративных сетях, клиента, который специально
// пришёл на этот адрес. Ради REJECT рекламы подмена не нужна вовсе: у
// отброшенного соединения адресата нет.
//
// force-dns-mapping ВЫКЛЮЧЕН. Соединение, чьё имя пришло из таблицы fake-ip,
// уже названо, и повторное чтение первых байтов не добавляет к нему ничего —
// зато ставит крайний срок чтения на КАЖДОЕ соединение вместо только
// безымянных.
//
// parse-pure-ip ВКЛЮЧЁН. Это и есть разбираемый случай: имени нет, и взять его
// больше неоткуда.
//
// Чужая секция sniffer из конфига подписки перезаписывается целиком, как и
// остальные верхнеуровневые секции политики: два источника правды про то,
// читает ли ядро первые байты, дают поведение, которого никто не выбирал.
func applySniffer(doc map[string]any, p Policy) {
	if !p.SniffDomains() {
		delete(doc, "sniffer")
		return
	}
	sniffer := map[string]any{
		"enable":               true,
		"parse-pure-ip":        true,
		"force-dns-mapping":    false,
		"override-destination": false,
		"sniff": map[string]any{
			"TLS":  map[string]any{"ports": snifferTLSPorts},
			"QUIC": map[string]any{"ports": snifferQUICPorts},
			"HTTP": map[string]any{"ports": snifferHTTPPorts},
		},
	}
	if len(p.Sniffer.SkipDomains) > 0 {
		sniffer["skip-domain"] = append([]string(nil), p.Sniffer.SkipDomains...)
	}
	doc["sniffer"] = sniffer
}

// applyRuleProviders формирует секцию rule-providers из активного пресета
// маршрутизации (если он задан). Без Routing секция не трогается.
func applyRuleProviders(doc map[string]any, p Policy) {
	route := effectiveRouting(p)
	if route == nil {
		return
	}
	if providers := route.CompiledProviders(); providers != nil {
		doc["rule-providers"] = providers
	}
}

// effectiveRouting возвращает конфигурацию маршрутизации в том виде, в каком
// она действительно попадёт в конфиг.
//
// Существует ради одного: секция rule-providers и секция rules ОБЯЗАНЫ
// строиться из одного и того же набора. Когда сайтовый allow-список отменяет
// страновой пресет, его списки правил больше никем не упоминаются — объявить
// их провайдерами значило бы заставить ядро качать файлы, к которым не ведёт
// ни одно правило, и показать в отчёте источники, которых в сборке нет.
func effectiveRouting(p Policy) *routing.Config {
	if p.Routing == nil {
		return nil
	}
	if !p.Split.AllowSitesActive() {
		return p.Routing
	}
	only := routing.AdBlockOnly(*p.Routing)
	return &only
}

// applyRules собирает список rules. Если задан Routing — используется движок
// «умной» маршрутизации (пресет + пользовательские правила), иначе — базовый
// набор без страновых geo-правил.
//
// Kill-switch НЕ превращает финал в MATCH,REJECT: финальное правило всегда
// ведёт в селектор CARAMBA (иначе при включённом kill-switch клиент просто
// терял бы весь трафик, даже когда прокси жив). Отказ «в закрытую» обеспечивает
// applyKillSwitch, который убирает DIRECT из фолбэка селектора.
//
// В allow-list split финал тоже НЕ REJECT, и это исправление, а не упущение:
// трафик вне списка идёт напрямую потому, что человек сам так попросил, а не
// потому, что туннель сломался. Пока здесь стоял REJECT, галочка «через VPN
// только выбранные сайты» при kill-switch (включённом по умолчанию) отрезала
// весь остальной интернет — проверено на устройстве.
func applyRules(doc map[string]any, p Policy) {
	if p.Routing != nil {
		doc["rules"] = compileSmartRules(p)
		return
	}

	rules := make([]string, 0, 16)

	// Байпас доменов (всегда напрямую).
	for _, dom := range p.Split.BypassDomains {
		rules = append(rules, fmt.Sprintf("DOMAIN-SUFFIX,%s,DIRECT", dom))
	}
	// Байпас процессов (allow-list имеет приоритет ниже — байпас всегда мимо).
	for _, proc := range p.Split.BypassProcesses {
		rules = append(rules, fmt.Sprintf("PROCESS-NAME,%s,DIRECT", proc))
	}

	// Локальные сети мимо туннеля. Страновые geo-правила (GEOIP,CN/GEOSITE,cn)
	// здесь намеренно НЕ добавляются: без выбранного пресета мы не знаем страну
	// пользователя, а зашитый Китай ломал маршрутизацию для РФ/Ирана/Беларуси.
	// Страновая логика живёт в пресетах (routing.Presets), см. Policy.Routing.
	rules = append(rules, "GEOIP,private,DIRECT,no-resolve")

	// Запасной блок рекламы для прямых вызовов сборки (CLI, тесты): здесь нет
	// ни каталога, ни зеркала, поэтому доступна только встроенная база mihomo.
	// На пути приложения сюда не попадают — там правила и провайдер приходят
	// готовыми в Policy.Routing вместе с судьбой списка.
	if p.BlockAds {
		rules = append(rules, "GEOSITE,"+routing.AdBlockGeositeTag+",REJECT")
	}

	if p.Split.AllowListActive() {
		// Allow-list: в туннель идёт ТОЛЬКО перечисленное.
		for _, dom := range p.Split.AllowDomains {
			rules = append(rules, fmt.Sprintf("DOMAIN-SUFFIX,%s,%s", dom, CarambaSelector))
		}
		for _, tag := range p.Split.AllowSites {
			rules = append(rules, fmt.Sprintf("GEOSITE,%s,%s", tag, CarambaSelector))
		}
		for _, proc := range p.Split.AllowProcesses {
			rules = append(rules, fmt.Sprintf("PROCESS-NAME,%s,%s", proc, CarambaSelector))
		}
		// Всё, что вне списка: при kill-switch отбрасывается, иначе идёт напрямую.
		if p.effectiveKillSwitch() {
			rules = append(rules, "MATCH,REJECT")
		} else {
			rules = append(rules, "MATCH,DIRECT")
		}
		doc["rules"] = rules
		return
	}

	rules = append(rules, "MATCH,"+CarambaSelector)
	doc["rules"] = rules
}

// compileSmartRules строит правила из активного пресета маршрутизации,
// накладывая поверх него пользовательский split-tunnel.
//
// Приоритет (сверху вниз): байпас-домены → байпас-процессы → allow-процессы →
// правила пресета → финальный MATCH. Финал берётся из пресета и kill-switch'ем
// НЕ подменяется (см. applyRules и applyKillSwitch); исключение — allow-list
// split, где трафик вне списка при kill-switch отбрасывается, а без него идёт
// напрямую.
func compileSmartRules(p Policy) []string {
	// Высокоприоритетные пользовательские правила.
	hp := routing.Config{}
	for _, dom := range p.Split.BypassDomains {
		hp.Rules = append(hp.Rules, routing.Rule{
			Type: routing.MatchDomainSuffix, Value: dom, Action: routing.ActionDirect,
		})
	}
	for _, proc := range p.Split.BypassProcesses {
		hp.Rules = append(hp.Rules, routing.Rule{
			Type: routing.MatchProcessName, Value: proc, Action: routing.ActionDirect,
		})
	}
	for _, dom := range p.Split.AllowDomains {
		hp.Rules = append(hp.Rules, routing.Rule{
			Type: routing.MatchDomainSuffix, Value: dom, Action: routing.ActionProxy,
		})
	}
	for _, tag := range p.Split.AllowSites {
		hp.Rules = append(hp.Rules, routing.Rule{
			Type: routing.MatchGeosite, Value: tag, Action: routing.ActionProxy,
		})
	}
	for _, proc := range p.Split.AllowProcesses {
		hp.Rules = append(hp.Rules, routing.Rule{
			Type: routing.MatchProcessName, Value: proc, Action: routing.ActionProxy,
		})
	}
	if p.Split.AllowListActive() {
		// Allow-list перебивает финал пресета: вне списка — НАПРЯМУЮ, и
		// kill-switch этого не меняет.
		//
		// Здесь смешивались два разных смысла. Kill-switch стережёт трафик,
		// который ДОЛЖЕН идти через туннель: если туннель упал, такой трафик
		// режется, чтобы не утечь мимо. Трафик вне allow-списка идёт напрямую
		// не потому, что что-то сломалось, а потому, что человек прямо этого
		// попросил, — утечкой он не является.
		//
		// Пока REJECT ставился и здесь, галочка «через VPN только выбранные
		// сайты» при kill-switch (а он включён по умолчанию) давала финальное
		// `MATCH,REJECT` и отрезала весь остальной интернет. Экран при этом
		// обещал «всё остальное — напрямую». Проверено на устройстве: после
		// включения списка ifconfig.me отвечал ERR_CONNECTION_CLOSED.
		hp.FinalAction = routing.ActionDirect
	}

	// «Через VPN только эти сайты» отменяет режим страны целиком (см.
	// effectiveRouting): иначе ru-smart продолжил бы уводить в туннель свой
	// список заблокированного, и человек, назвавший три сайта, получил бы
	// совсем не три. Блок рекламы при этом остаётся — он про то, ЧТО резать,
	// а не про то, что проксировать.
	merged := effectiveRouting(p).Merge(hp)

	return merged.CompiledRules(CarambaSelector)
}

// applyKillSwitch реализует отказ «в закрытую»: убирает DIRECT из фолбэка
// группы-селектора CARAMBA, чтобы при недоступном прокси трафик не утекал мимо
// туннеля. Финальное правило при этом остаётся MATCH,CARAMBA (см. applyRules).
//
// Если после удаления DIRECT в группе не осталось ни одного участника, DIRECT
// возвращается: mihomo отвергает пустую select-группу целиком, и туннель не
// поднялся бы вовсе. Это мягкая деградация, а не обход политики: пустая группа
// означает подписку без узлов, где защищать нечего.
//
// В proxy-режиме не применяется (см. effectiveKillSwitch).
func applyKillSwitch(doc map[string]any, p Policy) {
	if !p.effectiveKillSwitch() {
		return
	}
	groups, ok := doc["proxy-groups"].([]any)
	if !ok {
		return
	}
	for _, g := range groups {
		gm, ok := g.(map[string]any)
		if !ok {
			continue
		}
		if name, _ := gm["name"].(string); name != CarambaSelector {
			continue
		}
		list, ok := gm["proxies"].([]any)
		if !ok {
			continue
		}
		filtered := make([]any, 0, len(list))
		for _, item := range list {
			if s, ok := item.(string); ok && strings.EqualFold(s, "DIRECT") {
				continue
			}
			filtered = append(filtered, item)
		}
		if len(filtered) == 0 {
			continue // группа выродилась бы в пустую — оставляем как есть
		}
		gm["proxies"] = filtered
	}
}

// applyPin фиксирует конкретный узел как выбор по умолчанию: переставляет его
// первым в списке участников группы-селектора CARAMBA.
//
// Почему именно перестановка. У select-группы mihomo нет ключа default: при
// старте Selector.selected = имя первого участника (adapter/outboundgroup:
// NewSelector получает emptyFallback = proxies[0]), и selectedProxy возвращает
// его же, пока приложение не переключит выбор через API. Поэтому «первый в
// списке» и есть выбор по умолчанию.
//
// Пустое имя, отсутствие группы или отсутствие такого участника — без
// изменений (мягкая деградация: пользователь получит автоматический выбор
// вместо ошибки).
func applyPin(doc map[string]any, proxyName string) {
	proxyName = strings.TrimSpace(proxyName)
	if proxyName == "" {
		return
	}
	groups, ok := doc["proxy-groups"].([]any)
	if !ok {
		return
	}
	for _, g := range groups {
		gm, ok := g.(map[string]any)
		if !ok {
			continue
		}
		if name, _ := gm["name"].(string); name != CarambaSelector {
			continue
		}
		list, ok := gm["proxies"].([]any)
		if !ok {
			continue
		}
		idx := -1
		for i, item := range list {
			if s, ok := item.(string); ok && s == proxyName {
				idx = i
				break
			}
		}
		if idx < 0 {
			continue // такого узла в группе нет
		}
		reordered := make([]any, 0, len(list))
		reordered = append(reordered, list[idx])
		reordered = append(reordered, list[:idx]...)
		reordered = append(reordered, list[idx+1:]...)
		gm["proxies"] = reordered
	}
}

// protocolClashType сопоставляет дружелюбное имя протокола с типом прокси в
// clash/mihomo-конфиге (поле type у элемента proxies).
var protocolClashType = map[string]string{
	"AmneziaWG":     "wireguard",
	"VLESS-Reality": "vless",
	"VLESS":         "vless",
	"Hysteria2":     "hysteria2",
	"TUIC":          "tuic",
	"Shadowsocks":   "ss",
}

// CanonicalProtocol приводит имя протокола к каноничной форме, которую понимает
// applyProtocol, и сообщает, известно ли оно. Сравнение регистронезависимо.
//
// Пустая строка и "auto"/"Авто" — валидный ввод «оставить автоматику панели»;
// каноничная форма для них — пустая строка. Неизвестное имя возвращает
// (name, false), чтобы вызывающий мог назвать поле в тексте ошибки.
func CanonicalProtocol(name string) (string, bool) {
	name = strings.TrimSpace(name)
	if name == "" || strings.EqualFold(name, "auto") || strings.EqualFold(name, "Авто") {
		return "", true
	}
	for canonical := range protocolClashType {
		if strings.EqualFold(canonical, name) {
			return canonical, true
		}
	}
	return name, false
}

// CanonicalStack приводит имя сетевого стека TUN к каноничной форме и сообщает,
// известно ли оно. Пустая строка допустима и означает «оставить умолчание»
// (gvisor, см. stackOrDefault).
func CanonicalStack(name string) (Stack, bool) {
	switch strings.ToLower(strings.TrimSpace(name)) {
	case "":
		return "", true
	case string(StackGVisor):
		return StackGVisor, true
	case string(StackSystem):
		return StackSystem, true
	case string(StackMixed):
		return StackMixed, true
	}
	return Stack(name), false
}

// probeURLOf возвращает цель url-test: хост из подписанного пула, если каталог
// его назвал, иначе зашитое умолчание. Прежний безусловный gstatic.com в РФ
// блокируется, из-за чего КАЖДАЯ группа url-test считала все узлы мёртвыми.
func probeURLOf(p Policy) string {
	if u := strings.TrimSpace(p.Bootstrap.ProbeURL); u != "" {
		return u
	}
	return DefaultProbeURL
}

// protoGroupName — имя служебной url-test группы, которую applyProtocol ставит
// первой в селекторе CARAMBA, чтобы зафиксировать протокол.
const protoGroupName = "Caramba-Proto"

// applyProtocol фиксирует протокол подключения: собирает прокси нужного типа в
// отдельную url-test группу и ставит её первой в селекторе CARAMBA (первый
// элемент select — выбор по умолчанию). Узлы и остальные группы не трогаются.
//
// Пусто/"Авто"/"auto" — ничего не меняем (работает Auto-All панели). Неизвестное
// имя или отсутствие прокси такого типа — тоже без изменений (мягкая деградация).
func applyProtocol(doc map[string]any, p Policy) {
	proto := strings.TrimSpace(p.Protocol)
	if proto == "" || strings.EqualFold(proto, "auto") || strings.EqualFold(proto, "Авто") {
		return
	}
	want := protocolClashType[proto]
	if want == "" {
		return
	}

	proxies, ok := doc["proxies"].([]any)
	if !ok {
		return
	}
	var names []any
	for _, pr := range proxies {
		m, ok := pr.(map[string]any)
		if !ok {
			continue
		}
		if t, _ := m["type"].(string); t == want {
			if n, ok := m["name"].(string); ok {
				names = append(names, n)
			}
		}
	}
	if len(names) == 0 {
		return
	}

	groups, ok := doc["proxy-groups"].([]any)
	if !ok {
		return
	}
	// Ставим служебную группу первой в селекторе CARAMBA.
	for _, g := range groups {
		gm, ok := g.(map[string]any)
		if !ok {
			continue
		}
		if name, _ := gm["name"].(string); name == CarambaSelector {
			if list, ok := gm["proxies"].([]any); ok {
				gm["proxies"] = append([]any{protoGroupName}, list...)
			}
		}
	}
	// Добавляем саму url-test группу выбранного протокола.
	protoGroup := map[string]any{
		"name":      protoGroupName,
		"type":      "url-test",
		"proxies":   names,
		"url":       probeURLOf(p),
		"interval":  180,
		"tolerance": 50,
	}
	doc["proxy-groups"] = append(groups, protoGroup)
}
