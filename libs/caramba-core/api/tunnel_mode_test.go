package api

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/profile"
)

// newTestCore собирает Core с рабочим каталогом и хранилищем токенов внутри
// t.TempDir(), чтобы тест не трогал каталоги пользователя.
func newTestCore(t *testing.T) *Core {
	t.Helper()
	dir := t.TempDir()
	core, err := NewCore(Config{
		PanelBaseURL:   "https://panel.invalid",
		WorkDir:        dir,
		TokenStorePath: dir + "/tokens.json",
	})
	if err != nil {
		t.Fatalf("NewCore: %v", err)
	}
	return core
}

// По умолчанию ядро остаётся в tun-режиме, а порт mixed-инбаунда не показывается
// (UI не должен предлагать «Proxy on ...», когда прокси нет).
func TestTunnelModeDefaultsToTun(t *testing.T) {
	core := newTestCore(t)
	mode, port := core.TunnelMode()
	if mode != string(profile.ModeTun) {
		t.Fatalf("режим %q, ожидался %q", mode, profile.ModeTun)
	}
	if port != 0 {
		t.Fatalf("порт %d, в tun-режиме ожидался 0", port)
	}
}

func TestSetTunnelModeProxy(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetTunnelMode("proxy", 7899); err != nil {
		t.Fatalf("SetTunnelMode: %v", err)
	}
	mode, port := core.TunnelMode()
	if mode != string(profile.ModeProxy) || port != 7899 {
		t.Fatalf("получено (%q, %d), ожидалось (\"proxy\", 7899)", mode, port)
	}
}

// Порт <= 0 не сбрасывает ранее заданный и не превращается в «слушать 0».
func TestSetTunnelModeKeepsPortWhenNotGiven(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetTunnelMode("proxy", 7899); err != nil {
		t.Fatalf("SetTunnelMode: %v", err)
	}
	if err := core.SetTunnelMode("proxy", 0); err != nil {
		t.Fatalf("SetTunnelMode: %v", err)
	}
	if _, port := core.TunnelMode(); port != 7899 {
		t.Fatalf("порт %d, ожидался сохранённый 7899", port)
	}
}

// Регистр и пустая строка обрабатываются мягко, мусор — ошибка без изменения
// политики.
func TestSetTunnelModeValidation(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetTunnelMode("PROXY", 0); err != nil {
		t.Fatalf("регистр должен игнорироваться: %v", err)
	}
	if mode, port := core.TunnelMode(); mode != string(profile.ModeProxy) || port != profile.DefaultMixedPort {
		t.Fatalf("получено (%q, %d), ожидалось (\"proxy\", %d)", mode, port, profile.DefaultMixedPort)
	}
	if err := core.SetTunnelMode("bogus", 0); err == nil {
		t.Fatal("ожидалась ошибка для неизвестного режима")
	}
	if mode, _ := core.TunnelMode(); mode != string(profile.ModeProxy) {
		t.Fatalf("неудачный вызов не должен менять режим, получено %q", mode)
	}
	if err := core.SetTunnelMode("", 0); err != nil {
		t.Fatalf("пустой режим должен трактоваться как tun: %v", err)
	}
	if mode, _ := core.TunnelMode(); mode != string(profile.ModeTun) {
		t.Fatalf("режим %q, ожидался tun", mode)
	}
}

// Режим виден в агрегированном статусе, который читает UI/CLI.
func TestStatusResultCarriesMode(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetTunnelMode("proxy", 7891); err != nil {
		t.Fatalf("SetTunnelMode: %v", err)
	}
	st, err := core.Status(context.Background())
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if st.Mode != "proxy" || st.MixedPort != 7891 {
		t.Fatalf("получено (%q, %d), ожидалось (\"proxy\", 7891)", st.Mode, st.MixedPort)
	}
	raw, err := json.Marshal(st)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var doc map[string]any
	if err := json.Unmarshal(raw, &doc); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if doc["mode"] != "proxy" {
		t.Fatalf("в JSON нет mode=proxy: %s", raw)
	}
	if doc["mixed_port"] != float64(7891) {
		t.Fatalf("в JSON нет mixed_port=7891: %s", raw)
	}
}
