package csm

import (
	"net/netip"
	"strings"
)

// Помощники шага P11: типы, ограничения и межполевые правила разделов 8, 5 и
// 14 документа 03-WIRE.md. Все отказы здесь несут E_PARSE_FIELD, потому что
// они зависят от doc_type; всё, что декодер решает БЕЗ doc_type (ключ 0,
// ключи от 1024, структурные правила, размеры), решается в cbor.go и несёт
// E_PARSE_CBOR. Это разделение из коррекции cor-3 корпуса.

func fieldErr(format string, args ...any) *Error {
	return errf(EParseField, "P11", format, args...)
}

// keySet описывает, какие ключи критического диапазона известны документу.
type keySet map[uint64]bool

// checkCriticalKeys применяет правило 3.3: неизвестный ключ в 1..63 это отказ,
// неизвестный ключ в 64..1023 игнорируется. Ключи 0 и от 1024 уже отвергнуты
// декодером.
func checkCriticalKeys(m *Value, known keySet, what string) error {
	for i := range m.Map {
		k := m.Map[i].Key
		if k > CriticalKeyMax {
			continue // некритический диапазон, игнорируем по правилу 3.3
		}
		if !known[k] {
			return fieldErr("%s: unrecognized critical key %d", what, k)
		}
	}
	return nil
}

func getKind(m *Value, key uint64, kind Kind, what string) (*Value, bool, error) {
	v, ok := m.Get(key)
	if !ok {
		return nil, false, nil
	}
	if v.Kind != kind {
		return nil, false, fieldErr("%s: key %d is %s, expected %s", what, key, v.Kind, kind)
	}
	return v, true, nil
}

func reqUint(m *Value, key uint64, lo, hi uint64, what string) (uint64, error) {
	v, ok, err := getKind(m, key, KindUint, what)
	if err != nil {
		return 0, err
	}
	if !ok {
		return 0, fieldErr("%s: mandatory uint key %d is absent", what, key)
	}
	if v.U < lo || v.U > hi {
		return 0, fieldErr("%s: key %d value %d is outside %d..%d", what, key, v.U, lo, hi)
	}
	return v.U, nil
}

func optUint(m *Value, key uint64, lo, hi uint64, what string) (uint64, bool, error) {
	v, ok, err := getKind(m, key, KindUint, what)
	if err != nil || !ok {
		return 0, false, err
	}
	if v.U < lo || v.U > hi {
		return 0, false, fieldErr("%s: key %d value %d is outside %d..%d", what, key, v.U, lo, hi)
	}
	return v.U, true, nil
}

func reqBstr(m *Value, key uint64, minLen, maxLen int, what string) ([]byte, error) {
	v, ok, err := getKind(m, key, KindBstr, what)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, fieldErr("%s: mandatory bstr key %d is absent", what, key)
	}
	if len(v.B) < minLen || len(v.B) > maxLen {
		return nil, fieldErr("%s: key %d is %d bytes, cap is %d..%d", what, key, len(v.B), minLen, maxLen)
	}
	return v.B, nil
}

func optBstr(m *Value, key uint64, minLen, maxLen int, what string) ([]byte, bool, error) {
	v, ok, err := getKind(m, key, KindBstr, what)
	if err != nil || !ok {
		return nil, false, err
	}
	if len(v.B) < minLen || len(v.B) > maxLen {
		return nil, false, fieldErr("%s: key %d is %d bytes, cap is %d..%d", what, key, len(v.B), minLen, maxLen)
	}
	return v.B, true, nil
}

func reqTstr(m *Value, key uint64, minLen, maxLen int, what string) (string, error) {
	v, ok, err := getKind(m, key, KindTstr, what)
	if err != nil {
		return "", err
	}
	if !ok {
		return "", fieldErr("%s: mandatory tstr key %d is absent", what, key)
	}
	if len(v.S) < minLen || len(v.S) > maxLen {
		return "", fieldErr("%s: key %d is %d bytes, cap is %d..%d", what, key, len(v.S), minLen, maxLen)
	}
	return v.S, nil
}

func optTstr(m *Value, key uint64, minLen, maxLen int, what string) (string, bool, error) {
	v, ok, err := getKind(m, key, KindTstr, what)
	if err != nil || !ok {
		return "", false, err
	}
	if len(v.S) < minLen || len(v.S) > maxLen {
		return "", false, fieldErr("%s: key %d is %d bytes, cap is %d..%d", what, key, len(v.S), minLen, maxLen)
	}
	return v.S, true, nil
}

func reqArray(m *Value, key uint64, minN, maxN int, what string) ([]Value, error) {
	v, ok, err := getKind(m, key, KindArray, what)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, fieldErr("%s: mandatory array key %d is absent", what, key)
	}
	if len(v.Array) < minN || len(v.Array) > maxN {
		return nil, fieldErr("%s: key %d has %d entries, cap is %d..%d", what, key, len(v.Array), minN, maxN)
	}
	return v.Array, nil
}

func optArray(m *Value, key uint64, minN, maxN int, what string) ([]Value, bool, error) {
	v, ok, err := getKind(m, key, KindArray, what)
	if err != nil || !ok {
		return nil, false, err
	}
	if len(v.Array) < minN || len(v.Array) > maxN {
		return nil, false, fieldErr("%s: key %d has %d entries, cap is %d..%d", what, key, len(v.Array), minN, maxN)
	}
	return v.Array, true, nil
}

func optMap(m *Value, key uint64, what string) (*Value, bool, error) {
	v, ok, err := getKind(m, key, KindMap, what)
	return v, ok, err
}

func reqMap(m *Value, key uint64, what string) (*Value, error) {
	v, ok, err := getKind(m, key, KindMap, what)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, fieldErr("%s: mandatory map key %d is absent", what, key)
	}
	return v, nil
}

func optBool(m *Value, key uint64, what string) (bool, bool, error) {
	v, ok, err := getKind(m, key, KindBool, what)
	if err != nil || !ok {
		return false, false, err
	}
	return v.Bool, true, nil
}

// bstrArray читает массив байтовых строк фиксированной длины.
func bstrArray(items []Value, exact int, what string) ([][]byte, error) {
	out := make([][]byte, 0, len(items))
	for i := range items {
		if items[i].Kind != KindBstr {
			return nil, fieldErr("%s: entry %d is %s, expected bstr", what, i, items[i].Kind)
		}
		if len(items[i].B) != exact {
			return nil, fieldErr("%s: entry %d is %d bytes, must be exactly %d", what, i, len(items[i].B), exact)
		}
		out = append(out, items[i].B)
	}
	return out, nil
}

// ------------------------------------------------------------------ словари

// Закрытые словари раздела 5. Значение вне множества в поле критического
// диапазона это отказ разбора (инвариант 11: клиент сохраняет и повторяет
// только те значения, которые может проверить по закрытому словарю).
var (
	stValues   = rangeSet(1, 8)  // статус директивы
	roleValues = rangeSet(1, 2)  // 3 (timestamp) зарезервирована и запрещена в v1
	prValues   = rangeSet(1, 8)  // протокол узла
	nwValues   = rangeSet(1, 6)  // транспортная сеть
	seValues   = rangeSet(0, 2)  // безопасность
	fpValues   = rangeSet(1, 10) // отпечаток uTLS
	alpValues  = rangeSet(1, 3)  // ALPN
	cgValues   = rangeSet(1, 3)  // контроль перегрузки TUIC
	ssmValues  = rangeSet(1, 6)  // метод Shadowsocks
	rungValues = rangeSet(0, 6)  // ступени лестницы
	srcValues  = rangeSet(1, 3)  // происхождение настройки
	uiKValues  = rangeSet(1, 5)  // вид подсказки интерфейса
	polKeys    = rangeSet(1, 11) // пространство ключей pol
)

func rangeSet(lo, hi uint64) map[uint64]bool {
	m := make(map[uint64]bool, hi-lo+1)
	for i := lo; i <= hi; i++ {
		m[i] = true
	}
	return m
}

func enumUint(m *Value, key uint64, set map[uint64]bool, mandatory bool, what, name string) (uint64, bool, error) {
	v, ok, err := getKind(m, key, KindUint, what)
	if err != nil {
		return 0, false, err
	}
	if !ok {
		if mandatory {
			return 0, false, fieldErr("%s: mandatory %s (key %d) is absent", what, name, key)
		}
		return 0, false, nil
	}
	if !set[v.U] {
		return 0, false, fieldErr("%s: %s value %d is outside the closed vocabulary", what, name, v.U)
	}
	return v.U, true, nil
}

// ------------------------------------------------------------ имена и пути

// validHostname реализует 03-WIRE.md 14.1. Верхний регистр отвергается, а не
// приводится к нижнему, чтобы два написания одного хоста не дали два chash для
// одного каталога.
func validHostname(s string) bool {
	if len(s) < 1 || len(s) > 64 {
		return false
	}
	if strings.HasSuffix(s, ".") {
		return false
	}
	for i := 0; i < len(s); i++ {
		if s[i] >= 0x80 {
			return false // только ASCII, IDN несёт A-label
		}
	}
	for _, label := range strings.Split(s, ".") {
		if len(label) < 1 || len(label) > 63 {
			return false
		}
		if label[0] == '-' || label[len(label)-1] == '-' {
			return false
		}
		for i := 0; i < len(label); i++ {
			c := label[i]
			ok := (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-'
			if !ok {
				return false
			}
		}
	}
	return true
}

// validIPLiteral принимает только каноническое написание: dotted-quad для IPv4
// и RFC 5952 в нижнем регистре для IPv6.
func validIPLiteral(s string) bool {
	a, err := netip.ParseAddr(s)
	if err != nil {
		return false
	}
	return a.String() == s
}

// validHostOrIP: узел может нести IP-литерал в поле h, потому что
// NodeInfo.address сегодня это адрес узла.
func validHostOrIP(s string) bool { return validHostname(s) || validIPLiteral(s) }

const pathSubDelims = "!$&'()*+,;="

func isHex(c byte) bool {
	return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')
}

// validPathOnly реализует 03-WIRE.md 14.2. Подписанный документ может назвать
// путь, но никогда не хост, которого ещё нет в пуле.
func validPathOnly(s string, maxLen int) bool {
	if len(s) < 1 || len(s) > maxLen || len(s) > 128 {
		return false
	}
	if s[0] != '/' {
		return false
	}
	if len(s) > 1 && s[1] == '/' {
		return false // //host/path это URL относительно схемы
	}
	if strings.Contains(s, "://") {
		return false
	}
	lower := strings.ToLower(s)
	if strings.Contains(lower, "%2f") {
		return false
	}
	for _, seg := range strings.Split(s, "/") {
		if seg == ".." {
			return false
		}
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c < 0x21 || c > 0x7e {
			return false // управляющие символы, пробел и не-ASCII
		}
		switch {
		case c >= 'A' && c <= 'Z', c >= 'a' && c <= 'z', c >= '0' && c <= '9':
			continue
		case c == '-' || c == '.' || c == '_' || c == '~':
			continue
		case c == '/' || c == ':' || c == '@' || c == '?':
			continue
		case strings.IndexByte(pathSubDelims, c) >= 0:
			continue
		case c == '%':
			if i+2 >= len(s) || !isHex(s[i+1]) || !isHex(s[i+2]) {
				return false
			}
			i += 2
			continue
		default:
			return false // в том числе @ уже разрешён выше, \\ и всё прочее нет
		}
	}
	return true
}

// validOrigin реализует 03-WIRE.md 14.3: scheme://host[:port], только https,
// без пути, запроса и фрагмента.
func validOrigin(s string) bool {
	const scheme = "https://"
	if !strings.HasPrefix(s, scheme) {
		return false
	}
	rest := s[len(scheme):]
	if rest == "" || strings.ContainsAny(rest, "/?#@\\") {
		return false
	}
	host := rest
	if i := strings.LastIndexByte(rest, ':'); i >= 0 && !strings.Contains(rest, "]") {
		host = rest[:i]
		port := rest[i+1:]
		if port == "" || len(port) > 5 {
			return false
		}
		n := 0
		for j := 0; j < len(port); j++ {
			if port[j] < '0' || port[j] > '9' {
				return false
			}
			n = n*10 + int(port[j]-'0')
		}
		if n < 1 || n > 65535 {
			return false
		}
	}
	return validHostOrIP(host)
}

// validCountry: ровно две заглавные буквы ISO 3166-1 alpha-2.
func validCountry(s string) bool {
	if len(s) != 2 {
		return false
	}
	return s[0] >= 'A' && s[0] <= 'Z' && s[1] >= 'A' && s[1] <= 'Z'
}

// validCrockford проверяет, что строка это ровно 24 символа канонического
// алфавита base32 Crockford в верхнем регистре. Псевдонимы I, L, O и строчные
// буквы, которые декодер принимает, здесь ОТВЕРГАЮТСЯ: у идентификатора,
// уходящего в путь URL и в заголовок, должно быть ровно одно написание.
func validCrockford(s string) bool {
	if len(s) != 24 {
		return false
	}
	for i := 0; i < len(s); i++ {
		if strings.IndexByte(CrockfordAlphabet, s[i]) < 0 {
			return false
		}
	}
	return true
}

// TierMin и TierMax это диапазон идентификатора тарифа, 03-WIRE.md 8.1.
//
// Верхняя граница 1023, а не 65535, потому что идентификатор тарифа служит
// КЛЮЧОМ карты CBOR в поле tiers, а правило 3.3 отвергает ключ 0 и любой ключ
// от 1024. Пока в таблице стояло < 2^16, соответствующая панель могла подписать
// ключевой документ, который ни один соответствующий верификатор не декодирует:
// арендатор уходил в темноту на шаге P9 с кодом, называющим CBOR, а не тарифы.
// Потолок когорты равен 16, так что 1023 недостижимо на практике.
const (
	TierMin uint64 = 1
	TierMax uint64 = 1023
)

// NoRelaySentinel это разрешённое значение sel.rcc, означающее, что оператор
// разрешил "без реле". 03-WIRE.md 8.3 изначально задавал "NO", но NO это
// Норвегия; коррекция 02-SPEC.md 4 заменила его на "--".
const NoRelaySentinel = "--"

// ResetSentinel это значение want, означающее сброс к умолчанию оператора.
// Существует потому, что CBOR null запрещён правилом C7.
const ResetSentinel = "default"

// validNodeID: charset [0-9A-Za-z_-], 1..24, и не литерал "default".
func validNodeID(s string) bool {
	if len(s) < 1 || len(s) > 24 || s == ResetSentinel {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		ok := (c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '_' || c == '-'
		if !ok {
			return false
		}
	}
	return true
}

// validRouteID: charset [a-z0-9-], 1..32.
func validRouteID(s string) bool {
	if len(s) < 1 || len(s) > 32 {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		ok := (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || c == '-'
		if !ok {
			return false
		}
	}
	return true
}

// validHexString: строка из шестнадцатеричных цифр.
func validHexString(s string) bool {
	for i := 0; i < len(s); i++ {
		if !isHex(s[i]) {
			return false
		}
	}
	return true
}
