package profile

import (
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

// snifferOf собирает конфиг и возвращает его секцию sniffer. Второе значение
// ложно, когда секции нет вовсе — это отдельный ответ, а не пустая карта.
func snifferOf(t *testing.T, raw string, p Policy) (map[string]any, bool) {
	t.Helper()
	out, err := AssembleMihomoConfig([]byte(raw), p)
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	doc := map[string]any{}
	if err := yaml.Unmarshal(out, &doc); err != nil {
		t.Fatalf("разбор собранного конфига: %v", err)
	}
	s, ok := doc["sniffer"].(map[string]any)
	return s, ok
}

// Главное свойство этой правки: включённый блок рекламы обязан включить
// восстановление имени домена САМ.
//
// Без него галочка кладёт в конфиг живое правило и всё равно ничего не режет:
// браузер со своим DNS-over-HTTPS соединяется по IP, имени в соединении нет, и
// доменное правило не с чем сопоставить. Снято на устройстве — список рекламы
// загружен (911 записей), а googleads.g.doubleclick.net скачивается.
func TestBlockAdsTurnsOnTheSniffer(t *testing.T) {
	p := DefaultPolicy()
	p.Sniffer.Enable = false // политика про снифер молчит
	p.BlockAds = true

	s, ok := snifferOf(t, sampleConfig, p)
	if !ok {
		t.Fatal("секции sniffer нет: правило по домену не с чем сопоставлять, галочка блока рекламы пуста")
	}
	if s["enable"] != true {
		t.Errorf("enable = %v, ожидалось true", s["enable"])
	}
}

// Форма секции — это три решения, каждое из которых меняет поведение ядра
// (см. applySniffer). Тест фиксирует именно их, а не просто наличие секции.
func TestSnifferKeepsTheDestinationTheClientChose(t *testing.T) {
	s, ok := snifferOf(t, sampleConfig, DefaultPolicy())
	if !ok {
		t.Fatal("секции sniffer нет")
	}
	// Подмена назначения не нужна: доменные правила сопоставляются с
	// подсмотренным именем и без неё, а включённая подмена заставила бы ядро
	// разрешать имя заново и ломала бы привязку к узлу CDN и раздельный
	// горизонт DNS.
	if s["override-destination"] != false {
		t.Errorf("override-destination = %v, ожидалось false", s["override-destination"])
	}
	// Соединение, названное через fake-ip, уже названо: повторное чтение
	// первых байтов ничего не добавит, зато поставит крайний срок чтения на
	// каждое соединение.
	if s["force-dns-mapping"] != false {
		t.Errorf("force-dns-mapping = %v, ожидалось false", s["force-dns-mapping"])
	}
	// А вот это и есть разбираемый случай: имени нет и взять его больше негде.
	if s["parse-pure-ip"] != true {
		t.Errorf("parse-pure-ip = %v, ожидалось true", s["parse-pure-ip"])
	}
}

// Порты держатся узкими: разбор запускается только для безымянных соединений,
// но протокол, где первым говорит сервер, простоит крайний срок чтения впустую.
// Расширение этого списка — осознанное решение, а не побочный эффект правки.
func TestSnifferListensOnlyOnWebPorts(t *testing.T) {
	s, ok := snifferOf(t, sampleConfig, DefaultPolicy())
	if !ok {
		t.Fatal("секции sniffer нет")
	}
	sniff, ok := s["sniff"].(map[string]any)
	if !ok {
		t.Fatalf("sniff = %#v, ожидалась карта наборов", s["sniff"])
	}
	want := map[string][]string{
		"TLS":  {"443"},
		"QUIC": {"443"},
		"HTTP": {"80"},
	}
	for name, ports := range want {
		entry, ok := sniff[name].(map[string]any)
		if !ok {
			t.Errorf("нет набора %s: соответствующий трафик останется безымянным", name)
			continue
		}
		got, _ := entry["ports"].([]any)
		if len(got) != len(ports) {
			t.Errorf("%s: ports = %v, ожидалось %v", name, got, ports)
			continue
		}
		for i := range ports {
			// Ядро читает порты СТРОКАМИ (config.RawSniffingConfig.Ports —
			// []string): число здесь уронило бы разбор конфига целиком.
			if s, ok := got[i].(string); !ok || s != ports[i] {
				t.Errorf("%s: ports[%d] = %#v, ожидалась строка %q", name, i, got[i], ports[i])
			}
		}
	}
}

// Снифер выключается только явно — и тогда чужая секция из конфига подписки
// тоже убирается. Два источника правды про то, читает ли ядро первые байты,
// дают поведение, которого никто не выбирал.
func TestSnifferOffRemovesSubscriptionSection(t *testing.T) {
	const withForeignSniffer = `
proxies:
  - name: "DE Stealth"
    type: vless
    server: 1.2.3.4
    port: 443
proxy-groups:
  - name: CARAMBA
    type: select
    proxies: ["DE Stealth"]
sniffer:
  enable: true
  override-destination: true
rules:
  - MATCH,CARAMBA
`
	p := DefaultPolicy()
	p.Sniffer.Enable = false
	p.BlockAds = false

	if _, ok := snifferOf(t, withForeignSniffer, p); ok {
		t.Fatal("секция sniffer из подписки уцелела: политика клиента перестала быть единственным источником правды")
	}
}

// Умолчание — снифер включён, даже когда блок рекламы не просили: дыра
// «браузер сходил в чужой DoH и соединился по IP» обесценивает ВСЕ доменные
// правила, а не только рекламные (GEOSITE,youtube,PROXY промахивается так же).
func TestDefaultPolicySniffsDomains(t *testing.T) {
	p := DefaultPolicy()
	if p.BlockAds {
		t.Fatal("умолчание неожиданно включает блок рекламы — тест перестал проверять то, ради чего написан")
	}
	if !p.SniffDomains() {
		t.Fatal("умолчание не восстанавливает имя домена: доменные правила промахиваются по любому соединению, открытому по IP")
	}
}

// Прокси-режим тоже сниферит. Имя туда обычно приходит само (SOCKS5 и CONNECT
// адресуют по имени), но клиент, разрешивший имя сам и пришедший с IP, есть и
// здесь — и режим захвата трафика не повод обещать ему другую блокировку.
func TestSnifferAppliesInProxyMode(t *testing.T) {
	p := DefaultPolicy()
	p.Mode = ModeProxy
	if _, ok := snifferOf(t, sampleConfig, p); !ok {
		t.Fatal("в proxy-режиме секции sniffer нет")
	}
}

// Текст для экрана обязан назвать ОБА остатка, которые снифер не закрывает, и
// не обещать полноты.
//
// Проверено живым запросом через собранный этим кодом конфиг: соединение к
// голому адресу 172.217.77.156 без имени в ClientHello доехало до сервера
// (ответ 404 от Google, 1570 байт), тогда как то же соединение с именем
// получило GeoSite(category-ads-all) => REJECT. Пока этот остаток есть,
// формулировка «вся реклама заблокирована» была бы ложью.
func TestAdBlockLimitNoteNamesWhatSlipsThrough(t *testing.T) {
	for _, want := range []string{"IP", "ECH"} {
		if !strings.Contains(AdBlockLimitNote, want) {
			t.Errorf("в тексте про границы блока рекламы нет %q: %q", want, AdBlockLimitNote)
		}
	}
	for _, forbidden := range []string{"вся реклама", "полностью", "гарант"} {
		if strings.Contains(strings.ToLower(AdBlockLimitNote), forbidden) {
			t.Errorf("текст обещает больше сделанного (%q): %q", forbidden, AdBlockLimitNote)
		}
	}
}

// Пользовательские исключения доезжают до конфига: без них у оператора нет
// способа снять подмену имени с клиента, которому она вредит.
func TestSnifferSkipDomainsReachConfig(t *testing.T) {
	p := DefaultPolicy()
	p.Sniffer.SkipDomains = []string{"Mijia Cloud"}

	s, ok := snifferOf(t, sampleConfig, p)
	if !ok {
		t.Fatal("секции sniffer нет")
	}
	got, _ := s["skip-domain"].([]any)
	if len(got) != 1 || got[0] != "Mijia Cloud" {
		t.Fatalf("skip-domain = %v, ожидалось [Mijia Cloud]", s["skip-domain"])
	}
}
