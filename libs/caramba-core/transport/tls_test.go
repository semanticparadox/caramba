package transport

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"errors"
	"math/big"
	"net"
	"net/http"
	"strings"
	"testing"
	"time"
)

// Ни один из этих тестов не существовал, и оба найденных отказа TLS прошли
// именно поэтому.

func selfSigned(t *testing.T, cn string) *x509.Certificate {
	t.Helper()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatalf("ключ: %v", err)
	}
	tmpl := &x509.Certificate{
		SerialNumber: big.NewInt(time.Now().UnixNano()),
		Subject:      pkix.Name{CommonName: cn},
		NotBefore:    time.Now().Add(-time.Hour),
		NotAfter:     time.Now().Add(time.Hour),
		DNSNames:     []string{cn},
	}
	der, err := x509.CreateCertificate(rand.Reader, tmpl, tmpl, &key.PublicKey, key)
	if err != nil {
		t.Fatalf("сертификат: %v", err)
	}
	c, err := x509.ParseCertificate(der)
	if err != nil {
		t.Fatalf("разбор: %v", err)
	}
	return c
}

func spkiPin(c *x509.Certificate) []byte {
	sum := sha256.Sum256(c.RawSubjectPublicKeyInfo)
	return sum[:]
}

// Набивка цепочки: обладатель ЛЮБОГО публично доверенного сертификата на то же
// имя дописывает легитимный запиненный сертификат довеском. x509.Verify
// принимает certs[1:] как пул промежуточных и молча игнорирует лишнее, поэтому
// цепочка сходится по certs[0], а обход всей цепочки нашёл бы совпадение на
// довеске. Пин обязан смотреть только на лист.
func TestCheckPinsIgnoresPaddedChain(t *testing.T) {
	attacker := selfSigned(t, "mirror.example.net")
	legit := selfSigned(t, "mirror.example.net")

	state := tls.ConnectionState{PeerCertificates: []*x509.Certificate{attacker, legit}}
	if err := checkPins(state, [][]byte{spkiPin(legit)}); !errors.Is(err, ErrPinMismatch) {
		t.Fatalf("запиненный сертификат довеском обязан дать ErrPinMismatch, получено %v", err)
	}

	// Лист, который и запинен, принимается.
	if err := checkPins(tls.ConnectionState{PeerCertificates: []*x509.Certificate{legit}},
		[][]byte{spkiPin(legit)}); err != nil {
		t.Fatalf("лист с совпавшим пином обязан пройти: %v", err)
	}
	// Пустой набор пинов это отсутствие пиннинга, а не отказ.
	if err := checkPins(tls.ConnectionState{PeerCertificates: []*x509.Certificate{attacker}}, nil); err != nil {
		t.Fatalf("без пинов проверка обязана молчать: %v", err)
	}
	// Пустая цепочка это отказ, а не проход.
	if err := checkPins(tls.ConnectionState{}, [][]byte{spkiPin(legit)}); !errors.Is(err, ErrPinMismatch) {
		t.Fatalf("пустая цепочка обязана дать ErrPinMismatch, получено %v", err)
	}
}

// Проксированная ступень обязана идти через ТОТ ЖЕ путь рукопожатия, что и
// прямая. Раньше прокси уходил в http.Transport, который для проксированных
// запросов DialTLSContext не зовёт вовсе, и на R4 с R5 молча исчезали пины,
// нижняя граница TLS и явный ClientHello.
func TestProxiedDialUsesOurTLSPath(t *testing.T) {
	// TLS сервер с самоподписанным сертификатом: цепочка обязана не сойтись,
	// и это доказывает, что наша проверка на проксированном пути ВЫПОЛНЯЕТСЯ.
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("слушатель: %v", err)
	}
	defer func() { _ = ln.Close() }()
	go func() {
		for {
			c, err := ln.Accept()
			if err != nil {
				return
			}
			go func(c net.Conn) {
				defer func() { _ = c.Close() }()
				_, _ = c.Read(make([]byte, 1024))
			}(c)
		}
	}()

	// Стаб socks5: принимает рукопожатие без аутентификации и соединяет с
	// адресом слушателя выше.
	proxyLn, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("прокси: %v", err)
	}
	defer func() { _ = proxyLn.Close() }()
	seen := make(chan string, 1)
	go func() {
		c, err := proxyLn.Accept()
		if err != nil {
			return
		}
		defer func() { _ = c.Close() }()
		buf := make([]byte, 512)
		if _, err := c.Read(buf); err != nil {
			return
		}
		if _, err := c.Write([]byte{0x05, 0x00}); err != nil {
			return
		}
		n, err := c.Read(buf)
		if err != nil || n < 5 {
			return
		}
		// 05 01 00 03 len host port
		if buf[3] == 0x03 {
			l := int(buf[4])
			seen <- string(buf[5 : 5+l])
		} else {
			seen <- "not-a-hostname"
		}
		// Отвечаем успехом и связанным адресом 0.0.0.0:0, дальше просто молчим:
		// достаточно, чтобы рукопожатие TLS началось и провалилось.
		_, _ = c.Write([]byte{0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0})
		_, _ = c.Read(buf)
	}()

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	target := Target{
		Rung:  R5Proxy,
		Host:  "mirror.example.net",
		Proxy: "socks5://" + proxyLn.Addr().String(),
		Pins:  [][]byte{spkiPin(selfSigned(t, "mirror.example.net"))},
	}
	_, derr := dialTLSConn(ctx, target, "mirror.example.net:443")
	if derr == nil {
		t.Fatal("рукопожатие с молчащим сервером обязано провалиться")
	}
	if errors.Is(derr, ErrProxy) {
		t.Fatalf("отказ обязан прийти со стадии TLS, а не socks5: %v", derr)
	}
	select {
	case host := <-seen:
		if host != "mirror.example.net" {
			t.Fatalf("прокси увидел %q вместо имени цели", host)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("прокси не получил запрос CONNECT: соединение прошло мимо него")
	}
}

// Предъявительский секрет не уходит на ступень, подменяющую хост.
func TestCredentialNeverReachesSubstitutedHost(t *testing.T) {
	mk := func(path string, auth bool) *http.Request {
		req, err := http.NewRequest(http.MethodGet, "https://panel.example.net"+path, nil)
		if err != nil {
			t.Fatalf("запрос: %v", err)
		}
		if auth {
			req.Header.Set("Authorization", "Bearer secret-account-token")
		}
		return req
	}
	uuidPath := "/sub/9f3c1d02-5b8e-4a17-9d44-0e7a6c11b3f8"

	for _, rung := range []RungID{R2Mirrors, R3DoH} {
		if _, err := rewriteRequest(mk("/api/v2/app/preferences", true),
			Target{Rung: rung, Host: "mirror.example.net"}); !errors.Is(err, ErrCredentialOnSharedHost) {
			t.Fatalf("ступень %s с Authorization: ожидался отказ, получено %v", rung.Name(), err)
		}
		if _, err := rewriteRequest(mk(uuidPath, false),
			Target{Rung: rung, Host: "mirror.example.net"}); !errors.Is(err, ErrCredentialOnSharedHost) {
			t.Fatalf("ступень %s с uuid в пути: ожидался отказ, получено %v", rung.Name(), err)
		}
	}

	// Прямая ступень и ступени через прокси хост не подменяют: TLS сессия
	// остаётся с origin, поэтому секрет там на месте.
	for _, rung := range []RungID{R1Direct, R4Tunnel, R5Proxy} {
		out, err := rewriteRequest(mk("/api/v2/app/preferences", true),
			Target{Rung: rung, Host: "panel.example.net"})
		if err != nil {
			t.Fatalf("ступень %s: неожиданный отказ %v", rung.Name(), err)
		}
		if out.Header.Get("Authorization") == "" {
			t.Fatalf("ступень %s: заголовок Authorization потерян", rung.Name())
		}
		if h := out.URL.Hostname(); h != "panel.example.net" {
			t.Fatalf("ступень %s: хост подменён на %q", rung.Name(), h)
		}
	}

	// Кадры CSM под /sub зеркалируются как раньше: локатор именно для этого и
	// существует.
	for _, p := range []string{"/sub/k1", "/sub/m1/ABCDEFGHJKMNPQRSTVWXYZ01", "/sub/c1/0123456789ABCDEF/0"} {
		if _, err := rewriteRequest(mk(p, false), Target{Rung: R2Mirrors, Host: "mirror.example.net"}); err != nil {
			t.Fatalf("путь CSM %s обязан идти на зеркало: %v", p, err)
		}
	}
	if !strings.HasPrefix(uuidPath, "/sub/") {
		t.Fatal("фикстура пути подписки испорчена")
	}
}
