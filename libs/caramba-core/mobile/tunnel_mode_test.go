package mobile

import (
	"encoding/json"
	"testing"
)

// newTestClient собирает клиента с каталогами внутри t.TempDir().
func newTestClient(t *testing.T) *Client {
	t.Helper()
	dir := t.TempDir()
	cl, err := NewClient("https://panel.invalid", "", dir, dir+"/tokens.json")
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}
	return cl
}

// statusFields — плоский статус, который читает нативный слой.
type statusFields struct {
	Stage     string `json:"stage"`
	Mode      string `json:"mode"`
	MixedPort int    `json:"mixedPort"`
}

func decodeStatus(t *testing.T, raw string) statusFields {
	t.Helper()
	var st statusFields
	if err := json.Unmarshal([]byte(raw), &st); err != nil {
		t.Fatalf("разбор статуса %q: %v", raw, err)
	}
	return st
}

// В tun-режиме mixedPort отсутствует (omitempty), чтобы UI не показывал
// несуществующий локальный прокси.
func TestStatusJSONTunModeHasNoMixedPort(t *testing.T) {
	cl := newTestClient(t)
	raw, err := cl.StatusJSON()
	if err != nil {
		t.Fatalf("StatusJSON: %v", err)
	}
	st := decodeStatus(t, raw)
	if st.Mode != "tun" {
		t.Fatalf("mode %q, ожидался tun (%s)", st.Mode, raw)
	}
	if st.MixedPort != 0 {
		t.Fatalf("mixedPort %d, в tun-режиме ожидался 0 (%s)", st.MixedPort, raw)
	}
	if !json.Valid([]byte(raw)) {
		t.Fatalf("статус не является валидным JSON: %s", raw)
	}
}

func TestStatusJSONProxyModeReportsPort(t *testing.T) {
	cl := newTestClient(t)
	if err := cl.SetTunnelMode("proxy", 7893); err != nil {
		t.Fatalf("SetTunnelMode: %v", err)
	}
	raw, err := cl.StatusJSON()
	if err != nil {
		t.Fatalf("StatusJSON: %v", err)
	}
	st := decodeStatus(t, raw)
	if st.Mode != "proxy" || st.MixedPort != 7893 {
		t.Fatalf("получено (%q, %d), ожидалось (\"proxy\", 7893): %s", st.Mode, st.MixedPort, raw)
	}
	if st.Stage != "disconnected" {
		t.Fatalf("stage %q, ожидался disconnected до Up", st.Stage)
	}
}

func TestSetTunnelModeRejectsUnknown(t *testing.T) {
	cl := newTestClient(t)
	if err := cl.SetTunnelMode("tunnel", 0); err == nil {
		t.Fatal("ожидалась ошибка для неизвестного режима")
	}
}
