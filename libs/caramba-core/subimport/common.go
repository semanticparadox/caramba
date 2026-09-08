package subimport

import (
	"strconv"
	"strings"
)

// asInt приводит произвольное YAML/JSON-значение к int. Порт после разбора URI
// приходит строкой, из YAML — int/float64, из JSON — float64. Возвращает 0, если
// привести не удалось.
func asInt(v any) int {
	switch n := v.(type) {
	case int:
		return n
	case int64:
		return int(n)
	case float64:
		return int(n)
	case string:
		if i, err := strconv.Atoi(strings.TrimSpace(n)); err == nil {
			return i
		}
	}
	return 0
}

// asString приводит значение к строке без потери числовых форм (порт/uuid могут
// прийти числом из JSON). Возвращает "" для nil.
func asString(v any) string {
	switch s := v.(type) {
	case string:
		return s
	case float64:
		return strconv.FormatInt(int64(s), 10)
	case int:
		return strconv.Itoa(s)
	case int64:
		return strconv.FormatInt(s, 10)
	case bool:
		return strconv.FormatBool(s)
	case nil:
		return ""
	default:
		return ""
	}
}

// asBool приводит значение к bool (sing-box JSON отдаёт bool, URI-параметры —
// строки "1"/"true").
func asBool(v any) bool {
	switch b := v.(type) {
	case bool:
		return b
	case string:
		switch strings.ToLower(strings.TrimSpace(b)) {
		case "1", "true", "yes", "on":
			return true
		}
	case float64:
		return b != 0
	}
	return false
}

// CountryFromName извлекает ISO-2 код страны из имени прокси.
//
// Логика (в порядке приоритета):
//  1. флаг-эмодзи в любом месте имени (две Regional Indicator Symbol-руны);
//  2. первый «токен» ровно из двух латинских букв («DE Stealth» → DE,
//     «Amsterdam NL 01» → NL). Токены режутся по любым не-буквам.
//
// Пусто, если страну вывести не удалось. Функция экспортирована, потому что тем
// же правилом пользуется CarambaProbe (api): контракт ABI требует одинаковой
// страны у списка серверов импорта и у результатов замера.
func CountryFromName(name string) string {
	name = strings.TrimSpace(name)
	if name == "" {
		return ""
	}
	runes := []rune(name)
	for i := 0; i+1 < len(runes); i++ {
		if iso := flagToISO(string(runes[i : i+2])); iso != "" {
			return iso
		}
	}
	var token []rune
	for _, r := range runes {
		if (r >= 'A' && r <= 'Z') || (r >= 'a' && r <= 'z') {
			token = append(token, r)
			continue
		}
		if len(token) == 2 {
			return strings.ToUpper(string(token))
		}
		token = token[:0]
	}
	if len(token) == 2 {
		return strings.ToUpper(string(token))
	}
	return ""
}

// flagToISO декодирует флаг-эмодзи (две Regional Indicator Symbol-руны,
// U+1F1E6..U+1F1FF) в ISO-2 код. "" если строка не начинается с флага.
func flagToISO(name string) string {
	const base = 0x1F1E6
	runes := []rune(strings.TrimSpace(name))
	if len(runes) < 2 {
		return ""
	}
	a, b := runes[0], runes[1]
	if a < base || a > base+25 || b < base || b > base+25 {
		return ""
	}
	return string([]byte{byte('A' + (a - base)), byte('A' + (b - base))})
}

// fallbackName формирует имя прокси, если оно не задано в источнике (URI без
// fragment, sing-box outbound без tag). Берёт host:port, чтобы UI-список и
// автоподбор не показывали пустую строку (ServerID = name).
func fallbackName(server string, port int) string {
	if server == "" {
		return "imported"
	}
	if port > 0 {
		return server + ":" + strconv.Itoa(port)
	}
	return server
}

// putNonEmptyString кладёт строковое значение в map только если оно непустое.
// Так нормализованный proxy-map не засоряется пустыми ключами, которые mihomo
// мог бы интерпретировать как заданные (например пустой sni).
func putNonEmptyString(m map[string]any, key, val string) {
	if strings.TrimSpace(val) != "" {
		m[key] = val
	}
}
