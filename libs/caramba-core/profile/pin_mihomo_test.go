//go:build mihomo

package profile

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/metacubex/mihomo/hub/executor"
	"gopkg.in/yaml.v3"
)

// nower — интерфейс select-группы mihomo: Now() отдаёт текущий выбор.
type nower interface{ Now() string }

// Проверяем на настоящем ядре, что перестановка участников селектора реально
// задаёт выбор по умолчанию: у select-группы mihomo нет ключа default, выбор
// инициализируется первым участником (NewSelector: selected = proxies[0]).
//
// Конфиг разбирается executor.ParseWithPath (то же, что делает engine.Start),
// но туннель не поднимается: нам нужен только собранный объект группы. Секция
// rules перед разбором заменяется на MATCH,CARAMBA, а секция dns удаляется:
// правило GEOIP,private и fallback-фильтр DNS заставили бы ядро качать geo-базы
// из сети, а к проверке пина это отношения не имеет.
func TestMihomoHonoursPinnedSelector(t *testing.T) {
	p := DefaultPolicy()
	p.Mode = ModeProxy // без TUN: разбору конфига не нужны привилегии
	p.KillSwitch = false

	for _, want := range []string{"NL-1", "DE-2"} {
		assembled, err := AssembleMihomoConfigPinned([]byte(pinSample), p, want)
		if err != nil {
			t.Fatalf("assemble(%s): %v", want, err)
		}
		path := writeWithoutGeoRules(t, assembled)

		cfg, err := executor.ParseWithPath(path)
		if err != nil {
			t.Fatalf("ParseWithPath(%s): %v", want, err)
		}
		group, ok := cfg.Proxies[CarambaSelector]
		if !ok || group == nil {
			t.Fatalf("группа %s отсутствует в разобранном конфиге", CarambaSelector)
		}
		// tunnel.Proxies()/cfg.Proxies отдают обёртку *adapter.Proxy: Now()
		// живёт на самой группе, поэтому разворачиваем через Adapter().
		sel, ok := group.Adapter().(nower)
		if !ok {
			t.Fatalf("группа %s не является select-группой: %T", CarambaSelector, group.Adapter())
		}
		if got := sel.Now(); got != want {
			t.Fatalf("ядро выбрало %q, ожидался закреплённый узел %q", got, want)
		}
	}
}

// writeWithoutGeoRules пишет конфиг во временный файл, заменив rules на
// единственное MATCH,CARAMBA и убрав dns (см. комментарий выше).
func writeWithoutGeoRules(t *testing.T, assembled []byte) string {
	t.Helper()
	doc := map[string]any{}
	if err := yaml.Unmarshal(assembled, &doc); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	doc["rules"] = []string{"MATCH," + CarambaSelector}
	delete(doc, "dns")
	out, err := yaml.Marshal(doc)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	path := filepath.Join(t.TempDir(), "config.yaml")
	if err := os.WriteFile(path, out, 0o600); err != nil {
		t.Fatalf("write: %v", err)
	}
	return path
}
