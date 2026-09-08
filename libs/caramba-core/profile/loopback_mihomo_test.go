//go:build mihomo

package profile

import (
	"net"
	"strconv"
	"strings"
	"testing"

	"github.com/metacubex/mihomo/hub/executor"
	"github.com/metacubex/mihomo/listener/inbound"
)

// Проверяем на настоящем ядре, что служебный инбаунд и секция geox-url это
// действительно то, что mihomo принимает и понимает, а не форма, которая
// разбирается только у нас.
//
// Конфиг разбирается executor.ParseWithPath (то же делает engine.Start), но
// туннель не поднимается: слушатель нам нужен как объект, а не как открытый
// сокет. Правила и dns вырезаются writeWithoutGeoRules по той же причине, что
// и в TestMihomoHonoursPinnedSelector: GEOIP,private заставил бы ядро качать
// geo-базы из сети.
func TestMihomoAcceptsLoopbackListener(t *testing.T) {
	for _, mode := range []TunnelMode{ModeTun, ModeProxy} {
		p := credentialed(t, DefaultPolicy())
		p.Mode = mode
		p.KillSwitch = false
		if mode == ModeTun {
			// Разбор TUN-инбаунда привилегий не требует, но auto-route на
			// машине сборщика лишний.
			p.Tun.AutoRoute = false
		}
		assembled, err := AssembleMihomoConfig([]byte(pinSample), p)
		if err != nil {
			t.Fatalf("%s: сборка: %v", mode, err)
		}
		cfg, err := executor.ParseWithPath(writeWithoutGeoRules(t, assembled))
		if err != nil {
			t.Fatalf("%s: разбор ядром: %v", mode, err)
		}
		l, ok := cfg.Listeners[LoopbackListenerName]
		if !ok || l == nil {
			t.Fatalf("%s: ядро не собрало слушатель %q: %v", mode, LoopbackListenerName, cfg.Listeners)
		}
		want := net.JoinHostPort(LoopbackHost, strconv.Itoa(p.LoopbackPort()))
		if got := l.RawAddress(); got != want {
			t.Fatalf("%s: слушатель на %q, ожидался %q", mode, got, want)
		}
		if !strings.EqualFold(l.Config().Name(), LoopbackListenerName) {
			t.Fatalf("%s: имя слушателя %q", mode, l.Config().Name())
		}
		// Пара логин-пароль доезжает до ядра как users слушателя. Без неё
		// инбаунд с ключом proxy это открытый релей на петле, и проверять это
		// надо на настоящем разборе: форма, которую понимаем только мы,
		// защищает ровно ничего.
		opt, ok := l.Config().(*inbound.MixedOption)
		if !ok {
			t.Fatalf("%s: конфиг слушателя %T, ожидался MixedOption", mode, l.Config())
		}
		if len(opt.Users) != 1 || opt.Users[0].Username != p.Proxy.LoopbackUser ||
			opt.Users[0].Password != p.Proxy.LoopbackPass {
			t.Fatalf("%s: ядро не приняло учётные данные слушателя: %#v", mode, opt.Users)
		}
	}
}

// TestMihomoAcceptsGeoXMirrors: секция geox-url доезжает до geodata как
// адреса, с которых ядро качало бы базы. Это и есть замена SetGeoIpUrl и
// компании: вызывать их руками не нужно, executor делает это сам, а конфиг
// остаётся проверяемым.
func TestMihomoAcceptsGeoXMirrors(t *testing.T) {
	p := DefaultPolicy()
	p.Mode = ModeProxy
	p.KillSwitch = false
	p.Bootstrap = BootstrapConfig{
		Managed:    true,
		GeoSiteURL: "https://m1.example.net/geo/GeoSite.dat",
		MmdbURL:    "https://m1.example.net/geo/geoip.metadb",
	}
	assembled, err := AssembleMihomoConfig([]byte(pinSample), p)
	if err != nil {
		t.Fatalf("сборка: %v", err)
	}
	cfg, err := executor.ParseWithPath(writeWithoutGeoRules(t, assembled))
	if err != nil {
		t.Fatalf("разбор ядром: %v", err)
	}
	if got := cfg.General.GeoXUrl.GeoSite; got != p.Bootstrap.GeoSiteURL {
		t.Fatalf("geosite=%q", got)
	}
	if got := cfg.General.GeoXUrl.Mmdb; got != p.Bootstrap.MmdbURL {
		t.Fatalf("mmdb=%q", got)
	}
	// Неназванный каталогом ресурс обязан остаться БЕЗ адреса: это отказ
	// качать, а не разрешение сходить на github.com за неподписанным файлом.
	if got := cfg.General.GeoXUrl.GeoIp; got != "" {
		t.Fatalf("geoip=%q, ожидался пустой адрес вместо умолчания ядра", got)
	}
	if got := cfg.General.GeoXUrl.ASN; got != "" {
		t.Fatalf("asn=%q, ожидался пустой адрес вместо умолчания ядра", got)
	}
}
