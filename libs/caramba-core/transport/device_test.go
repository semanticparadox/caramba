package transport

import (
	"bytes"
	"crypto/elliptic"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"math/big"
	"net/http"
	"strconv"
	"testing"
)

// TestDeviceProofPathIsCanonical показывает, что подписывается канонический
// литерал, а не полученный путь: api_routes смонтирован дважды, и клиент,
// назвавший другое монтирование, обязан дать ТУ ЖЕ подпись.
func TestDeviceProofPathIsCanonical(t *testing.T) {
	a := WriteProofPreImage(http.MethodPut, PathPreferences, []byte("x"))
	b := WriteProofPreImage(http.MethodPut, PathPreferences, []byte("x"))
	if !bytes.Equal(a, b) {
		t.Fatalf("прообраз не детерминирован")
	}
	c := WriteProofPreImage(http.MethodPost, PathEnrollCode, []byte("x"))
	if bytes.Equal(a, c) {
		t.Fatalf("метод и путь не входят в прообраз")
	}
	d := WriteProofPreImage(http.MethodPut, PathPreferences, []byte("y"))
	if bytes.Equal(a, d) {
		t.Fatalf("тело не входит в прообраз")
	}
}

// TestDeviceSignatureForm проверяет ровно то, что фиксирует 03-WIRE.md 13.6:
// 64 байта r || s, не ASN.1 DER, и s в нижней половине порядка.
func TestDeviceSignatureForm(t *testing.T) {
	k, err := NewSoftwareDeviceKeys(t.TempDir())
	if err != nil {
		t.Fatalf("ключи: %v", err)
	}
	msg := WriteProofPreImage(http.MethodPut, PathPreferences, []byte("body"))
	for i := 0; i < 16; i++ {
		sig, err := k.Sign(msg)
		if err != nil {
			t.Fatalf("подпись: %v", err)
		}
		if len(sig) != SigLen {
			t.Fatalf("подпись %d байт, требуется %d", len(sig), SigLen)
		}
		if sig[0] == 0x30 && int(sig[1]) == len(sig)-2 {
			t.Fatalf("подпись выглядит как ASN.1 DER, требуется r || s")
		}
		if err := CheckSignatureForm(sig); err != nil {
			t.Fatalf("форма подписи: %v", err)
		}
	}
	// Заголовок это base64url без дополнения, 86 символов.
	sig, _ := k.Sign(msg)
	h := ProofHeader(sig)
	if len(h) != 86 {
		t.Fatalf("заголовок %d символов, требуется 86", len(h))
	}
	back, err := base64.RawURLEncoding.DecodeString(h)
	if err != nil || !bytes.Equal(back, sig) {
		t.Fatalf("заголовок не декодируется обратно в подпись: %v", err)
	}
}

// TestCheckSignatureFormRejectsHighS: проверяющий обязан ОТВЕРГАТЬ высокий s,
// а не нормализовать его. Нормализация на приёме оставила бы податливость
// подписи открытой, ради снятия которой правило и введено.
func TestCheckSignatureFormRejectsHighS(t *testing.T) {
	n := elliptic.P256().Params().N
	high := new(big.Int).Sub(n, big.NewInt(2)) // заведомо больше n/2
	sig := make([]byte, 64)
	big.NewInt(7).FillBytes(sig[:32])
	high.FillBytes(sig[32:])
	if err := CheckSignatureForm(sig); err == nil {
		t.Fatalf("высокий s принят")
	}
	if err := CheckSignatureForm(sig[:63]); err == nil {
		t.Fatalf("подпись длиной 63 принята")
	}
	zero := make([]byte, 64)
	if err := CheckSignatureForm(zero); err == nil {
		t.Fatalf("нулевые r и s приняты")
	}
}

// TestAgreeMatchesSoftwareScalar: ECDH через Agree и через скаляр совпадают,
// то есть аппаратный путь распечатывания эквивалентен программному.
func TestAgreeMatchesSoftwareScalar(t *testing.T) {
	k, err := NewSoftwareDeviceKeys(t.TempDir())
	if err != nil {
		t.Fatalf("ключи: %v", err)
	}
	peer, err := NewSoftwareDeviceKeys(t.TempDir())
	if err != nil {
		t.Fatalf("вторые ключи: %v", err)
	}
	peerPub, err := peer.AgreementPublic()
	if err != nil {
		t.Fatalf("открытый ключ: %v", err)
	}
	shared, ownPub, err := k.Agree(1, peerPub)
	if err != nil {
		t.Fatalf("согласование: %v", err)
	}
	if len(shared) != 32 || len(ownPub) != 65 {
		t.Fatalf("согласование дало %d и %d байт, требуется 32 и 65", len(shared), len(ownPub))
	}
	mine, err := k.AgreementPublic()
	if err != nil || !bytes.Equal(mine, ownPub) {
		t.Fatalf("Agree вернул чужой открытый ключ")
	}
	// Встречное направление даёт тот же секрет.
	other, _, err := peer.Agree(1, mine)
	if err != nil {
		t.Fatalf("встречное согласование: %v", err)
	}
	if !bytes.Equal(shared, other) {
		t.Fatalf("ECDH несимметричен")
	}
	if _, _, err := k.Agree(99, peerPub); err == nil {
		t.Fatalf("несуществующее поколение принято")
	}
}

// TestHKDFSHA256RFC5869 проверяет вспомогательный HKDF по вектору RFC 5869 A.1.
func TestHKDFSHA256RFC5869(t *testing.T) {
	ikm := mustHexBytes(t, "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b")
	salt := mustHexBytes(t, "000102030405060708090a0b0c")
	info := mustHexBytes(t, "f0f1f2f3f4f5f6f7f8f9")
	want := "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
	got := hex.EncodeToString(HKDFSHA256(salt, ikm, info, 42))
	if got != want {
		t.Fatalf("HKDF %s, RFC 5869 A.1 says %s", got, want)
	}
}

func mustHexBytes(t *testing.T, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatalf("hex %q: %v", s, err)
	}
	return b
}

// fakeHardwareBridge это платформенный мост, отвечающий как StrongBox: он
// выполняет ECDH и подписывает, но скаляр наружу не отдаёт. Реализован поверх
// программных ключей, потому что проверяется не хранилище, а граница.
type fakeHardwareBridge struct {
	inner *SoftwareDeviceKeys
	tier  int
	// derForm заставляет мост вернуть подпись в ASN.1 DER: ровно то, что оба
	// платформенных API отдают по умолчанию, и ровно то, что ядро обязано
	// поймать у себя, а не отправить панели.
	derForm bool
	calls   map[string]int
}

func newFakeBridge(t *testing.T, tier int) *fakeHardwareBridge {
	t.Helper()
	k, err := NewSoftwareDeviceKeys(t.TempDir())
	if err != nil {
		t.Fatalf("ключи моста: %v", err)
	}
	return &fakeHardwareBridge{inner: k, tier: tier, calls: map[string]int{}}
}

func (b *fakeHardwareBridge) Keygen(string) (string, error) {
	b.calls["keygen"]++
	spki, err := b.inner.SigningSPKI()
	if err != nil {
		return "", err
	}
	agree, err := b.inner.AgreementPublic()
	if err != nil {
		return "", err
	}
	return `{"spki_b64":"` + base64.StdEncoding.EncodeToString(spki) +
		`","agree_pub_b64":"` + base64.StdEncoding.EncodeToString(agree) +
		`","tier":` + strconv.Itoa(b.tier) + `,"generation":1}`, nil
}

func (b *fakeHardwareBridge) Sign(req string) (string, error) {
	b.calls["sign"]++
	var r struct {
		MessageB64 string `json:"message_b64"`
	}
	if err := json.Unmarshal([]byte(req), &r); err != nil {
		return "", err
	}
	msg, err := base64.StdEncoding.DecodeString(r.MessageB64)
	if err != nil {
		return "", err
	}
	sig, err := b.inner.Sign(msg)
	if err != nil {
		return "", err
	}
	if b.derForm {
		sig = append([]byte{0x30, 0x44}, sig...)
	}
	return `{"sig_b64":"` + base64.StdEncoding.EncodeToString(sig) + `"}`, nil
}

func (b *fakeHardwareBridge) Agree(req string) (string, error) {
	b.calls["agree"]++
	var r struct {
		RKV        uint64 `json:"rkv"`
		PeerPubB64 string `json:"peer_pub_b64"`
	}
	if err := json.Unmarshal([]byte(req), &r); err != nil {
		return "", err
	}
	peer, err := base64.StdEncoding.DecodeString(r.PeerPubB64)
	if err != nil {
		return "", err
	}
	shared, ownPub, err := b.inner.Agree(r.RKV, peer)
	if err != nil {
		return `{"error":"нет ключа согласования этого поколения"}`, nil
	}
	return `{"shared_b64":"` + base64.StdEncoding.EncodeToString(shared) +
		`","own_pub_b64":"` + base64.StdEncoding.EncodeToString(ownPub) + `"}`, nil
}

// TestBridgeDeviceKeysReportsTheBridgeTier: ядро повторяет уровень, который
// назвал мост, и НЕ улучшает его. Ложь здесь стоила бы дороже отсутствия
// аппаратуры: оператор принимает решения об устройстве по полю tier.
func TestBridgeDeviceKeysReportsTheBridgeTier(t *testing.T) {
	for _, tier := range []int{TierSecureEnclave, TierStrongBox, TierSoftware} {
		b := newFakeBridge(t, tier)
		k := NewBridgeDeviceKeys(b)
		if got := k.Tier(); got != tier {
			t.Fatalf("уровень %d, мост назвал %d", got, tier)
		}
	}
	// Уровень вне 1..3 это дефект моста, а не значение по умолчанию.
	bad := newFakeBridge(t, 7)
	if _, err := NewBridgeDeviceKeys(bad).SigningSPKI(); err == nil {
		t.Fatalf("уровень 7 принят")
	}
}

// TestBridgeDeviceKeysLazyAndIdempotent: ключи заводятся при первом обращении
// и ровно один раз. Личность устройства это долгоживущий идентификатор, и
// заводить её как побочный эффект запуска ядра нельзя.
func TestBridgeDeviceKeysLazyAndIdempotent(t *testing.T) {
	b := newFakeBridge(t, TierStrongBox)
	k := NewBridgeDeviceKeys(b)
	if b.calls["keygen"] != 0 {
		t.Fatalf("мост дёрнут до первого обращения")
	}
	spki, err := k.SigningSPKI()
	if err != nil {
		t.Fatalf("spki: %v", err)
	}
	if _, err := k.AgreementPublic(); err != nil {
		t.Fatalf("ключ согласования: %v", err)
	}
	if _, err := k.Sign([]byte("csm1")); err != nil {
		t.Fatalf("подпись: %v", err)
	}
	if b.calls["keygen"] != 1 {
		t.Fatalf("keygen вызван %d раз, ожидался один", b.calls["keygen"])
	}
	if len(Thumbprint(spki)) != 16 {
		t.Fatalf("отпечаток не 16 байт")
	}
	// Скаляр аппаратный носитель не отдаёт НИКОГДА.
	if _, ok := k.AgreementPrivate(1); ok {
		t.Fatalf("аппаратный носитель отдал скаляр")
	}
	if len(AgreementKeyMap(k, []uint64{1})) != 0 {
		t.Fatalf("карта скаляров на аппаратном носителе непуста")
	}
}

// TestBridgeDeviceKeysRejectsDER: и SecKeyCreateSignature, и Signature на
// Android возвращают ASN.1 DER по умолчанию. Ядро обязано поймать это у себя,
// иначе дефект клиента выглядел бы отказом панели.
func TestBridgeDeviceKeysRejectsDER(t *testing.T) {
	b := newFakeBridge(t, TierSecureEnclave)
	b.derForm = true
	k := NewBridgeDeviceKeys(b)
	if _, err := k.Sign([]byte("csm1")); err == nil {
		t.Fatalf("подпись в ASN.1 DER принята")
	}
}

// TestBridgeDeviceKeysAgree: согласование через мост даёт тот же секрет, что
// встречная сторона, и отдаёт собственный открытый ключ поколения.
func TestBridgeDeviceKeysAgree(t *testing.T) {
	b := newFakeBridge(t, TierStrongBox)
	k := NewBridgeDeviceKeys(b)
	peer, err := NewSoftwareDeviceKeys(t.TempDir())
	if err != nil {
		t.Fatalf("встречные ключи: %v", err)
	}
	peerPub, _ := peer.AgreementPublic()
	shared, ownPub, err := k.Agree(1, peerPub)
	if err != nil {
		t.Fatalf("согласование: %v", err)
	}
	back, _, err := peer.Agree(1, ownPub)
	if err != nil {
		t.Fatalf("встречное согласование: %v", err)
	}
	if !bytes.Equal(shared, back) {
		t.Fatalf("ECDH через мост несимметричен")
	}
	if _, _, err := k.Agree(9, peerPub); err == nil {
		t.Fatalf("несуществующее поколение принято")
	}
}
