package api

import (
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"

	"github.com/semanticparadox/caramba/libs/caramba-core/transport"
)

// Ключи устройства, ABI v3 (02-SPEC.md 12.2, 03-WIRE.md 13.6 и 13.8).
//
// Три символа этой группы существуют потому, что раздел 9.4 кладёт ключ
// подписи устройства в Secure Enclave или StrongBox, а до обоих не дотягивается
// ни Dart, ни Go: реализация на Go по построению кладёт ключ в файл, то есть в
// программный уровень. Аппаратный держатель ключа это платформенный мост, а
// ядро зовёт его через transport.DeviceKeyBridge. Там, где моста нет (Windows,
// Linux, любая сборка без хранилища), работает transport.SoftwareDeviceKeys, и
// он ЧЕСТНО докладывает уровень 3, а не выдаёт себя за аппаратный.

// DeviceKeygenRequest это вход CarambaDeviceKeygen.
type DeviceKeygenRequest struct {
	// Purpose это "sign" или "agree". Обе пары заводятся вместе, потому что
	// тело регистрации 03-WIRE.md 13.8 несёт обе сразу; поле выбирает, какой
	// ключ считать главным в ответе, и никогда не заводит половину личности.
	Purpose string `json:"purpose,omitempty"`
	// RequireHardware это ПРОСЬБА, а не требование. Ответ говорит правду о том,
	// что получилось: сборка без хранилища возвращает уровень 3, и вызывающий
	// видит это в поле tier, а не узнаёт из отказа.
	RequireHardware bool `json:"require_hardware,omitempty"`
}

// DeviceKeygenResponse это выход CarambaDeviceKeygen.
type DeviceKeygenResponse struct {
	// SPKIB64 это ключ подписи как DER SubjectPublicKeyInfo.
	SPKIB64 string `json:"spki_b64,omitempty"`
	// AgreePubB64 это открытый ключ согласования, 65 байт несжатой точки.
	AgreePubB64 string `json:"agree_pub_b64,omitempty"`
	// DTPHex это dtp = sha256(spki)[0..16], то, что клиент хранит и показывает
	// на экране "что это приложение отправляет".
	DTPHex string `json:"dtp_hex,omitempty"`
	// Tier это 1 Secure Enclave, 2 StrongBox или TEE, 3 программное хранилище.
	Tier int `json:"tier,omitempty"`
	// Generation это поколение ключа согласования, rkv, начинается с 1.
	Generation uint64 `json:"generation,omitempty"`
	Error      string `json:"error,omitempty"`
}

// DeviceSignRequest это вход CarambaDeviceSign.
type DeviceSignRequest struct {
	// MessageB64 это СООБЩЕНИЕ, не дайджест: операция хеширует его сама.
	MessageB64 string `json:"message_b64"`
}

// DeviceSignResponse это выход CarambaDeviceSign: 64 байта r || s, низкий s.
type DeviceSignResponse struct {
	SigB64 string `json:"sig_b64,omitempty"`
	// ProofHeader это та же подпись как base64url без дополнения, 86 символов,
	// то есть готовое значение заголовка X-CSM-Proof.
	ProofHeader string `json:"proof_header,omitempty"`
	Error       string `json:"error,omitempty"`
}

// DeviceAgreeRequest это вход CarambaDeviceAgree.
type DeviceAgreeRequest struct {
	// RKV это поколение ключа согласования. Ноль означает текущее.
	RKV uint64 `json:"rkv,omitempty"`
	// PeerPubB64 это 65 байт несжатой точки P-256 отправителя.
	PeerPubB64 string `json:"peer_pub_b64"`
	// KDFInfoB64 пусто для CSM/1: DHKEM берёт сырую общую координату X, а
	// kem_context собирает сам. Непустое значение это ЯВНАЯ просьба выдать
	// HKDF-SHA256 от общего секрета с этим info, а не тот же ответ в другой
	// обёртке.
	KDFInfoB64 string `json:"kdf_info_b64,omitempty"`
}

// DeviceAgreeResponse это выход CarambaDeviceAgree.
type DeviceAgreeResponse struct {
	SharedB64 string `json:"shared_b64,omitempty"`
	// OwnPubB64 это собственный открытый ключ этого поколения. Он входит в
	// kem_context DHKEM и известен только держателю ключа, поэтому уходит
	// вместе с секретом.
	OwnPubB64 string `json:"own_pub_b64,omitempty"`
	Error     string `json:"error,omitempty"`
}

// SetDeviceKeyBridge устанавливает платформенный держатель ключей устройства.
//
// Вызывается ДО первого обращения к CSM. Замена моста после того, как личность
// устройства уже заведена, это ошибка, а не тихая подмена: dtp уже отправлен
// оператору, а ключ, которым его подтверждали, у прежнего держателя.
func (c *Core) SetDeviceKeyBridge(b transport.DeviceKeyBridge) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.csm != nil {
		return fmt.Errorf("api: личность устройства уже заведена, мост ключей не подменяется")
	}
	if b == nil {
		c.deviceBridge = nil
		return nil
	}
	c.deviceBridge = b
	return nil
}

// deviceKeys возвращает ключи устройства выборщика, создавая его при первом
// обращении.
func (c *Core) deviceKeys() (transport.DeviceKeys, error) {
	f, err := c.csmFetcher()
	if err != nil {
		return nil, err
	}
	k := f.Keys()
	if k == nil {
		return nil, fmt.Errorf("api: ключи устройства недоступны")
	}
	return k, nil
}

// DeviceKeygenJSON это символ CarambaDeviceKeygen.
//
// Идемпотентен: личность устройства заводится ОДИН раз и переживает
// перезапуск. Повторный вызов отдаёт ту же личность, а не новую, потому что
// новая означала бы новый dtp, а значит второе устройство в списке оператора
// после каждого запуска приложения.
func (c *Core) DeviceKeygenJSON(jsonStr string) (string, error) {
	var req DeviceKeygenRequest
	if jsonStr != "" {
		if err := json.Unmarshal([]byte(jsonStr), &req); err != nil {
			return "", fmt.Errorf("api: разбор запроса ключей устройства: %w", err)
		}
	}
	switch req.Purpose {
	case "", "sign", "agree":
	default:
		return "", fmt.Errorf("api: purpose %q не sign и не agree", req.Purpose)
	}
	k, err := c.deviceKeys()
	if err != nil {
		return "", err
	}
	spki, err := k.SigningSPKI()
	if err != nil {
		return "", err
	}
	agree, err := k.AgreementPublic()
	if err != nil {
		return "", err
	}
	return toJSONString(DeviceKeygenResponse{
		SPKIB64:     base64.StdEncoding.EncodeToString(spki),
		AgreePubB64: base64.StdEncoding.EncodeToString(agree),
		DTPHex:      hex.EncodeToString(transport.Thumbprint(spki)),
		Tier:        k.Tier(),
		Generation:  k.Generation(),
	})
}

// DeviceSignJSON это символ CarambaDeviceSign.
//
// Подписывается СООБЩЕНИЕ, а не дайджест: 03-WIRE.md 13.6 фиксирует
// message-API платформ, и реализация, которая хеширует заранее и подписывает
// дайджест как сообщение, собрала бы верный прообраз и всё равно выдала
// подпись, которую панель отвергнет.
func (c *Core) DeviceSignJSON(jsonStr string) (string, error) {
	var req DeviceSignRequest
	if err := json.Unmarshal([]byte(jsonStr), &req); err != nil {
		return "", fmt.Errorf("api: разбор запроса подписи: %w", err)
	}
	msg, err := decodeB64(req.MessageB64)
	if err != nil {
		return "", fmt.Errorf("api: message_b64: %w", err)
	}
	if len(msg) == 0 {
		return "", fmt.Errorf("api: пустое сообщение не подписывается")
	}
	// Ключ устройства подписывает ОДНУ конструкцию (03-WIRE.md 13.6), и здесь
	// это проверяется, а не подразумевается. Символ ABI открыт всему процессу
	// приложения, и держатель, подписывающий произвольные байты, стоит вместо
	// извлечения ключа из Secure Enclave: непроверенный вызов дал бы подпись
	// под телом регистрации на враждебный origin или под записью по другому
	// каноническому пути, а аппаратная неизвлекаемость от этого не спасает.
	if err := transport.CheckWriteProofPreImage(msg); err != nil {
		return "", err
	}
	k, err := c.deviceKeys()
	if err != nil {
		return "", err
	}
	sig, err := k.Sign(msg)
	if err != nil {
		return "", err
	}
	// Форма проверяется на выходе, а не только у моста: подпись в ASN.1 DER
	// или с высоким s выглядела бы отказом панели, а не дефектом клиента.
	if err := transport.CheckSignatureForm(sig); err != nil {
		return "", err
	}
	return toJSONString(DeviceSignResponse{
		SigB64:      base64.StdEncoding.EncodeToString(sig),
		ProofHeader: transport.ProofHeader(sig),
	})
}

// DeviceAgreeJSON это символ CarambaDeviceAgree.
func (c *Core) DeviceAgreeJSON(jsonStr string) (string, error) {
	var req DeviceAgreeRequest
	if err := json.Unmarshal([]byte(jsonStr), &req); err != nil {
		return "", fmt.Errorf("api: разбор запроса согласования: %w", err)
	}
	peer, err := decodeB64(req.PeerPubB64)
	if err != nil {
		return "", fmt.Errorf("api: peer_pub_b64: %w", err)
	}
	if len(peer) != 65 || peer[0] != 0x04 {
		return "", fmt.Errorf("api: peer_pub_b64 обязан быть несжатой точкой P-256, 65 байт")
	}
	k, err := c.deviceKeys()
	if err != nil {
		return "", err
	}
	rkv := req.RKV
	if rkv == 0 {
		rkv = k.Generation()
	}
	shared, ownPub, err := k.Agree(rkv, peer)
	if err != nil {
		return "", err
	}
	if req.KDFInfoB64 != "" {
		info, err := decodeB64(req.KDFInfoB64)
		if err != nil {
			return "", fmt.Errorf("api: kdf_info_b64: %w", err)
		}
		shared = transport.HKDFSHA256(nil, shared, info, 32)
	}
	return toJSONString(DeviceAgreeResponse{
		SharedB64: base64.StdEncoding.EncodeToString(shared),
		OwnPubB64: base64.StdEncoding.EncodeToString(ownPub),
	})
}
