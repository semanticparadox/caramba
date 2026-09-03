package profile

import (
	"testing"

	"gopkg.in/yaml.v3"
)

// loopbackYAML это минимальная подписка: узлы и селектор, ничего лишнего.
const loopbackYAML = `
mixed-port: 7890
proxies:
  - {name: "N1", type: vless, server: a.example.com, port: 443}
proxy-groups:
  - {name: CARAMBA, type: select, proxies: ["N1", "DIRECT"]}
rules:
  - MATCH,CARAMBA
`

// listenerNamed достаёт запись слушателя по имени.
func listenerNamed(t *testing.T, doc map[string]any, name string) map[string]any {
	t.Helper()
	list, ok := doc["listeners"].([]any)
	if !ok {
		t.Fatalf("секция listeners отсутствует или не список: %#v", doc["listeners"])
	}
	for _, it := range list {
		m, ok := it.(map[string]any)
		if !ok {
			continue
		}
		if got, _ := m["name"].(string); got == name {
			return m
		}
	}
	t.Fatalf("слушатель %q не найден среди %#v", name, list)
	return nil
}

// credentialed возвращает политику с выпущенной парой логин-пароль служебного
// инбаунда. Без пары слушатель не собирается вовсе, и это отдельно проверяет
// TestLoopbackRefusesWithoutCredential.
func credentialed(t *testing.T, p Policy) Policy {
	t.Helper()
	user, pass, err := NewLoopbackCredential()
	if err != nil {
		t.Fatalf("выпуск пары: %v", err)
	}
	p.Proxy.LoopbackUser, p.Proxy.LoopbackPass = user, pass
	return p
}

func assembleDoc(t *testing.T, p Policy) map[string]any {
	t.Helper()
	out, err := AssembleMihomoConfig([]byte(loopbackYAML), p)
	if err != nil {
		t.Fatalf("сборка: %v", err)
	}
	doc := map[string]any{}
	if err := yaml.Unmarshal(out, &doc); err != nil {
		t.Fatalf("разбор собранного: %v", err)
	}
	return doc
}

// TestLoopbackInboundInBothModes это то, ради чего это вообще писалось.
// 02-SPEC.md 8.2 требует mixed-инбаунд на петле в ОБОИХ режимах: без него у
// ступени R4 на Android нет пути и лестница вечно рисует not_configured.
func TestLoopbackInboundInBothModes(t *testing.T) {
	for _, mode := range []TunnelMode{ModeTun, ModeProxy} {
		p := credentialed(t, DefaultPolicy())
		p.Mode = mode
		doc := assembleDoc(t, p)
		l := listenerNamed(t, doc, LoopbackListenerName)
		if got, _ := l["type"].(string); got != "mixed" {
			t.Fatalf("%s: тип слушателя %q, ожидался mixed", mode, got)
		}
		if got, _ := l["listen"].(string); got != LoopbackHost {
			t.Fatalf("%s: привязка %q, ожидалась %q", mode, got, LoopbackHost)
		}
		if got := l["port"]; got != DefaultLoopbackPort {
			t.Fatalf("%s: порт %v, ожидался %d", mode, got, DefaultLoopbackPort)
		}
		// Без этого ключа R4 попадала бы под пресет маршрутизации и у «умного»
		// пресета РФ ушла бы DIRECT, то есть стала бы копией R1.
		if got, _ := l["proxy"].(string); got != CarambaSelector {
			t.Fatalf("%s: слушатель без принудительного исходящего: proxy=%q", mode, got)
		}
		if addr := p.LoopbackAddr(); addr != "127.0.0.1:7893" {
			t.Fatalf("%s: адрес для лестницы %q", mode, addr)
		}
		// Без users это открытый релей: на Android в него ходит любое
		// приложение с разрешением INTERNET, и его трафик уходит через
		// оплаченный узел мимо правил.
		users, ok := l["users"].([]any)
		if !ok || len(users) != 1 {
			t.Fatalf("%s: слушатель без учётных данных: %#v", mode, l["users"])
		}
		u, _ := users[0].(map[string]any)
		if u["username"] != p.Proxy.LoopbackUser || u["password"] != p.Proxy.LoopbackPass {
			t.Fatalf("%s: в слушателе не та пара: %#v", mode, u)
		}
		// Лестница обязана получить пару вместе с адресом, иначе она
		// постучится в слушатель и получит отказ аутентификации.
		want := "socks5://" + p.Proxy.LoopbackUser + ":" + p.Proxy.LoopbackPass + "@127.0.0.1:7893"
		if got := p.LoopbackProxyURL(); got != want {
			t.Fatalf("%s: адрес с учётными данными %q, ожидался %q", mode, got, want)
		}
	}
}

// TestLoopbackRefusesWithoutCredential: политика без пары логин-пароль не
// собирает слушателя ВООБЩЕ и не отдаёт лестнице адреса. Ступень R4 в этом
// случае недоступна, и это правильная сторона отказа: открытый релей на петле
// хуже отсутствующей ступени.
func TestLoopbackRefusesWithoutCredential(t *testing.T) {
	for _, mode := range []TunnelMode{ModeTun, ModeProxy} {
		p := DefaultPolicy()
		p.Mode = mode
		doc := assembleDoc(t, p)
		if list, ok := doc["listeners"].([]any); ok && len(list) > 0 {
			t.Fatalf("%s: слушатель собран без учётных данных: %#v", mode, list)
		}
		if got := p.LoopbackProxyURL(); got != "" {
			t.Fatalf("%s: адрес для лестницы без пары: %q", mode, got)
		}
		// Половина пары это тоже отсутствие пары.
		p.Proxy.LoopbackUser = "u"
		if got := p.LoopbackProxyURL(); got != "" {
			t.Fatalf("%s: адрес по половине пары: %q", mode, got)
		}
		if list, ok := assembleDoc(t, p)["listeners"].([]any); ok && len(list) > 0 {
			t.Fatalf("%s: слушатель собран по половине пары: %#v", mode, list)
		}
	}
}

// TestLoopbackPortAvoidsUserMixedPort: два слушателя на одном порту ядро не
// поднимет, а mixed-порт принадлежит пользователю, поэтому уступает служебный.
func TestLoopbackPortAvoidsUserMixedPort(t *testing.T) {
	p := credentialed(t, DefaultPolicy())
	p.Mode = ModeProxy
	p.Proxy.MixedPort = DefaultLoopbackPort
	if got := p.LoopbackPort(); got != DefaultLoopbackPort+1 {
		t.Fatalf("порт %d, ожидался сдвиг на %d", got, DefaultLoopbackPort+1)
	}
	doc := assembleDoc(t, p)
	if got := doc["mixed-port"]; got != DefaultLoopbackPort {
		t.Fatalf("mixed-port пользователя изменён: %v", got)
	}
	l := listenerNamed(t, doc, LoopbackListenerName)
	if got := l["port"]; got != DefaultLoopbackPort+1 {
		t.Fatalf("служебный порт %v, ожидался %d", got, DefaultLoopbackPort+1)
	}
}

// TestLoopbackPortExplicitAndDisabled: явный порт уважается, отрицательный
// выключает слушателя целиком (тогда обвязка обязана объявить R4 недоступной).
func TestLoopbackPortExplicitAndDisabled(t *testing.T) {
	p := credentialed(t, DefaultPolicy())
	p.Proxy.LoopbackPort = 9999
	if got := p.LoopbackPort(); got != 9999 {
		t.Fatalf("явный порт %d", got)
	}
	l := listenerNamed(t, assembleDoc(t, p), LoopbackListenerName)
	if got := l["port"]; got != 9999 {
		t.Fatalf("слушатель на порту %v", got)
	}

	p.Proxy.LoopbackPort = -1
	if got := p.LoopbackPort(); got != 0 {
		t.Fatalf("выключенный слушатель отдал порт %d", got)
	}
	if addr := p.LoopbackAddr(); addr != "" {
		t.Fatalf("выключенный слушатель отдал адрес %q", addr)
	}
	doc := assembleDoc(t, p)
	if _, ok := doc["listeners"]; ok {
		t.Fatalf("секция listeners осталась: %#v", doc["listeners"])
	}
}

// TestLoopbackKeepsForeignListeners: чужие именованные инбаунды подписки не
// наши, и пересборка не имеет права их выкидывать. Свой дубликат при этом
// вычищается, иначе повторная сборка дала бы два слушателя на одном порту.
func TestLoopbackKeepsForeignListeners(t *testing.T) {
	raw := `
proxies: []
proxy-groups:
  - {name: CARAMBA, type: select, proxies: ["DIRECT"]}
listeners:
  - {name: panel-socks, type: socks, listen: 127.0.0.1, port: 1080}
  - {name: ` + LoopbackListenerName + `, type: mixed, listen: 0.0.0.0, port: 1}
`
	out, err := AssembleMihomoConfig([]byte(raw), credentialed(t, DefaultPolicy()))
	if err != nil {
		t.Fatalf("сборка: %v", err)
	}
	doc := map[string]any{}
	if err := yaml.Unmarshal(out, &doc); err != nil {
		t.Fatalf("разбор: %v", err)
	}
	list, _ := doc["listeners"].([]any)
	if len(list) != 2 {
		t.Fatalf("слушателей %d, ожидалось 2: %#v", len(list), list)
	}
	listenerNamed(t, doc, "panel-socks")
	l := listenerNamed(t, doc, LoopbackListenerName)
	if got, _ := l["listen"].(string); got != LoopbackHost {
		t.Fatalf("наш слушатель не пересобран: listen=%q", got)
	}
	if got := l["port"]; got != DefaultLoopbackPort {
		t.Fatalf("наш слушатель не пересобран: port=%v", got)
	}
}
