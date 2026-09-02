// Package profile собирает готовый mihomo-конфиг из подписки панели, применяя
// локальную политику клиента: TUN-режим, kill-switch, split-tunnel (списки
// разрешённых/запрещённых приложений и доменов) и настройки DNS.
//
// На вход подаётся сырой clash/mihomo YAML, отданный панелью (он уже содержит
// proxies/proxy-groups, включая amnezia-wg). Мы дополняем/переопределяем
// верхнеуровневые секции и список rules, не трогая узлы.
package profile

import (
	"fmt"
	"strings"

	"github.com/semanticparadox/caramba/libs/caramba-core/routing"
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

// ProxyConfig описывает inbound в режиме ModeProxy.
type ProxyConfig struct {
	// MixedPort — порт mixed-инбаунда. 0 → DefaultMixedPort.
	MixedPort int
	// BindAddress — адрес привязки ("127.0.0.1" или "*"). Пусто →
	// DefaultBindAddress. Ядро учитывает его ТОЛЬКО при AllowLAN.
	BindAddress string
	// AllowLAN открывает mixed-инбаунд для локальной сети.
	AllowLAN bool
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
	// FallbackNameservers — резолверы для прямого/национального трафика.
	FallbackNameservers []string
	// EnhancedMode — "fake-ip" или "redir-host".
	EnhancedMode string
	// FakeIPRange — диапазон fake-ip, например "198.18.0.1/16".
	FakeIPRange string
}

// SplitTunnel описывает раздельное туннелирование.
//
// Семантика:
//   - BypassDomains/BypassProcesses всегда идут напрямую (DIRECT) мимо туннеля.
//   - Если задан AllowProcesses (allow-list), то ТОЛЬКО эти процессы идут в
//     туннель, остальные — напрямую.
type SplitTunnel struct {
	BypassDomains   []string
	BypassProcesses []string
	AllowProcesses  []string
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
	// KillSwitch: при включении весь нераспознанный трафик в туннеле, который
	// не может выйти через прокси, отбрасывается (REJECT) вместо утечки в
	// DIRECT. Достигается заменой финального fallback-правила.
	KillSwitch bool
	Split      SplitTunnel
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
}

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
			Enable:              true,
			Listen:              "127.0.0.1:1053",
			Nameservers:         []string{"https://1.1.1.1/dns-query", "https://8.8.8.8/dns-query"},
			FallbackNameservers: []string{"tls://1.1.1.1:853"},
			EnhancedMode:        "fake-ip",
			FakeIPRange:         "198.18.0.1/16",
		},
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

	applyGeneral(doc, policy)
	if policy.EffectiveMode() == ModeProxy {
		applyProxyInbound(doc, policy)
	} else {
		applyTun(doc, policy.Tun)
	}
	applyDNS(doc, policy)
	applyRuleProviders(doc, policy)
	applyRules(doc, policy)
	applyProtocol(doc, policy.Protocol)
	applyPin(doc, pinProxy)
	applyKillSwitch(doc, policy)

	out, err := yaml.Marshal(doc)
	if err != nil {
		return nil, fmt.Errorf("profile: сериализация конфигурации: %w", err)
	}
	return out, nil
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
	doc["dns"] = dns
}

// applyRuleProviders формирует секцию rule-providers из активного пресета
// маршрутизации (если он задан). Без Routing секция не трогается.
func applyRuleProviders(doc map[string]any, p Policy) {
	if p.Routing == nil {
		return
	}
	if providers := p.Routing.CompiledProviders(); providers != nil {
		doc["rule-providers"] = providers
	}
}

// applyRules собирает список rules. Если задан Routing — используется движок
// «умной» маршрутизации (пресет + пользовательские правила), иначе — базовый
// набор без страновых geo-правил.
//
// Kill-switch НЕ превращает финал в MATCH,REJECT: финальное правило всегда
// ведёт в селектор CARAMBA (иначе при включённом kill-switch клиент просто
// терял бы весь трафик, даже когда прокси жив). Отказ «в закрытую» обеспечивает
// applyKillSwitch, который убирает DIRECT из фолбэка селектора. Единственное
// место, где REJECT уместен, — allow-list split: трафик ВНЕ списка при
// kill-switch отбрасывается, а без него идёт напрямую.
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

	if len(p.Split.AllowProcesses) > 0 {
		// Allow-list: в туннель идут ТОЛЬКО перечисленные процессы.
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
	for _, proc := range p.Split.AllowProcesses {
		hp.Rules = append(hp.Rules, routing.Rule{
			Type: routing.MatchProcessName, Value: proc, Action: routing.ActionProxy,
		})
	}
	if len(p.Split.AllowProcesses) > 0 {
		// Allow-list перебивает финал пресета: вне списка — REJECT при
		// kill-switch, иначе DIRECT.
		if p.effectiveKillSwitch() {
			hp.FinalAction = routing.ActionReject
		} else {
			hp.FinalAction = routing.ActionDirect
		}
	}

	merged := p.Routing.Merge(hp)

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

// protoGroupName — имя служебной url-test группы, которую applyProtocol ставит
// первой в селекторе CARAMBA, чтобы зафиксировать протокол.
const protoGroupName = "Caramba-Proto"

// applyProtocol фиксирует протокол подключения: собирает прокси нужного типа в
// отдельную url-test группу и ставит её первой в селекторе CARAMBA (первый
// элемент select — выбор по умолчанию). Узлы и остальные группы не трогаются.
//
// Пусто/"Авто"/"auto" — ничего не меняем (работает Auto-All панели). Неизвестное
// имя или отсутствие прокси такого типа — тоже без изменений (мягкая деградация).
func applyProtocol(doc map[string]any, proto string) {
	proto = strings.TrimSpace(proto)
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
		"url":       "https://www.gstatic.com/generate_204",
		"interval":  180,
		"tolerance": 50,
	}
	doc["proxy-groups"] = append(groups, protoGroup)
}
