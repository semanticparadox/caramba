package api

import (
	"crypto/ecdsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"math/big"
	"net/http"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/transport"
)

func deviceCore(t *testing.T) *Core {
	t.Helper()
	dir := t.TempDir()
	c, err := NewCore(Config{PanelBaseURL: "https://panel.example.net", WorkDir: dir, TokenStorePath: dir + "/t.json"})
	if err != nil {
		t.Fatalf("ядро: %v", err)
	}
	return c
}

// TestDeviceKeygenIsIdempotent: личность устройства заводится один раз.
// Новая при каждом вызове означала бы новый dtp, то есть второе устройство в
// списке оператора после каждого запуска приложения.
func TestDeviceKeygenIsIdempotent(t *testing.T) {
	c := deviceCore(t)
	var a, b DeviceKeygenResponse
	out, err := c.DeviceKeygenJSON(`{"purpose":"sign","require_hardware":true}`)
	if err != nil {
		t.Fatalf("keygen: %v", err)
	}
	if err := json.Unmarshal([]byte(out), &a); err != nil {
		t.Fatalf("разбор: %v", err)
	}
	out2, err := c.DeviceKeygenJSON("")
	if err != nil {
		t.Fatalf("keygen повторно: %v", err)
	}
	if err := json.Unmarshal([]byte(out2), &b); err != nil {
		t.Fatalf("разбор: %v", err)
	}
	if a.SPKIB64 == "" || a.SPKIB64 != b.SPKIB64 || a.DTPHex != b.DTPHex {
		t.Fatalf("личность устройства сменилась между вызовами")
	}
	if len(a.DTPHex) != 32 {
		t.Fatalf("dtp %q, ожидалось 16 байт в hex", a.DTPHex)
	}
	// Уровень докладывается честно: сборка без хранилища это 3, даже когда
	// вызывающий попросил аппаратный.
	if a.Tier != transport.TierSoftware {
		t.Fatalf("уровень %d, сборка без хранилища обязана называть себя %d", a.Tier, transport.TierSoftware)
	}
	if a.Generation != 1 {
		t.Fatalf("поколение ключа согласования %d, начинается с 1", a.Generation)
	}
	agree, err := base64.StdEncoding.DecodeString(a.AgreePubB64)
	if err != nil || len(agree) != 65 || agree[0] != 0x04 {
		t.Fatalf("ключ согласования не несжатая точка P-256")
	}
	if _, err := c.DeviceKeygenJSON(`{"purpose":"encrypt"}`); err == nil {
		t.Fatalf("purpose вне sign и agree принят")
	}
}

// TestDeviceSignVerifiesOverTheWorkedPreImage подписывает РОВНО тот прообраз,
// который 03-WIRE.md 13.6 расписывает побайтово, и проверяет подпись открытым
// ключом, который вернул CarambaDeviceKeygen. Это то, чем ABI v3 доказывает
// себя без фикстуры ECDSA в корпусе.
func TestDeviceSignVerifiesOverTheWorkedPreImage(t *testing.T) {
	c := deviceCore(t)
	out, err := c.DeviceKeygenJSON("")
	if err != nil {
		t.Fatalf("keygen: %v", err)
	}
	var kg DeviceKeygenResponse
	if err := json.Unmarshal([]byte(out), &kg); err != nil {
		t.Fatalf("разбор: %v", err)
	}
	spki, err := base64.StdEncoding.DecodeString(kg.SPKIB64)
	if err != nil {
		t.Fatalf("spki: %v", err)
	}
	pub, err := x509.ParsePKIXPublicKey(spki)
	if err != nil {
		t.Fatalf("spki не разбирается как SubjectPublicKeyInfo: %v", err)
	}
	ec, ok := pub.(*ecdsa.PublicKey)
	if !ok {
		t.Fatalf("ключ подписи не ECDSA")
	}

	msg := transport.WriteProofPreImage(http.MethodPut, transport.PathPreferences, nil)
	req, _ := json.Marshal(DeviceSignRequest{MessageB64: base64.StdEncoding.EncodeToString(msg)})
	sout, err := c.DeviceSignJSON(string(req))
	if err != nil {
		t.Fatalf("подпись: %v", err)
	}
	var sr DeviceSignResponse
	if err := json.Unmarshal([]byte(sout), &sr); err != nil {
		t.Fatalf("разбор подписи: %v", err)
	}
	sig, err := base64.StdEncoding.DecodeString(sr.SigB64)
	if err != nil || len(sig) != transport.SigLen {
		t.Fatalf("подпись %d байт, требуется %d (r || s)", len(sig), transport.SigLen)
	}
	if err := transport.CheckSignatureForm(sig); err != nil {
		t.Fatalf("форма подписи: %v", err)
	}
	if len(sr.ProofHeader) != 86 {
		t.Fatalf("proof_header %d символов, требуется 86", len(sr.ProofHeader))
	}
	// Подписывается СООБЩЕНИЕ: проверяющий хеширует его сам, ровно как
	// .ecdsaSignatureMessageX962SHA256 и SHA256withECDSA.
	sum := sha256.Sum256(msg)
	r := new(big.Int).SetBytes(sig[:32])
	s := new(big.Int).SetBytes(sig[32:])
	if !ecdsa.Verify(ec, sum[:], r, s) {
		t.Fatalf("подпись не проверяется над прообразом 03-WIRE.md 13.6")
	}
	// Тот же ключ над ДРУГИМ прообразом не проходит: метод и путь связаны.
	other := sha256.Sum256(transport.WriteProofPreImage(http.MethodPost, transport.PathEnrollCode, nil))
	if ecdsa.Verify(ec, other[:], r, s) {
		t.Fatalf("подпись проходит над чужим прообразом")
	}
	if _, err := c.DeviceSignJSON(`{"message_b64":""}`); err == nil {
		t.Fatalf("пустое сообщение подписано")
	}
}

// TestDeviceAgreeRoundTrip: ECDH через ABI даёт тот же секрет, что встречная
// сторона, и отдаёт собственный открытый ключ, без которого kem_context DHKEM
// не собирается.
func TestDeviceAgreeRoundTrip(t *testing.T) {
	c := deviceCore(t)
	peer, err := transport.NewSoftwareDeviceKeys(t.TempDir())
	if err != nil {
		t.Fatalf("встречные ключи: %v", err)
	}
	peerPub, err := peer.AgreementPublic()
	if err != nil {
		t.Fatalf("встречный открытый ключ: %v", err)
	}
	req, _ := json.Marshal(DeviceAgreeRequest{PeerPubB64: base64.StdEncoding.EncodeToString(peerPub)})
	out, err := c.DeviceAgreeJSON(string(req))
	if err != nil {
		t.Fatalf("согласование: %v", err)
	}
	var ar DeviceAgreeResponse
	if err := json.Unmarshal([]byte(out), &ar); err != nil {
		t.Fatalf("разбор: %v", err)
	}
	shared, _ := base64.StdEncoding.DecodeString(ar.SharedB64)
	ownPub, _ := base64.StdEncoding.DecodeString(ar.OwnPubB64)
	if len(shared) != 32 || len(ownPub) != 65 {
		t.Fatalf("согласование дало %d и %d байт, требуется 32 и 65", len(shared), len(ownPub))
	}
	back, _, err := peer.Agree(1, ownPub)
	if err != nil {
		t.Fatalf("встречное согласование: %v", err)
	}
	if base64.StdEncoding.EncodeToString(back) != ar.SharedB64 {
		t.Fatalf("ECDH несимметричен")
	}
	// Непустое kdf_info это ДРУГОЙ ответ, а не тот же в другой обёртке.
	req2, _ := json.Marshal(DeviceAgreeRequest{
		PeerPubB64: base64.StdEncoding.EncodeToString(peerPub),
		KDFInfoB64: base64.StdEncoding.EncodeToString([]byte("csm1")),
	})
	out2, err := c.DeviceAgreeJSON(string(req2))
	if err != nil {
		t.Fatalf("согласование с kdf_info: %v", err)
	}
	var ar2 DeviceAgreeResponse
	if err := json.Unmarshal([]byte(out2), &ar2); err != nil {
		t.Fatalf("разбор: %v", err)
	}
	if ar2.SharedB64 == ar.SharedB64 {
		t.Fatalf("kdf_info не применён")
	}
	if _, err := c.DeviceAgreeJSON(`{"peer_pub_b64":"AAAA"}`); err == nil {
		t.Fatalf("короткая точка принята")
	}
}

// TestDeviceKeyBridgeRefusesLateSwap: мост нельзя подменить после того, как
// личность устройства заведена. dtp уже у оператора, и ключ, которым его
// подтверждали, у прежнего держателя.
func TestDeviceKeyBridgeRefusesLateSwap(t *testing.T) {
	c := deviceCore(t)
	if err := c.SetDeviceKeyBridge(nil); err != nil {
		t.Fatalf("снятие моста до заведения личности: %v", err)
	}
	if _, err := c.DeviceKeygenJSON(""); err != nil {
		t.Fatalf("keygen: %v", err)
	}
	if err := c.SetDeviceKeyBridge(fakeBridge{}); err == nil {
		t.Fatalf("мост подменён после заведения личности")
	}
}

type fakeBridge struct{}

func (fakeBridge) Keygen(string) (string, error) { return "{}", nil }
func (fakeBridge) Sign(string) (string, error)   { return "{}", nil }
func (fakeBridge) Agree(string) (string, error)  { return "{}", nil }

// TestWantItemKeepsTheDirectiveTypes: карта want типизирована так же, как pol
// директивы. Запись, отправляющая mtu текстом, а kill switch строкой "true",
// была бы отвергнута разборщиком панели по типу, и выглядело бы это как отказ
// записи вообще, а не как неверная кодировка одного поля.
func TestWantItemKeepsTheDirectiveTypes(t *testing.T) {
	cases := []struct {
		json string
		want string // ожидаемый CBOR в hex
		note string
	}{
		{`"auto"`, "646175746f", "текст"},
		{`"default"`, "6764656661756c74", "сентинел сброса это текст для любого ключа"},
		{`1280`, "190500", "uint кратчайшей головой"},
		{`true`, "f5", "булево это простое значение 21"},
		{`false`, "f4", "булево это простое значение 20"},
		{`["1.1.1.1"]`, "8167312e312e312e31", "массив текстов"},
	}
	for _, c := range cases {
		item, _, _, err := wantItem([]byte(c.json))
		if err != nil {
			t.Fatalf("%s: %v", c.note, err)
		}
		enc, err := transport.EncodeCBOR(item)
		if err != nil {
			t.Fatalf("%s: кодирование: %v", c.note, err)
		}
		got := ""
		for _, b := range enc {
			const d = "0123456789abcdef"
			got += string([]byte{d[b>>4], d[b&0x0f]})
		}
		if got != c.want {
			t.Fatalf("%s: %s дало %s, ожидалось %s", c.note, c.json, got, c.want)
		}
	}
	for _, bad := range []string{`null`, `1.5`, `{"a":1}`, `[1]`, `-3`} {
		if _, _, _, err := wantItem([]byte(bad)); err == nil {
			t.Fatalf("значение %s принято", bad)
		}
	}
}

// TestWantVocabularyIsClosed: граница ABI не шире правила клиента.
//
// Инвариант 15 это запрет ПЕРЕДАЧИ split.apps в любую сторону, а номера у
// этого поля нет вовсе (03-WIRE.md 8.3). Проверка, живущая только в слое Dart,
// оставила бы символ CarambaCsmRequestSettings открытым для любого номера с
// любым типом.
func TestWantVocabularyIsClosed(t *testing.T) {
	for _, key := range []uint64{0, 12, 13, 63, 65, 1000} {
		if err := checkWantField(key, wantText, false); err == nil {
			t.Fatalf("неназначенное поле want %d принято", key)
		}
	}
	if err := checkWantField(5, wantText, false); err == nil {
		t.Fatalf("mtu принят текстом")
	}
	if err := checkWantField(9, wantText, false); err == nil {
		t.Fatalf("dns.nameservers принят текстом")
	}
	if err := checkWantField(6, wantUint, false); err == nil {
		t.Fatalf("ipv6 принят числом")
	}
	// Сентинел сброса это текст для поля любого типа: CBOR null запрещён.
	for _, key := range []uint64{1, 5, 6, 9, 64} {
		if err := checkWantField(key, wantText, true); err != nil {
			t.Fatalf("сентинел default отвергнут для поля %d: %v", key, err)
		}
	}
	if err := checkWantField(5, wantUint, false); err != nil {
		t.Fatalf("mtu числом отвергнут: %v", err)
	}
}
