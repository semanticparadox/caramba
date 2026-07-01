package subimport

import (
	"encoding/json"
	"fmt"
	"net/url"
	"strconv"
	"strings"
)

// parseURIList разбирает текст, в котором каждая непустая строка — отдельный URI,
// и возвращает по одному proxy-map на каждую распознанную строку. Нераспознанные
// строки пропускаются. Ошибка возвращается только если не удалось разобрать ни
// одной строки.
func parseURIList(raw []byte) (proxyList, error) {
	lines := strings.FieldsFunc(string(raw), func(r rune) bool { return r == '\n' || r == '\r' })
	out := make(proxyList, 0, len(lines))
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" || !hasKnownScheme(line) {
			continue
		}
		px, err := parseURI(line)
		if err != nil || px == nil {
			continue
		}
		out = append(out, px)
	}
	if len(out) == 0 {
		return nil, fmt.Errorf("subimport: ни один URI не распознан")
	}
	return out, nil
}

// parseURI разбирает одиночный URI в clash proxy-map по схеме. Возвращает nil без
// ошибки для пустой строки; ошибку — для нераспознанной/битой схемы.
func parseURI(raw string) (map[string]any, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return nil, nil
	}
	scheme := strings.ToLower(schemeOf(raw))
	switch scheme {
	case "vless":
		return parseVlessURI(raw)
	case "vmess":
		return parseVmessURI(raw)
	case "trojan":
		return parseTrojanURI(raw)
	case "ss":
		return parseShadowsocksURI(raw)
	case "hysteria2", "hy2":
		return parseHysteria2URI(raw)
	case "tuic":
		return parseTuicURI(raw)
	case "wireguard", "wg":
		return parseWireguardURI(raw)
	case "naive+https", "naive":
		return parseNaiveURI(raw)
	default:
		return nil, fmt.Errorf("subimport: неподдерживаемая схема URI %q", scheme)
	}
}

// schemeOf возвращает часть до "://".
func schemeOf(raw string) string {
	if i := strings.Index(raw, "://"); i > 0 {
		return raw[:i]
	}
	return ""
}

// fragmentName декодирует fragment URL как имя прокси (часто percent-encoded).
func fragmentName(u *url.URL) string {
	if u.Fragment == "" {
		return ""
	}
	if dec, err := url.QueryUnescape(u.Fragment); err == nil {
		return dec
	}
	return u.Fragment
}

// --- VLESS ---

// parseVlessURI разбирает vless://uuid@host:port?params#name. Транспорт берётся
// из ?type= (tcp/ws/grpc/httpupgrade), security — из ?security= (tls/reality).
// Reality добавляет reality-opts (public-key/short-id) и flow (xtls-rprx-vision).
func parseVlessURI(raw string) (map[string]any, error) {
	u, err := url.Parse(raw)
	if err != nil {
		return nil, fmt.Errorf("subimport: разбор vless URI: %w", err)
	}
	uuid := u.User.Username()
	host := u.Hostname()
	port := atoiPort(u.Port())
	q := u.Query()

	px := map[string]any{
		"type":   "vless",
		"name":   nameOr(fragmentName(u), host, port),
		"server": host,
		"port":   port,
		"uuid":   uuid,
		"udp":    true,
	}
	if v := q.Get("flow"); v != "" {
		px["flow"] = v
	}
	if fp := q.Get("fp"); fp != "" {
		px["client-fingerprint"] = fp
	}

	security := strings.ToLower(q.Get("security"))
	sni := firstNonEmpty(q.Get("sni"), q.Get("peer"), q.Get("host"))
	switch security {
	case "tls":
		px["tls"] = true
		putNonEmptyString(px, "servername", sni)
	case "reality":
		px["tls"] = true
		putNonEmptyString(px, "servername", sni)
		reality := map[string]any{}
		putNonEmptyString(reality, "public-key", q.Get("pbk"))
		putNonEmptyString(reality, "short-id", q.Get("sid"))
		if len(reality) > 0 {
			px["reality-opts"] = reality
		}
		// reality по умолчанию требует vision-flow, если не задан явно.
		if _, ok := px["flow"]; !ok {
			px["flow"] = "xtls-rprx-vision"
		}
	}
	if alpn := q.Get("alpn"); alpn != "" {
		px["alpn"] = splitCSVList(alpn)
	}

	applyTransport(px, q)
	return px, nil
}

// --- VMess ---

// parseVmessURI разбирает vmess://base64(json). Тело — JSON формата v2rayN
// ({v,ps,add,port,id,aid,net,type,host,path,tls,sni,...}). Также поддерживается
// «raw» форма vmess://uuid@host:port?params#name (как у vless) на случай
// нестандартных генераторов.
func parseVmessURI(raw string) (map[string]any, error) {
	body := strings.TrimPrefix(raw, "vmess://")
	body = strings.TrimPrefix(body, "VMESS://")

	if dec, ok := tryBase64(body); ok && strings.HasPrefix(strings.TrimSpace(string(dec)), "{") {
		return vmessFromJSON(dec)
	}
	// Fallback: URI-форма с user@host.
	return vmessFromURI(raw)
}

// vmessFromJSON строит proxy-map из v2rayN JSON.
func vmessFromJSON(dec []byte) (map[string]any, error) {
	var j map[string]any
	if err := json.Unmarshal(dec, &j); err != nil {
		return nil, fmt.Errorf("subimport: разбор vmess JSON: %w", err)
	}
	host := asString(j["add"])
	port := asInt(j["port"])
	px := map[string]any{
		"type":    "vmess",
		"name":    nameOr(asString(j["ps"]), host, port),
		"server":  host,
		"port":    port,
		"uuid":    asString(j["id"]),
		"alterId": asInt(j["aid"]),
		"cipher":  firstNonEmpty(asString(j["scy"]), "auto"),
		"udp":     true,
	}
	if tls := strings.ToLower(asString(j["tls"])); tls == "tls" || asBool(j["tls"]) {
		px["tls"] = true
		putNonEmptyString(px, "servername", firstNonEmpty(asString(j["sni"]), asString(j["host"])))
	}

	net := strings.ToLower(asString(j["net"]))
	switch net {
	case "ws":
		opts := map[string]any{}
		putNonEmptyString(opts, "path", firstNonEmpty(asString(j["path"]), "/"))
		if h := asString(j["host"]); h != "" {
			opts["headers"] = map[string]any{"Host": h}
		}
		px["network"] = "ws"
		px["ws-opts"] = opts
	case "grpc":
		px["network"] = "grpc"
		px["grpc-opts"] = map[string]any{"grpc-service-name": asString(j["path"])}
	case "h2", "httpupgrade":
		px["network"] = "httpupgrade"
		opts := map[string]any{}
		putNonEmptyString(opts, "path", asString(j["path"]))
		putNonEmptyString(opts, "host", asString(j["host"]))
		px["http-upgrade-opts"] = opts
	default:
		px["network"] = "tcp"
	}
	return px, nil
}

// vmessFromURI разбирает нестандартную vmess://uuid@host:port?params форму.
func vmessFromURI(raw string) (map[string]any, error) {
	u, err := url.Parse(raw)
	if err != nil {
		return nil, fmt.Errorf("subimport: разбор vmess URI: %w", err)
	}
	host := u.Hostname()
	port := atoiPort(u.Port())
	q := u.Query()
	px := map[string]any{
		"type":    "vmess",
		"name":    nameOr(fragmentName(u), host, port),
		"server":  host,
		"port":    port,
		"uuid":    u.User.Username(),
		"alterId": 0,
		"cipher":  "auto",
		"udp":     true,
	}
	if strings.EqualFold(q.Get("security"), "tls") {
		px["tls"] = true
		putNonEmptyString(px, "servername", firstNonEmpty(q.Get("sni"), q.Get("host")))
	}
	applyTransport(px, q)
	return px, nil
}

// --- Trojan ---

// parseTrojanURI разбирает trojan://password@host:port?sni=&type=#name.
func parseTrojanURI(raw string) (map[string]any, error) {
	u, err := url.Parse(raw)
	if err != nil {
		return nil, fmt.Errorf("subimport: разбор trojan URI: %w", err)
	}
	host := u.Hostname()
	port := atoiPort(u.Port())
	q := u.Query()
	px := map[string]any{
		"type":     "trojan",
		"name":     nameOr(fragmentName(u), host, port),
		"server":   host,
		"port":     port,
		"password": u.User.Username(),
		"udp":      true,
	}
	putNonEmptyString(px, "sni", firstNonEmpty(q.Get("sni"), q.Get("peer"), q.Get("host")))
	if fp := q.Get("fp"); fp != "" {
		px["client-fingerprint"] = fp
	}
	if alpn := q.Get("alpn"); alpn != "" {
		px["alpn"] = splitCSVList(alpn)
	}
	if asBool(q.Get("allowInsecure")) || asBool(q.Get("insecure")) {
		px["skip-cert-verify"] = true
	}
	if strings.EqualFold(q.Get("security"), "reality") {
		reality := map[string]any{}
		putNonEmptyString(reality, "public-key", q.Get("pbk"))
		putNonEmptyString(reality, "short-id", q.Get("sid"))
		if len(reality) > 0 {
			px["reality-opts"] = reality
		}
	}
	applyTransport(px, q)
	return px, nil
}

// --- Shadowsocks ---

// parseShadowsocksURI разбирает ss://. Поддерживаются обе формы:
//
//	SIP002:  ss://base64(method:password)@host:port?plugin=...#name
//	legacy:  ss://base64(method:password@host:port)#name
//
// ShadowTLS подключается через ?plugin=shadow-tls;host=...;password=...;version=...
func parseShadowsocksURI(raw string) (map[string]any, error) {
	body := strings.TrimPrefix(raw, "ss://")
	body = strings.TrimPrefix(body, "SS://")

	var name string
	if i := strings.Index(body, "#"); i >= 0 {
		if dec, err := url.QueryUnescape(body[i+1:]); err == nil {
			name = dec
		} else {
			name = body[i+1:]
		}
		body = body[:i]
	}

	var query string
	if i := strings.Index(body, "?"); i >= 0 {
		query = body[i+1:]
		body = body[:i]
	}

	method, password, host, port, err := decodeSSUserHost(body)
	if err != nil {
		return nil, err
	}

	px := map[string]any{
		"type":     "ss",
		"name":     nameOr(name, host, port),
		"server":   host,
		"port":     port,
		"cipher":   method,
		"password": password,
		"udp":      true,
	}
	applySSPlugin(px, query)
	return px, nil
}

// decodeSSUserHost извлекает method/password/host/port из тела ss-URI (обе формы).
func decodeSSUserHost(body string) (method, password, host string, port int, err error) {
	if at := strings.LastIndex(body, "@"); at >= 0 {
		// SIP002: userinfo (base64 method:password) @ host:port.
		userinfo := body[:at]
		hostport := body[at+1:]
		if dec, ok := tryBase64(userinfo); ok {
			method, password = splitColon(string(dec))
		} else if dec, derr := url.QueryUnescape(userinfo); derr == nil {
			method, password = splitColon(dec)
		} else {
			method, password = splitColon(userinfo)
		}
		host, port = splitHostPort(hostport)
	} else {
		// legacy: всё тело — base64(method:password@host:port).
		dec, ok := tryBase64(body)
		if !ok {
			return "", "", "", 0, fmt.Errorf("subimport: ss-URI: не удалось декодировать тело")
		}
		s := string(dec)
		at2 := strings.LastIndex(s, "@")
		if at2 < 0 {
			return "", "", "", 0, fmt.Errorf("subimport: ss-URI: нет '@' в декодированном теле")
		}
		method, password = splitColon(s[:at2])
		host, port = splitHostPort(s[at2+1:])
	}
	if host == "" {
		return "", "", "", 0, fmt.Errorf("subimport: ss-URI: пустой host")
	}
	return method, password, host, port, nil
}

// applySSPlugin добавляет plugin/plugin-opts (в т.ч. shadow-tls) из query.
func applySSPlugin(px map[string]any, query string) {
	if query == "" {
		return
	}
	q, _ := url.ParseQuery(query)
	plugin := q.Get("plugin")
	if plugin == "" {
		return
	}
	// plugin приходит формой "shadow-tls;host=...;password=...;version=3".
	parts := strings.Split(plugin, ";")
	name := parts[0]
	opts := map[string]any{}
	for _, p := range parts[1:] {
		kv := strings.SplitN(p, "=", 2)
		if len(kv) != 2 {
			continue
		}
		switch kv[0] {
		case "host", "sni":
			opts["host"] = kv[1]
		case "password", "passwd":
			opts["password"] = kv[1]
		case "version":
			opts["version"] = asInt(kv[1])
		}
	}
	switch {
	case strings.Contains(name, "shadow-tls"):
		px["plugin"] = "shadow-tls"
		if len(opts) > 0 {
			px["plugin-opts"] = opts
		}
	case strings.Contains(name, "obfs"):
		px["plugin"] = "obfs"
		if len(opts) > 0 {
			px["plugin-opts"] = opts
		}
	}
}

// --- Hysteria2 ---

// parseHysteria2URI разбирает hysteria2://password@host:port?sni=&obfs=#name
// (и алиас hy2://).
func parseHysteria2URI(raw string) (map[string]any, error) {
	u, err := url.Parse(raw)
	if err != nil {
		return nil, fmt.Errorf("subimport: разбор hysteria2 URI: %w", err)
	}
	host := u.Hostname()
	port := atoiPort(u.Port())
	q := u.Query()
	password := u.User.Username()
	if pw, ok := u.User.Password(); ok && pw != "" {
		// hysteria2://user:pass@ — некоторые генераторы кладут пароль во вторую часть.
		password = password + ":" + pw
	}
	px := map[string]any{
		"type":     "hysteria2",
		"name":     nameOr(fragmentName(u), host, port),
		"server":   host,
		"port":     port,
		"password": password,
	}
	putNonEmptyString(px, "sni", firstNonEmpty(q.Get("sni"), q.Get("peer")))
	if asBool(q.Get("insecure")) || asBool(q.Get("allowInsecure")) {
		px["skip-cert-verify"] = true
	}
	if obfs := q.Get("obfs"); obfs != "" {
		px["obfs"] = obfs
		putNonEmptyString(px, "obfs-password", q.Get("obfs-password"))
	}
	if alpn := q.Get("alpn"); alpn != "" {
		px["alpn"] = splitCSVList(alpn)
	}
	return px, nil
}

// --- TUIC v5 ---

// parseTuicURI разбирает tuic://uuid:password@host:port?congestion_control=&alpn=#name.
// В clash ключ — congestion-controller (sing-box использует congestion_control).
func parseTuicURI(raw string) (map[string]any, error) {
	u, err := url.Parse(raw)
	if err != nil {
		return nil, fmt.Errorf("subimport: разбор tuic URI: %w", err)
	}
	host := u.Hostname()
	port := atoiPort(u.Port())
	q := u.Query()
	uuid := u.User.Username()
	password, _ := u.User.Password()
	px := map[string]any{
		"type":     "tuic",
		"name":     nameOr(fragmentName(u), host, port),
		"server":   host,
		"port":     port,
		"uuid":     uuid,
		"password": password,
	}
	putNonEmptyString(px, "sni", q.Get("sni"))
	if cc := firstNonEmpty(q.Get("congestion_control"), q.Get("congestion-controller")); cc != "" {
		px["congestion-controller"] = cc
	} else {
		px["congestion-controller"] = "bbr"
	}
	if asBool(q.Get("allow_insecure")) || asBool(q.Get("insecure")) {
		px["skip-cert-verify"] = true
	}
	if alpn := q.Get("alpn"); alpn != "" {
		px["alpn"] = splitCSVList(alpn)
	} else {
		px["alpn"] = []any{"h3"}
	}
	return px, nil
}

// --- WireGuard / AmneziaWG ---

// parseWireguardURI разбирает wireguard://private-key@host:port?params#name (и
// алиас wg://). Если присутствуют jc/jmin/jmax/s1..s4/h1..h4 — добавляется
// amnezia-wg-option (h1..h4 как СТРОКИ — иначе mihomo деградирует до plain WG).
func parseWireguardURI(raw string) (map[string]any, error) {
	u, err := url.Parse(raw)
	if err != nil {
		return nil, fmt.Errorf("subimport: разбор wireguard URI: %w", err)
	}
	host := u.Hostname()
	port := atoiPort(u.Port())
	q := u.Query()

	privateKey := u.User.Username()
	if privateKey == "" {
		privateKey = firstNonEmpty(q.Get("privatekey"), q.Get("private-key"), q.Get("secretKey"))
	}

	px := map[string]any{
		"type":        "wireguard",
		"name":        nameOr(fragmentName(u), host, port),
		"server":      host,
		"port":        port,
		"private-key": privateKey,
		"public-key":  firstNonEmpty(q.Get("publickey"), q.Get("public-key"), q.Get("peerPublicKey"), q.Get("peer")),
		"udp":         true,
	}
	if ip := firstNonEmpty(q.Get("address"), q.Get("ip"), q.Get("local-address")); ip != "" {
		px["ip"] = ip
	}
	if mtu := asInt(q.Get("mtu")); mtu > 0 {
		px["mtu"] = mtu
	} else {
		px["mtu"] = 1280
	}
	if psk := firstNonEmpty(q.Get("presharedkey"), q.Get("pre-shared-key")); psk != "" {
		px["pre-shared-key"] = psk
	}

	if opt := amneziaOptionFromQuery(q); opt != nil {
		px["amnezia-wg-option"] = opt
	}
	return px, nil
}

// amneziaOptionFromQuery собирает amnezia-wg-option из query-параметров. Возвращает
// nil, если ни одного обфускационного параметра нет (значит это plain WireGuard).
// КРИТИЧНО: h1..h4 кладутся СТРОКАМИ, jc/jmin/jmax/s1..s4 — числами.
func amneziaOptionFromQuery(q url.Values) map[string]any {
	opt := map[string]any{}
	for _, key := range []string{"jc", "jmin", "jmax", "s1", "s2", "s3", "s4"} {
		if v := q.Get(key); v != "" {
			opt[key] = asInt(v)
		}
	}
	for _, key := range []string{"h1", "h2", "h3", "h4"} {
		if v := q.Get(key); v != "" {
			opt[key] = v // строка — обязательно для mihomo
		}
	}
	if len(opt) == 0 {
		return nil
	}
	return opt
}

// --- NaiveProxy ---

// parseNaiveURI разбирает naive+https://user:pass@host:port#name (и naive://).
// mihomo не имеет нативного типа naive; Naive — это HTTP/2 CONNECT поверх TLS,
// поэтому мапим в clash type:http с tls:true. Это единственный путь импорта Naive
// (панель clash-наив не эмитит — см. RECON, наивысший риск расхождения).
func parseNaiveURI(raw string) (map[string]any, error) {
	trimmed := strings.TrimPrefix(raw, "naive+")
	u, err := url.Parse(trimmed)
	if err != nil {
		return nil, fmt.Errorf("subimport: разбор naive URI: %w", err)
	}
	host := u.Hostname()
	port := atoiPort(u.Port())
	password, _ := u.User.Password()
	px := map[string]any{
		"type":   "http",
		"name":   nameOr(fragmentName(u), host, port),
		"server": host,
		"port":   port,
		"tls":    true,
	}
	putNonEmptyString(px, "username", u.User.Username())
	putNonEmptyString(px, "password", password)
	q := u.Query()
	putNonEmptyString(px, "sni", q.Get("sni"))
	if asBool(q.Get("insecure")) {
		px["skip-cert-verify"] = true
	}
	return px, nil
}

// --- транспортные хелперы ---

// applyTransport заполняет network + транспортные опции (ws/grpc/httpupgrade) из
// query-параметров URI. Используется vless/vmess/trojan. tcp оставляет network
// незаданным (mihomo default), кроме случаев когда явно нужен.
func applyTransport(px map[string]any, q url.Values) {
	typ := strings.ToLower(firstNonEmpty(q.Get("type"), q.Get("network")))
	switch typ {
	case "ws":
		px["network"] = "ws"
		opts := map[string]any{}
		putNonEmptyString(opts, "path", firstNonEmpty(q.Get("path"), "/"))
		if h := firstNonEmpty(q.Get("host"), q.Get("sni")); h != "" {
			opts["headers"] = map[string]any{"Host": h}
		}
		px["ws-opts"] = opts
	case "grpc":
		px["network"] = "grpc"
		opts := map[string]any{}
		putNonEmptyString(opts, "grpc-service-name", firstNonEmpty(q.Get("serviceName"), q.Get("path")))
		px["grpc-opts"] = opts
	case "httpupgrade":
		px["network"] = "httpupgrade"
		opts := map[string]any{}
		putNonEmptyString(opts, "path", firstNonEmpty(q.Get("path"), "/"))
		putNonEmptyString(opts, "host", q.Get("host"))
		px["http-upgrade-opts"] = opts
	case "tcp", "":
		// plain TCP — транспорт по умолчанию, опции не задаём.
	default:
		px["network"] = typ
	}
}

// --- мелкие хелперы ---

// atoiPort парсит строку порта в int (0 при ошибке/пустоте).
func atoiPort(s string) int {
	if s == "" {
		return 0
	}
	p, _ := strconv.Atoi(s)
	return p
}

// nameOr возвращает name, если он непустой, иначе синтезирует host:port.
func nameOr(name, host string, port int) string {
	if strings.TrimSpace(name) != "" {
		return name
	}
	return fallbackName(host, port)
}

// firstNonEmpty возвращает первый непустой аргумент.
func firstNonEmpty(vals ...string) string {
	for _, v := range vals {
		if strings.TrimSpace(v) != "" {
			return v
		}
	}
	return ""
}

// splitColon делит "a:b" по первому двоеточию.
func splitColon(s string) (string, string) {
	if i := strings.Index(s, ":"); i >= 0 {
		return s[:i], s[i+1:]
	}
	return s, ""
}

// splitHostPort делит "host:port" по последнему двоеточию (host может быть IPv6
// в скобках). Возвращает host и int-порт.
func splitHostPort(s string) (string, int) {
	s = strings.TrimSpace(s)
	// IPv6 в скобках: [::1]:443
	if strings.HasPrefix(s, "[") {
		if i := strings.Index(s, "]"); i >= 0 {
			host := s[1:i]
			rest := s[i+1:]
			port := 0
			if strings.HasPrefix(rest, ":") {
				port = atoiPort(rest[1:])
			}
			return host, port
		}
	}
	if i := strings.LastIndex(s, ":"); i >= 0 {
		return s[:i], atoiPort(s[i+1:])
	}
	return s, 0
}

// splitCSVList делит "h3,h2" в []any (alpn в clash — список строк).
func splitCSVList(s string) []any {
	parts := strings.Split(s, ",")
	out := make([]any, 0, len(parts))
	for _, p := range parts {
		if p = strings.TrimSpace(p); p != "" {
			out = append(out, p)
		}
	}
	return out
}
