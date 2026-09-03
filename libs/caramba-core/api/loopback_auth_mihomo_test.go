//go:build mihomo

package api

import (
	"context"
	"net"
	"net/url"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/profile"
)

// TestLoopbackInboundDemandsCredentials поднимает НАСТОЯЩИЙ движок и стучится в
// служебный инбаунд на петле сначала без пары логин-пароль, потом с ней.
//
// Это и есть проверка того, ради чего пара появилась. Инбаунд несёт ключ proxy,
// то есть уводит ВСЁ, что на него пришло, прямо в группу-селектор мимо правил
// маршрутизации. Без аутентификации это открытый релей: на Android в него ходит
// любое приложение с разрешением INTERNET, на десктопе любой локальный процесс,
// и весь их трафик уходит через оплаченный пользователем узел мимо пресета и
// мимо раздельного туннелирования. Проверять это разбором YAML мало: слушателя
// поднимает ядро, и отвечает на рукопожатие тоже оно.
//
// Узел в фикстуре недостижим намеренно: до исходящего соединения дело не
// доходит, весь ответ на вопрос даётся на этапе метода аутентификации socks5.
func TestLoopbackInboundDemandsCredentials(t *testing.T) {
	dir := t.TempDir()
	const fixture = `
mixed-port: 7890
proxies:
  - {name: "N1", type: socks5, server: 127.0.0.1, port: 1}
proxy-groups:
  - {name: CARAMBA, type: select, proxies: ["N1", "DIRECT"]}
rules:
  - MATCH,CARAMBA
`
	c, err := NewCore(Config{
		PanelBaseURL:   "https://panel.example.net",
		WorkDir:        dir,
		TokenStorePath: filepath.Join(dir, "tokens.json"),
	})
	if err != nil {
		t.Fatalf("ядро: %v", err)
	}
	c.SetImportedConfig([]byte(fixture))

	p := profile.DefaultPolicy()
	p.Mode = profile.ModeProxy
	p.KillSwitch = false
	p.Proxy.MixedPort = freePort(t)
	p.Proxy.LoopbackPort = freePort(t)
	// Правила GEOIP/GEOSITE заставили бы ядро качать geo-базы из сети.
	p.Routing = nil
	c.SetPolicy(p)

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	if _, err := c.Up(ctx, ""); err != nil {
		t.Skipf("движок не поднялся в этой среде: %v", err)
	}
	defer func() { _ = c.Down() }()

	raw := c.LoopbackProxyURL()
	if raw == "" {
		t.Fatalf("ядро не отдало адрес служебного инбаунда")
	}
	u, err := url.Parse(raw)
	if err != nil {
		t.Fatalf("адрес %q: %v", raw, err)
	}
	if u.User == nil || u.User.Username() == "" {
		t.Fatalf("адрес без учётных данных: %q", raw)
	}
	addr := u.Host

	waitListening(t, addr)

	// Ядро выбирает метод 0x02 (логин и пароль) незавиcимо от того, что
	// предложил клиент. Ответ 0x00 здесь означал бы слушатель БЕЗ
	// аутентификации, то есть открытый релей на петле, и это ровно то, что
	// проверяется.
	if method := socks5Greet(t, addr, []byte{0x00}); method != 0x02 {
		t.Fatalf("служебный инбаунд выбрал метод 0x%02x: анонимный socks5 принят, это открытый релей на петле", method)
	}

	// Чужая пара отвергается.
	if status := socks5Auth(t, addr, "someone", "else"); status != 0x01 {
		t.Fatalf("чужая пара логин-пароль принята, статус 0x%02x", status)
	}
	// Своя принимается.
	pass, _ := u.User.Password()
	if status := socks5Auth(t, addr, u.User.Username(), pass); status != 0x00 {
		t.Fatalf("выпущенная ядром пара отвергнута, статус 0x%02x", status)
	}

	// Вторая половина mixed-инбаунда, http, обязана требовать того же.
	if code := httpConnectStatus(t, addr); code != 407 {
		t.Fatalf("http-половина служебного инбаунда ответила %d вместо 407: она открыта", code)
	}
}

// socks5Auth проходит выбор метода и отправляет пару логин-пароль, возвращая
// статус подпротокола RFC 1929 (0x00 принято, иначе отказ).
func socks5Auth(t *testing.T, addr, user, pass string) byte {
	t.Helper()
	conn, err := net.DialTimeout("tcp", addr, 5*time.Second)
	if err != nil {
		t.Fatalf("соединение с %s: %v", addr, err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(5 * time.Second))
	if _, err := conn.Write([]byte{0x05, 0x01, 0x02}); err != nil {
		t.Fatalf("приветствие: %v", err)
	}
	var sel [2]byte
	if _, err := readFullConn(conn, sel[:]); err != nil {
		t.Fatalf("ответ на приветствие: %v", err)
	}
	if sel[1] != 0x02 {
		t.Fatalf("метод 0x%02x вместо пары логин-пароль", sel[1])
	}
	buf := []byte{0x01, byte(len(user))}
	buf = append(buf, user...)
	buf = append(buf, byte(len(pass)))
	buf = append(buf, pass...)
	if _, err := conn.Write(buf); err != nil {
		t.Fatalf("пара: %v", err)
	}
	var ans [2]byte
	if _, err := readFullConn(conn, ans[:]); err != nil {
		t.Fatalf("ответ на пару: %v", err)
	}
	return ans[1]
}

// httpConnectStatus отправляет CONNECT без Proxy-Authorization и возвращает код
// состояния ответа.
func httpConnectStatus(t *testing.T, addr string) int {
	t.Helper()
	conn, err := net.DialTimeout("tcp", addr, 5*time.Second)
	if err != nil {
		t.Fatalf("соединение с %s: %v", addr, err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(5 * time.Second))
	req := "CONNECT example.com:80 HTTP/1.1\r\nHost: example.com:80\r\n\r\n"
	if _, err := conn.Write([]byte(req)); err != nil {
		t.Fatalf("CONNECT: %v", err)
	}
	line := make([]byte, 0, 64)
	one := make([]byte, 1)
	for len(line) < 64 {
		if _, err := conn.Read(one); err != nil {
			break
		}
		if one[0] == '\n' {
			break
		}
		line = append(line, one[0])
	}
	parts := strings.Fields(string(line))
	if len(parts) < 2 {
		t.Fatalf("ответ http-половины: %q", string(line))
	}
	code, err := strconv.Atoi(parts[1])
	if err != nil {
		t.Fatalf("код в ответе %q", string(line))
	}
	return code
}

// socks5Greet шлёт приветствие socks5 с указанными методами и возвращает
// выбранный сервером метод.
func socks5Greet(t *testing.T, addr string, methods []byte) byte {
	t.Helper()
	conn, err := net.DialTimeout("tcp", addr, 5*time.Second)
	if err != nil {
		t.Fatalf("соединение с %s: %v", addr, err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(5 * time.Second))
	greet := append([]byte{0x05, byte(len(methods))}, methods...)
	if _, err := conn.Write(greet); err != nil {
		t.Fatalf("приветствие: %v", err)
	}
	var sel [2]byte
	if _, err := readFullConn(conn, sel[:]); err != nil {
		t.Fatalf("ответ на приветствие: %v", err)
	}
	if sel[0] != 0x05 {
		t.Fatalf("не socks5: 0x%02x", sel[0])
	}
	return sel[1]
}

func readFullConn(c net.Conn, b []byte) (int, error) {
	n := 0
	for n < len(b) {
		m, err := c.Read(b[n:])
		if m > 0 {
			n += m
		}
		if err != nil {
			return n, err
		}
	}
	return n, nil
}

func waitListening(t *testing.T, addr string) {
	t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		conn, err := net.DialTimeout("tcp", addr, time.Second)
		if err == nil {
			_ = conn.Close()
			return
		}
		time.Sleep(100 * time.Millisecond)
	}
	t.Fatalf("служебный инбаунд не поднялся на %s", addr)
}

func freePort(t *testing.T) int {
	t.Helper()
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("свободный порт: %v", err)
	}
	defer l.Close()
	_, portS, _ := net.SplitHostPort(l.Addr().String())
	port, _ := strconv.Atoi(portS)
	return port
}
