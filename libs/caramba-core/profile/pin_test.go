package profile

import (
	"testing"
)

// pinSample — конфиг с двумя узлами и DIRECT в селекторе: на нём проверяется,
// что Up(serverID) действительно двигает выбранный узел на первое место.
const pinSample = `
proxies:
  - name: "NL-1"
    type: ss
    server: 127.0.0.1
    port: 8388
    cipher: aes-128-gcm
    password: pass
  - name: "DE-2"
    type: ss
    server: 127.0.0.1
    port: 8389
    cipher: aes-128-gcm
    password: pass
proxy-groups:
  - name: CARAMBA
    type: select
    proxies: ["NL-1", "DE-2", "DIRECT"]
rules:
  - MATCH,CARAMBA
`

// Выбранный узел встаёт первым, остальные сохраняют относительный порядок:
// первый участник select-группы и есть выбор mihomo по умолчанию.
func TestPinReordersSelector(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = false
	out, err := AssembleMihomoConfigPinned([]byte(pinSample), p, "DE-2")
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	got := selectorMembers(t, out)
	want := []string{"DE-2", "NL-1", "DIRECT"}
	if len(got) != len(want) {
		t.Fatalf("участники селектора %v, ожидались %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("участники селектора %v, ожидались %v", got, want)
		}
	}
}

// Пустое имя — автоматический выбор: порядок не трогаем.
func TestPinEmptyKeepsOrder(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = false
	out, err := AssembleMihomoConfigPinned([]byte(pinSample), p, "")
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	if got := selectorMembers(t, out); got[0] != "NL-1" {
		t.Fatalf("порядок изменился без пина: %v", got)
	}
}

// Неизвестное имя — мягкая деградация: пользователь получает автоматический
// выбор, а не ошибку сборки конфига.
func TestPinUnknownNameIsIgnored(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = false
	out, err := AssembleMihomoConfigPinned([]byte(pinSample), p, "нет такого узла")
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	if got := selectorMembers(t, out); got[0] != "NL-1" {
		t.Fatalf("порядок изменился при неизвестном пине: %v", got)
	}
}

// Пин важнее служебной группы протокола: явный выбор пользователя должен
// оставаться первым, даже когда applyProtocol уже поставил Caramba-Proto.
func TestPinWinsOverProtocolGroup(t *testing.T) {
	p := DefaultPolicy()
	p.KillSwitch = false
	p.Protocol = "Shadowsocks"
	out, err := AssembleMihomoConfigPinned([]byte(pinSample), p, "DE-2")
	if err != nil {
		t.Fatalf("assemble: %v", err)
	}
	got := selectorMembers(t, out)
	if got[0] != "DE-2" {
		t.Fatalf("первым должен быть пин DE-2, получено %v", got)
	}
	found := false
	for _, n := range got {
		if n == protoGroupName {
			found = true
		}
	}
	if !found {
		t.Fatalf("служебная группа протокола пропала из селектора: %v", got)
	}
}
