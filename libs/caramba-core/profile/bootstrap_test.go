package profile

import (
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

// TestGeoXUntouchedWithoutCatalog: без доверенного каталога секции geox-url
// быть не должно. Пустая секция означала бы отказ качать geo, а клиент без
// каталога права так поступать не имеет: он остаётся на умолчаниях ядра.
func TestGeoXUntouchedWithoutCatalog(t *testing.T) {
	doc := assembleDoc(t, DefaultPolicy())
	if _, ok := doc["geox-url"]; ok {
		t.Fatalf("geox-url записан без каталога: %#v", doc["geox-url"])
	}
}

// TestGeoXPointsAtMirrors: при доверенном каталоге адреса geo-баз переезжают
// на зеркала, а ресурс, которого каталог не назвал, получает ПУСТОЙ адрес,
// отказ, а не тихая докачка с github.com.
func TestGeoXPointsAtMirrors(t *testing.T) {
	p := DefaultPolicy()
	p.Bootstrap = BootstrapConfig{
		Managed:    true,
		GeoSiteURL: "https://m1.example.net/geo/GeoSite.dat",
		MmdbURL:    "https://m1.example.net/geo/geoip.metadb",
	}
	doc := assembleDoc(t, p)
	geox, ok := doc["geox-url"].(map[string]any)
	if !ok {
		t.Fatalf("geox-url отсутствует или не карта: %#v", doc["geox-url"])
	}
	if got, _ := geox["geosite"].(string); got != p.Bootstrap.GeoSiteURL {
		t.Fatalf("geosite=%q", got)
	}
	if got, _ := geox["mmdb"].(string); got != p.Bootstrap.MmdbURL {
		t.Fatalf("mmdb=%q", got)
	}
	for _, key := range []string{"geoip", "asn"} {
		got, ok := geox[key]
		if !ok {
			t.Fatalf("ключ %s не записан: неназванный ресурс обязан получить пустой адрес, а не умолчание ядра", key)
		}
		if s, _ := got.(string); s != "" {
			t.Fatalf("%s=%q, ожидался пустой адрес", key, s)
		}
	}
	raw, _ := yaml.Marshal(doc)
	if strings.Contains(string(raw), "github.com") {
		t.Fatalf("в собранном конфиге остался github.com:\n%s", raw)
	}
}

// TestProbeTargetFromCatalog: цель url-test берётся из каталога. Зашитый
// gstatic.com в РФ заблокирован, и группа объявляла мёртвыми все узлы.
func TestProbeTargetFromCatalog(t *testing.T) {
	p := DefaultPolicy()
	p.Protocol = "VLESS-Reality"
	p.Bootstrap.ProbeURL = "https://m1.example.net/"
	doc := assembleDoc(t, p)
	groups, _ := doc["proxy-groups"].([]any)
	var found bool
	for _, g := range groups {
		m, _ := g.(map[string]any)
		if name, _ := m["name"].(string); name != protoGroupName {
			continue
		}
		found = true
		if got, _ := m["url"].(string); got != "https://m1.example.net/" {
			t.Fatalf("цель пробы %q", got)
		}
	}
	if !found {
		t.Fatal("служебная группа протокола не собрана")
	}
	raw, _ := yaml.Marshal(doc)
	if strings.Contains(string(raw), "gstatic.com") {
		t.Fatalf("gstatic.com остался в конфиге:\n%s", raw)
	}
}

// TestDefaultResolversAreNotTheBlockedPair: 1.1.1.1 и 8.8.8.8 блокируются, а
// порт 853 закрыт целиком. Умолчание, которое их называет, оставляет клиента
// без DNS ровно там, где он нужен.
func TestDefaultResolversAreNotTheBlockedPair(t *testing.T) {
	p := DefaultPolicy()
	all := append(append([]string{}, p.DNS.Nameservers...), p.DNS.FallbackNameservers...)
	all = append(all, p.DNS.DirectNameservers...)
	all = append(all, p.DNS.ProxyServerNameservers...)
	if len(all) == 0 {
		t.Fatal("умолчание без единого резолвера")
	}
	for _, ns := range all {
		if strings.Contains(ns, "1.1.1.1") || strings.Contains(ns, "8.8.8.8") {
			t.Fatalf("заблокированный резолвер в умолчании: %s", ns)
		}
		if !strings.HasPrefix(ns, "https://") {
			t.Fatalf("резолвер не DoH: %s (порт 853 закрыт, полевое указание: только https)", ns)
		}
	}
}

// TestDNSSplitEmitted: национальный раскол резолверов обязан попадать в конфиг
// отдельными ключами, иначе домашние имена уезжают к чужому резолверу.
func TestDNSSplitEmitted(t *testing.T) {
	p := DefaultPolicy()
	p.DNS.Nameservers = []string{"https://doh.operator.example/dns-query"}
	p.DNS.DirectNameservers = DomesticResolvers("RU")
	p.DNS.ProxyServerNameservers = DomesticResolvers("RU")
	doc := assembleDoc(t, p)
	dns, ok := doc["dns"].(map[string]any)
	if !ok {
		t.Fatalf("секция dns отсутствует: %#v", doc["dns"])
	}
	for _, key := range []string{"nameserver", "direct-nameserver", "proxy-server-nameserver"} {
		v, _ := dns[key].([]any)
		if len(v) == 0 {
			t.Fatalf("ключ %s пуст: %#v", key, dns[key])
		}
	}
	got, _ := dns["direct-nameserver"].([]any)
	if s, _ := got[0].(string); !strings.Contains(s, "yandex") {
		t.Fatalf("домашний резолвер РФ = %q", s)
	}
}

// TestDomesticResolversFallback: неизвестная страна не остаётся без резолвера.
func TestDomesticResolversFallback(t *testing.T) {
	if got := DomesticResolvers("ZZ"); len(got) == 0 {
		t.Fatal("неизвестная страна осталась без резолвера")
	}
	if got := DomesticResolvers("ru"); len(got) == 0 || !strings.Contains(got[0], "yandex") {
		t.Fatalf("регистр кода страны учтён неверно: %v", got)
	}
}

// TestApplyBootstrapDNSReplacesDefaultsOnly: подписанные резолверы вытесняют
// компилируемое умолчание, но НЕ выбор пользователя. Операторское изменение
// пользовательского поля проходит карточкой «оставить или вернуть» в
// приложении, а не молча здесь.
func TestApplyBootstrapDNSReplacesDefaultsOnly(t *testing.T) {
	catalog := []string{"https://doh1.operator.example/dns-query", "https://doh2.operator.example/q"}

	p := DefaultPolicy()
	p.ApplyBootstrapDNS(catalog, "RU")
	if len(p.DNS.Nameservers) != 2 || p.DNS.Nameservers[0] != catalog[0] {
		t.Fatalf("умолчание не заменено каталогом: %v", p.DNS.Nameservers)
	}

	chosen := DefaultPolicy()
	chosen.DNS.Nameservers = []string{"https://my.resolver.example/dns-query"}
	chosen.ApplyBootstrapDNS(catalog, "RU")
	if chosen.DNS.Nameservers[0] != "https://my.resolver.example/dns-query" {
		t.Fatalf("выбор пользователя затёрт каталогом: %v", chosen.DNS.Nameservers)
	}

	// Пустой каталог ничего не меняет.
	untouched := DefaultPolicy()
	untouched.ApplyBootstrapDNS(nil, "")
	if !sameList(untouched.DNS.Nameservers, DefaultNameservers) {
		t.Fatalf("пустой каталог изменил резолверы: %v", untouched.DNS.Nameservers)
	}
}

// TestApplyBootstrapDNSCountrySplit: домашняя половина раскола берётся по
// стране пресета, и тоже только поверх умолчания.
func TestApplyBootstrapDNSCountrySplit(t *testing.T) {
	p := DefaultPolicy()
	p.ApplyBootstrapDNS(nil, "CN")
	if len(p.DNS.DirectNameservers) == 0 || !strings.Contains(p.DNS.DirectNameservers[0], "doh.pub") {
		t.Fatalf("домашний резолвер CN не применён: %v", p.DNS.DirectNameservers)
	}
	if len(p.DNS.ProxyServerNameservers) == 0 || !strings.Contains(p.DNS.ProxyServerNameservers[0], "doh.pub") {
		t.Fatalf("резолвер имён узлов не применён: %v", p.DNS.ProxyServerNameservers)
	}

	chosen := DefaultPolicy()
	chosen.DNS.DirectNameservers = []string{"https://my.local/dns-query"}
	chosen.ApplyBootstrapDNS(nil, "CN")
	if chosen.DNS.DirectNameservers[0] != "https://my.local/dns-query" {
		t.Fatalf("выбор пользователя затёрт страной: %v", chosen.DNS.DirectNameservers)
	}
}
