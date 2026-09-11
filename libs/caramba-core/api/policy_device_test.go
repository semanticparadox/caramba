package api

import "testing"

// Идентичность устройства едет тем же JSON-швом, что и политика: это
// единственный канал, который уже доходит до ядра со всех пяти платформ без
// нативных правок. Проверяем, что она доезжает до КЛИЕНТА ПОДПИСКИ — именно он
// ставит заголовки на /sub/{uuid}.
func TestSetPolicyJSONCarriesDeviceIdentity(t *testing.T) {
	core := newTestCore(t)
	err := core.SetPolicyJSON(`{"preset":"ru-smart","device":{"id":"dev-1","name":"Pixel 8","platform":"Android"}}`)
	if err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	d := core.sub.DeviceIdentity()
	if d.ID != "dev-1" || d.Name != "Pixel 8" || d.Platform != "android" {
		t.Fatalf("идентичность не доехала до клиента подписки: %+v", d)
	}
}

// Поля device нет — идентичность не трогаем. Политика приходит из нескольких
// мест (пикеры, CSM-оператор), и сборка без идентичности не имеет права её
// стереть: следующая выборка подписки снова завела бы лизу по User-Agent.
func TestSetPolicyJSONWithoutDeviceKeepsIdentity(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetPolicyJSON(`{"device":{"id":"dev-1","name":"Mac","platform":"macos"}}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	if err := core.SetPolicyJSON(`{"preset":"global"}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	if got := core.sub.DeviceIdentity().ID; got != "dev-1" {
		t.Fatalf("идентичность потеряна: %q", got)
	}
}

// Смена панели пересобирает клиента подписки. Идентичность обязана переехать
// вместе с ним: иначе после enroll в другую панель тот же телефон снова качает
// подписку безымянным и заводит себе вторую лизу.
func TestSetPanelURLKeepsDeviceIdentity(t *testing.T) {
	core := newTestCore(t)
	if err := core.SetPolicyJSON(`{"device":{"id":"dev-1","name":"Mac","platform":"macos"}}`); err != nil {
		t.Fatalf("SetPolicyJSON: %v", err)
	}
	if err := core.SetPanelURL("https://other-panel.invalid"); err != nil {
		t.Fatalf("SetPanelURL: %v", err)
	}
	d := core.sub.DeviceIdentity()
	if d.ID != "dev-1" || d.Platform != "macos" {
		t.Fatalf("идентичность не пережила смену панели: %+v", d)
	}
}

// Недопустимое значение перечислимого поля откатывает применение ЦЕЛИКОМ,
// включая идентичность: политика применяется атомарно, и половина принятого
// сообщения — это состояние, которого не выбирал никто.
func TestSetPolicyJSONRejectsWholePatchWithDevice(t *testing.T) {
	core := newTestCore(t)
	err := core.SetPolicyJSON(`{"stack":"quantum","device":{"id":"dev-1","name":"Mac","platform":"macos"}}`)
	if err == nil {
		t.Fatal("ожидалась ошибка по полю stack")
	}
	if got := core.sub.DeviceIdentity().ID; got != "" {
		t.Fatalf("идентичность применена из отвергнутой политики: %q", got)
	}
}
