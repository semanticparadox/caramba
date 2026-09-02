package subimport

import (
	"fmt"
	"strings"
)

// parseV2ray разбирает подписку формата v2ray: base64-кодированный список URI
// (по одному на строку). Часть генераторов отдаёт уже декодированный plain-список
// URI — его тоже принимаем. После декодирования каждая строка проходит через
// parseURI (см. uri.go).
func parseV2ray(raw []byte) (proxyList, error) {
	text := string(raw)
	// Пробуем целиком декодировать из base64; если не выходит — считаем, что это
	// уже plain-список URI.
	if dec, ok := tryBase64(text); ok {
		if hasAnyKnownScheme(string(dec)) {
			text = string(dec)
		}
	}

	lines := strings.FieldsFunc(text, func(r rune) bool { return r == '\n' || r == '\r' })
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
		return nil, fmt.Errorf("subimport: v2ray-список не дал ни одного прокси")
	}
	return out, nil
}

// hasAnyKnownScheme сообщает, есть ли в тексте хотя бы одна строка с известной
// URI-схемой.
func hasAnyKnownScheme(text string) bool {
	for _, line := range strings.FieldsFunc(text, func(r rune) bool { return r == '\n' || r == '\r' }) {
		if hasKnownScheme(strings.TrimSpace(line)) {
			return true
		}
	}
	return false
}
