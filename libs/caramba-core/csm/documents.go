package csm

import (
	"bytes"
	"crypto/sha256"
	"strings"
)

// Типизированные документы по реестру полей 03-WIRE.md раздел 8.
// Разбор полезной нагрузки это шаги P9 (CBOR), P10 (общий конверт),
// P11 (поля документа) и P12 (материал ключей внутри ключевого документа).

// Максимальные сроки жизни, 02-SPEC.md 5.2. Панель НЕ ИМЕЕТ ПРАВА подписать
// документ, у которого exp - iat больше этих значений.
var lifetimeMax = map[uint8]uint64{
	DocKey:       604800,
	DocCatalog:   2592000,
	DocDirective: 3600,
	DocChunk:     2592000,
	DocBootstrap: 2592000,
	DocSealed:    3600,
	DocReserve:   604800,
}

// LifetimeMax возвращает LIFETIME_MAX[doc_type] в секундах.
func LifetimeMax(dt uint8) uint64 { return lifetimeMax[dt] }

// Envelope это общий конверт, ключи 1..5 обязательны в каждом документе,
// ключ 9 это дополнение (03-WIRE.md 8.0).
type Envelope struct {
	V   uint64
	PID []byte
	Ver uint64
	IAT uint64
	Exp uint64
	Pad []byte
}

// Document это любой разобранный документ CSM/1.
type Document interface {
	DocType() uint8
	Envelope() Envelope
}

// ------------------------------------------------------------------ 0x01

// keyEntry это запись массива keys. Роли у записи нет: роль живёт только в
// roles, чтобы две записи не могли разойтись.
type keyEntry struct {
	KID []byte
	Alg uint64
	PK  []byte
}

// RoleEntry это значение в roles: набор ключей и порог.
type RoleEntry struct {
	KS  [][]byte
	Thr uint64
}

// Revocation это список отзыва.
type Revocation struct {
	KIDs  [][]byte
	Nodes []string
}

// Deprecation это запись dep.
type Deprecation struct {
	Surface string
	Sunset  uint64
}

// KeyDocument, doc_type 0x01. Подписан ролью root. Это якорь доверия для
// всего остального.
//
// Массив keys НЕ экспортирован намеренно. 02-SPEC.md 3 и 01-DECISION.md 5.1.3
// требуют, чтобы ни один путь кода и ни одна точка сериализации не отдавали
// открытый ключ без его роли, а запись keys роли не несёт: роль живёт только
// в roles. Наружу ключевой материал уходит единственным способом, через
// AuthorizedKey, который спрашивает роль первым аргументом.
type KeyDocument struct {
	Env   Envelope
	Roles map[uint64]RoleEntry // роль -> набор усечённых идентификаторов и порог
	Rev   Revocation
	Tiers map[uint64][]byte
	Dep   []Deprecation
	TTLK  uint64

	keys []keyEntry
}

func (d *KeyDocument) DocType() uint8     { return DocKey }
func (d *KeyDocument) Envelope() Envelope { return d.Env }

// KeyCount возвращает число записей в keys.
func (d *KeyDocument) KeyCount() int { return len(d.keys) }

// AuthorizedKey возвращает открытый ключ ТОЛЬКО вместе с ролью, которая его
// авторизует: ключ, не названный этой ролью, не выдаётся, даже если он
// присутствует в keys.
func (d *KeyDocument) AuthorizedKey(role uint64, kid []byte) ([]byte, bool) {
	re, ok := d.Roles[role]
	if !ok {
		return nil, false
	}
	named := false
	for _, k := range re.KS {
		if bytes.Equal(k, kid) {
			named = true
			break
		}
	}
	if !named {
		return nil, false
	}
	return d.publicKey(kid)
}

// publicKey это внутренний поиск по усечённому идентификатору. Он не
// экспортируется: снаружи ключ доступен только через AuthorizedKey.
func (d *KeyDocument) publicKey(kid []byte) ([]byte, bool) {
	for i := range d.keys {
		if bytes.Equal(d.keys[i].KID, kid) {
			return d.keys[i].PK, true
		}
	}
	return nil, false
}

// ------------------------------------------------------------------ 0x02

// Node это запись узла, 03-WIRE.md 8.2.1.
type Node struct {
	ID  string
	PN  string
	CC  string
	H   string
	P   uint64
	PR  uint64
	NW  uint64
	SE  uint64
	SNI string
	PBK []byte
	SID string
	FP  uint64
	FL  uint64
	PT  string
	HST string
	ALP []uint64
	Hop string
	Obf string
	CG  uint64
	ZR  bool
	Ins bool
	RL  string
	SSM uint64
	MTU uint64
}

// Mirror это запись подписанного пула зеркал.
type Mirror struct {
	H      string
	SNI    string
	Pin    [][]byte
	ASN    uint64
	CC     string
	Weight uint64
	IP     []string
}

// DoHEntry это запись загрузочного резолвера DoH.
type DoHEntry struct {
	H    string
	Path string
	IP   []string
	Pin  [][]byte
}

// Resource это запись rs или geo с sha256 содержимого.
type Resource struct {
	Name     string
	URL      string
	Hash     []byte
	Interval uint64
}

// PinEntry это набор SPKI-пинов для одного хоста.
type PinEntry struct {
	H    string
	SPKI [][]byte
}

// Route это предустановка маршрутизации.
type Route struct {
	ID   string
	Name string
	RS   []string
}

// Thresholds это подписанные пороги размеров (инвариант 5).
type Thresholds struct {
	ConnBytes   uint64
	ConnPackets uint64
	RespMax     uint64
}

// Ladder это умолчания лестницы транспортов.
type Ladder struct {
	Ord []uint64
	En  []uint64
}

// Catalog, doc_type 0x02. Подписан ролью online, адресуется по содержимому.
type Catalog struct {
	Env  Envelope
	Tier uint64
	Ex   []Node
	Re   []Node
	Ro   []Route
	Cap  [4]byte
	Mir  []Mirror
	DoH  []DoHEntry
	RS   []Resource
	Geo  []Resource
	TTL  uint64
	Jit  uint64
	Thr  Thresholds
	PB   [2]uint64
	Lad  *Ladder
	Pin  []PinEntry
	HPK  []byte
	HPKV uint64
}

func (d *Catalog) DocType() uint8     { return DocCatalog }
func (d *Catalog) Envelope() Envelope { return d.Env }

// ------------------------------------------------------------------ 0x03

// Selection это авторитетный выбор, поле sel.
type Selection struct {
	Exit    string
	Relay   string
	Preset  string
	Variant uint64
	Proto   uint64
	RCC     string
	NID     uint64
}

// PolicyItem это одна запись pol: значение плюс происхождение.
type PolicyItem struct {
	Value Value
	Src   uint64
}

// Hint это подсказка интерфейса, инертный текст.
type Hint struct {
	Kind uint64
	Text string
}

// Traffic это подписанные счётчики трафика.
type Traffic struct {
	Up  uint64
	Dn  uint64
	Tot uint64
	Exp uint64
}

// Directive, doc_type 0x03. Подписана ролью online, привязана к nonce и к
// устройству, живёт один час.
type Directive struct {
	Env   Envelope
	Nonce []byte
	DTP   []byte
	St    uint64
	RC    uint64
	Cat   []byte
	CN    uint64
	Tier  uint64
	Cap   [4]byte
	Sel   *Selection
	Pol   map[uint64]PolicyItem
	Ann   string
	Sup   string
	UI    []Hint
	TTL   uint64
	ExpH  uint64
	Loc   string
	Traf  *Traffic
}

func (d *Directive) DocType() uint8     { return DocDirective }
func (d *Directive) Envelope() Envelope { return d.Env }

// ------------------------------------------------------------------ 0x04

// CatalogChunk, doc_type 0x04. Несёт срез полного КАДРА каталога, не его
// полезной нагрузки.
type CatalogChunk struct {
	Env Envelope
	CID []byte
	I   uint64
	N   uint64
	TL  uint64
	D   []byte
}

func (d *CatalogChunk) DocType() uint8     { return DocChunk }
func (d *CatalogChunk) Envelope() Envelope { return d.Env }

// ------------------------------------------------------------------ 0x05

// BootstrapBlob, doc_type 0x05. Подписан ролью root, приходит вне полосы.
type BootstrapBlob struct {
	Env  Envelope
	Org  string
	Code string
	RK   []byte
	Mir  []Mirror
	DoH  []DoHEntry
	Name string
}

func (d *BootstrapBlob) DocType() uint8     { return DocBootstrap }
func (d *BootstrapBlob) Envelope() Envelope { return d.Env }

// ------------------------------------------------------------------ 0x06

// SealedDirective, doc_type 0x06. Внешний кадр подписан ролью online, внутри
// лежит ПОЛНЫЙ кадр 0x03 под HPKE.
type SealedDirective struct {
	Env  Envelope
	DTP  []byte
	KEM  uint64
	KDF  uint64
	AEAD uint64
	Enc  []byte
	CT   []byte
	RKV  uint64
}

func (d *SealedDirective) DocType() uint8     { return DocSealed }
func (d *SealedDirective) Envelope() Envelope { return d.Env }

// ------------------------------------------------------------------ 0x08

// ReservePool, doc_type 0x08. Подписан ролью root, привязан к локатору.
type ReservePool struct {
	Env    Envelope
	Mir    []Mirror
	DoH    []DoHEntry
	Cohort uint64
}

func (d *ReservePool) DocType() uint8     { return DocReserve }
func (d *ReservePool) Envelope() Envelope { return d.Env }

// ------------------------------------------------------------------ разбор

// ParseDocument выполняет шаги P9..P12 над кадром, уже прошедшим P1..P8.
func ParseDocument(f *Frame) (Document, error) {
	root, err := decodeCBORPayload(f.Payload)
	if err != nil {
		return nil, err
	}
	env, err := parseEnvelope(&root)
	if err != nil {
		return nil, err
	}
	switch f.DocType {
	case DocKey:
		return parseKeyDocument(&root, env)
	case DocCatalog:
		return parseCatalog(&root, env)
	case DocDirective:
		return parseDirective(&root, env)
	case DocChunk:
		return parseChunk(&root, env)
	case DocBootstrap:
		return parseBlob(&root, env)
	case DocSealed:
		return parseSealed(&root, env)
	case DocReserve:
		return parseReserve(&root, env)
	}
	return nil, errf(EParseDocType, "P3", "doc_type 0x%02x has no payload table", f.DocType)
}

func envErr(format string, args ...any) *Error {
	return errf(EParseEnvelope, "P10", format, args...)
}

// parseEnvelope это шаг P10: ключи 1..5 присутствуют, типизированы и v == 1.
func parseEnvelope(m *Value) (Envelope, error) {
	var e Envelope

	v, ok := m.Get(1)
	if !ok || v.Kind != KindUint {
		return e, envErr("v (key 1) is absent or not a uint")
	}
	if v.U != 1 {
		return e, envErr("v is %d, this is a v1 verifier", v.U)
	}
	e.V = v.U

	v, ok = m.Get(2)
	if !ok || v.Kind != KindBstr {
		return e, envErr("pid (key 2) is absent or not a bstr")
	}
	if len(v.B) != PIDLen {
		return e, envErr("pid is %d bytes, must be exactly %d", len(v.B), PIDLen)
	}
	e.PID = v.B

	v, ok = m.Get(3)
	if !ok || v.Kind != KindUint {
		return e, envErr("ver (key 3) is absent or not a uint")
	}
	if v.U >= 1<<32 {
		return e, envErr("ver %d is at or above 2^32", v.U)
	}
	e.Ver = v.U

	v, ok = m.Get(4)
	if !ok || v.Kind != KindUint {
		return e, envErr("iat (key 4) is absent or not a uint")
	}
	e.IAT = v.U

	v, ok = m.Get(5)
	if !ok || v.Kind != KindUint {
		return e, envErr("exp (key 5) is absent or not a uint")
	}
	e.Exp = v.U

	return e, nil
}

// parsePadding это шаг P11 для ключа 9: дополнение обязано быть нулевым, иначе
// подписант мог бы использовать его как скрытый канал.
func parsePadding(m *Value, e *Envelope) error {
	pd, ok, err := optBstr(m, 9, 0, MaxBstrBytes, "envelope")
	if err != nil {
		return err
	}
	if !ok {
		return nil
	}
	for i := range pd {
		if pd[i] != 0 {
			return fieldErr("envelope: pd carries a non-zero byte at offset %d", i)
		}
	}
	e.Pad = pd
	return nil
}

// envelopeKeys это ключи конверта, известные в каждом документе. 6, 7 и 8
// зарезервированы в критическом диапазоне и запрещены в v1, поэтому их здесь
// нет и checkCriticalKeys их отвергнет.
func envelopeKeys(extra ...uint64) keySet {
	ks := keySet{1: true, 2: true, 3: true, 4: true, 5: true, 9: true}
	for _, k := range extra {
		ks[k] = true
	}
	return ks
}

// ------------------------------------------------------------ 0x01 разбор

func parseKeyDocument(m *Value, env Envelope) (*KeyDocument, error) {
	// Ключ 14 зарезервирован навсегда (коррекция 6 раздела 16) и в набор не входит.
	if err := checkCriticalKeys(m, envelopeKeys(10, 11, 12, 13, 15, 16), "key document"); err != nil {
		return nil, err
	}
	if err := parsePadding(m, &env); err != nil {
		return nil, err
	}
	d := &KeyDocument{Env: env, Roles: map[uint64]RoleEntry{}}

	items, err := reqArray(m, 10, 1, 16, "key document keys")
	if err != nil {
		return nil, err
	}
	for i := range items {
		if items[i].Kind != KindMap {
			return nil, fieldErr("key entry %d is %s, expected map", i, items[i].Kind)
		}
		ke := &items[i]
		if err := checkCriticalKeys(ke, keySet{1: true, 2: true, 3: true}, "key entry"); err != nil {
			return nil, err
		}
		kid, err := reqBstr(ke, 1, KeyIDTruncLen, KeyIDTruncLen, "key entry")
		if err != nil {
			return nil, err
		}
		alg, err := reqUint(ke, 2, 1, 1, "key entry alg")
		if err != nil {
			return nil, err
		}
		pk, err := reqBstr(ke, 3, 32, 32, "key entry pk")
		if err != nil {
			return nil, err
		}
		// kid обязан равняться sha256(pk)[0..12]. Нести его непроверенным
		// значило бы подарить атакующему бесплатный псевдоним ключа.
		sum := sha256.Sum256(pk)
		if !bytes.Equal(kid, sum[:KeyIDTruncLen]) {
			return nil, fieldErr("key entry %d: kid %x is not sha256(pk)[0..12] %x", i, kid, sum[:KeyIDTruncLen])
		}
		// Шаг P12: единственное место, где материал ключа входит в доверенное
		// множество, поэтому здесь применяются все три пункта раздела 2.1.
		if err := CheckPublicKey(pk); err != nil {
			return nil, errf(EParseField, "P12", "key entry %d: %v", i, err)
		}
		d.keys = append(d.keys, keyEntry{KID: kid, Alg: alg, PK: pk})
	}

	rolesV, err := reqMap(m, 11, "key document roles")
	if err != nil {
		return nil, err
	}
	if len(rolesV.Map) < 1 || len(rolesV.Map) > 3 {
		return nil, fieldErr("roles has %d pairs, cap is 1..3", len(rolesV.Map))
	}
	for i := range rolesV.Map {
		role := rolesV.Map[i].Key
		if !roleValues[role] {
			return nil, fieldErr("roles: role %d is outside the v1 vocabulary", role)
		}
		rv := &rolesV.Map[i].Val
		if rv.Kind != KindMap {
			return nil, fieldErr("roles[%d] is %s, expected map", role, rv.Kind)
		}
		if err := checkCriticalKeys(rv, keySet{1: true, 2: true}, "role entry"); err != nil {
			return nil, err
		}
		ksItems, err := reqArray(rv, 1, 1, 16, "role ks")
		if err != nil {
			return nil, err
		}
		ks, err := bstrArray(ksItems, KeyIDTruncLen, "role ks")
		if err != nil {
			return nil, err
		}
		thr, err := reqUint(rv, 2, 1, 16, "role thr")
		if err != nil {
			return nil, err
		}
		if thr > uint64(len(ks)) {
			return nil, fieldErr("roles[%d]: thr %d exceeds len(ks) %d", role, thr, len(ks))
		}
		d.Roles[role] = RoleEntry{KS: ks, Thr: thr}
	}
	if _, ok := d.Roles[RoleRoot]; !ok {
		return nil, fieldErr("roles: role 1 (root) MUST be present in every key document")
	}

	// Роль живёт только в roles. Каждый kid из ks обязан быть в keys, и каждая
	// запись keys обязана быть названа хотя бы одной ролью: ключ без роли это
	// то, что 01-DECISION.md 5.1.3 запрещает возвращать любым путём.
	referenced := make(map[string]bool, len(d.keys))
	for role, re := range d.Roles {
		for _, kid := range re.KS {
			if _, ok := d.publicKey(kid); !ok {
				return nil, fieldErr("roles[%d]: kid %x is not present in keys", role, kid)
			}
			referenced[string(kid)] = true
		}
	}
	for i := range d.keys {
		if !referenced[string(d.keys[i].KID)] {
			return nil, fieldErr("keys: entry %d (kid %x) is referenced by no role", i, d.keys[i].KID)
		}
	}

	if rev, ok, err := optMap(m, 12, "key document rev"); err != nil {
		return nil, err
	} else if ok {
		if err := checkCriticalKeys(rev, keySet{1: true, 2: true}, "rev"); err != nil {
			return nil, err
		}
		if items, ok, err := optArray(rev, 1, 0, 64, "rev kids"); err != nil {
			return nil, err
		} else if ok {
			if d.Rev.KIDs, err = bstrArray(items, KeyIDTruncLen, "rev kids"); err != nil {
				return nil, err
			}
		}
		if items, ok, err := optArray(rev, 2, 0, 256, "rev nodes"); err != nil {
			return nil, err
		} else if ok {
			for i := range items {
				if items[i].Kind != KindTstr {
					return nil, fieldErr("rev nodes: entry %d is %s, expected tstr", i, items[i].Kind)
				}
				if !validNodeID(items[i].S) {
					return nil, fieldErr("rev nodes: entry %d is not a valid node id", i)
				}
				d.Rev.Nodes = append(d.Rev.Nodes, items[i].S)
			}
		}
	}

	if tiers, ok, err := optMap(m, 13, "key document tiers"); err != nil {
		return nil, err
	} else if ok {
		if len(tiers.Map) > 16 {
			return nil, fieldErr("tiers has %d pairs, cap is 16", len(tiers.Map))
		}
		d.Tiers = make(map[uint64][]byte, len(tiers.Map))
		for i := range tiers.Map {
			// Идентификатор тарифа здесь это КЛЮЧ карты CBOR, поэтому он
			// подчиняется правилу 3.3: ключ 0 и ключи от 1024 отвергаются
			// декодером ещё до этого места. Диапазон 1..1023 назван в
			// 03-WIRE.md 8.1 явно, чтобы панель не могла подписать документ,
			// который ни один соответствующий верификатор не прочитает.
			if k := tiers.Map[i].Key; k < TierMin || k > TierMax {
				return nil, fieldErr("tiers key %d is outside the tier range %d..%d", k, TierMin, TierMax)
			}
			tv := &tiers.Map[i].Val
			if tv.Kind != KindBstr || len(tv.B) != 32 {
				return nil, fieldErr("tiers[%d] must be a 32 byte chash", tiers.Map[i].Key)
			}
			d.Tiers[tiers.Map[i].Key] = tv.B
		}
	}

	if items, ok, err := optArray(m, 15, 0, 16, "key document dep"); err != nil {
		return nil, err
	} else if ok {
		for i := range items {
			if items[i].Kind != KindMap {
				return nil, fieldErr("dep entry %d is %s, expected map", i, items[i].Kind)
			}
			de := &items[i]
			if err := checkCriticalKeys(de, keySet{1: true, 2: true}, "dep entry"); err != nil {
				return nil, err
			}
			s, err := reqTstr(de, 1, 1, 48, "dep s")
			if err != nil {
				return nil, err
			}
			sun, err := reqUint(de, 2, 0, MaxUint, "dep sun")
			if err != nil {
				return nil, err
			}
			// Минимальный срок уведомления 180 дней.
			if sun < env.IAT+15552000 {
				return nil, fieldErr("dep entry %d: sunset is less than iat + 15552000", i)
			}
			d.Dep = append(d.Dep, Deprecation{Surface: s, Sunset: sun})
		}
	}

	if ttlk, ok, err := optUint(m, 16, 300, 86400, "key document ttlk"); err != nil {
		return nil, err
	} else if ok {
		d.TTLK = ttlk
	}

	return d, nil
}

// ------------------------------------------------------------ 0x02 разбор

func parseNode(v *Value, what string) (Node, error) {
	var n Node
	if v.Kind != KindMap {
		return n, fieldErr("%s: entry is %s, expected map", what, v.Kind)
	}
	known := keySet{}
	for k := uint64(1); k <= 24; k++ {
		known[k] = true
	}
	if err := checkCriticalKeys(v, known, what); err != nil {
		return n, err
	}

	var err error
	if n.ID, err = reqTstr(v, 1, 1, 24, what+" id"); err != nil {
		return n, err
	}
	if !validNodeID(n.ID) {
		return n, fieldErr("%s: id %q is outside the charset [0-9A-Za-z_-]", what, n.ID)
	}
	if n.PN, err = reqTstr(v, 2, 0, 64, what+" pn"); err != nil {
		return n, err
	}
	if n.CC, err = reqTstr(v, 3, 2, 2, what+" cc"); err != nil {
		return n, err
	}
	if !validCountry(n.CC) {
		return n, fieldErr("%s: cc %q is not an uppercase ISO 3166-1 alpha-2 code", what, n.CC)
	}
	if n.H, err = reqTstr(v, 4, 1, 64, what+" h"); err != nil {
		return n, err
	}
	if !validHostOrIP(n.H) {
		return n, fieldErr("%s: h %q is not a valid hostname or IP literal", what, n.H)
	}
	if n.P, err = reqUint(v, 5, 1, 65535, what+" p"); err != nil {
		return n, err
	}
	if n.PR, _, err = enumUint(v, 6, prValues, true, what, "pr"); err != nil {
		return n, err
	}
	if n.NW, _, err = enumUint(v, 7, nwValues, true, what, "nw"); err != nil {
		return n, err
	}
	if n.SE, _, err = enumUint(v, 8, seValues, true, what, "se"); err != nil {
		return n, err
	}
	if s, ok, e := optTstr(v, 9, 1, 64, what+" sni"); e != nil {
		return n, e
	} else if ok {
		if !validHostname(s) {
			return n, fieldErr("%s: sni %q is not a valid hostname", what, s)
		}
		n.SNI = s
	}
	if b, ok, e := optBstr(v, 10, 32, 32, what+" pbk"); e != nil {
		return n, e
	} else if ok {
		n.PBK = b
	}
	if s, ok, e := optTstr(v, 11, 1, 16, what+" sid"); e != nil {
		return n, e
	} else if ok {
		if !validHexString(s) {
			return n, fieldErr("%s: sid %q is not hex", what, s)
		}
		n.SID = s
	}
	if n.FP, _, err = enumUint(v, 12, fpValues, false, what, "fp"); err != nil {
		return n, err
	}
	// fl = 0 означает "отсутствует; рендерер обязан опустить ключ целиком",
	// поэтому сам факт наличия ключа со значением 0 это ошибка.
	if fv, ok, e := getKind(v, 13, KindUint, what); e != nil {
		return n, e
	} else if ok {
		if fv.U != 1 {
			return n, fieldErr("%s: fl is %d; value 0 means the key MUST be omitted entirely", what, fv.U)
		}
		n.FL = fv.U
	}
	if s, ok, e := optTstr(v, 14, 0, 96, what+" pt"); e != nil {
		return n, e
	} else if ok {
		n.PT = s
	}
	if s, ok, e := optTstr(v, 15, 1, 64, what+" hst"); e != nil {
		return n, e
	} else if ok {
		if !validHostname(s) {
			return n, fieldErr("%s: hst %q is not a valid hostname", what, s)
		}
		n.HST = s
	}
	if items, ok, e := optArray(v, 16, 0, 3, what+" alp"); e != nil {
		return n, e
	} else if ok {
		for i := range items {
			if items[i].Kind != KindUint || !alpValues[items[i].U] {
				return n, fieldErr("%s: alp entry %d is outside the closed vocabulary", what, i)
			}
			n.ALP = append(n.ALP, items[i].U)
		}
	}
	if s, ok, e := optTstr(v, 17, 0, 32, what+" hop"); e != nil {
		return n, e
	} else if ok {
		n.Hop = s
	}
	if s, ok, e := optTstr(v, 18, 0, 32, what+" obf"); e != nil {
		return n, e
	} else if ok {
		n.Obf = s
	}
	if n.CG, _, err = enumUint(v, 19, cgValues, false, what, "cg"); err != nil {
		return n, err
	}
	if b, ok, e := optBool(v, 20, what+" zr"); e != nil {
		return n, e
	} else if ok {
		n.ZR = b
	}
	if b, ok, e := optBool(v, 21, what+" ins"); e != nil {
		return n, e
	} else if ok {
		n.Ins = b
	}
	if s, ok, e := optTstr(v, 22, 1, 24, what+" rl"); e != nil {
		return n, e
	} else if ok {
		if !validNodeID(s) {
			return n, fieldErr("%s: rl %q is not a valid node id", what, s)
		}
		n.RL = s
	}
	if n.SSM, _, err = enumUint(v, 23, ssmValues, false, what, "ssm"); err != nil {
		return n, err
	}
	if u, ok, e := optUint(v, 24, 576, 1500, what+" mtu"); e != nil {
		return n, e
	} else if ok {
		n.MTU = u
	}
	return n, nil
}

func parseMirror(v *Value, what string) (Mirror, error) {
	var mr Mirror
	if v.Kind != KindMap {
		return mr, fieldErr("%s: entry is %s, expected map", what, v.Kind)
	}
	if err := checkCriticalKeys(v, keySet{1: true, 2: true, 3: true, 4: true, 5: true, 6: true, 7: true}, what); err != nil {
		return mr, err
	}
	var err error
	if mr.H, err = reqTstr(v, 1, 1, 64, what+" h"); err != nil {
		return mr, err
	}
	if !validHostname(mr.H) {
		return mr, fieldErr("%s: h %q is not a valid hostname", what, mr.H)
	}
	if mr.SNI, err = reqTstr(v, 2, 1, 64, what+" sni"); err != nil {
		return mr, err
	}
	if !validHostname(mr.SNI) {
		return mr, fieldErr("%s: sni %q is not a valid hostname", what, mr.SNI)
	}
	pins, err := reqArray(v, 3, 1, 4, what+" pin")
	if err != nil {
		return mr, err
	}
	if mr.Pin, err = bstrArray(pins, 32, what+" pin"); err != nil {
		return mr, err
	}
	if mr.ASN, err = reqUint(v, 4, 0, 1<<32-1, what+" asn"); err != nil {
		return mr, err
	}
	if mr.CC, err = reqTstr(v, 5, 2, 2, what+" cc"); err != nil {
		return mr, err
	}
	if !validCountry(mr.CC) {
		return mr, fieldErr("%s: cc %q is not an uppercase ISO 3166-1 alpha-2 code", what, mr.CC)
	}
	if w, ok, e := optUint(v, 6, 1, 100, what+" w"); e != nil {
		return mr, e
	} else if ok {
		mr.Weight = w
	}
	if items, ok, e := optArray(v, 7, 0, 4, what+" ip"); e != nil {
		return mr, e
	} else if ok {
		for i := range items {
			if items[i].Kind != KindTstr || !validIPLiteral(items[i].S) {
				return mr, fieldErr("%s: ip entry %d is not a canonical IP literal", what, i)
			}
			mr.IP = append(mr.IP, items[i].S)
		}
	}
	return mr, nil
}

func parseDoH(v *Value, what string) (DoHEntry, error) {
	var d DoHEntry
	if v.Kind != KindMap {
		return d, fieldErr("%s: entry is %s, expected map", what, v.Kind)
	}
	if err := checkCriticalKeys(v, keySet{1: true, 2: true, 3: true, 4: true}, what); err != nil {
		return d, err
	}
	var err error
	if d.H, err = reqTstr(v, 1, 1, 64, what+" h"); err != nil {
		return d, err
	}
	if !validHostname(d.H) {
		return d, fieldErr("%s: h %q is not a valid hostname", what, d.H)
	}
	if d.Path, err = reqTstr(v, 2, 1, 64, what+" p"); err != nil {
		return d, err
	}
	if !validPathOnly(d.Path, 64) {
		return d, fieldErr("%s: p %q violates the path-only rules of 14.2", what, d.Path)
	}
	ips, err := reqArray(v, 3, 1, 4, what+" ip")
	if err != nil {
		return d, err
	}
	for i := range ips {
		if ips[i].Kind != KindTstr || !validIPLiteral(ips[i].S) {
			return d, fieldErr("%s: ip entry %d is not a canonical IP literal", what, i)
		}
		d.IP = append(d.IP, ips[i].S)
	}
	pins, err := reqArray(v, 4, 1, 4, what+" pin")
	if err != nil {
		return d, err
	}
	if d.Pin, err = bstrArray(pins, 32, what+" pin"); err != nil {
		return d, err
	}
	return d, nil
}

func parseResource(v *Value, what string) (Resource, error) {
	var r Resource
	if v.Kind != KindMap {
		return r, fieldErr("%s: entry is %s, expected map", what, v.Kind)
	}
	if err := checkCriticalKeys(v, keySet{1: true, 2: true, 3: true, 4: true}, what); err != nil {
		return r, err
	}
	var err error
	if r.Name, err = reqTstr(v, 1, 1, 48, what+" n"); err != nil {
		return r, err
	}
	if r.URL, err = reqTstr(v, 2, 1, 128, what+" u"); err != nil {
		return r, err
	}
	if !validPathOnly(r.URL, 128) {
		return r, fieldErr("%s: u %q violates the path-only rules of 14.2", what, r.URL)
	}
	if r.Hash, err = reqBstr(v, 3, 32, 32, what+" h"); err != nil {
		return r, err
	}
	if iv, ok, e := optUint(v, 4, 3600, 604800, what+" iv"); e != nil {
		return r, e
	} else if ok {
		r.Interval = iv
	}
	return r, nil
}

func parseCatalog(m *Value, env Envelope) (*Catalog, error) {
	known := envelopeKeys()
	for k := uint64(10); k <= 26; k++ {
		known[k] = true
	}
	if err := checkCriticalKeys(m, known, "catalog"); err != nil {
		return nil, err
	}
	if err := parsePadding(m, &env); err != nil {
		return nil, err
	}
	d := &Catalog{Env: env}

	var err error
	if d.Tier, err = reqUint(m, 10, TierMin, TierMax, "catalog tier"); err != nil {
		return nil, err
	}
	exItems, err := reqArray(m, 11, 1, 512, "catalog ex")
	if err != nil {
		return nil, err
	}
	for i := range exItems {
		n, err := parseNode(&exItems[i], "catalog ex node")
		if err != nil {
			return nil, err
		}
		d.Ex = append(d.Ex, n)
	}
	if items, ok, e := optArray(m, 12, 0, 64, "catalog re"); e != nil {
		return nil, e
	} else if ok {
		for i := range items {
			n, err := parseNode(&items[i], "catalog re node")
			if err != nil {
				return nil, err
			}
			d.Re = append(d.Re, n)
		}
	}
	if items, ok, e := optArray(m, 13, 0, 32, "catalog ro"); e != nil {
		return nil, e
	} else if ok {
		for i := range items {
			rv := &items[i]
			if rv.Kind != KindMap {
				return nil, fieldErr("catalog ro entry %d is %s, expected map", i, rv.Kind)
			}
			if err := checkCriticalKeys(rv, keySet{1: true, 2: true, 3: true}, "route entry"); err != nil {
				return nil, err
			}
			var r Route
			if r.ID, err = reqTstr(rv, 1, 1, 32, "route id"); err != nil {
				return nil, err
			}
			if !validRouteID(r.ID) {
				return nil, fieldErr("route id %q is outside the charset [a-z0-9-]", r.ID)
			}
			if r.Name, err = reqTstr(rv, 2, 0, 40, "route nm"); err != nil {
				return nil, err
			}
			rs, err := reqArray(rv, 3, 0, 32, "route rs")
			if err != nil {
				return nil, err
			}
			for j := range rs {
				if rs[j].Kind != KindTstr || len(rs[j].S) > 48 {
					return nil, fieldErr("route rs entry %d is not a resource name", j)
				}
				r.RS = append(r.RS, rs[j].S)
			}
			d.Ro = append(d.Ro, r)
		}
	}
	capb, err := reqBstr(m, 14, 4, 4, "catalog cap")
	if err != nil {
		return nil, err
	}
	copy(d.Cap[:], capb)

	if items, ok, e := optArray(m, 15, 0, 32, "catalog mir"); e != nil {
		return nil, e
	} else if ok {
		for i := range items {
			mr, err := parseMirror(&items[i], "catalog mir")
			if err != nil {
				return nil, err
			}
			d.Mir = append(d.Mir, mr)
		}
	}
	if items, ok, e := optArray(m, 16, 0, 8, "catalog doh"); e != nil {
		return nil, e
	} else if ok {
		for i := range items {
			dh, err := parseDoH(&items[i], "catalog doh")
			if err != nil {
				return nil, err
			}
			d.DoH = append(d.DoH, dh)
		}
	}
	if items, ok, e := optArray(m, 17, 0, 32, "catalog rs"); e != nil {
		return nil, e
	} else if ok {
		for i := range items {
			r, err := parseResource(&items[i], "catalog rs")
			if err != nil {
				return nil, err
			}
			d.RS = append(d.RS, r)
		}
	}
	if items, ok, e := optArray(m, 18, 0, 8, "catalog geo"); e != nil {
		return nil, e
	} else if ok {
		for i := range items {
			r, err := parseResource(&items[i], "catalog geo")
			if err != nil {
				return nil, err
			}
			d.Geo = append(d.Geo, r)
		}
	}
	if d.TTL, err = reqUint(m, 19, 300, 86400, "catalog ttl"); err != nil {
		return nil, err
	}
	if d.Jit, err = reqUint(m, 20, 0, 50, "catalog jit"); err != nil {
		return nil, err
	}
	thr, err := reqMap(m, 21, "catalog thr")
	if err != nil {
		return nil, err
	}
	if err := checkCriticalKeys(thr, keySet{1: true, 2: true, 3: true}, "catalog thr"); err != nil {
		return nil, err
	}
	d.Thr = Thresholds{ConnBytes: 8192, ConnPackets: 22, RespMax: 4096}
	if u, ok, e := optUint(thr, 1, 0, MaxUint, "thr conn_bytes"); e != nil {
		return nil, e
	} else if ok {
		d.Thr.ConnBytes = u
	}
	if u, ok, e := optUint(thr, 2, 0, MaxUint, "thr conn_packets"); e != nil {
		return nil, e
	} else if ok {
		d.Thr.ConnPackets = u
	}
	if u, ok, e := optUint(thr, 3, 0, MaxUint, "thr resp_max"); e != nil {
		return nil, e
	} else if ok {
		d.Thr.RespMax = u
	}

	pb, err := reqArray(m, 22, 2, 2, "catalog pb")
	if err != nil {
		return nil, err
	}
	for i := range pb {
		if pb[i].Kind != KindUint || pb[i].U > 15 {
			return nil, fieldErr("catalog pb entry %d is outside 0..15", i)
		}
		d.PB[i] = pb[i].U
	}
	if d.PB[0] > d.PB[1] {
		return nil, fieldErr("catalog pb: pb[0] %d exceeds pb[1] %d", d.PB[0], d.PB[1])
	}

	if lad, ok, e := optMap(m, 23, "catalog lad"); e != nil {
		return nil, e
	} else if ok {
		if err := checkCriticalKeys(lad, keySet{1: true, 2: true}, "catalog lad"); err != nil {
			return nil, err
		}
		ord, err := reqArray(lad, 1, 1, 7, "lad ord")
		if err != nil {
			return nil, err
		}
		l := &Ladder{}
		seen := map[uint64]bool{}
		for i := range ord {
			if ord[i].Kind != KindUint || !rungValues[ord[i].U] {
				return nil, fieldErr("lad ord entry %d is not a rung", i)
			}
			if seen[ord[i].U] {
				return nil, fieldErr("lad ord carries rung %d twice", ord[i].U)
			}
			seen[ord[i].U] = true
			l.Ord = append(l.Ord, ord[i].U)
		}
		if en, ok, e := optArray(lad, 2, 0, 7, "lad en"); e != nil {
			return nil, e
		} else if ok {
			enSeen := map[uint64]bool{}
			for i := range en {
				if en[i].Kind != KindUint || !rungValues[en[i].U] {
					return nil, fieldErr("lad en entry %d is not a rung", i)
				}
				if !seen[en[i].U] {
					return nil, fieldErr("lad en rung %d is not in ord", en[i].U)
				}
				enSeen[en[i].U] = true
				l.En = append(l.En, en[i].U)
			}
			// R0 и R6 никогда не отключаются.
			if !enSeen[0] || !enSeen[6] {
				return nil, fieldErr("lad en must contain rung 0 and rung 6")
			}
		}
		d.Lad = l
	}

	if items, ok, e := optArray(m, 24, 0, 32, "catalog pin"); e != nil {
		return nil, e
	} else if ok {
		for i := range items {
			pv := &items[i]
			if pv.Kind != KindMap {
				return nil, fieldErr("catalog pin entry %d is %s, expected map", i, pv.Kind)
			}
			if err := checkCriticalKeys(pv, keySet{1: true, 2: true}, "pin entry"); err != nil {
				return nil, err
			}
			var pe PinEntry
			if pe.H, err = reqTstr(pv, 1, 1, 64, "pin h"); err != nil {
				return nil, err
			}
			if !validHostname(pe.H) {
				return nil, fieldErr("pin h %q is not a valid hostname", pe.H)
			}
			sp, err := reqArray(pv, 2, 1, 4, "pin spki")
			if err != nil {
				return nil, err
			}
			if pe.SPKI, err = bstrArray(sp, 32, "pin spki"); err != nil {
				return nil, err
			}
			d.Pin = append(d.Pin, pe)
		}
	}

	hpk, hasHPK, err := optBstr(m, 25, 65, 65, "catalog hpk")
	if err != nil {
		return nil, err
	}
	hpkv, hasHPKV, err := optUint(m, 26, 0, 65535, "catalog hpkv")
	if err != nil {
		return nil, err
	}
	if hasHPK != hasHPKV {
		return nil, fieldErr("catalog: hpkv MUST be present exactly when hpk is present")
	}
	d.HPK, d.HPKV = hpk, hpkv

	return d, nil
}

// ------------------------------------------------------------ 0x03 разбор

func parseDirective(m *Value, env Envelope) (*Directive, error) {
	known := envelopeKeys()
	for k := uint64(10); k <= 26; k++ {
		known[k] = true
	}
	if err := checkCriticalKeys(m, known, "directive"); err != nil {
		return nil, err
	}
	if err := parsePadding(m, &env); err != nil {
		return nil, err
	}
	d := &Directive{Env: env}

	var err error
	if d.Nonce, err = reqBstr(m, 10, 16, 16, "directive nonce"); err != nil {
		return nil, err
	}
	if d.DTP, err = reqBstr(m, 11, 16, 16, "directive dtp"); err != nil {
		return nil, err
	}
	if d.St, _, err = enumUint(m, 12, stValues, true, "directive", "st"); err != nil {
		return nil, err
	}
	// rc это машинный код с диапазонной структурой, а не закрытый словарь:
	// незнакомое значение обязано отрисоваться общим текстом своего st и НЕ
	// является ошибкой разбора.
	if rc, ok, e := optUint(m, 13, 0, MaxUint, "directive rc"); e != nil {
		return nil, e
	} else if ok {
		d.RC = rc
	}
	if d.Cat, err = reqBstr(m, 14, 32, 32, "directive cat"); err != nil {
		return nil, err
	}
	if d.CN, err = reqUint(m, 15, 1, 64, "directive cn"); err != nil {
		return nil, err
	}
	if d.Tier, err = reqUint(m, 16, TierMin, TierMax, "directive tier"); err != nil {
		return nil, err
	}
	capb, err := reqBstr(m, 17, 4, 4, "directive cap")
	if err != nil {
		return nil, err
	}
	copy(d.Cap[:], capb)

	// Присутствие полей sel нужно трём предикатам согласия sel и pol
	// (02-SPEC.md 7.4): пустая строка и ноль это законные ПРИСУТСТВУЮЩИЕ
	// значения, поэтому нулевое значение поля присутствием считать нельзя.
	var selPresetSet, selProtoSet, selRCCSet bool
	if sv, ok, e := optMap(m, 18, "directive sel"); e != nil {
		return nil, e
	} else if ok {
		if err := checkCriticalKeys(sv, keySet{1: true, 2: true, 3: true, 4: true, 5: true, 6: true, 7: true}, "sel"); err != nil {
			return nil, err
		}
		sel := &Selection{}
		// Нижняя граница длины здесь 0, а не 1: 03-WIRE.md 8.3 задаёт только
		// верхний предел, и пустая строка отвергалась бы правилом, которого в
		// таблице полей нет.
		if s, ok, e := optTstr(sv, 1, 0, 24, "sel exit"); e != nil {
			return nil, e
		} else if ok {
			sel.Exit = s
		}
		if s, ok, e := optTstr(sv, 2, 0, 24, "sel relay"); e != nil {
			return nil, e
		} else if ok {
			sel.Relay = s
		}
		if s, ok, e := optTstr(sv, 3, 0, 32, "sel preset"); e != nil {
			return nil, e
		} else if ok {
			sel.Preset = s
			selPresetSet = true
		}
		if u, ok, e := optUint(sv, 4, 0, 255, "sel variant"); e != nil {
			return nil, e
		} else if ok {
			sel.Variant = u
		}
		if u, ok, e := getKind(sv, 5, KindUint, "sel proto"); e != nil {
			return nil, e
		} else if ok {
			if u.U != 0 && !prValues[u.U] {
				return nil, fieldErr("sel: proto %d is outside the closed vocabulary", u.U)
			}
			sel.Proto = u.U
			selProtoSet = true
		}
		if s, ok, e := optTstr(sv, 6, 2, 2, "sel rcc"); e != nil {
			return nil, e
		} else if ok {
			if s != NoRelaySentinel && !validCountry(s) {
				return nil, fieldErr("sel: rcc %q is neither an alpha-2 code nor the no-relay sentinel", s)
			}
			sel.RCC = s
			selRCCSet = true
		}
		if u, ok, e := optUint(sv, 7, 0, 1<<63-1, "sel nid"); e != nil {
			return nil, e
		} else if ok {
			sel.NID = u
		}
		d.Sel = sel
	}

	if pv, ok, e := optMap(m, 19, "directive pol"); e != nil {
		return nil, e
	} else if ok {
		d.Pol = make(map[uint64]PolicyItem, len(pv.Map))
		for i := range pv.Map {
			k := pv.Map[i].Key
			if k <= CriticalKeyMax && !polKeys[k] {
				return nil, fieldErr("pol: unrecognized critical key %d", k)
			}
			if k > CriticalKeyMax {
				continue // некритическое расширение pol игнорируется
			}
			item := &pv.Map[i].Val
			if item.Kind != KindArray || len(item.Array) != 2 {
				return nil, fieldErr("pol[%d] must be a 2 element array [value, src]", k)
			}
			src := item.Array[1]
			if src.Kind != KindUint || !srcValues[src.U] {
				return nil, fieldErr("pol[%d]: src is outside the closed vocabulary", k)
			}
			if err := checkPolicyValue(k, &item.Array[0]); err != nil {
				return nil, err
			}
			d.Pol[k] = PolicyItem{Value: item.Array[0], Src: src.U}
		}
	}

	// 02-SPEC.md 7.4: три самодостаточных предиката согласия sel и pol.
	// Каждый решается по пришедшим байтам целиком, поэтому проверяется на
	// разборе и даёт E_PARSE_FIELD. Два предиката, зависящих от каталога,
	// здесь НЕ проверяются: на разборе связанного каталога может ещё не быть.
	if d.Sel != nil {
		if err := checkSelPolAgreement(d, selPresetSet, selProtoSet, selRCCSet); err != nil {
			return nil, err
		}
	}

	if s, ok, e := optTstr(m, 20, 0, 80, "directive ann"); e != nil {
		return nil, e
	} else if ok {
		d.Ann = s
	}
	if s, ok, e := optTstr(m, 21, 0, 80, "directive sup"); e != nil {
		return nil, e
	} else if ok {
		d.Sup = s
	}
	if items, ok, e := optArray(m, 22, 0, 4, "directive ui"); e != nil {
		return nil, e
	} else if ok {
		for i := range items {
			hv := &items[i]
			if hv.Kind != KindMap {
				return nil, fieldErr("ui entry %d is %s, expected map", i, hv.Kind)
			}
			if err := checkCriticalKeys(hv, keySet{1: true, 2: true}, "ui entry"); err != nil {
				return nil, err
			}
			k, _, err := enumUint(hv, 1, uiKValues, true, "ui entry", "k")
			if err != nil {
				return nil, err
			}
			t, err := reqTstr(hv, 2, 0, 80, "ui entry t")
			if err != nil {
				return nil, err
			}
			d.UI = append(d.UI, Hint{Kind: k, Text: t})
		}
	}
	if d.TTL, err = reqUint(m, 23, 300, 86400, "directive ttl"); err != nil {
		return nil, err
	}
	if u, ok, e := optUint(m, 24, 0, 2592000, "directive exph"); e != nil {
		return nil, e
	} else if ok {
		d.ExpH = u
	}
	if d.Loc, err = reqTstr(m, 25, 24, 24, "directive loc"); err != nil {
		return nil, err
	}
	// loc это base32 Crockford над 120 битами (03-WIRE.md раздел 4). Он уходит
	// в путь URL и в заголовок, поэтому набор символов проверяется здесь, на
	// границе разбора, а не там, где строку уже склеили.
	if !validCrockford(d.Loc) {
		return nil, fieldErr("directive loc is not 24 base32 Crockford characters")
	}
	if tv, ok, e := optMap(m, 26, "directive traf"); e != nil {
		return nil, e
	} else if ok {
		if err := checkCriticalKeys(tv, keySet{1: true, 2: true, 3: true, 4: true}, "traf"); err != nil {
			return nil, err
		}
		t := &Traffic{}
		if t.Up, err = reqUint(tv, 1, 0, MaxUint, "traf up"); err != nil {
			return nil, err
		}
		if t.Dn, err = reqUint(tv, 2, 0, MaxUint, "traf dn"); err != nil {
			return nil, err
		}
		if t.Tot, err = reqUint(tv, 3, 0, MaxUint, "traf tot"); err != nil {
			return nil, err
		}
		if t.Exp, err = reqUint(tv, 4, 0, MaxUint, "traf exp"); err != nil {
			return nil, err
		}
		d.Traf = t
	}
	return d, nil
}

// checkPolicyValue проверяет тип значения настройки по таблице 03-WIRE.md 8.3.
// Содержимое строк это словарь CorePolicy и НЕ является закрытым словарём на
// уровне разбора: незнакомое значение обрабатывается клиентом, а не отвергается.
// protoWire это отображение 02-SPEC.md 7.4 из строки протокола pol[1] в
// перечисление pr раздела 5. Оно хранится ДАННЫМИ, чтобы не разъезжаться с
// таблицей спецификации, и авторитетно ровно в одну сторону: VLESS и
// VLESS-Reality оба дают 1, поэтому восстанавливать pol[1] из sel.proto
// нельзя, и ни один путь кода этого не делает.
var protoWire = map[string]uint64{
	"auto":          0,
	"VLESS-Reality": 1,
	"VLESS":         1,
	"Hysteria2":     4,
	"TUIC":          5,
	"Shadowsocks":   6,
	"AmneziaWG":     8,
}

// checkSelPolAgreement это три самодостаточных предиката 02-SPEC.md 7.4.
// pol это то, что клиент применяет к ядру, sel это то, что он ставит в
// легаси-URL; директива, у которой они расходятся, заставила бы клиент
// применить обе версии сразу.
func checkSelPolAgreement(d *Directive, presetSet, protoSet, rccSet bool) error {
	polTstr := func(key uint64) (string, bool) {
		it, ok := d.Pol[key]
		if !ok || it.Value.Kind != KindTstr {
			return "", false
		}
		return it.Value.S, true
	}

	// 1. sel.preset и pol[2] присутствуют оба и различаются.
	if presetSet {
		if p, ok := polTstr(2); ok && p != d.Sel.Preset {
			return fieldErr("sel: preset %q disagrees with pol[2] %q", d.Sel.Preset, p)
		}
	}

	// 2. sel.proto не равен PROTO_WIRE[pol[1]]. Незнакомая строка протокола
	// отображения не имеет, и предикат по ней не решается: отвергать её здесь
	// значило бы вводить словарь, которого в таблице нет.
	if protoSet {
		if p, ok := polTstr(1); ok {
			if want, known := protoWire[p]; known && want != d.Sel.Proto {
				return fieldErr("sel: proto %d is not PROTO_WIRE[%q] = %d", d.Sel.Proto, p, want)
			}
		}
	}

	// 3. sel.rcc против pol[3] по таблице 02-SPEC.md 7.4: код страны требует
	// себя же в верхнем регистре, "--" требует "--", пустое значение
	// разрешает любое законное.
	if rccSet {
		if p, ok := polTstr(3); ok && p != "" {
			want := strings.ToUpper(p)
			if p == NoRelaySentinel {
				want = NoRelaySentinel
			}
			if d.Sel.RCC != want {
				return fieldErr("sel: rcc %q disagrees with pol[3] %q", d.Sel.RCC, p)
			}
		}
	}
	return nil
}

func checkPolicyValue(key uint64, v *Value) error {
	switch key {
	case 1, 2, 3, 4, 11: // protocol, preset, relay, stack, split.mode
		if v.Kind != KindTstr {
			return fieldErr("pol[%d] must be a tstr", key)
		}
	case 5: // mtu
		if v.Kind != KindUint {
			return fieldErr("pol[%d] must be a uint", key)
		}
	case 6, 7, 8: // ipv6, fakeIp, killSwitch
		if v.Kind != KindBool {
			return fieldErr("pol[%d] must be a bool", key)
		}
	case 9, 10: // dns.nameservers, dns.fallback
		if v.Kind != KindArray {
			return fieldErr("pol[%d] must be an array of tstr", key)
		}
		for i := range v.Array {
			if v.Array[i].Kind != KindTstr {
				return fieldErr("pol[%d] entry %d is not a tstr", key, i)
			}
		}
	}
	return nil
}

// ------------------------------------------------------------ 0x04 разбор

func parseChunk(m *Value, env Envelope) (*CatalogChunk, error) {
	if err := checkCriticalKeys(m, envelopeKeys(10, 11, 12, 13, 14), "catalog chunk"); err != nil {
		return nil, err
	}
	if err := parsePadding(m, &env); err != nil {
		return nil, err
	}
	d := &CatalogChunk{Env: env}
	var err error
	if d.CID, err = reqBstr(m, 10, 10, 10, "chunk cid"); err != nil {
		return nil, err
	}
	if d.I, err = reqUint(m, 11, 0, 63, "chunk i"); err != nil {
		return nil, err
	}
	if d.N, err = reqUint(m, 12, 1, 64, "chunk n"); err != nil {
		return nil, err
	}
	if d.TL, err = reqUint(m, 13, 1, MaxPayloadLen, "chunk tl"); err != nil {
		return nil, err
	}
	if d.D, err = reqBstr(m, 14, 1, ChunkPayloadMax, "chunk d"); err != nil {
		return nil, err
	}
	if d.I >= d.N {
		return nil, fieldErr("chunk: i %d is not below n %d", d.I, d.N)
	}
	// Каждый фрагмент кроме последнего обязан нести ровно CHUNK_PAYLOAD_MAX
	// байт: иначе зеркало выбирало бы смещения пересборки.
	lo := d.I * ChunkPayloadMax
	if lo >= d.TL {
		return nil, fieldErr("chunk: index %d starts past tl %d", d.I, d.TL)
	}
	want := uint64(ChunkPayloadMax)
	if lo+want > d.TL {
		want = d.TL - lo
	}
	if uint64(len(d.D)) != want {
		return nil, fieldErr("chunk %d of %d carries %d bytes where %d is required", d.I, d.N, len(d.D), want)
	}
	wantN := (d.TL + ChunkPayloadMax - 1) / ChunkPayloadMax
	if d.N != wantN {
		return nil, fieldErr("chunk: n %d disagrees with ceil(tl/%d) = %d", d.N, ChunkPayloadMax, wantN)
	}
	return d, nil
}

// ------------------------------------------------------------ 0x05 разбор

func parseBlob(m *Value, env Envelope) (*BootstrapBlob, error) {
	if err := checkCriticalKeys(m, envelopeKeys(10, 11, 12, 13, 14, 15), "bootstrap blob"); err != nil {
		return nil, err
	}
	if err := parsePadding(m, &env); err != nil {
		return nil, err
	}
	d := &BootstrapBlob{Env: env}
	var err error
	if d.Org, err = reqTstr(m, 10, 1, 96, "blob org"); err != nil {
		return nil, err
	}
	if !validOrigin(d.Org) {
		return nil, fieldErr("blob: org %q is not an https origin without a path", d.Org)
	}
	if d.Code, err = reqTstr(m, 11, 1, 32, "blob code"); err != nil {
		return nil, err
	}
	if d.RK, err = reqBstr(m, 12, 32, 32, "blob rk"); err != nil {
		return nil, err
	}
	if err := CheckPublicKey(d.RK); err != nil {
		return nil, errf(EParseField, "P12", "blob rk: %v", err)
	}
	mirs, err := reqArray(m, 13, 1, 32, "blob mir")
	if err != nil {
		return nil, err
	}
	for i := range mirs {
		mr, err := parseMirror(&mirs[i], "blob mir")
		if err != nil {
			return nil, err
		}
		d.Mir = append(d.Mir, mr)
	}
	dohs, err := reqArray(m, 14, 1, 8, "blob doh")
	if err != nil {
		return nil, err
	}
	for i := range dohs {
		dh, err := parseDoH(&dohs[i], "blob doh")
		if err != nil {
			return nil, err
		}
		d.DoH = append(d.DoH, dh)
	}
	if s, ok, e := optTstr(m, 15, 0, 40, "blob nm"); e != nil {
		return nil, e
	} else if ok {
		d.Name = s
	}
	return d, nil
}

// ------------------------------------------------------------ 0x06 разбор

func parseSealed(m *Value, env Envelope) (*SealedDirective, error) {
	if err := checkCriticalKeys(m, envelopeKeys(10, 11, 12, 13, 14, 15, 16), "sealed directive"); err != nil {
		return nil, err
	}
	if err := parsePadding(m, &env); err != nil {
		return nil, err
	}
	d := &SealedDirective{Env: env}
	var err error
	if d.DTP, err = reqBstr(m, 10, 16, 16, "sealed dtp"); err != nil {
		return nil, err
	}
	// kem, kdf и aead типизируются здесь, но их ЗНАЧЕНИЯ проверяются на шаге 4
	// раздела 9.4 и дают E_SEAL_SUITE, а не E_PARSE_FIELD.
	if d.KEM, err = reqUint(m, 11, 0, 65535, "sealed kem"); err != nil {
		return nil, err
	}
	if d.KDF, err = reqUint(m, 12, 0, 65535, "sealed kdf"); err != nil {
		return nil, err
	}
	if d.AEAD, err = reqUint(m, 13, 0, 65535, "sealed aead"); err != nil {
		return nil, err
	}
	if d.Enc, err = reqBstr(m, 14, 65, 65, "sealed enc"); err != nil {
		return nil, err
	}
	if d.CT, err = reqBstr(m, 15, 242, 3072, "sealed ct"); err != nil {
		return nil, err
	}
	if d.RKV, err = reqUint(m, 16, 0, 65535, "sealed rkv"); err != nil {
		return nil, err
	}
	return d, nil
}

// ------------------------------------------------------------ 0x08 разбор

func parseReserve(m *Value, env Envelope) (*ReservePool, error) {
	if err := checkCriticalKeys(m, envelopeKeys(10, 11, 12), "reserve pool"); err != nil {
		return nil, err
	}
	if err := parsePadding(m, &env); err != nil {
		return nil, err
	}
	d := &ReservePool{Env: env}
	mirs, err := reqArray(m, 10, 1, 32, "reserve mir")
	if err != nil {
		return nil, err
	}
	for i := range mirs {
		mr, err := parseMirror(&mirs[i], "reserve mir")
		if err != nil {
			return nil, err
		}
		d.Mir = append(d.Mir, mr)
	}
	if items, ok, e := optArray(m, 11, 0, 8, "reserve doh"); e != nil {
		return nil, e
	} else if ok {
		for i := range items {
			dh, err := parseDoH(&items[i], "reserve doh")
			if err != nil {
				return nil, err
			}
			d.DoH = append(d.DoH, dh)
		}
	}
	if u, ok, e := optUint(m, 12, 0, 65535, "reserve coh"); e != nil {
		return nil, e
	} else if ok {
		d.Cohort = u
	}
	return d, nil
}

// ------------------------------------------------------- отзыв узлов, rev.nodes

// DropRevokedNodes удаляет из РАЗОБРАННОГО каталога записи ex и re, чей id
// назван в rev.nodes доверенного ключевого документа, и возвращает число
// удалённых записей.
//
// 03-WIRE.md 8.1 ключ 12: "A node id in rev MUST be honored against the cached
// catalog, so a seized node is dropped even while the client is running offline
// with no network at all." Это правило применения, а не шаг проверки: подпись
// каталога остаётся действительной, отзыв её не отменяет, поэтому фильтр живёт
// там, где каталог становится пригодным к использованию, и обязан отработать
// на КАЖДОЙ загрузке кеша, а не только на свежей выборке.
//
// Байты кадра не трогаются: отфильтрован разобранный вид, кадр на диске
// остаётся тем, что был проверен.
func DropRevokedNodes(cat *Catalog, anchor *KeyDocument) int {
	if cat == nil || anchor == nil || len(anchor.Rev.Nodes) == 0 {
		return 0
	}
	revoked := make(map[string]bool, len(anchor.Rev.Nodes))
	for _, id := range anchor.Rev.Nodes {
		revoked[id] = true
	}
	filter := func(in []Node) ([]Node, int) {
		out := in[:0]
		dropped := 0
		for _, n := range in {
			if revoked[n.ID] {
				dropped++
				continue
			}
			out = append(out, n)
		}
		return out, dropped
	}
	var a, b int
	cat.Ex, a = filter(cat.Ex)
	cat.Re, b = filter(cat.Re)
	return a + b
}
