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
	Tun TunConfig
	DNS DNSConfig
	// KillSwitch: при включении весь нераспознанный трафик в туннеле, который
	// не может выйти через прокси, отбрасывается (REJECT) вместо утечки в
	// DIRECT. Достигается заменой финального fallback-правила.
	KillSwitch bool
	Split      SplitTunnel
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
		Tun: TunConfig{
			Enable:     true,
			Stack:      StackGVisor,
			DeviceName: "caramba-tun",
			MTU:        1280,
			AutoRoute:  true,
			DNSHijack:  []string{"any:53"},
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

// AssembleMihomoConfig принимает сырой mihomo YAML из подписки и возвращает
// новый YAML с применённой локальной политикой. Узлы (proxies/proxy-groups)
// сохраняются как есть.
func AssembleMihomoConfig(rawYAML []byte, policy Policy) ([]byte, error) {
	// Разбираем в общую карту, чтобы не терять неизвестные ключи панели.
	doc := map[string]any{}
	if err := yaml.Unmarshal(rawYAML, &doc); err != nil {
		return nil, fmt.Errorf("profile: разбор конфигурации подписки: %w", err)
	}

	applyGeneral(doc, policy)
	applyTun(doc, policy.Tun)
	applyDNS(doc, policy.DNS)
	applyRuleProviders(doc, policy)
	applyRules(doc, policy)
	applyProtocol(doc, policy.Protocol)

	out, err := yaml.Marshal(doc)
	if err != nil {
		return nil, fmt.Errorf("profile: сериализация конфигурации: %w", err)
	}
	return out, nil
}

// applyGeneral переопределяет верхнеуровневые флаги.
func applyGeneral(doc map[string]any, p Policy) {
	doc["allow-lan"] = p.AllowLAN
	doc["mode"] = "rule"
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

func stackOrDefault(s Stack) Stack {
	if s == "" {
		return StackGVisor
	}
	return s
}

// applyDNS формирует секцию dns.
func applyDNS(doc map[string]any, d DNSConfig) {
	if !d.Enable {
		return
	}
	dns := map[string]any{
		"enable": true,
		"ipv6":   false,
	}
	if d.Listen != "" {
		dns["listen"] = d.Listen
	}
	if d.EnhancedMode != "" {
		dns["enhanced-mode"] = d.EnhancedMode
	}
	if d.FakeIPRange != "" {
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
// «умной» маршрутизации (пресет + пользовательские правила), иначе — прежнее
// поведение (geo-CN + split-tunnel).
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

	// Национальный трафик напрямую (как в конфиге панели).
	rules = append(rules,
		"GEOIP,CN,DIRECT",
		"GEOSITE,cn,DIRECT",
		"GEOIP,private,DIRECT,no-resolve",
	)

	// Allow-list: только перечисленные процессы идут в туннель, остальные —
	// напрямую (правило-«ловушка» перед финальным MATCH).
	finalTarget := CarambaSelector
	if p.KillSwitch {
		// При kill-switch нераспознанный трафик отбрасывается, а не утекает.
		finalTarget = "REJECT"
	}

	if len(p.Split.AllowProcesses) > 0 {
		for _, proc := range p.Split.AllowProcesses {
			rules = append(rules, fmt.Sprintf("PROCESS-NAME,%s,%s", proc, CarambaSelector))
		}
		// Всё, что не в allow-list, идёт напрямую (split-tunnel) — но если
		// включён kill-switch, политика всё равно отбрасывает неизвестное.
		if p.KillSwitch {
			rules = append(rules, "MATCH,REJECT")
		} else {
			rules = append(rules, "MATCH,DIRECT")
		}
	} else {
		rules = append(rules, "MATCH,"+finalTarget)
	}

	doc["rules"] = rules
}

// compileSmartRules строит правила из активного пресета маршрутизации,
// накладывая поверх него пользовательский split-tunnel и kill-switch.
//
// Приоритет (сверху вниз): байпас-домены → байпас-процессы → allow-процессы →
// правила пресета → финальный MATCH. Kill-switch меняет финал PROXY→REJECT,
// чтобы при отсутствии прокси нераспознанный трафик не утекал.
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

	merged := p.Routing.Merge(hp)

	// Kill-switch: если по умолчанию весь трафик идёт в прокси, при разрыве
	// туннеля нераспознанное должно блокироваться, а не утекать в DIRECT.
	if p.KillSwitch && merged.FinalAction == routing.ActionProxy {
		merged.FinalAction = routing.ActionReject
	}

	return merged.CompiledRules(CarambaSelector)
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
