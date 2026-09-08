package api

import (
	"context"
	"crypto/sha256"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/auth"
	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
	"github.com/semanticparadox/caramba/libs/caramba-core/transport"
)

// newBootstrapCore собирает ядро с рабочим каталогом внутри t.TempDir().
func newBootstrapCore(t *testing.T) *Core {
	t.Helper()
	dir := t.TempDir()
	c, err := NewCore(Config{
		PanelBaseURL:   "https://panel.example.net",
		WorkDir:        dir,
		TokenStorePath: filepath.Join(dir, "tokens.json"),
	})
	if err != nil {
		t.Fatalf("ядро: %v", err)
	}
	return c
}

// guardOver собирает страж по каталогу с одним подписанным ресурсом.
func guardOver(name, path string, body []byte) *transport.ResourceGuard {
	sum := sha256.Sum256(body)
	cat := &csm.Catalog{
		RS: []csm.Resource{{Name: name, URL: path, Hash: sum[:], Interval: 43200}},
	}
	return transport.NewResourceGuard(cat, 1<<transport.CapResourceHashes)
}

// TestMaterializeAcceptsMatchingBytes: сошедшийся файл берётся с диска и сети
// не стоит. Проверка идёт и на уже лежащий файл: диск не доверенная среда.
func TestMaterializeAcceptsMatchingBytes(t *testing.T) {
	c := newBootstrapCore(t)
	body := []byte("example.com\nexample.org\n")
	g := guardOver("ru-blocked", "/rs/ru-blocked", body)
	dst := filepath.Join(c.workDir, "ru-blocked")
	if err := os.WriteFile(dst, body, 0o600); err != nil {
		t.Fatal(err)
	}
	// Контекст уже отменён: если функция полезет в сеть, тест это заметит.
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := c.materializeResource(ctx, g, c.ladder, "https://panel.example.net", "ru-blocked", dst); err != nil {
		t.Fatalf("сошедшийся файл отвергнут: %v", err)
	}
	got, err := os.ReadFile(dst)
	if err != nil || string(got) != string(body) {
		t.Fatalf("файл повреждён: %q %v", got, err)
	}
}

// TestMaterializeRefusesMismatchedBytes это главная проверка инварианта 12.
// Файл, который не сошёлся с подписанным sha256, обязан быть удалён, а сборка
// обязана отказать. Оставить его на диске значило бы ровно тот непроверенный
// откат, который запрещён.
func TestMaterializeRefusesMismatchedBytes(t *testing.T) {
	c := newBootstrapCore(t)
	g := guardOver("ru-blocked", "/rs/ru-blocked", []byte("подписанное содержимое"))
	dst := filepath.Join(c.workDir, "ru-blocked")
	if err := os.WriteFile(dst, []byte("подменённое содержимое"), 0o600); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	err := c.materializeResource(ctx, g, c.ladder, "https://panel.example.net", "ru-blocked", dst)
	if err == nil {
		t.Fatal("несовпадение хеша принято")
	}
	if _, statErr := os.Stat(dst); !os.IsNotExist(statErr) {
		t.Fatalf("непроверенный файл остался на диске: %v", statErr)
	}
}

// TestMaterializeRefusesUnknownResource: ресурс, которого каталог не назвал,
// не качается вовсе: подписанного хеша для него нет.
func TestMaterializeRefusesUnknownResource(t *testing.T) {
	c := newBootstrapCore(t)
	g := guardOver("ru-blocked", "/rs/ru-blocked", []byte("x"))
	dst := filepath.Join(c.workDir, "ir-blocked")
	err := c.materializeResource(context.Background(), g, c.ladder, "https://panel.example.net", "ir-blocked", dst)
	if !errors.Is(err, transport.ErrResourceUnknown) {
		t.Fatalf("ошибка %v, ожидалась ErrResourceUnknown", err)
	}
	if _, statErr := os.Stat(dst); !os.IsNotExist(statErr) {
		t.Fatal("файл создан для ресурса вне каталога")
	}
}

// TestMaterializeRefusesWhenHashesNotOffered: снятый бит 6 означает откат на
// встроенные данные ядра, а не загрузку по адресам каталога без подписи.
func TestMaterializeRefusesWhenHashesNotOffered(t *testing.T) {
	c := newBootstrapCore(t)
	sum := sha256.Sum256([]byte("x"))
	cat := &csm.Catalog{RS: []csm.Resource{{Name: "ru-blocked", URL: "/rs/ru-blocked", Hash: sum[:]}}}
	g := transport.NewResourceGuard(cat, 0)
	err := c.materializeResource(context.Background(), g, c.ladder, "https://panel.example.net",
		"ru-blocked", filepath.Join(c.workDir, "ru-blocked"))
	if !errors.Is(err, transport.ErrResourcesDisabled) {
		t.Fatalf("ошибка %v, ожидалась ErrResourcesDisabled", err)
	}
}

// TestPrepareBootstrapWithoutCatalog: без каталога поведение прежнее: пул это
// адрес панели, geox-url не трогается, отказов нет.
func TestPrepareBootstrapWithoutCatalog(t *testing.T) {
	c := newBootstrapCore(t)
	plan, err := c.prepareBootstrap(context.Background(), []string{"ru-blocked"})
	if err != nil {
		t.Fatalf("подготовка без каталога отказала: %v", err)
	}
	if plan.boot.Managed {
		t.Fatal("geox-url перехвачен без доверенного каталога")
	}
	if plan.pool.Verified {
		t.Fatal("режим проверенных файлов включён без хешей")
	}
	if len(plan.pool.Bases) != 1 || plan.pool.Bases[0] != "https://panel.example.net" {
		t.Fatalf("пул %v", plan.pool.Bases)
	}
	if plan.pool.Proxy == "" {
		t.Fatal("списки поедут мимо туннеля: ключ proxy пуст")
	}
}

// TestMirrorURLRefusesAbsoluteSignedPath: 03-WIRE.md 14.3 запрещает: ни одно подписанное
// поле не несёт абсолютный URL, и ветки «а вдруг это уже URL» здесь нет.
func TestMirrorURLRefusesAbsoluteSignedPath(t *testing.T) {
	if _, err := mirrorURL([]string{"https://m1.example.net"}, "https://o", "https://evil.example/x"); err == nil {
		t.Fatal("абсолютный подписанный адрес принят")
	}
	got, err := mirrorURL([]string{"https://m1.example.net"}, "https://panel.example.net", "/geo/GeoSite.dat")
	if err != nil {
		t.Fatalf("резолв пути: %v", err)
	}
	if got != "https://m1.example.net/geo/GeoSite.dat" {
		t.Fatalf("адрес %q, ожидалось первое зеркало", got)
	}
}

// TestSafeRuleSetPath: имя приходит подписанным, но подпись говорит «это
// написал оператор», а не «это безопасно как имя файла».
func TestSafeRuleSetPath(t *testing.T) {
	for _, bad := range []string{"", ".", "..", "../etc/passwd", "a/b", "a\\b", "a..b", "имя"} {
		if _, err := safeRuleSetPath(bad); err == nil {
			t.Fatalf("имя %q принято", bad)
		}
	}
	got, err := safeRuleSetPath("ru-blocked-ip")
	if err != nil {
		t.Fatalf("нормальное имя отвергнуто: %v", err)
	}
	if !strings.HasPrefix(got, ruleSetDir+"/") {
		t.Fatalf("путь %q вне каталога списков", got)
	}
}

// TestLadderMirrorOrigins: пул зеркал отдаётся в порядке пула и без дублей,
// именно он подставляется вместо {BASE}.
func TestLadderMirrorOrigins(t *testing.T) {
	l := transport.NewLadder(transport.NewNetExchange(auth.DefaultUserAgent))
	cat := &csm.Catalog{
		Mir: []csm.Mirror{{H: "m1.example.net"}, {H: "m2.example.net"}, {H: "m1.example.net"}},
	}
	if err := l.ApplyCatalog(cat, 1<<4); err != nil {
		t.Fatalf("каталог: %v", err)
	}
	got := l.MirrorOrigins()
	if len(got) != 2 || got[0] != "https://m1.example.net" || got[1] != "https://m2.example.net" {
		t.Fatalf("пул %v", got)
	}
}

// TestLadderDoHURLs: загрузочный DoH берётся из каталога, а не из зашитых
// 1.1.1.1 и 8.8.8.8, которые в РФ блокируются с 2026-08-26. Пустой path
// достраивается до /dns-query.
func TestLadderDoHURLs(t *testing.T) {
	l := transport.NewLadder(transport.NewNetExchange(auth.DefaultUserAgent))
	cat := &csm.Catalog{
		Mir: []csm.Mirror{{H: "m1.example.net"}},
		DoH: []csm.DoHEntry{
			{H: "doh.example.net", Path: "/q"},
			{H: "doh2.example.net"},
			{H: "doh.example.net", Path: "/q"},
		},
	}
	if err := l.ApplyCatalog(cat, 1<<4|1<<5); err != nil {
		t.Fatalf("каталог: %v", err)
	}
	got := l.DoHURLs()
	want := []string{"https://doh.example.net/q", "https://doh2.example.net/dns-query"}
	if len(got) != len(want) {
		t.Fatalf("резолверы %v", got)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("резолвер %d = %q, ожидался %q", i, got[i], want[i])
		}
	}
	for _, u := range got {
		if strings.Contains(u, "1.1.1.1") || strings.Contains(u, "8.8.8.8") {
			t.Fatalf("заблокированный резолвер: %s", u)
		}
	}
}
