package transport

import (
	"crypto/ecdh"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"os"
	"path/filepath"
	"sync"
)

// Уровни аппаратного хранения ключа, 03-WIRE.md 13.8 поле tier.
const (
	TierSecureEnclave = 1
	TierStrongBox     = 2
	TierSoftware      = 3
)

// ErrNoAgreementKey это отказ распечатывания: устройство не держит закрытого
// ключа для предложенного rkv. Правильная реакция это немедленная смена ключа
// согласования и повторный запрос (02-SPEC.md 10.3).
var ErrNoAgreementKey = errors.New("transport: нет закрытого ключа согласования для этого rkv")

// DeviceKeys это две пары ключей устройства.
//
// Аппаратная реализация в Go невозможна по определению: Secure Enclave и
// StrongBox достижимы только через SecKeyCreateRandomKey и KeyGenParameterSpec,
// а реализация на Go по построению кладёт ключ в файл, то есть в программный
// уровень. Поэтому на платформах с аппаратным хранилищем этот интерфейс
// реализует мост, а SoftwareDeviceKeys ниже это честный уровень 3 для Windows,
// Linux и любой сборки без хранилища.
type DeviceKeys interface {
	// SigningSPKI возвращает открытый ключ подписи как DER SubjectPublicKeyInfo.
	SigningSPKI() ([]byte, error)
	// Sign подписывает СООБЩЕНИЕ (не дайджест) ключом подписи и возвращает
	// ровно 64 байта r || s, big-endian, с нормализованным низким s.
	Sign(message []byte) ([]byte, error)
	// AgreementPublic возвращает открытый ключ согласования, 65 байт,
	// несжатая точка P-256.
	AgreementPublic() ([]byte, error)
	// AgreementPrivate отдаёт скаляр закрытого ключа согласования поколения
	// rkv. Нужен пакету csm для распечатывания 0x06.
	AgreementPrivate(rkv uint64) ([]byte, bool)
	// Generation это текущее поколение ключа согласования, начинается с 1.
	Generation() uint64
	// Tier это уровень аппаратного хранения.
	Tier() int
}

// Thumbprint возвращает dtp = sha256(device_signing_SPKI_DER)[0..16].
func Thumbprint(spkiDER []byte) []byte {
	sum := sha256.Sum256(spkiDER)
	out := make([]byte, 16)
	copy(out, sum[:16])
	return out
}

// WriteProofPreImage собирает прообраз подписи устройства, 03-WIRE.md 13.6:
//
//	sha256("csm1-write" || 0x00 || method || 0x00 || path || 0x00 || sha256(body))
//
// path это КАНОНИЧЕСКИЙ литерал эндпойнта, а не полученный путь: api_routes
// смонтирован дважды, и проверяющий, который сверял бы полученный путь,
// отверг бы клиента, написавшего другое монтирование, а выглядело бы это
// ровно как подделка.
func WriteProofPreImage(method, canonicalPath string, body []byte) []byte {
	bodySum := sha256.Sum256(body)
	msg := make([]byte, 0, 16+len(method)+len(canonicalPath)+32)
	msg = append(msg, []byte("csm1-write")...)
	msg = append(msg, 0x00)
	msg = append(msg, []byte(method)...)
	msg = append(msg, 0x00)
	msg = append(msg, []byte(canonicalPath)...)
	msg = append(msg, 0x00)
	msg = append(msg, bodySum[:]...)
	return msg
}

// ProofHeader кодирует 64-байтовую подпись как base64url без дополнения,
// 86 символов.
func ProofHeader(sig []byte) string {
	return base64.RawURLEncoding.EncodeToString(sig)
}

// Канонические литералы путей, 03-WIRE.md 13.6.
const (
	PathPreferences  = "/api/v2/app/preferences"
	PathEnrollCode   = "/api/v2/app/csm/enroll/code"
	PathEnrollDevice = "/api/v2/app/csm/enroll/device"
)

// softwareKeys это сериализуемая форма программных ключей.
type softwareKeys struct {
	SignD    string            `json:"sign_d"`
	AgreeGen uint64            `json:"agree_gen"`
	AgreeD   map[string]string `json:"agree_d"`
}

// SoftwareDeviceKeys это уровень 3: ключи лежат в файле рядом с состоянием
// профиля, с правами 0600. Это честный, названный своим именем программный
// уровень, а не имитация аппаратного.
type SoftwareDeviceKeys struct {
	mu   sync.Mutex
	path string
	sign *ecdsa.PrivateKey
	gen  uint64
	// agree это закрытые ключи согласования по поколениям.
	agree map[uint64]*ecdh.PrivateKey
}

// NewSoftwareDeviceKeys загружает или создаёт программные ключи в каталоге dir.
func NewSoftwareDeviceKeys(dir string) (*SoftwareDeviceKeys, error) {
	k := &SoftwareDeviceKeys{path: filepath.Join(dir, "device.json"), agree: map[uint64]*ecdh.PrivateKey{}}
	b, err := os.ReadFile(k.path)
	if err == nil {
		var sk softwareKeys
		if err := json.Unmarshal(b, &sk); err != nil {
			return nil, fmt.Errorf("%w: ключи устройства: %v", ErrStoreInconsistent, err)
		}
		if err := k.restore(sk); err == nil {
			return k, nil
		}
	}
	if err := k.generate(); err != nil {
		return nil, err
	}
	return k, nil
}

func (k *SoftwareDeviceKeys) restore(sk softwareKeys) error {
	d, err := hex.DecodeString(sk.SignD)
	if err != nil || len(d) == 0 {
		return errors.New("transport: пустой ключ подписи")
	}
	priv := new(ecdsa.PrivateKey)
	priv.Curve = elliptic.P256()
	priv.D = new(big.Int).SetBytes(d)
	priv.X, priv.Y = priv.Curve.ScalarBaseMult(d)
	k.sign = priv
	k.gen = sk.AgreeGen
	if k.gen == 0 {
		k.gen = 1
	}
	for gs, hs := range sk.AgreeD {
		var g uint64
		if _, err := fmt.Sscanf(gs, "%d", &g); err != nil {
			continue
		}
		raw, err := hex.DecodeString(hs)
		if err != nil {
			continue
		}
		p, err := ecdh.P256().NewPrivateKey(raw)
		if err != nil {
			continue
		}
		k.agree[g] = p
	}
	if len(k.agree) == 0 {
		return errors.New("transport: нет ключей согласования")
	}
	return nil
}

func (k *SoftwareDeviceKeys) generate() error {
	sign, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return err
	}
	agree, err := ecdh.P256().GenerateKey(rand.Reader)
	if err != nil {
		return err
	}
	k.sign = sign
	k.gen = 1
	k.agree = map[uint64]*ecdh.PrivateKey{1: agree}
	return k.persist()
}

func (k *SoftwareDeviceKeys) persist() error {
	sk := softwareKeys{
		SignD:    hex.EncodeToString(k.sign.D.Bytes()),
		AgreeGen: k.gen,
		AgreeD:   map[string]string{},
	}
	for g, p := range k.agree {
		sk.AgreeD[fmt.Sprintf("%d", g)] = hex.EncodeToString(p.Bytes())
	}
	b, err := json.MarshalIndent(sk, "", "  ")
	if err != nil {
		return err
	}
	return writeFile0600(k.path, b)
}

// Relocate переносит файл ключей вслед за каталогом профиля.
//
// Старая копия УДАЛЯЕТСЯ. Файл лежит с правами 0600, так что оставленная копия
// это не раскрытие, но вторая копия личности устройства это второе, что
// придётся отзывать, и лежит она под csm/pending, куда никто больше не смотрит.
func (k *SoftwareDeviceKeys) Relocate(dir string) error {
	k.mu.Lock()
	defer k.mu.Unlock()
	old := k.path
	k.path = filepath.Join(dir, "device.json")
	if err := k.persist(); err != nil {
		return err
	}
	if old != "" && old != k.path {
		_ = os.Remove(old)
	}
	return nil
}

// SigningSPKI реализует DeviceKeys.
func (k *SoftwareDeviceKeys) SigningSPKI() ([]byte, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	return x509.MarshalPKIXPublicKey(&k.sign.PublicKey)
}

// Sign подписывает сообщение и возвращает r || s с низким s.
//
// Кодировка фиксирована именно так, потому что платформы расходятся по
// умолчанию: и SecKeyCreateSignature, и Signature на Android возвращают
// ASN.1 DER. Нормализация s в нижнюю половину порядка заодно снимает вопрос
// податливости подписи, а не оставляет его открытым.
func (k *SoftwareDeviceKeys) Sign(message []byte) ([]byte, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	sum := sha256.Sum256(message)
	r, s, err := ecdsa.Sign(rand.Reader, k.sign, sum[:])
	if err != nil {
		return nil, err
	}
	n := elliptic.P256().Params().N
	half := new(big.Int).Rsh(n, 1)
	if s.Cmp(half) > 0 {
		s = new(big.Int).Sub(n, s)
	}
	out := make([]byte, 64)
	r.FillBytes(out[:32])
	s.FillBytes(out[32:])
	return out, nil
}

// AgreementPublic реализует DeviceKeys.
func (k *SoftwareDeviceKeys) AgreementPublic() ([]byte, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	p, ok := k.agree[k.gen]
	if !ok {
		return nil, ErrNoAgreementKey
	}
	return p.PublicKey().Bytes(), nil
}

// AgreementPrivate реализует DeviceKeys.
func (k *SoftwareDeviceKeys) AgreementPrivate(rkv uint64) ([]byte, bool) {
	k.mu.Lock()
	defer k.mu.Unlock()
	p, ok := k.agree[rkv]
	if !ok {
		return nil, false
	}
	return p.Bytes(), true
}

// Generation реализует DeviceKeys.
func (k *SoftwareDeviceKeys) Generation() uint64 {
	k.mu.Lock()
	defer k.mu.Unlock()
	return k.gen
}

// Tier реализует DeviceKeys: программное хранилище это уровень 3, и оно
// называет себя своим именем.
func (k *SoftwareDeviceKeys) Tier() int { return TierSoftware }

// Rekey создаёт новое поколение ключа согласования. Устройство обязано уметь
// заменить свой ключ без действия оператора, и состояния, в котором оно
// застряло с ключом, который не может заменить, не существует.
func (k *SoftwareDeviceKeys) Rekey() (uint64, []byte, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	p, err := ecdh.P256().GenerateKey(rand.Reader)
	if err != nil {
		return 0, nil, err
	}
	k.gen++
	k.agree[k.gen] = p
	if err := k.persist(); err != nil {
		return 0, nil, err
	}
	return k.gen, p.PublicKey().Bytes(), nil
}

// AgreementKeyMap собирает карту поколений для csm.TrustState.
func AgreementKeyMap(d DeviceKeys, generations []uint64) map[uint64][]byte {
	out := map[uint64][]byte{}
	for _, g := range generations {
		if b, ok := d.AgreementPrivate(g); ok {
			out[g] = b
		}
	}
	return out
}
