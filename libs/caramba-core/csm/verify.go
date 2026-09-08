package csm

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"errors"
)

// Порядок проверки, 03-WIRE.md раздел 6. Разделение на разбор и проверку
// несущее: шаги P решаются целиком по входным байтам, без ключевого материала
// и без сохранённого состояния, шаги V требуют доверенного документа,
// закреплённого pid, отметки версии, часов или выданного nonce. Всё, что
// вычислимо без секретов, происходит первым, поэтому враждебный кадр не может
// увести верификатор в ключевой материал до того, как он оказался корректным.

// TrustState это состояние, против которого проверяется документ. Оно
// приходит от вызывающего: пакет не читает часы, не ходит в хранилище и не
// хранит ничего между вызовами.
type TrustState struct {
	// PinnedPID это закреплённая идентичность арендатора, 8 байт.
	PinnedPID []byte
	// LinkPin это sha256(root_pk)[0..12], 12 байт. Якорь первого доверия для
	// doc_type 0x01 и 0x05.
	LinkPin []byte
	// Anchor это РАНЕЕ доверенный ключевой документ. Именно из него читаются
	// роли, пороги и список отзыва, никогда из проверяемого документа.
	// Истёкший якорь остаётся действительной точкой авторизации: V12
	// применяется к проверяемому документу, а не к якорю.
	Anchor *KeyDocument

	// Now это текущее время Unix в секундах, ClockTrusted говорит, можно ли
	// ему верить. Пока часам нельзя верить, шаги iat <= now + 300 и
	// now <= exp + 300 пропускаются целиком (02-SPEC.md 5.5).
	Now          int64
	ClockTrusted bool
	// TimeFloor это первичная временная отметка, монотонная, растёт только на
	// принятой директиве.
	TimeFloor int64

	// HWM это отметка старшей версии на (pid, doc_type, scope). Область
	// действия задаёт вызывающий, выбирая, какое состояние подставить.
	HWM map[uint8]uint64
	// StoredFrame это кадр, сохранённый на ver == HWM[doc_type]. Равенство
	// версий принимается ТОЛЬКО при побайтовом совпадении с ним.
	StoredFrame []byte

	// CachedReplay объявляет, что проверяется УЖЕ ПРИНЯТЫЙ кадр, перечитанный
	// с диска. Он снимает шаг V13 и только его, и действует ТОЛЬКО когда кадр
	// побайтово совпадает со StoredFrame: nonce привязан к конкретному
	// запросу, повторить его на перезагрузке нечем, а подмена кадра тем же
	// флагом невозможна, потому что байты уже сравнены. Значение по умолчанию
	// false, то есть директива без выданного nonce отвергается.
	CachedReplay bool

	// ExpectedNonce это 16 байт, которые устройство только что отправило.
	ExpectedNonce []byte
	// DeviceDTP это отпечаток этого устройства, 16 байт.
	DeviceDTP []byte

	// BoundCatHash это cat из доверенной директивы. Когда он задан, шаг V14a
	// требует sha256(кадр каталога) == BoundCatHash.
	BoundCatHash []byte
	// BoundTier это tier ДОВЕРЕННОЙ директивы для шага V14b. Он обязателен
	// всякий раз, когда проверяется каталог против якоря, публикующего tiers:
	// подставить сюда tier проверяемого каталога нельзя, иначе строку таблицы
	// выбирает тот, кого проверяют.
	BoundTier *uint64

	// AgreementKeys это закрытые ключи согласования устройства по поколениям
	// (rkv -> 32-байтовый скаляр P-256). Нужны только для doc_type 0x06.
	//
	// Заполняется ТОЛЬКО программным уровнем ключей. Ключ в Secure Enclave или
	// StrongBox скаляр не отдаёт по построению, поэтому у аппаратного носителя
	// эта карта пуста, а согласование выполняет Agreement.
	AgreementKeys map[uint64][]byte
	// Agreement это источник согласования, когда закрытого скаляра у нас нет.
	// Когда он задан, он побеждает AgreementKeys: аппаратный носитель ключа
	// это точный ответ на вопрос "чем открывать", а карта скаляров это
	// программный частный случай.
	Agreement AgreementSource
}

// AgreementSource это устройство, умеющее выполнить ECDH ключом согласования
// поколения rkv, не выдавая закрытого скаляра.
//
// Существует потому, что 03-WIRE.md 9.1 фиксирует KEM как DHKEM(P-256), а
// 02-SPEC.md 9.4 требует держать ключ согласования в Secure Enclave или
// StrongBox. Оба хранилища выполняют ECDH и НИКОГДА не отдают скаляр наружу,
// поэтому распечатывание 0x06 обязано уметь работать через них.
type AgreementSource interface {
	// Agree выполняет ECDH поколения rkv с несжатой точкой peer (65 байт) и
	// возвращает 32 байта общей координаты X и 65 байт СОБСТВЕННОГО открытого
	// ключа этого поколения. Второе значение входит в kem_context DHKEM и
	// известно только держателю ключа.
	Agree(rkv uint64, peer []byte) (shared []byte, ownPub []byte, err error)
}

// rawAgreement это AgreementSource поверх карты скаляров: программный уровень
// ключей и корпус фикстур.
type rawAgreement map[uint64][]byte

func (m rawAgreement) Agree(rkv uint64, peer []byte) ([]byte, []byte, error) {
	sk, ok := m[rkv]
	if !ok {
		return nil, nil, ErrNoAgreementGeneration
	}
	return ecdhP256(sk, peer)
}

// ErrNoAgreementGeneration отделяет "у устройства нет такого поколения" от
// любого криптографического отказа: первое это шаг 5 раздела 9.4, второе
// это шаг 6.
//
// Различие не косметическое. E_SEAL_RECIPIENT предписывает клиенту сменить
// ключ согласования и перезапросить (02-SPEC.md 10.3), то есть сжечь
// поколение; E_SEAL_OPEN не предписывает ничего подобного. Источник
// согласования, который не умеет сказать "такого поколения у меня нет"
// отдельно от "ECDH не сошёлся", заставляет клиента жечь ключи по любому
// испорченному enc, поэтому сентинел ЭКСПОРТИРОВАН: его обязан возвращать и
// мост аппаратного хранилища, а не только карта скаляров.
var ErrNoAgreementGeneration = errors.New("csm: нет ключа согласования этого поколения")

// Result это итог успешной проверки.
type Result struct {
	Frame *Frame
	Doc   Document
	// Role это роль, которой документ обязан был быть подписан.
	Role uint64
	// Signers это усечённые идентификаторы слотов, чьи подписи проверены.
	Signers [][]byte
	// Inner заполняется для doc_type 0x06: результат полной проверки
	// восстановленного кадра 0x03.
	Inner *Result
	// InnerFrame это восстановленные байты внутреннего кадра.
	InnerFrame []byte
}

// Parse выполняет только шаги P1..P12: кадр и полезная нагрузка, без
// ключевого материала и без состояния. Отказ здесь означает, что перед нами
// не кадр CSM/1, и правильная реакция это выбросить байты и перейти на
// следующую ступень лестницы, а НЕ заявить пользователю о подделке.
func Parse(raw []byte) (*Frame, Document, error) {
	f, err := ParseFrame(raw)
	if err != nil {
		return nil, nil, err
	}
	doc, err := ParseDocument(f)
	if err != nil {
		return f, nil, err
	}
	return f, doc, nil
}

// ParseKeyDocument разбирает кадр ключевого документа. Используется для
// загрузки якоря доверия из хранилища: кадр там уже был проверен, когда его
// принимали, и повторная проверка требовала бы предыдущего якоря.
func ParseKeyDocument(raw []byte) (*KeyDocument, *Frame, error) {
	f, doc, err := Parse(raw)
	if err != nil {
		return nil, f, err
	}
	kd, ok := doc.(*KeyDocument)
	if !ok {
		return nil, f, errf(EParseDocType, "P3", "frame is %s, not a key document", DocTypeName(f.DocType))
	}
	return kd, f, nil
}

// Verify выполняет P1..P12, затем V1..V14 (и шаги распечатывания раздела 9.4
// для doc_type 0x06). Проверка идёт по ПРИНЯТЫМ байтам: ни один путь не
// пересериализует полезную нагрузку.
func Verify(raw []byte, st *TrustState) (*Result, error) {
	f, doc, err := Parse(raw)
	if err != nil {
		return nil, err
	}
	return verifyParsed(f, doc, st)
}

func verifyParsed(f *Frame, doc Document, st *TrustState) (*Result, error) {
	// V1: роль выводится из doc_type. Сбоя быть не может: P3 уже отверг все
	// неопределённые, зарезервированные и частные значения, а строка таблицы
	// есть для каждого выжившего.
	rule, ok := AuthRuleFor(f.DocType)
	if !ok {
		return nil, errf(EParseDocType, "P3", "doc_type 0x%02x has no authorization row", f.DocType)
	}
	env := doc.Envelope()

	// V2 и V3: разрешить якорь и прочитать из него набор ключей и порог.
	anchorSet, selfSet, err := resolveAuthorization(rule, doc, st)
	if err != nil {
		return nil, err
	}

	// V4: каждый слот обязан быть в разрешённом наборе. Слот вне набора
	// отвергает ВЕСЬ кадр и никогда не пропускается: кадр с посторонней
	// подписью это кадр, который кто-то пытался отмыть.
	for _, slot := range f.Sigs {
		kid := slot.KeyID[:]
		if (anchorSet == nil || !anchorSet.contains(kid)) && (selfSet == nil || !selfSet.contains(kid)) {
			return nil, errf(EVerifyUnauthorized, "V4",
				"slot keyid_trunc %x is in no authorized key set for role %s", kid, RoleName(rule.Role))
		}
	}

	// V5: отзыв проверяется ДО арифметики подписи, чтобы отозванный ключ
	// вообще не получил проверки.
	if st.Anchor != nil {
		for _, slot := range f.Sigs {
			for _, rk := range st.Anchor.Rev.KIDs {
				if bytes.Equal(rk, slot.KeyID[:]) {
					return nil, errf(EVerifyRevoked, "V5", "slot keyid_trunc %x appears in the anchor rev.kids", rk)
				}
			}
		}
	}

	// V6: строгий профиль Ed25519 над прообразом, то есть над первыми
	// 7 + payload_len байтами кадра как они пришли.
	validAnchor := map[string]bool{}
	validSelf := map[string]bool{}
	signers := make([][]byte, 0, len(f.Sigs))
	for _, slot := range f.Sigs {
		kid := slot.KeyID[:]
		var pk []byte
		if anchorSet != nil {
			if p, ok := anchorSet.publicKey(kid); ok {
				pk = p
			}
		}
		if pk == nil && selfSet != nil {
			if p, ok := selfSet.publicKey(kid); ok {
				pk = p
			}
		}
		if pk == nil {
			// V4 это уже исключил; страховка, чтобы ключ без роли не всплыл.
			return nil, errf(EVerifyUnauthorized, "V4", "slot keyid_trunc %x resolved to no key", kid)
		}
		if err := CheckPublicKey(pk); err != nil {
			return nil, errf(EVerifySig, "V6", "slot keyid_trunc %x: %v", kid, err)
		}
		if !VerifySignature(pk, f.PreImage, slot.Sig[:]) {
			return nil, errf(EVerifySig, "V6", "slot keyid_trunc %x: signature does not verify over the pre-image", kid)
		}
		signers = append(signers, append([]byte(nil), kid...))
		if anchorSet != nil && anchorSet.contains(kid) {
			validAnchor[string(kid)] = true
		}
		if selfSet != nil && selfSet.contains(kid) {
			validSelf[string(kid)] = true
		}
	}

	// V7: порог. Для ключевого документа обе стороны правила ротации
	// проверяются на шаге V10 и дают E_VERIFY_ROTATION, поэтому здесь для
	// 0x01 порог не считается дважды.
	if f.DocType != DocKey {
		set := anchorSet
		if set == nil {
			set = selfSet
		}
		if uint64(len(validAnchor)+len(validSelf)) < set.Thr {
			return nil, errf(EVerifyThreshold, "V7",
				"%d distinct valid signer(s) against a threshold of %d for role %s",
				len(validAnchor)+len(validSelf), set.Thr, RoleName(rule.Role))
		}
	}

	// V8: pid байт в байт.
	if len(st.PinnedPID) > 0 && !bytes.Equal(env.PID, st.PinnedPID) {
		return nil, errf(EVerifyPID, "V8", "payload pid %x is not the pinned pid %x", env.PID, st.PinnedPID)
	}

	// V9: правило версии.
	if err := checkVersion(f, env, st); err != nil {
		return nil, err
	}

	// V10: ротация корня, только для 0x01 и только когда версия РАСТЁТ.
	//
	// 03-WIRE.md 6.3 разрешает ver == hwm при побайтовом совпадении с
	// сохранённым кадром и говорит прямо: "accepted as the same document and
	// no state changes". Ротации там не происходит, а 7.3 описывает правило
	// для документа с ver = N+1, поэтому применять V10 к перечитанному кадру
	// значило бы отвергать ровно то, что V9 только что принял. Порог при этом
	// не пропадает: на ветке перечитывания он считается здесь, потому что для
	// 0x01 шаг V7 выше намеренно пропущен.
	if f.DocType == DocKey {
		if env.Ver > st.HWM[DocKey] {
			if err := checkRotation(env, st, anchorSet, selfSet, validAnchor, validSelf); err != nil {
				return nil, err
			}
		} else {
			set := anchorSet
			if set == nil {
				set = selfSet
			}
			if set == nil {
				return nil, errf(EVerifyRole, "V3", "key document resolves to no authorized key set")
			}
			if uint64(len(validAnchor)+len(validSelf)) < set.Thr {
				return nil, errf(EVerifyThreshold, "V7",
					"%d distinct valid signer(s) against a threshold of %d for role root",
					len(validAnchor)+len(validSelf), set.Thr)
			}
		}
	}

	// V11 и V12: свежесть.
	if err := checkFreshness(f.DocType, env, st); err != nil {
		return nil, err
	}

	res := &Result{Frame: f, Doc: doc, Role: rule.Role, Signers: signers}

	// V13: nonce и отпечаток устройства, только для 0x03.
	// Шаг безусловен: 03-WIRE.md 6.2 не даёт директиве ветки "nonce не
	// выдавали". Директива, проверяемая без выданного nonce, это ошибка
	// вызывающего, и молча пропустить её значило бы обезоружить единственную
	// проверку, переживающую неверные часы (инвариант 9).
	cachedReplay := st.CachedReplay && len(st.StoredFrame) > 0 && bytes.Equal(st.StoredFrame, f.Raw)
	if dir, ok := doc.(*Directive); ok && !cachedReplay {
		if len(st.ExpectedNonce) != 16 || !bytes.Equal(dir.Nonce, st.ExpectedNonce) {
			return nil, errf(EVerifyNonce, "V13", "echoed nonce is not the outstanding one")
		}
		if len(st.DeviceDTP) != 16 || !bytes.Equal(dir.DTP, st.DeviceDTP) {
			return nil, errf(EVerifyDevice, "V13", "dtp names another device")
		}
	}

	// V14: привязка каталога, только для 0x02.
	if _, ok := doc.(*Catalog); ok {
		if err := checkCatalogBinding(f, st); err != nil {
			return nil, err
		}
	}

	// Шаги 3..7 раздела 9.4 для запечатанной директивы.
	if sd, ok := doc.(*SealedDirective); ok {
		inner, innerFrame, err := openSealed(sd, st)
		if err != nil {
			return nil, err
		}
		res.Inner = inner
		res.InnerFrame = innerFrame
	}

	return res, nil
}

// resolveAuthorization это шаги V2 и V3. Возвращает набор из ЯКОРЯ и, только
// для правила ротации, набор из САМОГО документа.
func resolveAuthorization(rule AuthRule, doc Document, st *TrustState) (anchorSet, selfSet *authorizedSet, err error) {
	switch rule.KeySet {
	case FromLinkPin:
		// Bootstrap blob: якорь это link_pin, порог единица. Ключ берётся из
		// поля rk самого документа, и он принимается только если его
		// sha256[0..12] равен продиктованному пину.
		if len(st.LinkPin) != KeyIDTruncLen {
			return nil, nil, errf(EVerifyNoAnchor, "V2", "no link_pin is pinned for this profile")
		}
		blob, ok := doc.(*BootstrapBlob)
		if !ok {
			return nil, nil, errf(EVerifyNoAnchor, "V2", "link_pin anchoring applies only to a bootstrap blob")
		}
		kid := KeyIDOf(blob.RK)
		set := &authorizedSet{Role: rule.Role, keys: map[string][]byte{}, Thr: 1, origin: "link_pin"}
		if bytes.Equal(kid, st.LinkPin) {
			set.keys[string(kid)] = blob.RK
		}
		// Несовпадение пина оставляет набор пустым, и V4 отвергает кадр с
		// E_VERIFY_UNAUTHORIZED. Пути "всё равно продолжить" не существует.
		return set, nil, nil

	case FromAnchorAndSelf:
		kd, ok := doc.(*KeyDocument)
		if !ok {
			return nil, nil, errf(EVerifyRole, "V3", "rotation rule applies only to a key document")
		}
		self, ok := authorizedSetFrom(kd, rule.Role, "self")
		if !ok {
			return nil, nil, errf(EVerifyRole, "V3", "the document under verification has no role %s", RoleName(rule.Role))
		}
		if st.Anchor == nil {
			// Первое доверие: якоря нет, роль якоря заменяет link_pin.
			if len(st.LinkPin) != KeyIDTruncLen {
				return nil, nil, errf(EVerifyNoAnchor, "V2", "no trusted key document and no link_pin")
			}
			if len(kd.Roles[RoleRoot].KS) != 1 {
				return nil, nil, errf(EVerifyUnauthorized, "V4",
					"first trust: the key document carries %d keys under role root, exactly one is required",
					len(kd.Roles[RoleRoot].KS))
			}
			if !bytes.Equal(kd.Roles[RoleRoot].KS[0], st.LinkPin) {
				return nil, nil, errf(EVerifyUnauthorized, "V4",
					"first trust: root kid %x does not match link_pin %x", kd.Roles[RoleRoot].KS[0], st.LinkPin)
			}
			// Сторона якоря это сам link_pin, и она уже проверена выше.
			// Остаётся набор самого документа.
			return nil, self, nil
		}
		anchor, ok := authorizedSetFrom(st.Anchor, rule.Role, "anchor")
		if !ok {
			return nil, nil, errf(EVerifyRole, "V3", "the trusted key document has no role %s", RoleName(rule.Role))
		}
		return anchor, self, nil

	default: // FromAnchor
		if st.Anchor == nil {
			return nil, nil, errf(EVerifyNoAnchor, "V2",
				"no trusted key document for this profile; link_pin anchors only doc_type 0x01 and 0x05")
		}
		anchor, ok := authorizedSetFrom(st.Anchor, rule.Role, "anchor")
		if !ok {
			return nil, nil, errf(EVerifyRole, "V3", "the trusted key document has no role %s", RoleName(rule.Role))
		}
		return anchor, nil, nil
	}
}

// checkVersion это шаг V9, 03-WIRE.md 6.3.
//
// Для 0x02 и 0x04 шаг инертен по построению: областью действия отметки служит
// cat_id, производный от собственных байт каталога, поэтому два разных
// каталога никогда не делят область и старый всегда встречает нулевую отметку.
// Настоящая граница отката для каталога это V14a плюс монотонность директивы,
// которая его назвала.
func checkVersion(f *Frame, env Envelope, st *TrustState) error {
	hwm := st.HWM[f.DocType]
	switch {
	case env.Ver < hwm:
		return errf(EVerifyVersion, "V9", "ver %d is below the high-water mark %d", env.Ver, hwm)
	case env.Ver == hwm:
		// Равенство принимается только при побайтовом совпадении с
		// сохранённым кадром: это то, что позволяет перечитать собственный
		// кеш, не ослабляя монотонное правило.
		if len(st.StoredFrame) == 0 || !bytes.Equal(st.StoredFrame, f.Raw) {
			return errf(EVerifyVersion, "V9",
				"ver %d equals the high-water mark but the frame is not byte-identical to the stored one", env.Ver)
		}
	}
	return nil
}

// checkRotation это шаг V10, 03-WIRE.md 7.3. Клиент обязан отказаться
// пропустить версию, и обе стороны двойной проверки обязаны пройти.
func checkRotation(env Envelope, st *TrustState, anchorSet, selfSet *authorizedSet, validAnchor, validSelf map[string]bool) error {
	n := st.HWM[DocKey]
	if env.Ver != n+1 {
		return errf(EVerifyRotation, "V10",
			"key document ver %d against a trusted version %d; a client MUST refuse to skip a version", env.Ver, n)
	}
	if selfSet == nil {
		return errf(EVerifyRotation, "V10", "key document carries no root role to verify against")
	}
	if anchorSet == nil {
		// Первое доверие: сторона якоря это сам link_pin, и она уже
		// проверена в resolveAuthorization. Остаётся порог документа.
		if uint64(len(validSelf)) < selfSet.Thr {
			return errf(EVerifyThreshold, "V7",
				"%d valid signer(s) against the document threshold %d", len(validSelf), selfSet.Thr)
		}
		return nil
	}
	if uint64(len(validAnchor)) < anchorSet.Thr {
		return errf(EVerifyRotation, "V10",
			"%d valid signer(s) from the trusted key set against its threshold %d; both sides MUST pass",
			len(validAnchor), anchorSet.Thr)
	}
	if uint64(len(validSelf)) < selfSet.Thr {
		return errf(EVerifyRotation, "V10",
			"%d valid signer(s) from the new key set against its threshold %d; both sides MUST pass",
			len(validSelf), selfSet.Thr)
	}
	return nil
}

// checkFreshness это шаги V11 и V12, 03-WIRE.md 6.4 и 02-SPEC.md 5.4.
//
// Нижний порог применяется в буквальной нормативной форме
// iat + LIFETIME_MAX[doc_type] + 300 >= time_floor. Более ранняя редакция
// этого пакета несла двойной срок жизни, чтобы развести два вектора корпуса,
// которые при буквальной форме оба отвергались на V11; вместо этого исправлена
// сама фикстура neg-verify-expired (её iat поднят внутрь окна, где документ
// уже просрочен относительно now, но ещё не был мёртв на момент floor), так
// что коды E_VERIFY_IAT и E_VERIFY_EXPIRED разделяет предикат спецификации, а
// не выдуманная константа.
func checkFreshness(dt uint8, env Envelope, st *TrustState) error {
	// V11, часовая половина.
	if st.ClockTrusted && int64(env.IAT) > st.Now+SkewSeconds {
		return errf(EVerifyIAT, "V11", "iat %d is more than %d seconds ahead of now %d", env.IAT, SkewSeconds, st.Now)
	}
	// V11, нижний порог времени. Документ, не переживший его, был уже мёртв в
	// тот момент, когда профиль в последний раз слышал панель.
	if st.TimeFloor > 0 {
		life := lifetimeMax[dt]
		if int64(env.IAT)+int64(life)+SkewSeconds < st.TimeFloor {
			return errf(EVerifyIAT, "V11",
				"iat %d plus its lifetime is below the time floor %d", env.IAT, st.TimeFloor)
		}
	}
	// V12. Истечение означает отказ принимать НОВЫЕ инструкции и НОВЫЙ статус.
	// Оно не отключает пользователя, не рвёт туннель и не чистит кеш
	// (инвариант 16, и он абсолютен).
	if st.ClockTrusted && st.Now > int64(env.Exp)+SkewSeconds {
		return errf(EVerifyExpired, "V12", "now %d is past exp %d plus %d seconds of skew", st.Now, env.Exp, SkewSeconds)
	}
	return nil
}

// checkCatalogBinding это шаги V14a и V14b.
//
// Тариф для V14b берётся ТОЛЬКО из доверенной директивы (st.BoundTier).
// Прочитать его из проверяемого каталога значило бы отдать выбор строки
// tiers тому, кого проверяют: скомпрометированный онлайн-ключ подписал бы
// каталог с tier, для которого корень ничего не публиковал, шаг тихо не
// выполнился бы, и 03-WIRE.md 6.2 "V14b is what stops a compromised online
// key inventing a fleet" перестал бы что-либо значить.
func checkCatalogBinding(f *Frame, st *TrustState) error {
	sum := sha256.Sum256(f.Raw)
	// V14a: безусловная привязка каталога к директиве, которая его назвала.
	if len(st.BoundCatHash) == 32 && !bytes.Equal(sum[:], st.BoundCatHash) {
		return errf(EVerifyCatHash, "V14a", "sha256(frame) %x is not the cat the trusted directive named %x",
			sum[:8], st.BoundCatHash[:8])
	}
	// V14b: работает только когда доверенный ключевой документ опубликовал
	// хеш для тарифа ДИРЕКТИВЫ. Его отсутствие НЕ повод отвергнуть каталог, но
	// клиент обязан записать в верификационной обвязке "fleet not
	// root-anchored" и не показывать флот как проверенный.
	if st.Anchor == nil || len(st.Anchor.Tiers) == 0 {
		return nil
	}
	if st.BoundTier == nil {
		// Якорь публикует tiers, а вызывающий не назвал тариф директивы:
		// проверить нечего и подставить нечего. Это ошибка вызывающего, и
		// принять каталог на ней значило бы вернуть ровно ту дыру, ради
		// которой шаг существует.
		return errf(EVerifyCatHash, "V14b",
			"the trusted key document publishes tiers but no directive tier was bound for this catalog")
	}
	if want, ok := st.Anchor.Tiers[*st.BoundTier]; ok && !bytes.Equal(sum[:], want) {
		return errf(EVerifyCatHash, "V14b", "sha256(frame) does not match the root-signed tiers[%d] entry", *st.BoundTier)
	}
	return nil
}

// FleetRootAnchored сообщает, покрыт ли каталог хешем тарифа из корневого
// документа. Тариф передаётся вызывающим и берётся из доверенной директивы, а
// не из каталога: иначе строку таблицы выбирал бы проверяемый документ. Ложь
// означает пониженное сдерживание, которое клиент обязан показать как
// "fleet not root-anchored" (02-SPEC.md 4.3).
func FleetRootAnchored(tier uint64, anchor *KeyDocument) bool {
	if anchor == nil || len(anchor.Tiers) == 0 {
		return false
	}
	_, ok := anchor.Tiers[tier]
	return ok
}

// ------------------------------------------------------------ распечатывание

// Параметры набора HPKE, 03-WIRE.md 9.1.
const (
	HPKEModeBase uint8  = 0x00
	HPKEKemID    uint64 = 16 // DHKEM(P-256, HKDF-SHA256)
	HPKEKdfID    uint64 = 1  // HKDF-SHA256
	HPKEAeadID   uint64 = 3  // ChaCha20Poly1305

	// HPKEInfo это строка info для направления панель -> устройство.
	HPKEInfo = "CSM1-seal-v1"
)

// SealAAD собирает 33-байтовый aad раздела 9.2:
// "CSM1" || 0x06 || pid(8) || dtp(16) || u32be(ver).
// Получатель ОБЯЗАН пересчитать его из полей внешней полезной нагрузки и НЕ
// ИМЕЕТ ПРАВА принимать aad с провода.
func SealAAD(pid, dtp []byte, ver uint32) []byte {
	out := make([]byte, 0, 4+1+PIDLen+16+4)
	out = append(out, Magic[:]...)
	out = append(out, DocSealed)
	out = append(out, pid...)
	out = append(out, dtp...)
	var v [4]byte
	binary.BigEndian.PutUint32(v[:], ver)
	return append(out, v[:]...)
}

// openSealed выполняет шаги 3..7 раздела 9.4.
func openSealed(sd *SealedDirective, st *TrustState) (*Result, []byte, error) {
	// Шаг 3: получатель. Это ошибка корректности, а не событие безопасности,
	// и она НЕ ДОЛЖНА подаваться как подделка: так выглядит зеркало,
	// отдавшее кешированный ответ для другого устройства.
	if len(st.DeviceDTP) != 16 || !bytes.Equal(sd.DTP, st.DeviceDTP) {
		return nil, nil, errf(ESealRecipient, "seal step 3", "the sealed directive is addressed to another device")
	}
	// Шаг 4: набор шифров.
	if sd.KEM != HPKEKemID || sd.KDF != HPKEKdfID || sd.AEAD != HPKEAeadID {
		return nil, nil, errf(ESealSuite, "seal step 4",
			"suite is kem %d kdf %d aead %d, required is %d/%d/%d", sd.KEM, sd.KDF, sd.AEAD,
			HPKEKemID, HPKEKdfID, HPKEAeadID)
	}
	if len(sd.Enc) != 65 || sd.Enc[0] != 0x04 {
		return nil, nil, errf(ESealSuite, "seal step 4",
			"enc must be an uncompressed P-256 point 0x04 || X || Y")
	}
	// Шаг 5: ключ согласования поколения rkv. Аппаратный носитель скаляра не
	// отдаёт, поэтому спрашивается не сам ключ, а результат ECDH.
	src := st.Agreement
	if src == nil {
		src = rawAgreement(st.AgreementKeys)
	}
	shared, ownPub, aerr := src.Agree(sd.RKV, sd.Enc)
	if aerr != nil {
		// Шаг 5 и шаг 6 отвечают разными кодами, потому что предписывают
		// разные действия. "Поколения нет" это E_SEAL_RECIPIENT, и клиент
		// ОБЯЗАН сменить ключ согласования. Любой другой отказ согласования
		// (enc с верным префиксом, но точкой не на кривой, отказ хранилища)
		// это шаг 6: там нечего перевыпускать, и ответить кодом шага 5 значило
		// бы предписать сжигание поколения по испорченному входу.
		if errors.Is(aerr, ErrNoAgreementGeneration) {
			return nil, nil, errf(ESealRecipient, "seal step 5", "this device holds no agreement key of generation %d", sd.RKV)
		}
		return nil, nil, errf(ESealOpen, "seal step 6", "%v", aerr)
	}
	// Шаг 6: открыть. aad пересчитывается из внешней полезной нагрузки.
	aad := SealAAD(sd.Env.PID, sd.DTP, uint32(sd.Env.Ver))
	pt, err := hpkeOpenBaseDH(shared, ownPub, sd.Enc, []byte(HPKEInfo), aad, sd.CT)
	if err != nil {
		return nil, nil, errf(ESealOpen, "seal step 6", "%v", err)
	}
	// Шаг 7: восстановленный открытый текст это ПОЛНЫЙ кадр 0x03. Получатель,
	// восстановивший что-то другое, обязан трактовать это как E_SEAL_OPEN и НЕ
	// пытаться разобрать частично.
	if len(pt) < HeaderLen || !bytes.Equal(pt[:MagicLen], Magic[:]) || pt[4] != DocDirective {
		return nil, nil, errf(ESealOpen, "seal step 6", "the recovered plaintext is not a 0x03 frame")
	}
	inner, err := Verify(pt, st)
	if err != nil {
		return nil, nil, err
	}
	return inner, pt, nil
}
