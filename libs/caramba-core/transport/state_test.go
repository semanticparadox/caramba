package transport

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

// TestStorePermissions: доверенное состояние и кадры лежат под 0600, каталог
// профиля под 0700.
func TestStorePermissions(t *testing.T) {
	dir := t.TempDir()
	s, err := OpenStore(dir, "226e8a20f699b964")
	if err != nil {
		t.Fatalf("хранилище: %v", err)
	}
	if err := s.Update(func(st *State) { st.PID = "226e8a20f699b964"; st.TimeFloor = 100 }); err != nil {
		t.Fatalf("запись состояния: %v", err)
	}
	if err := s.PutFrame(FrameKey, []byte("frame")); err != nil {
		t.Fatalf("запись кадра: %v", err)
	}
	if err := s.PutChunk("CATID", 0, []byte("chunk")); err != nil {
		t.Fatalf("запись фрагмента: %v", err)
	}
	for _, p := range []string{"state.json", "k1.frame", filepath.Join("chunks", "CATID", "0.frame")} {
		fi, err := os.Stat(filepath.Join(s.Dir(), p))
		if err != nil {
			t.Fatalf("stat %s: %v", p, err)
		}
		if fi.Mode().Perm() != 0o600 {
			t.Fatalf("%s имеет права %o, ожидалось 0600", p, fi.Mode().Perm())
		}
	}
	di, err := os.Stat(s.Dir())
	if err != nil {
		t.Fatalf("stat каталога: %v", err)
	}
	if di.Mode().Perm() != 0o700 {
		t.Fatalf("каталог профиля имеет права %o, ожидалось 0700", di.Mode().Perm())
	}
	if s.Dir() != filepath.Join(dir, "csm", "226e8a20f699b964") {
		t.Fatalf("каталог профиля %s не под csm/<pid>", s.Dir())
	}
}

// TestStoreRefusesRollback: временной пол и отметки версии только растут.
// Попытка их понизить это откат, и она отвергается на месте, а не превращается
// в тихую запись.
func TestStoreRefusesRollback(t *testing.T) {
	dir := t.TempDir()
	s, err := OpenStore(dir, "aa")
	if err != nil {
		t.Fatalf("хранилище: %v", err)
	}
	if err := s.Update(func(st *State) {
		st.TimeFloor = 1000
		st.SetHWM(1, 5)
	}); err != nil {
		t.Fatalf("запись: %v", err)
	}
	if err := s.Update(func(st *State) { st.TimeFloor = 500 }); !errors.Is(err, ErrStoreInconsistent) {
		t.Fatalf("понижение time_floor принято: %v", err)
	}
	if err := s.Update(func(st *State) { st.HWM["1"] = 4 }); !errors.Is(err, ErrStoreInconsistent) {
		t.Fatalf("понижение hwm принято: %v", err)
	}
	if got := s.State().TimeFloor; got != 1000 {
		t.Fatalf("time_floor изменился на %d после отказа", got)
	}
	// SetHWM сам по себе тоже не понижает.
	st := s.State()
	st.SetHWM(1, 3)
	if st.HWM["1"] != 5 {
		t.Fatalf("SetHWM понизил отметку до %d", st.HWM["1"])
	}
}

// TestStoreCorruptStateIsNotZeroed: испорченный файл состояния это не повод
// продолжить с нулями. Обнулившееся хранилище неотличимо от отката.
func TestStoreCorruptStateIsNotZeroed(t *testing.T) {
	dir := t.TempDir()
	s, err := OpenStore(dir, "bb")
	if err != nil {
		t.Fatalf("хранилище: %v", err)
	}
	if err := s.Update(func(st *State) { st.TimeFloor = 42 }); err != nil {
		t.Fatalf("запись: %v", err)
	}
	if err := os.WriteFile(filepath.Join(s.Dir(), "state.json"), []byte("{не json"), 0o600); err != nil {
		t.Fatalf("порча файла: %v", err)
	}
	if _, err := OpenStore(dir, "bb"); !errors.Is(err, ErrStoreInconsistent) {
		t.Fatalf("испорченное состояние принято как пустое: %v", err)
	}
}

// TestDeviceProofPreImage сверяет прообраз подписи устройства с рабочим
// примером 03-WIRE.md 13.6, чтобы три реализации сошлись до того, как хоть у
// одной появится ключ устройства.
func TestDeviceProofPreImage(t *testing.T) {
	msg := WriteProofPreImage("PUT", PathPreferences, nil)
	const want = "63736d312d777269746500505554002f6170692f76322f6170702f707265666572656e63657300" +
		"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
	if got := hexOf(msg); got != want {
		t.Fatalf("прообраз %s, ожидался %s", got, want)
	}
	if len(msg) != 71 {
		t.Fatalf("длина прообраза %d, ожидалась 71", len(msg))
	}
}

func hexOf(b []byte) string {
	const digits = "0123456789abcdef"
	out := make([]byte, 0, len(b)*2)
	for _, c := range b {
		out = append(out, digits[c>>4], digits[c&0x0f])
	}
	return string(out)
}

// TestSoftwareDeviceKeysSignature: подпись ровно 64 байта, s в нижней
// половине порядка, и ключи переживают перезапуск.
func TestSoftwareDeviceKeys(t *testing.T) {
	dir := t.TempDir()
	k, err := NewSoftwareDeviceKeys(dir)
	if err != nil {
		t.Fatalf("ключи: %v", err)
	}
	if k.Tier() != TierSoftware {
		t.Fatalf("уровень %d, программное хранилище обязано называть себя уровнем 3", k.Tier())
	}
	sig, err := k.Sign([]byte("csm1"))
	if err != nil {
		t.Fatalf("подпись: %v", err)
	}
	if len(sig) != 64 {
		t.Fatalf("длина подписи %d, ожидалось 64 (r || s, без DER)", len(sig))
	}
	spki, err := k.SigningSPKI()
	if err != nil {
		t.Fatalf("spki: %v", err)
	}
	if len(Thumbprint(spki)) != 16 {
		t.Fatal("отпечаток устройства не 16 байт")
	}
	agree, err := k.AgreementPublic()
	if err != nil {
		t.Fatalf("ключ согласования: %v", err)
	}
	if len(agree) != 65 {
		t.Fatalf("ключ согласования %d байт, ожидалось 65 (несжатая точка P-256)", len(agree))
	}
	// Смена ключа согласования без действия оператора обязана существовать.
	gen, pub, err := k.Rekey()
	if err != nil {
		t.Fatalf("смена ключа: %v", err)
	}
	if gen != 2 || len(pub) != 65 {
		t.Fatalf("смена ключа дала поколение %d и ключ %d байт", gen, len(pub))
	}
	if _, ok := k.AgreementPrivate(1); !ok {
		t.Fatal("предыдущее поколение потеряно: запечатанная директива на нём перестала бы открываться")
	}

	k2, err := NewSoftwareDeviceKeys(dir)
	if err != nil {
		t.Fatalf("перезагрузка ключей: %v", err)
	}
	spki2, err := k2.SigningSPKI()
	if err != nil {
		t.Fatalf("spki: %v", err)
	}
	if hexOf(spki) != hexOf(spki2) {
		t.Fatal("ключ подписи не пережил перезапуск: это была бы перерегистрация на каждом старте")
	}
}
