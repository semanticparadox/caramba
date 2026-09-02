package api

import "testing"

// Смена адреса панели пересобирает auth-клиента, клиент подписок и клиент
// /subscription, но НЕ теряет уже инъецированный токен (хранилище общее).
func TestSetPanelURLRebuildsClients(t *testing.T) {
	core := newTestCore(t)
	if err := core.InjectToken("access-token", "", 0, "sub-uuid"); err != nil {
		t.Fatalf("InjectToken: %v", err)
	}
	if !core.auth.IsAuthenticated() {
		t.Fatal("ядро должно быть аутентифицировано после InjectToken")
	}

	oldAuth, oldSub, oldInfo := core.auth, core.sub, core.subInfo
	if err := core.SetPanelURL("https://other-panel.invalid"); err != nil {
		t.Fatalf("SetPanelURL: %v", err)
	}

	if core.cfg.PanelBaseURL != "https://other-panel.invalid" {
		t.Fatalf("PanelBaseURL %q", core.cfg.PanelBaseURL)
	}
	if core.auth == oldAuth || core.sub == oldSub || core.subInfo == oldInfo {
		t.Fatal("клиенты не пересобраны под новую панель")
	}
	if !core.auth.IsAuthenticated() {
		t.Fatal("токен потерян при смене панели")
	}
	// Кэш UUID и последний профиль относились к прежней панели.
	if core.subscriptionID != "" {
		t.Fatalf("кэш UUID подписки не сброшен: %q", core.subscriptionID)
	}
}

// Пустой и совпадающий с текущим URL — no-op: приложение шлёт Configure на
// каждом обновлении токена, и лишняя пересборка сбрасывала бы кэш UUID зря.
func TestSetPanelURLNoopCases(t *testing.T) {
	core := newTestCore(t)
	core.SetSubscriptionID("sub-uuid")
	oldAuth := core.auth

	for _, url := range []string{"", "   ", "https://panel.invalid"} {
		if err := core.SetPanelURL(url); err != nil {
			t.Fatalf("SetPanelURL(%q): %v", url, err)
		}
		if core.auth != oldAuth {
			t.Fatalf("SetPanelURL(%q) пересобрал клиентов зря", url)
		}
		if core.subscriptionID != "sub-uuid" {
			t.Fatalf("SetPanelURL(%q) сбросил кэш UUID", url)
		}
	}
}
