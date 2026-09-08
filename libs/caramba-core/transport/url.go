package transport

import (
	"errors"
	"fmt"
	"net"
	"net/url"
	"strings"
)

// Отказы правил URL, 03-WIRE.md 14 и инвариант 8.
var (
	// ErrSchemeNotTLS это инвариант 8: http:// отвергается для любой выборки
	// манифеста, конфигурации, rule-set или geo. Единственное исключение это
	// .onion, потому что onion-адрес самоаутентифицируется.
	ErrSchemeNotTLS = errors.New("transport: схема не TLS, а хост не .onion")
	// ErrBadHostname это нарушение 03-WIRE.md 14.1.
	ErrBadHostname = errors.New("transport: недопустимое имя хоста")
	// ErrBadPath это нарушение 03-WIRE.md 14.2.
	ErrBadPath = errors.New("transport: недопустимый путь")
	// ErrRedirectRefused это отказ перехода: разрешён ровно один переход и
	// только на настроенный subscription_domain арендатора.
	ErrRedirectRefused = errors.New("transport: переход отвергнут")
	// ErrNotOrigin это нарушение 03-WIRE.md 14.3: org это origin, без пути,
	// запроса и фрагмента, и его схема обязана быть https.
	ErrNotOrigin = errors.New("transport: значение не является origin вида https://host[:port]")
)

// IsOnion сообщает, что хост это onion-адрес. Регистр уже приведён к нижнему
// правилом 14.1, но проверка сама по себе регистронезависима, потому что она
// применяется и к введённому пользователем адресу.
func IsOnion(host string) bool {
	h := strings.ToLower(strings.TrimSuffix(host, "."))
	return h == "onion" || strings.HasSuffix(h, ".onion")
}

// CheckFetchURL применяет инвариант 8 к готовому URL. Вызывается на КАЖДОМ
// переходе, а не один раз на исходном адресе: переход на http это ровно тот
// случай, ради которого правило существует.
func CheckFetchURL(u *url.URL) error {
	if u == nil {
		return fmt.Errorf("%w: пустой URL", ErrBadHostname)
	}
	host := u.Hostname()
	if host == "" {
		return fmt.Errorf("%w: пустой хост", ErrBadHostname)
	}
	switch strings.ToLower(u.Scheme) {
	case "https":
		return nil
	case "http":
		if IsOnion(host) {
			return nil
		}
		return fmt.Errorf("%w: %s://%s", ErrSchemeNotTLS, u.Scheme, host)
	default:
		return fmt.Errorf("%w: %s://%s", ErrSchemeNotTLS, u.Scheme, host)
	}
}

// CheckFetchURLString это то же самое для строки.
func CheckFetchURLString(raw string) error {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil {
		return fmt.Errorf("%w: %v", ErrBadHostname, err)
	}
	return CheckFetchURL(u)
}

// NormalizeOrigin приводит origin регистрации к https://host[:port] и
// отвергает всё остальное. Именно здесь останавливается приём plain http,
// который EnrollLink.normalizePanelUrl принимает сегодня.
func NormalizeOrigin(raw string) (string, error) {
	s := strings.TrimSpace(raw)
	if s == "" {
		return "", fmt.Errorf("%w: пусто", ErrNotOrigin)
	}
	if !strings.Contains(s, "://") {
		s = "https://" + s
	}
	u, err := url.Parse(s)
	if err != nil {
		return "", fmt.Errorf("%w: %v", ErrNotOrigin, err)
	}
	if !strings.EqualFold(u.Scheme, "https") {
		return "", fmt.Errorf("%w: схема %q", ErrNotOrigin, u.Scheme)
	}
	if u.User != nil {
		return "", fmt.Errorf("%w: userinfo запрещён", ErrNotOrigin)
	}
	if u.Path != "" && u.Path != "/" {
		return "", fmt.Errorf("%w: путь %q", ErrNotOrigin, u.Path)
	}
	if u.RawQuery != "" || u.Fragment != "" {
		return "", fmt.Errorf("%w: запрос или фрагмент присутствуют", ErrNotOrigin)
	}
	host := strings.ToLower(u.Hostname())
	if err := CheckHostname(host); err != nil {
		return "", err
	}
	out := "https://" + host
	if p := u.Port(); p != "" {
		out += ":" + p
	}
	return out, nil
}

// CheckHostname применяет 03-WIRE.md 14.1 к полю имени хоста.
//
// Верхний регистр ОТВЕРГАЕТСЯ, а не приводится к нижнему: два написания одного
// хоста не должны давать два значения chash для одного каталога.
func CheckHostname(h string) error {
	if h == "" || len(h) > 64 {
		return fmt.Errorf("%w: длина %d", ErrBadHostname, len(h))
	}
	for i := 0; i < len(h); i++ {
		if h[i] >= 0x80 {
			return fmt.Errorf("%w: не ASCII", ErrBadHostname)
		}
	}
	if strings.ContainsAny(h, "/@:\\ \t") {
		return fmt.Errorf("%w: схема, порт, путь или userinfo в поле хоста", ErrBadHostname)
	}
	if strings.HasSuffix(h, ".") {
		return fmt.Errorf("%w: завершающая точка", ErrBadHostname)
	}
	if strings.HasPrefix(h, "xn--") || strings.Contains(h, ".xn--") {
		// A-label допустима; U-label уже отвергнута проверкой ASCII выше.
		_ = h
	}
	for _, label := range strings.Split(h, ".") {
		if len(label) == 0 || len(label) > 63 {
			return fmt.Errorf("%w: длина метки %d", ErrBadHostname, len(label))
		}
		if label[0] == '-' || label[len(label)-1] == '-' {
			return fmt.Errorf("%w: метка начинается или заканчивается дефисом", ErrBadHostname)
		}
		for i := 0; i < len(label); i++ {
			c := label[i]
			ok := (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-'
			if !ok {
				return fmt.Errorf("%w: символ %q в метке", ErrBadHostname, string(c))
			}
		}
	}
	return nil
}

// CheckIPLiteral применяет правило литеральных адресов 14.1: только mir.ip и
// doh.ip, только IPv4 в точечной записи или IPv6 в нижнем регистре по RFC 5952.
func CheckIPLiteral(s string) error {
	ip := net.ParseIP(s)
	if ip == nil {
		return fmt.Errorf("%w: не IP литерал %q", ErrBadHostname, s)
	}
	if v4 := ip.To4(); v4 != nil {
		if v4.String() != s {
			return fmt.Errorf("%w: неканоническая запись IPv4 %q", ErrBadHostname, s)
		}
		return nil
	}
	if s != strings.ToLower(s) || ip.String() != s {
		return fmt.Errorf("%w: неканоническая запись IPv6 %q", ErrBadHostname, s)
	}
	return nil
}

// pathUnreserved это множество символов, допустимых в поле пути помимо
// корректных escape-последовательностей %XX (03-WIRE.md 14.2).
const pathUnreserved = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~!$&'()*+,;=/:@?"

// CheckPath применяет 03-WIRE.md 14.2 к полю пути (doh.p, resource u).
func CheckPath(p string) error {
	if p == "" || len(p) > 128 {
		return fmt.Errorf("%w: длина %d", ErrBadPath, len(p))
	}
	if p[0] != '/' {
		return fmt.Errorf("%w: путь не начинается с /", ErrBadPath)
	}
	if strings.HasPrefix(p, "//") {
		// //host/path это URL относительно схемы, а не путь.
		return fmt.Errorf("%w: вторая ведущая косая черта", ErrBadPath)
	}
	if strings.Contains(p, "://") || strings.ContainsAny(p, "@\\") {
		return fmt.Errorf("%w: схема, authority, @ или обратная косая черта", ErrBadPath)
	}
	lower := strings.ToLower(p)
	if strings.Contains(lower, "%2f") {
		return fmt.Errorf("%w: %%2f в пути", ErrBadPath)
	}
	for _, seg := range strings.Split(p, "/") {
		if seg == ".." {
			return fmt.Errorf("%w: сегмент ..", ErrBadPath)
		}
	}
	for i := 0; i < len(p); i++ {
		c := p[i]
		if c <= 0x20 || c >= 0x7f {
			return fmt.Errorf("%w: управляющий символ или не ASCII", ErrBadPath)
		}
		if c == '%' {
			if i+2 >= len(p) || !isHex(p[i+1]) || !isHex(p[i+2]) {
				return fmt.Errorf("%w: некорректная escape-последовательность", ErrBadPath)
			}
			i += 2
			continue
		}
		if !strings.ContainsRune(pathUnreserved, rune(c)) {
			return fmt.Errorf("%w: символ %q", ErrBadPath, string(c))
		}
	}
	return nil
}

func isHex(c byte) bool {
	return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')
}

// ResolveAgainst собирает абсолютный URL из origin или хоста пула и поля пути.
// Подписанный документ может назвать путь; он НИКОГДА не может назвать хост,
// которого ещё нет в пуле, поэтому origin приходит отдельным аргументом.
func ResolveAgainst(origin, path string) (string, error) {
	if err := CheckPath(path); err != nil {
		return "", err
	}
	o, err := NormalizeOrigin(origin)
	if err != nil {
		return "", err
	}
	return o + path, nil
}

// CheckRedirect применяет правило 02-SPEC.md 8.10 и 01-DECISION.md 5.1.8:
// разрешён ровно один переход, и только когда хост цели равен настроенному
// subscription_domain арендатора. Схема цели проверяется тем же инвариантом 8.
func CheckRedirect(from, to *url.URL, hops int, subscriptionDomain string) error {
	if hops >= 1 {
		return fmt.Errorf("%w: больше одного перехода", ErrRedirectRefused)
	}
	if err := CheckFetchURL(to); err != nil {
		return err
	}
	sd := strings.ToLower(strings.TrimSpace(subscriptionDomain))
	if sd == "" {
		// subscription_domain не настроен. Панель выдаёт свой 308 безусловно
		// при несовпадении Host, поэтому полный отказ здесь означал бы, что
		// легаси-путь перестаёт работать у каждого арендатора, чей Host не
		// совпал: то есть проверка, написанная ради одного перехода, запрещает
		// именно тот переход, ради которого написана.
		//
		// Запасное правило узкое и названо запасным: ровно один переход, только
		// на тот же зарегистрированный домен, что у источника, и только по
		// правилам инварианта 8, которые уже применены выше. Когда
		// subscription_domain задан, работает точное равенство и это правило не
		// достигается вовсе.
		if from == nil || !sameRegistrableDomain(from.Hostname(), to.Hostname()) {
			return fmt.Errorf("%w: subscription_domain не настроен, а цель %q вне домена источника",
				ErrRedirectRefused, to.Hostname())
		}
		return nil
	}
	if !strings.EqualFold(to.Hostname(), sd) {
		return fmt.Errorf("%w: цель %q не равна subscription_domain %q", ErrRedirectRefused, to.Hostname(), sd)
	}
	return nil
}

// sameRegistrableDomain сравнивает две последние метки имени. Это грубое
// приближение публичного суффикса, и оно применяется ТОЛЬКО в запасной ветке
// выше, где точного значения у клиента нет.
func sameRegistrableDomain(a, b string) bool {
	la, lb := strings.ToLower(strings.TrimSuffix(a, ".")), strings.ToLower(strings.TrimSuffix(b, "."))
	if la == "" || lb == "" {
		return false
	}
	if la == lb {
		return true
	}
	tail := func(h string) string {
		parts := strings.Split(h, ".")
		if len(parts) < 2 {
			return h
		}
		return strings.Join(parts[len(parts)-2:], ".")
	}
	ta, tb := tail(la), tail(lb)
	return ta != "" && ta == tb
}
