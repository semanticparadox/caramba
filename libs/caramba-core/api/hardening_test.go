package api

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
	"github.com/semanticparadox/caramba/libs/caramba-core/profile"
	"github.com/semanticparadox/caramba/libs/caramba-core/transport"
)

// TestDeviceSignRefusesAnythingButTheWriteProof: символ ABI подписи это НЕ
// оракул.
//
// Ключ устройства живёт в Secure Enclave или StrongBox, откуда его не достать,
// и ровно поэтому доставать его никто и не пытается: держатель, подписывающий
// произвольные байты, стоит вместо извлечения. Всё, что дотягивается до канала
// внутри процесса приложения, иначе получило бы подпись под телом регистрации,
// нацеленным на враждебный origin, под доказательством смены ключа или под
// записью настроек по другому каноническому пути.
func TestDeviceSignRefusesAnythingButTheWriteProof(t *testing.T) {
	c := deviceCore(t)
	sign := func(msg []byte) error {
		req, _ := json.Marshal(DeviceSignRequest{MessageB64: base64.StdEncoding.EncodeToString(msg)})
		_, err := c.DeviceSignJSON(string(req))
		return err
	}

	// Правильный прообраз проходит: и запись настроек, и оба тела регистрации.
	for _, ok := range [][]byte{
		transport.WriteProofPreImage(http.MethodPut, transport.PathPreferences, []byte("body")),
		transport.WriteProofPreImage(http.MethodPost, transport.PathEnrollCode, nil),
		transport.WriteProofPreImage(http.MethodPost, transport.PathEnrollDevice, []byte("x")),
	} {
		if err := sign(ok); err != nil {
			t.Fatalf("законный прообраз отвергнут: %v", err)
		}
	}

	bad := map[string][]byte{
		"произвольные байты":      []byte("please sign this"),
		"чужая метка":             transport.WriteProofPreImage(http.MethodPut, transport.PathPreferences, nil)[:9],
		"чужой канонический путь": append([]byte("csm1-write\x00PUT\x00/api/v2/app/csm/rekey\x00"), make([]byte, 32)...),
		"чужой метод":             append([]byte("csm1-write\x00DELETE\x00/api/v2/app/preferences\x00"), make([]byte, 32)...),
		"PUT по пути регистрации": append([]byte("csm1-write\x00PUT\x00/api/v2/app/csm/enroll/code\x00"), make([]byte, 32)...),
		"дайджест короче 32":      append([]byte("csm1-write\x00PUT\x00/api/v2/app/preferences\x00"), make([]byte, 31)...),
	}
	for name, msg := range bad {
		if err := sign(msg); err == nil {
			t.Fatalf("%s: подпись выдана", name)
		} else if !errors.Is(err, transport.ErrNotWriteProof) {
			t.Fatalf("%s: отказ не назван своим именем: %v", name, err)
		}
	}
}

// TestDamagedDeviceStoreIsNotSilentlyReplaced: уничтожение личности устройства
// не бывает реакцией на ошибку чтения.
//
// Повреждённый device.json прежде вёл к тихой генерации новой пары: новый dtp,
// вторая строка устройства у оператора и все закешированные запечатанные
// директивы, адресованные прежнему dtp, навсегда нераспечатываемы. Одной
// оборванной записи для этого хватало.
func TestDamagedDeviceStoreIsNotSilentlyReplaced(t *testing.T) {
	for name, body := range map[string]string{
		"битый JSON":                   "{ not json",
		"пустой ключ подписи":          `{"sign_d":"","agree_gen":1,"agree_d":{"1":"aa"}}`,
		"нет ключей согласования":      `{"sign_d":"01","agree_gen":1,"agree_d":{}}`,
		"негодный скаляр согласования": `{"sign_d":"01","agree_gen":1,"agree_d":{"1":"zz"}}`,
	} {
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "device.json"), []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
		_, err := transport.NewSoftwareDeviceKeys(dir)
		if err == nil {
			t.Fatalf("%s: повреждённое хранилище принято, личность заменена молча", name)
		}
		if !errors.Is(err, transport.ErrStoreInconsistent) {
			t.Fatalf("%s: отказ не назван своим именем: %v", name, err)
		}
	}

	// Отсутствие файла это НЕ повреждение: личность заводится впервые.
	if _, err := transport.NewSoftwareDeviceKeys(t.TempDir()); err != nil {
		t.Fatalf("первый запуск: %v", err)
	}
}

// TestCsmSelectProfileRefusesUnsafeKeys: ключ профиля попадает в путь.
func TestCsmSelectProfileRefusesUnsafeKeys(t *testing.T) {
	c := deviceCore(t)
	for _, bad := range []string{"..", "../etc", "a/b", "a.b", "A B", strings.Repeat("x", 65)} {
		if err := c.CsmSelectProfile(bad); err == nil {
			t.Fatalf("ключ %q принят", bad)
		}
	}
	for _, ok := range []string{"", "cp_1", "cp-1", "226e8a20f699b964"} {
		if err := c.CsmSelectProfile(ok); err != nil {
			t.Fatalf("ключ %q отвергнут: %v", ok, err)
		}
	}
	if got := c.CsmProfileKey(); got != "226e8a20f699b964" {
		t.Fatalf("выбранный профиль %q", got)
	}
}

// TestLoopbackListenerNeverShipsWithoutCredentials: собранный конфиг не
// содержит открытого релея на петле.
//
// Инбаунд с ключом proxy уводит ВСЁ, что на него пришло, прямо в
// группу-селектор мимо правил. На Android в него ходит любое приложение с
// разрешением INTERNET, на десктопе любой локальный процесс, и весь их трафик
// уходит через оплаченный пользователем узел мимо пресета маршрутизации и мимо
// раздельного туннелирования.
func TestLoopbackListenerNeverShipsWithoutCredentials(t *testing.T) {
	p := profile.DefaultPolicy()
	out, err := profile.AssembleMihomoConfig([]byte("proxies: []\n"), p)
	if err != nil {
		t.Fatalf("сборка: %v", err)
	}
	if strings.Contains(string(out), profile.LoopbackListenerName) {
		t.Fatalf("слушатель собран без учётных данных:\n%s", out)
	}

	user, pass, err := profile.NewLoopbackCredential()
	if err != nil {
		t.Fatal(err)
	}
	if user == pass {
		t.Fatalf("логин и пароль совпали")
	}
	p.Proxy.LoopbackUser, p.Proxy.LoopbackPass = user, pass
	out, err = profile.AssembleMihomoConfig([]byte("proxies: []\n"), p)
	if err != nil {
		t.Fatalf("сборка: %v", err)
	}
	if !strings.Contains(string(out), "users:") || !strings.Contains(string(out), user) {
		t.Fatalf("слушатель без учётных данных:\n%s", out)
	}
	// Пара уходит лестнице ВМЕСТЕ с адресом: голый host:port означал бы
	// слушатель без аутентификации.
	if got := p.LoopbackProxyURL(); !strings.HasPrefix(got, "socks5://"+user+":"+pass+"@") {
		t.Fatalf("адрес для лестницы %q", got)
	}
}

// TestSealAgreementFailureKeepsStepFiveAndSixApart: код отказа предписывает
// действие, и не то предписание дороже неточности.
//
// E_SEAL_RECIPIENT велит клиенту сменить ключ согласования и перезапросить
// (02-SPEC.md 10.3); E_SEAL_OPEN не велит ничего подобного. Источник, который
// отвечает на всё одинаково, заставляет жечь поколение ключа по каждому
// испорченному enc.
func TestSealAgreementFailureKeepsStepFiveAndSixApart(t *testing.T) {
	// Мост, у которого поколения нет, обязан дойти до проверяющего как
	// "поколения нет", а не как произвольный отказ.
	src := transport.AgreementOf(&noAgreementKeys{})
	if _, _, err := src.Agree(9, make([]byte, 65)); !errors.Is(err, csm.ErrNoAgreementGeneration) {
		t.Fatalf("отказ моста не переведён в словарь проверяющего: %v", err)
	}

	// Любой другой отказ держателя остаётся собой.
	other := transport.AgreementOf(&brokenKeys{})
	if _, _, err := other.Agree(1, make([]byte, 65)); errors.Is(err, csm.ErrNoAgreementGeneration) {
		t.Fatalf("криптографический отказ выдан за отсутствие поколения")
	}
}

type noAgreementKeys struct{ transport.DeviceKeys }

func (noAgreementKeys) Agree(uint64, []byte) ([]byte, []byte, error) {
	return nil, nil, transport.ErrNoAgreementKey
}

type brokenKeys struct{ transport.DeviceKeys }

func (brokenKeys) Agree(uint64, []byte) ([]byte, []byte, error) {
	return nil, nil, errors.New("transport: точка не на кривой")
}
