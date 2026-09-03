package transport

import (
	"bufio"
	"context"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

// Прокси набирается ЗДЕСЬ, а не в http.Transport.
//
// http.Transport документирует DialTLSContext как путь "for non-proxied HTTPS
// requests": при непустом Proxy он берёт TLSClientConfig и наш обработчик не
// зовёт вовсе. Ступени R4 и R5 задают Proxy, поэтому передача его транспорту
// молча снимала бы SPKI пины, нижнюю границу TLS и явный ClientHello uTLS
// ровно на тех двух ступенях, на которые скатывается пользователь под
// цензурой. Соединение через прокси поэтому набирается вручную, а рукопожатие
// идёт по тому же коду, что и на прямой ступени.

// ErrProxy это отказ на этапе установления соединения через прокси.
var ErrProxy = errors.New("transport: прокси не установил соединение")

// dialRawConn открывает TCP до цели: напрямую, по литеральному адресу
// подписанной записи doh, либо через прокси ступени.
func dialRawConn(ctx context.Context, t Target, addr string) (net.Conn, error) {
	host, port, err := net.SplitHostPort(addr)
	if err != nil {
		host, port = addr, "443"
	}
	if t.Addr != "" {
		// R3: подключаемся к литеральному адресу из подписанной записи doh.
		// Порт берём из исходного адреса, потому что запись адреса его не
		// несёт.
		host = t.Addr
	}
	dst := net.JoinHostPort(host, port)
	if t.Proxy == "" {
		d := &net.Dialer{Timeout: TCPConnectTimeout}
		return d.DialContext(ctx, "tcp", dst)
	}
	return dialThroughProxy(ctx, t.Proxy, dst)
}

// dialThroughProxy устанавливает туннель до dst через socks5 или http прокси и
// возвращает уже готовый к TLS поток.
func dialThroughProxy(ctx context.Context, proxy, dst string) (net.Conn, error) {
	pu, err := parseProxyURL(proxy)
	if err != nil {
		return nil, err
	}
	d := &net.Dialer{Timeout: TCPConnectTimeout}
	raw, err := d.DialContext(ctx, "tcp", pu.Host)
	if err != nil {
		return nil, err
	}
	if dl, ok := ctx.Deadline(); ok {
		_ = raw.SetDeadline(dl)
	}
	switch strings.ToLower(pu.Scheme) {
	case "socks5", "socks5h":
		err = socks5Connect(raw, pu, dst)
	default: // http, https
		err = httpConnect(raw, pu, dst)
	}
	if err != nil {
		_ = raw.Close()
		return nil, err
	}
	// Дедлайн установления снимается: дальше временем управляет http.Client.
	_ = raw.SetDeadline(time.Time{})
	return raw, nil
}

// socks5Connect выполняет рукопожатие RFC 1928 с методами 0x00 и 0x02.
func socks5Connect(c net.Conn, pu *url.URL, dst string) error {
	user := pu.User.Username()
	pass, _ := pu.User.Password()
	methods := []byte{0x00}
	if user != "" {
		methods = []byte{0x00, 0x02}
	}
	greet := append([]byte{0x05, byte(len(methods))}, methods...)
	if _, err := c.Write(greet); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	var sel [2]byte
	if _, err := readFull(c, sel[:]); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	if sel[0] != 0x05 {
		return fmt.Errorf("%w: не socks5", ErrProxy)
	}
	switch sel[1] {
	case 0x00:
	case 0x02:
		if len(user) > 255 || len(pass) > 255 {
			return fmt.Errorf("%w: учётные данные длиннее 255 байт", ErrProxy)
		}
		buf := []byte{0x01, byte(len(user))}
		buf = append(buf, user...)
		buf = append(buf, byte(len(pass)))
		buf = append(buf, pass...)
		if _, err := c.Write(buf); err != nil {
			return fmt.Errorf("%w: %v", ErrProxy, err)
		}
		var ok [2]byte
		if _, err := readFull(c, ok[:]); err != nil {
			return fmt.Errorf("%w: %v", ErrProxy, err)
		}
		if ok[1] != 0x00 {
			return fmt.Errorf("%w: пара логин и пароль отвергнута", ErrProxy)
		}
	default:
		return fmt.Errorf("%w: метод аутентификации 0x%02x не поддерживается", ErrProxy, sel[1])
	}

	host, portS, err := net.SplitHostPort(dst)
	if err != nil {
		return fmt.Errorf("%w: адрес %q", ErrProxy, dst)
	}
	port, err := strconv.ParseUint(portS, 10, 16)
	if err != nil {
		return fmt.Errorf("%w: порт %q", ErrProxy, portS)
	}
	req := []byte{0x05, 0x01, 0x00}
	if ip := net.ParseIP(host); ip != nil {
		if v4 := ip.To4(); v4 != nil {
			req = append(req, 0x01)
			req = append(req, v4...)
		} else {
			req = append(req, 0x04)
			req = append(req, ip.To16()...)
		}
	} else {
		if len(host) > 255 {
			return fmt.Errorf("%w: имя хоста длиннее 255 байт", ErrProxy)
		}
		req = append(req, 0x03, byte(len(host)))
		req = append(req, host...)
	}
	var pb [2]byte
	binary.BigEndian.PutUint16(pb[:], uint16(port))
	req = append(req, pb[:]...)
	if _, err := c.Write(req); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}

	var head [4]byte
	if _, err := readFull(c, head[:]); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	if head[1] != 0x00 {
		return fmt.Errorf("%w: код ответа 0x%02x", ErrProxy, head[1])
	}
	// Связанный адрес читается и отбрасывается, иначе он останется в потоке
	// перед ClientHello.
	switch head[3] {
	case 0x01:
		var skip [4 + 2]byte
		_, err = readFull(c, skip[:])
	case 0x04:
		var skip [16 + 2]byte
		_, err = readFull(c, skip[:])
	case 0x03:
		var l [1]byte
		if _, err = readFull(c, l[:]); err == nil {
			skip := make([]byte, int(l[0])+2)
			_, err = readFull(c, skip)
		}
	default:
		return fmt.Errorf("%w: тип адреса 0x%02x", ErrProxy, head[3])
	}
	if err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	return nil
}

// httpConnect выполняет CONNECT к http прокси.
func httpConnect(c net.Conn, pu *url.URL, dst string) error {
	req := &http.Request{
		Method: http.MethodConnect,
		URL:    &url.URL{Opaque: dst},
		Host:   dst,
		Header: http.Header{},
	}
	if u := pu.User.Username(); u != "" {
		p, _ := pu.User.Password()
		req.Header.Set("Proxy-Authorization", "Basic "+basicAuth(u, p))
	}
	if err := req.Write(c); err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	br := bufio.NewReader(c)
	resp, err := http.ReadResponse(br, req)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrProxy, err)
	}
	defer func() {
		if resp.Body != nil {
			_ = resp.Body.Close()
		}
	}()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("%w: CONNECT вернул %d", ErrProxy, resp.StatusCode)
	}
	if br.Buffered() > 0 {
		// Прокси, дописавший байты за ответом CONNECT, сдвинул бы весь
		// последующий поток TLS. Такое соединение не используется.
		return fmt.Errorf("%w: лишние байты после ответа CONNECT", ErrProxy)
	}
	return nil
}

func basicAuth(user, pass string) string {
	return base64.StdEncoding.EncodeToString([]byte(user + ":" + pass))
}

func readFull(c net.Conn, b []byte) (int, error) { return io.ReadFull(c, b) }
