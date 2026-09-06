package subimport

import (
	"encoding/json"
	"fmt"
	"strings"
)

// parseSingbox разбирает конфиг sing-box (JSON) и преобразует массив outbounds в
// clash proxy-map'ы. Служебные outbounds (direct/block/dns/selector/urltest)
// пропускаются. Каждый поддерживаемый тип маппится на соответствующий clash type
// (contract E).
//
// sing-box и clash расходятся в именовании ключей (snake_case против kebab-case,
// congestion_control против congestion-controller, server_port против port),
// поэтому это явная перекладка поле-в-поле, а не passthrough.
func parseSingbox(raw []byte) (proxyList, error) {
	var doc struct {
		Outbounds []map[string]any `json:"outbounds"`
	}
	if err := json.Unmarshal(raw, &doc); err != nil {
		return nil, fmt.Errorf("subimport: разбор sing-box JSON: %w", err)
	}
	out := make(proxyList, 0, len(doc.Outbounds))
	for _, ob := range doc.Outbounds {
		px := singboxOutboundToClash(ob)
		if px != nil {
			out = append(out, px)
		}
	}
	if len(out) == 0 {
		return nil, fmt.Errorf("subimport: sing-box конфиг не дал ни одного прокси")
	}
	dropDanglingDialerProxy(out)
	return out, nil
}

// dropDanglingDialerProxy убирает ссылки dialer-proxy в никуда.
//
// sing-box отправляет detour и в служебные outbound'ы (direct/block), которых в
// clash-списке нет по определению. Ссылка на несуществующее имя не ошибка
// конфига: mihomo её проглотит при загрузке и провалит КАЖДЫЙ набор через этот
// узел уже в бою. Дешевле снять её здесь и набирать напрямую — ровно так узел
// и вёл себя до переноса detour.
func dropDanglingDialerProxy(list proxyList) {
	names := make(map[string]struct{}, len(list))
	for _, px := range list {
		if n := asString(px["name"]); n != "" {
			names[n] = struct{}{}
		}
	}
	for _, px := range list {
		target := asString(px["dialer-proxy"])
		if target == "" {
			continue
		}
		if _, ok := names[target]; !ok {
			delete(px, "dialer-proxy")
		}
	}
}

// singboxOutboundToClash маппит один sing-box outbound на clash proxy-map и
// переносит detour.
//
// detour в sing-box означает «этот outbound НАБИРАТЬ ЧЕРЕЗ тот». У mihomo то же
// самое называется dialer-proxy. Поле молча терялось, и цепочка «через 🇷🇺»
// превращалась в прямой набор адреса выхода — другой маршрут под тем же именем.
// Ссылка проверяется позже, в parseSingbox: sing-box часто отправляет detour в
// служебный direct, которого в clash-списке нет.
func singboxOutboundToClash(ob map[string]any) map[string]any {
	px := singboxOutboundBody(ob)
	if px == nil {
		return nil
	}
	putNonEmptyString(px, "dialer-proxy", asString(ob["detour"]))
	return px
}

// singboxOutboundBody строит сам proxy-map по типу outbound'а.
// Возвращает nil для служебных/неподдерживаемых типов.
func singboxOutboundBody(ob map[string]any) map[string]any {
	typ := strings.ToLower(asString(ob["type"]))
	tag := asString(ob["tag"])
	server := asString(ob["server"])
	port := asInt(ob["server_port"])
	name := nameOr(tag, server, port)

	base := func(clashType string) map[string]any {
		return map[string]any{
			"name":   name,
			"type":   clashType,
			"server": server,
			"port":   port,
		}
	}

	switch typ {
	case "vless":
		px := base("vless")
		px["uuid"] = asString(ob["uuid"])
		px["udp"] = true
		putNonEmptyString(px, "flow", asString(ob["flow"]))
		applySingboxTLS(px, ob, "vless")
		applySingboxTransport(px, ob)
		return px

	case "vmess":
		px := base("vmess")
		px["uuid"] = asString(ob["uuid"])
		px["alterId"] = asInt(ob["alter_id"])
		px["cipher"] = firstNonEmpty(asString(ob["security"]), "auto")
		px["udp"] = true
		applySingboxTLS(px, ob, "vmess")
		applySingboxTransport(px, ob)
		return px

	case "trojan":
		px := base("trojan")
		px["password"] = asString(ob["password"])
		px["udp"] = true
		applySingboxTLS(px, ob, "trojan")
		applySingboxTransport(px, ob)
		return px

	case "shadowsocks":
		px := base("ss")
		px["cipher"] = asString(ob["method"])
		px["password"] = asString(ob["password"])
		px["udp"] = true
		applySingboxSSPlugin(px, ob)
		return px

	case "hysteria2":
		px := base("hysteria2")
		px["password"] = asString(ob["password"])
		if obfs, ok := ob["obfs"].(map[string]any); ok {
			putNonEmptyString(px, "obfs", asString(obfs["type"]))
			putNonEmptyString(px, "obfs-password", asString(obfs["password"]))
		}
		applySingboxTLS(px, ob, "hysteria2")
		return px

	case "tuic":
		px := base("tuic")
		px["uuid"] = asString(ob["uuid"])
		px["password"] = asString(ob["password"])
		px["congestion-controller"] = firstNonEmpty(asString(ob["congestion_control"]), "bbr")
		applySingboxTLS(px, ob, "tuic")
		return px

	case "wireguard":
		return singboxWireguardToClash(ob, name, server, port)

	case "naive":
		// Тип остаётся naive, хотя ядро его не строит. Подмена на type:http
		// была ложью дважды: экран называл чужой протокол, а отказ объяснял
		// чужой причиной («вход не принял ключ» вместо «ядро не умеет
		// Naive»). Из конфига для ядра такой прокси убирается на сборке
		// профиля, поэтому туннель от честного имени не ломается.
		px := base("naive")
		px["tls"] = true
		putNonEmptyString(px, "username", asString(ob["username"]))
		putNonEmptyString(px, "password", asString(ob["password"]))
		applySingboxTLS(px, ob, "naive")
		return px

	default:
		// direct/block/dns/selector/urltest и неизвестные типы — пропускаем.
		return nil
	}
}

// singboxWireguardToClash маппит sing-box wireguard (включая AmneziaWG-поля) на
// clash type:wireguard. h1..h4 кладутся СТРОКАМИ (иначе mihomo деградирует).
func singboxWireguardToClash(ob map[string]any, name, server string, port int) map[string]any {
	px := map[string]any{
		"name":        name,
		"type":        "wireguard",
		"server":      server,
		"port":        port,
		"private-key": asString(ob["private_key"]),
		"public-key":  asString(ob["peer_public_key"]),
		"udp":         true,
	}
	// local_address в sing-box — массив CIDR; clash ждёт ip (первый /32).
	if addrs, ok := ob["local_address"].([]any); ok && len(addrs) > 0 {
		px["ip"] = asString(addrs[0])
	} else if s := asString(ob["local_address"]); s != "" {
		px["ip"] = s
	}
	if mtu := asInt(ob["mtu"]); mtu > 0 {
		px["mtu"] = mtu
	} else {
		px["mtu"] = 1280
	}
	putNonEmptyString(px, "pre-shared-key", asString(ob["pre_shared_key"]))

	// AmneziaWG-обфускация: jc/jmin/jmax/s1..s4 (int), h1..h4 (string).
	opt := map[string]any{}
	for _, key := range []string{"jc", "jmin", "jmax", "s1", "s2", "s3", "s4"} {
		if v, ok := ob[key]; ok && v != nil {
			opt[key] = asInt(v)
		}
	}
	for _, key := range []string{"h1", "h2", "h3", "h4"} {
		if v, ok := ob[key]; ok && v != nil {
			opt[key] = asString(v)
		}
	}
	if len(opt) > 0 {
		px["amnezia-wg-option"] = opt
	}
	return px
}

// applySingboxTLS переносит секцию tls{} sing-box в clash-поля (tls/servername/
// alpn/skip-cert-verify/reality-opts/client-fingerprint).
func applySingboxTLS(px map[string]any, ob map[string]any, proto string) {
	tls, ok := ob["tls"].(map[string]any)
	if !ok || !asBool(tls["enabled"]) {
		return
	}
	// hysteria2/tuic несут TLS неявно — у них нет ключа tls/servername в clash,
	// но sni переносится в sni; для vless/vmess/trojan ключ servername.
	sni := asString(tls["server_name"])
	switch proto {
	case "hysteria2", "tuic":
		putNonEmptyString(px, "sni", sni)
	default:
		px["tls"] = true
		putNonEmptyString(px, "servername", sni)
	}
	if asBool(tls["insecure"]) {
		px["skip-cert-verify"] = true
	}
	if alpn, ok := tls["alpn"].([]any); ok && len(alpn) > 0 {
		px["alpn"] = alpn
	}
	if utls, ok := tls["utls"].(map[string]any); ok {
		putNonEmptyString(px, "client-fingerprint", asString(utls["fingerprint"]))
	}
	if reality, ok := tls["reality"].(map[string]any); ok && asBool(reality["enabled"]) {
		ro := map[string]any{}
		putNonEmptyString(ro, "public-key", asString(reality["public_key"]))
		putNonEmptyString(ro, "short-id", asString(reality["short_id"]))
		if len(ro) > 0 {
			px["reality-opts"] = ro
		}
	}
}

// applySingboxTransport переносит секцию transport{} sing-box в clash network +
// *-opts (ws/grpc/httpupgrade).
func applySingboxTransport(px map[string]any, ob map[string]any) {
	tr, ok := ob["transport"].(map[string]any)
	if !ok {
		return
	}
	switch strings.ToLower(asString(tr["type"])) {
	case "ws":
		px["network"] = "ws"
		opts := map[string]any{}
		putNonEmptyString(opts, "path", firstNonEmpty(asString(tr["path"]), "/"))
		if hdrs, ok := tr["headers"].(map[string]any); ok {
			if host := asString(hdrs["Host"]); host != "" {
				opts["headers"] = map[string]any{"Host": host}
			}
		}
		px["ws-opts"] = opts
	case "grpc":
		px["network"] = "grpc"
		px["grpc-opts"] = map[string]any{"grpc-service-name": asString(tr["service_name"])}
	case "httpupgrade":
		// В sing-box это законный transport.type, у mihomo — ws с флагом
		// v2ray-http-upgrade. Пока сюда писалось «httpupgrade», адаптер
		// СТРОИЛСЯ (отказа нет), но шёл голым TCP+TLS, и целое семейство
		// входов молча не отвечало на замер, прячась за общей строкой VLESS.
		px["network"] = "ws"
		opts := map[string]any{"v2ray-http-upgrade": true}
		putNonEmptyString(opts, "path", firstNonEmpty(asString(tr["path"]), "/"))
		host := asString(tr["host"])
		if host == "" {
			if hdrs, ok := tr["headers"].(map[string]any); ok {
				host = asString(hdrs["Host"])
			}
		}
		if host != "" {
			opts["headers"] = map[string]any{"Host": host}
		}
		px["ws-opts"] = opts
	}
}

// applySingboxSSPlugin переносит shadowtls/obfs плагин shadowsocks из sing-box.
// В sing-box ShadowTLS — отдельный outbound (detour), но некоторые конфиги несут
// inline plugin-поля; поддерживаем простую inline-форму.
func applySingboxSSPlugin(px map[string]any, ob map[string]any) {
	plugin := asString(ob["plugin"])
	if plugin == "" {
		return
	}
	opts := map[string]any{}
	if po, ok := ob["plugin_opts"].(map[string]any); ok {
		for k, v := range po {
			opts[k] = v
		}
	}
	switch {
	case strings.Contains(plugin, "shadow-tls"):
		px["plugin"] = "shadow-tls"
	case strings.Contains(plugin, "obfs"):
		px["plugin"] = "obfs"
	default:
		px["plugin"] = plugin
	}
	if len(opts) > 0 {
		px["plugin-opts"] = opts
	}
}
