package transport

import (
	"bytes"
	"crypto/ecdh"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"net/http"
	"os"
	"path/filepath"
	"sync"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
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
	// rkv. Программный уровень отдаёт его, аппаратный НИКОГДА: Secure Enclave
	// и StrongBox по построению не выпускают скаляр наружу, и второе значение
	// у них всегда false. Распечатывание 0x06 поэтому идёт через Agree, а не
	// через эту карту.
	AgreementPrivate(rkv uint64) ([]byte, bool)
	// Agree выполняет ECDH ключом согласования поколения rkv с несжатой
	// точкой peer и возвращает 32 байта общей координаты X и 65 байт
	// собственного открытого ключа этого поколения (csm.AgreementSource).
	Agree(rkv uint64, peer []byte) (shared []byte, ownPub []byte, err error)
	// Generation это текущее поколение ключа согласования, начинается с 1.
	Generation() uint64
	// Tier это уровень аппаратного хранения.
	Tier() int
}

// ProofLen это длина 71-байтового прообраза подписи устройства для записи
// настроек: 10 байт метки, три нулевых разделителя, "PUT", канонический путь
// записи настроек и 32 байта sha256 тела (03-WIRE.md 13.6).
//
// Константа существует ради одной проверки. Прообраз это ЕДИНСТВЕННЫЙ якорь
// согласия между реализациями: подпись ECDSA рандомизирована, и фикстуры для
// неё в корпусе нет, поэтому расходиться реализации могут только здесь, и
// заметить это должен тест, а не поле.
const ProofLen = 71

// SigLen это длина подписи устройства: 64 байта r || s.
const SigLen = 64

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
	msg = append(msg, []byte(writeProofLabel)...)
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

// writeProofLabel это метка прообраза. Вынесена, потому что её проверяет и
// сборщик, и разборщик.
const writeProofLabel = "csm1-write"

// ErrNotWriteProof означает, что предложенное к подписи сообщение не является
// прообразом 03-WIRE.md 13.6.
var ErrNotWriteProof = errors.New("transport: сообщение не является прообразом подписи записи CSM/1")

// CheckWriteProofPreImage отвергает всё, что не собрано WriteProofPreImage.
//
// Это ЕДИНСТВЕННОЕ, что отделяет ключ устройства от оракула подписи. Ключ
// живёт в Secure Enclave или StrongBox, оттуда его не достать, и ровно поэтому
// достать его никто и не пытается: держатель, подписывающий произвольные
// байты, стоит вместо извлечения. Всё, что в состоянии дотянуться до канала
// (сторонний пакет во внутреннем процессе, второй изолят, внедрённый код),
// иначе получило бы подпись под телом регистрации, нацеленным на враждебный
// origin, под доказательством смены ключа или под записью настроек по другому
// каноническому пути.
//
// 03-WIRE.md 13.6 фиксирует ОДНУ конструкцию, которую этот ключ подписывает,
// и проверяется здесь именно она:
//
//	"csm1-write" || 0x00 || method || 0x00 || canonicalPath || 0x00 || sha256(body)
//
// Разбор идёт С КОНЦА, а не разрезанием по нулю: последние 32 байта это
// дайджест, и нулевой байт внутри него совершенно законен.
func CheckWriteProofPreImage(msg []byte) error {
	head := []byte(writeProofLabel + "\x00")
	// Минимум это метка, два разделителя, непустой метод, непустой путь,
	// разделитель и 32 байта дайджеста.
	if len(msg) < len(head)+1+1+1+sha256.Size {
		return fmt.Errorf("%w: длина %d", ErrNotWriteProof, len(msg))
	}
	if !bytes.Equal(msg[:len(head)], head) {
		return fmt.Errorf("%w: метка не %q", ErrNotWriteProof, writeProofLabel)
	}
	sep := len(msg) - sha256.Size - 1
	if msg[sep] != 0x00 {
		return fmt.Errorf("%w: перед дайджестом нет разделителя", ErrNotWriteProof)
	}
	mid := msg[len(head):sep]
	parts := bytes.Split(mid, []byte{0x00})
	if len(parts) != 2 {
		return fmt.Errorf("%w: разделителей %d, требуется ровно три", ErrNotWriteProof, len(parts)+1)
	}
	method, path := string(parts[0]), string(parts[1])
	switch method {
	case http.MethodPut, http.MethodPost:
	default:
		return fmt.Errorf("%w: метод %q", ErrNotWriteProof, method)
	}
	switch path {
	case PathPreferences, PathEnrollCode, PathEnrollDevice:
	default:
		return fmt.Errorf("%w: канонический путь %q не из 03-WIRE.md 13.6", ErrNotWriteProof, path)
	}
	// PUT ходит только в настройки, POST только в регистрацию: пара
	// метод-путь фиксирована так же жёстко, как и каждая её половина.
	if (method == http.MethodPut) != (path == PathPreferences) {
		return fmt.Errorf("%w: пара %s %s не встречается в 03-WIRE.md 13.6", ErrNotWriteProof, method, path)
	}
	return nil
}

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
	switch {
	case err == nil:
		var sk softwareKeys
		if err := json.Unmarshal(b, &sk); err != nil {
			return nil, fmt.Errorf("%w: ключи устройства: %v", ErrStoreInconsistent, err)
		}
		// Файл есть, но не читается как личность: это ПОВРЕЖДЕНИЕ, а не
		// отсутствие. Завести здесь новую пару значило бы сделать уничтожение
		// личности реакцией на ошибку чтения: новый dtp, вторая строка
		// устройства у оператора и все закешированные запечатанные директивы,
		// адресованные прежнему dtp, навсегда нераспечатываемы. Одной
		// оборванной записи или одного перевёрнутого байта для этого хватило
		// бы. Наверх уходит "личность на диске повреждена", и решает вызывающий.
		if err := k.restore(sk); err != nil {
			return nil, fmt.Errorf("%w: ключи устройства: %v", ErrStoreInconsistent, err)
		}
		return k, nil
	case !errors.Is(err, os.ErrNotExist):
		return nil, fmt.Errorf("%w: ключи устройства: %v", ErrStoreInconsistent, err)
	}
	// Файла нет: личность заводится впервые.
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

// Agree реализует DeviceKeys: ECDH программным скаляром.
func (k *SoftwareDeviceKeys) Agree(rkv uint64, peer []byte) ([]byte, []byte, error) {
	k.mu.Lock()
	p, ok := k.agree[rkv]
	k.mu.Unlock()
	if !ok {
		return nil, nil, ErrNoAgreementKey
	}
	pub, err := ecdh.P256().NewPublicKey(peer)
	if err != nil {
		return nil, nil, err
	}
	z, err := p.ECDH(pub)
	if err != nil {
		return nil, nil, err
	}
	return z, p.PublicKey().Bytes(), nil
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
//
// На аппаратном носителе карта выходит пустой, и это правильно: скаляра там
// нет. Распечатывание в этом случае идёт через AgreementOf.
func AgreementKeyMap(d DeviceKeys, generations []uint64) map[uint64][]byte {
	out := map[uint64][]byte{}
	for _, g := range generations {
		if b, ok := d.AgreementPrivate(g); ok {
			out[g] = b
		}
	}
	return out
}

// deviceAgreement связывает DeviceKeys с csm.AgreementSource.
type deviceAgreement struct{ d DeviceKeys }

// Agree переводит отказ держателя в словарь проверяющего.
//
// ErrNoAgreementKey это ровно шаг 5 раздела 9.4, и проверяющий обязан узнать
// его отдельно от любого другого отказа: шаг 5 предписывает клиенту сменить
// ключ согласования, шаг 6 не предписывает ничего. Без этого перевода мост
// аппаратного хранилища отвечал бы на всё одинаково, и клиент жёг бы поколение
// ключа по каждому испорченному enc.
func (a deviceAgreement) Agree(rkv uint64, peer []byte) ([]byte, []byte, error) {
	shared, ownPub, err := a.d.Agree(rkv, peer)
	if err != nil && errors.Is(err, ErrNoAgreementKey) {
		return nil, nil, fmt.Errorf("%w: %v", csm.ErrNoAgreementGeneration, err)
	}
	return shared, ownPub, err
}

// AgreementOf отдаёт источник согласования для csm.TrustState.
func AgreementOf(d DeviceKeys) csm.AgreementSource { return deviceAgreement{d: d} }

// ------------------------------------------------- мост аппаратных хранилищ

// DeviceKeyBridge это платформенный держатель ключей устройства.
//
// Аппаратная реализация в Go невозможна по определению (см. DeviceKeys), а
// граница gomobile не пропускает ни карт, ни срезов чужих структур, поэтому
// каждый метод принимает и отдаёт строку JSON ровно так же, как символы
// ABI v3, и Kotlin со Swift реализуют один и тот же интерфейс.
//
//	Keygen {"purpose":"sign"|"agree","require_hardware":bool}
//	       -> {"spki_b64","agree_pub_b64","tier":1|2|3,"generation":n}
//	Sign   {"message_b64"} -> {"sig_b64"}   64 байта r || s, низкий s
//	Agree  {"rkv":n,"peer_pub_b64","kdf_info_b64"}
//	       -> {"shared_b64","own_pub_b64"}
//
// own_pub_b64 добавлен к форме 02-SPEC.md 12.2: kem_context DHKEM несёт
// открытый ключ получателя, и знает его только держатель ключа.
type DeviceKeyBridge interface {
	Keygen(reqJSON string) (string, error)
	Sign(reqJSON string) (string, error)
	Agree(reqJSON string) (string, error)
}

// bridgeKeygenReq и остальные формы это ровно тот JSON, что ходит по ABI v3.
type bridgeKeygenReq struct {
	Purpose         string `json:"purpose"`
	RequireHardware bool   `json:"require_hardware"`
}

type bridgeKeygenResp struct {
	SPKIB64     string `json:"spki_b64"`
	AgreePubB64 string `json:"agree_pub_b64"`
	Tier        int    `json:"tier"`
	Generation  uint64 `json:"generation"`
	Error       string `json:"error,omitempty"`
}

type bridgeSignReq struct {
	MessageB64 string `json:"message_b64"`
}

type bridgeSignResp struct {
	SigB64 string `json:"sig_b64"`
	Error  string `json:"error,omitempty"`
}

type bridgeAgreeReq struct {
	RKV        uint64 `json:"rkv"`
	PeerPubB64 string `json:"peer_pub_b64"`
	KDFInfoB64 string `json:"kdf_info_b64,omitempty"`
}

type bridgeAgreeResp struct {
	SharedB64 string `json:"shared_b64"`
	OwnPubB64 string `json:"own_pub_b64"`
	Error     string `json:"error,omitempty"`
	// Code это МАШИННАЯ причина отказа. Значение CodeNoAgreementGeneration
	// означает шаг 5 раздела 9.4 ("такого поколения у меня нет") и ничего
	// больше; любой другой отказ хранилища это шаг 6. Без этого поля мост
	// отвечает свободным текстом, ядро не может их различить, и клиент жжёт
	// поколение ключа согласования по каждому испорченному enc.
	Code string `json:"code,omitempty"`
}

// CodeNoAgreementGeneration это значение поля code моста, означающее
// отсутствие закрытого ключа запрошенного поколения.
const CodeNoAgreementGeneration = "no_agreement_generation"

// BridgeDeviceKeys это DeviceKeys поверх платформенного хранилища.
//
// Уровень докладывается ЧЕСТНО: мост, не нашедший StrongBox или Secure
// Enclave, возвращает 3, и ядро повторяет за ним, а не выдаёт программный
// ключ за аппаратный. Ложь здесь стоила бы дороже отсутствия аппаратуры:
// оператор принимает решения об устройстве по полю tier.
type BridgeDeviceKeys struct {
	mu     sync.Mutex
	b      DeviceKeyBridge
	spki   []byte
	agree  []byte
	tier   int
	gen    uint64
	loaded bool
}

// NewBridgeDeviceKeys оборачивает мост. Ключи создаются лениво, при первом
// обращении: ключ устройства это долгоживущий идентификатор, и заводить его
// как побочный эффект запуска ядра нельзя.
func NewBridgeDeviceKeys(b DeviceKeyBridge) *BridgeDeviceKeys {
	return &BridgeDeviceKeys{b: b, tier: TierSoftware}
}

func (k *BridgeDeviceKeys) ensure() error {
	if k.loaded {
		return nil
	}
	out, err := k.b.Keygen(`{"purpose":"sign","require_hardware":true}`)
	if err != nil {
		return err
	}
	var r bridgeKeygenResp
	if err := json.Unmarshal([]byte(out), &r); err != nil {
		return fmt.Errorf("transport: ответ моста ключей: %w", err)
	}
	if r.Error != "" {
		return errors.New(r.Error)
	}
	spki, err := base64.StdEncoding.DecodeString(r.SPKIB64)
	if err != nil || len(spki) == 0 {
		return errors.New("transport: мост не вернул SPKI ключа подписи")
	}
	agree, err := base64.StdEncoding.DecodeString(r.AgreePubB64)
	if err != nil || len(agree) != 65 {
		return errors.New("transport: мост не вернул открытый ключ согласования")
	}
	if r.Tier < TierSecureEnclave || r.Tier > TierSoftware {
		return fmt.Errorf("transport: мост назвал уровень %d вне 1..3", r.Tier)
	}
	gen := r.Generation
	if gen == 0 {
		gen = 1
	}
	k.spki, k.agree, k.tier, k.gen, k.loaded = spki, agree, r.Tier, gen, true
	return nil
}

// SigningSPKI реализует DeviceKeys.
func (k *BridgeDeviceKeys) SigningSPKI() ([]byte, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if err := k.ensure(); err != nil {
		return nil, err
	}
	return k.spki, nil
}

// Sign реализует DeviceKeys. Проверка формы делается ЗДЕСЬ, а не у моста:
// ошибка в кодировании подписи на одной из платформ иначе выглядела бы как
// отказ панели, а не как дефект клиента.
func (k *BridgeDeviceKeys) Sign(message []byte) ([]byte, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if err := k.ensure(); err != nil {
		return nil, err
	}
	req, err := json.Marshal(bridgeSignReq{MessageB64: base64.StdEncoding.EncodeToString(message)})
	if err != nil {
		return nil, err
	}
	out, err := k.b.Sign(string(req))
	if err != nil {
		return nil, err
	}
	var r bridgeSignResp
	if err := json.Unmarshal([]byte(out), &r); err != nil {
		return nil, fmt.Errorf("transport: ответ моста подписи: %w", err)
	}
	if r.Error != "" {
		return nil, errors.New(r.Error)
	}
	sig, err := base64.StdEncoding.DecodeString(r.SigB64)
	if err != nil {
		return nil, err
	}
	if err := CheckSignatureForm(sig); err != nil {
		return nil, err
	}
	return sig, nil
}

// AgreementPublic реализует DeviceKeys.
func (k *BridgeDeviceKeys) AgreementPublic() ([]byte, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if err := k.ensure(); err != nil {
		return nil, err
	}
	return k.agree, nil
}

// AgreementPrivate реализует DeviceKeys: аппаратный носитель скаляра НЕ
// отдаёт, и вторым значением всегда идёт false.
func (k *BridgeDeviceKeys) AgreementPrivate(uint64) ([]byte, bool) { return nil, false }

// Agree реализует DeviceKeys через ECDH платформенного хранилища.
func (k *BridgeDeviceKeys) Agree(rkv uint64, peer []byte) ([]byte, []byte, error) {
	k.mu.Lock()
	defer k.mu.Unlock()
	if err := k.ensure(); err != nil {
		return nil, nil, err
	}
	req, err := json.Marshal(bridgeAgreeReq{RKV: rkv, PeerPubB64: base64.StdEncoding.EncodeToString(peer)})
	if err != nil {
		return nil, nil, err
	}
	out, err := k.b.Agree(string(req))
	if err != nil {
		return nil, nil, err
	}
	var r bridgeAgreeResp
	if err := json.Unmarshal([]byte(out), &r); err != nil {
		return nil, nil, fmt.Errorf("transport: ответ моста согласования: %w", err)
	}
	if r.Error != "" {
		if r.Code == CodeNoAgreementGeneration {
			return nil, nil, fmt.Errorf("%w: %s", ErrNoAgreementKey, r.Error)
		}
		return nil, nil, errors.New(r.Error)
	}
	// Испорченный ответ моста это НЕ "поколения нет": ответить так значило бы
	// предписать клиенту сменить ключ из-за дефекта обвязки.
	shared, err := base64.StdEncoding.DecodeString(r.SharedB64)
	if err != nil || len(shared) != 32 {
		return nil, nil, errors.New("transport: мост согласования не вернул 32 байта общего секрета")
	}
	ownPub, err := base64.StdEncoding.DecodeString(r.OwnPubB64)
	if err != nil || len(ownPub) != 65 || ownPub[0] != 0x04 {
		return nil, nil, errors.New("transport: мост согласования не вернул несжатую точку своего ключа")
	}
	// Собственная точка этого поколения обязана совпасть с той, что уже
	// зарегистрирована у оператора. Она входит в kem_context DHKEM, и мост,
	// вернувший другую, молча дал бы неверный контекст: распечатывание
	// провалилось бы на AEAD, а выглядело бы это как испорченная директива.
	// Проверять можно только текущее поколение: точки прошлых поколений ядро
	// не хранит.
	if rkv == k.gen && len(k.agree) == 65 && !bytes.Equal(ownPub, k.agree) {
		return nil, nil, fmt.Errorf("transport: мост вернул чужой открытый ключ согласования поколения %d", rkv)
	}
	return shared, ownPub, nil
}

// Generation реализует DeviceKeys.
func (k *BridgeDeviceKeys) Generation() uint64 {
	k.mu.Lock()
	defer k.mu.Unlock()
	if err := k.ensure(); err != nil {
		return 1
	}
	return k.gen
}

// Tier реализует DeviceKeys: то, что назвал мост, без улучшения.
func (k *BridgeDeviceKeys) Tier() int {
	k.mu.Lock()
	defer k.mu.Unlock()
	if err := k.ensure(); err != nil {
		return TierSoftware
	}
	return k.tier
}

// HKDFSHA256 это RFC 5869 на SHA-256: Extract(salt, ikm) и Expand(info) до
// length байт.
//
// Нужен ровно одному месту: полю kdf_info_b64 символа CarambaDeviceAgree
// (02-SPEC.md 12.2). CSM/1 передаёт его ПУСТЫМ, потому что DHKEM берёт сырую
// общую координату X, а kem_context собирает сам. Непустое info поэтому не
// молчаливый вариант того же результата, а другой ответ, и вызывающий,
// заполнивший поле, получает именно то, что попросил.
func HKDFSHA256(salt, ikm, info []byte, length int) []byte {
	if len(salt) == 0 {
		salt = make([]byte, sha256.Size)
	}
	m := hmac.New(sha256.New, salt)
	m.Write(ikm)
	prk := m.Sum(nil)

	out := make([]byte, 0, length)
	var prev []byte
	for i := 1; len(out) < length; i++ {
		h := hmac.New(sha256.New, prk)
		h.Write(prev)
		h.Write(info)
		h.Write([]byte{byte(i)})
		prev = h.Sum(nil)
		out = append(out, prev...)
	}
	return out[:length]
}

// CheckSignatureForm проверяет форму подписи устройства: ровно 64 байта
// r || s, оба в диапазоне 1..n-1, и s в нижней половине порядка.
//
// Проверяющий ОБЯЗАН отвергать высокий s, а не нормализовать его, поэтому
// клиент, который его отправил, узнаёт об этом здесь, а не по 401 от панели.
func CheckSignatureForm(sig []byte) error {
	if len(sig) != SigLen {
		return fmt.Errorf("transport: подпись устройства %d байт, требуется %d (r || s)", len(sig), SigLen)
	}
	n := elliptic.P256().Params().N
	half := new(big.Int).Rsh(n, 1)
	r := new(big.Int).SetBytes(sig[:32])
	s := new(big.Int).SetBytes(sig[32:])
	if r.Sign() == 0 || s.Sign() == 0 || r.Cmp(n) >= 0 || s.Cmp(n) >= 0 {
		return errors.New("transport: r или s подписи устройства вне диапазона")
	}
	if s.Cmp(half) > 0 {
		return errors.New("transport: высокий s в подписи устройства, требуется s <= n/2")
	}
	return nil
}
