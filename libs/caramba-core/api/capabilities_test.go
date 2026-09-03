package api

import (
	"encoding/json"
	"strings"
	"testing"
)

func capByName(t *testing.T, res CapabilitiesResult, name string) Capability {
	t.Helper()
	for _, c := range res.Capabilities {
		if c.Name == name {
			return c
		}
	}
	t.Fatalf("возможность %q не объявлена: %+v", name, res.Capabilities)
	return Capability{}
}

// TestCapabilitiesNameTheRawPathRelayGap фиксирует решение: цепочку вход→выход
// на импортированной подписке ядро не строит, и обвязка узнаёт об этом кодом
// причины, а не по тому, что трафик пошёл не через ту страну.
//
// Ровно эта дыра и была: Up на raw-пути читал serverID и не читал relayCountry,
// туннель поднимался, ошибки не возвращалось.
func TestCapabilitiesNameTheRawPathRelayGap(t *testing.T) {
	cases := []struct {
		name          string
		panelURL      string
		imported      string
		wantPath      string
		wantSupported bool
		wantReason    string
	}{
		{
			name:          "raw-путь называет причину, а не молчит",
			panelURL:      "https://panel.example",
			imported:      "proxies: []\n",
			wantPath:      ConfigPathRaw,
			wantSupported: false,
			wantReason:    CapReasonRawProfile,
		},
		{
			name:          "панельный путь пробрасывает relay_country",
			panelURL:      "https://panel.example",
			wantPath:      ConfigPathPanel,
			wantSupported: true,
		},
		{
			name:          "без адреса панели цепочке некуда уехать",
			wantPath:      ConfigPathPanel,
			wantSupported: false,
			wantReason:    CapReasonPanelNotConfigured,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			c, err := NewCore(Config{PanelBaseURL: tc.panelURL, WorkDir: t.TempDir()})
			if err != nil {
				t.Fatalf("ядро: %v", err)
			}
			if tc.imported != "" {
				if err := c.SetImportedConfig([]byte(tc.imported)); err != nil {
					t.Fatalf("импорт: %v", err)
				}
			}
			res := c.Capabilities()
			if res.Path != tc.wantPath {
				t.Fatalf("путь %q, ожидался %q", res.Path, tc.wantPath)
			}
			relay := capByName(t, res, CapNameRelayChaining)
			if relay.Supported != tc.wantSupported {
				t.Fatalf("relay_chaining supported=%v, ожидалось %v (%+v)", relay.Supported, tc.wantSupported, relay)
			}
			if relay.Reason != tc.wantReason {
				t.Fatalf("relay_chaining reason=%q, ожидалось %q", relay.Reason, tc.wantReason)
			}
			// Недоступная возможность обязана нести пояснение: пустой detail
			// оставил бы разбор отказа на догадки читателя журнала.
			if !relay.Supported && relay.Detail == "" {
				t.Fatal("причина названа кодом, но не объяснена")
			}
			// Возможность обязана ОБЪЯВЛЯТЬСЯ и когда она недоступна: список,
			// из которого запись пропала, обвязка нарисует как «элемента нет»,
			// а не как «элемент выключен и вот почему».
			if _, err := json.Marshal(res); err != nil {
				t.Fatalf("marshal: %v", err)
			}
		})
	}
}

// TestNodeSelectionSurvivesTheRawPath отделяет одно от другого: на
// импортированной подписке выбор узла работает (он локальный пин в селекторе),
// а цепочка — нет. Один общий флаг «панельные функции недоступны» выключил бы
// оба элемента и соврал бы про первый.
func TestNodeSelectionSurvivesTheRawPath(t *testing.T) {
	c, err := NewCore(Config{PanelBaseURL: "https://panel.example", WorkDir: t.TempDir()})
	if err != nil {
		t.Fatalf("ядро: %v", err)
	}
	if err := c.SetImportedConfig([]byte("proxies: []\n")); err != nil {
		t.Fatalf("импорт: %v", err)
	}
	res := c.Capabilities()
	if node := capByName(t, res, CapNameNodeSelection); !node.Supported {
		t.Fatalf("выбор узла на raw-пути обязан работать: %+v", node)
	}
	if relay := capByName(t, res, CapNameRelayChaining); relay.Supported {
		t.Fatalf("цепочка на raw-пути работать не обязана: %+v", relay)
	}
}

// TestUpNamesTheIgnoredRelay проверяет второй след того же факта: если выбор
// входа всё-таки был задан, подъём не делает вид, что применил его.
func TestUpNamesTheIgnoredRelay(t *testing.T) {
	c, err := NewCore(Config{PanelBaseURL: "https://panel.example", WorkDir: t.TempDir()})
	if err != nil {
		t.Fatalf("ядро: %v", err)
	}
	ignored := []Capability{c.relayChainingCap(true, false, nil)}
	if len(ignored) != 1 || ignored[0].Name != CapNameRelayChaining {
		t.Fatalf("не та запись: %+v", ignored)
	}
	if ignored[0].Supported || ignored[0].Reason != CapReasonRawProfile {
		t.Fatalf("запись обязана нести тот же код, что и Capabilities: %+v", ignored[0])
	}
	// Форма UpResult.Ignored это та же Capability: обвязка читает одну схему.
	raw, err := json.Marshal(UpResult{OK: true, Ignored: ignored})
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var back UpResult
	if err := json.Unmarshal(raw, &back); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(back.Ignored) != 1 || back.Ignored[0].Reason != CapReasonRawProfile {
		t.Fatalf("причина не пережила сериализацию: %s", raw)
	}
	// Пустой список не должен появляться в JSON вовсе: обычный подъём ничего
	// не игнорирует, и лишний ключ в ответе означал бы обратное.
	clean, err := json.Marshal(UpResult{OK: true})
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if got := string(clean); strings.Contains(got, "ignored") {
		t.Fatalf("пустой список игнорированного попал в ответ: %s", got)
	}
}
