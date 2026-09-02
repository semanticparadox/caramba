package subimport

import (
	"encoding/base64"
	"strings"
)

// detectFormat определяет формат подписки по содержимому. Порядок проверок важен:
//
//  1. одиночный URI с известной схемой → FormatURI;
//  2. JSON-документ (начинается с '{') → FormatSingbox;
//  3. текст с явной секцией clash (proxies:) → FormatClash;
//  4. строка, целиком декодируемая из base64 в список URI → FormatV2ray;
//  5. иначе пробуем как clash YAML (наиболее частый «сырой» источник).
//
// Эвристика не идеальна; вызывающий всегда может задать формат явно.
func detectFormat(raw []byte) string {
	trimmed := strings.TrimSpace(string(raw))
	if trimmed == "" {
		return FormatClash
	}

	// Одиночный URI: одна строка с поддерживаемой схемой.
	if !strings.ContainsAny(trimmed, "\r\n") && hasKnownScheme(trimmed) {
		return FormatURI
	}

	// sing-box: JSON-объект.
	if strings.HasPrefix(trimmed, "{") {
		return FormatSingbox
	}

	// clash/mihomo: присутствует секция proxies.
	if looksLikeClash(trimmed) {
		return FormatClash
	}

	// v2ray: base64-список (часто с переносами/паддингом). Пробуем декодировать и
	// убеждаемся, что внутри есть хотя бы один известный URI.
	if looksLikeV2rayBase64(trimmed) {
		return FormatV2ray
	}

	// Несколько строк, каждая из которых URI (plain, без base64).
	if multilineURIs(trimmed) {
		return FormatV2ray
	}

	return FormatClash
}

// knownSchemes — поддерживаемые схемы одиночных URI.
var knownSchemes = []string{
	"vless://", "vmess://", "trojan://", "ss://", "ssr://",
	"hysteria2://", "hy2://", "tuic://", "wireguard://", "wg://",
	"naive+https://", "naive://",
}

// hasKnownScheme сообщает, начинается ли строка с поддерживаемой URI-схемы.
func hasKnownScheme(s string) bool {
	low := strings.ToLower(strings.TrimSpace(s))
	for _, sch := range knownSchemes {
		if strings.HasPrefix(low, sch) {
			return true
		}
	}
	return false
}

// looksLikeClash грубо распознаёт clash/mihomo YAML по наличию ключа proxies.
func looksLikeClash(s string) bool {
	for _, line := range strings.Split(s, "\n") {
		t := strings.TrimSpace(line)
		if t == "proxies:" || strings.HasPrefix(t, "proxies:") {
			return true
		}
	}
	return false
}

// looksLikeV2rayBase64 проверяет, что строка целиком декодируется из base64 и
// содержит хотя бы один известный URI.
func looksLikeV2rayBase64(s string) bool {
	dec, ok := tryBase64(s)
	if !ok {
		return false
	}
	return multilineURIs(string(dec))
}

// multilineURIs сообщает, что хотя бы одна непустая строка текста — известный URI.
func multilineURIs(s string) bool {
	for _, line := range strings.FieldsFunc(s, func(r rune) bool { return r == '\n' || r == '\r' }) {
		if hasKnownScheme(strings.TrimSpace(line)) {
			return true
		}
	}
	return false
}

// tryBase64 декодирует строку, перебирая standard/url-варианты и с/без паддинга.
// Пробелы и переносы строк удаляются перед декодированием. Возвращает false, если
// ни один вариант не сработал.
func tryBase64(s string) ([]byte, bool) {
	clean := strings.Map(func(r rune) rune {
		if r == '\n' || r == '\r' || r == ' ' || r == '\t' {
			return -1
		}
		return r
	}, strings.TrimSpace(s))
	if clean == "" {
		return nil, false
	}
	encodings := []*base64.Encoding{
		base64.StdEncoding,
		base64.RawStdEncoding,
		base64.URLEncoding,
		base64.RawURLEncoding,
	}
	for _, enc := range encodings {
		if dec, err := enc.DecodeString(clean); err == nil && len(dec) > 0 {
			return dec, true
		}
	}
	return nil, false
}
