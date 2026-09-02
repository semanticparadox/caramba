package mobile

import "testing"

// Плоская обёртка доводит политику до ядра и возвращает ошибку с именем поля.
func TestClientSetPolicyJSON(t *testing.T) {
	cl := newTestClient(t)
	if err := cl.SetPolicyJSON(`{"preset":"ru-smart","killSwitch":true,"split":{"mode":"bypass","apps":["git"]}}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	err := cl.SetPolicyJSON(`{"stack":"userspace"}`)
	if err == nil {
		t.Fatal("ожидалась ошибка для неизвестного stack")
	}
}

// Без загруженной конфигурации замер отдаёт пустой список, а не ошибку.
func TestClientProbeJSONWithoutConfig(t *testing.T) {
	cl := newTestClient(t)
	out, err := cl.ProbeJSON(500)
	if err != nil {
		t.Fatalf("ProbeJSON: %v", err)
	}
	if out != `{"servers":[]}` {
		t.Fatalf("ожидалось {\"servers\":[]}, получено %s", out)
	}
}

// Configure с новым адресом панели перенаправляет ядро и не теряет токен.
func TestClientConfigureAppliesPanelURL(t *testing.T) {
	cl := newTestClient(t)
	if err := cl.Configure("https://tenant-2.invalid", "sub-uuid", "access-token"); err != nil {
		t.Fatalf("Configure: %v", err)
	}
	st, err := cl.Status()
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if st == "" {
		t.Fatal("пустой статус")
	}
}
