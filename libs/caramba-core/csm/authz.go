package csm

import "fmt"

// Таблица авторизации doc_type -> роль -> порог, 02-SPEC.md раздел 3 и
// 03-WIRE.md 7.1. Она записана как ДАННЫЕ, а не разбросана по условиям: это
// то самое правило, которое три реализации иначе изобрели бы каждая по-своему.
//
// Ключевое свойство: набор ключей и порог читаются из РАНЕЕ доверенного
// ключевого документа, никогда из проверяемого. Единственное исключение это
// ротация корня, которой нужны ОБА набора и оба порога.

// Роли, 03-WIRE.md раздел 5.
const (
	RoleRoot      uint64 = 1
	RoleOnline    uint64 = 2
	RoleTimestamp uint64 = 3 // зарезервирована, в v1 появляться не должна
)

// KeySetSource говорит, откуда берётся авторизованный набор ключей.
type KeySetSource uint8

const (
	// FromAnchor: roles[role] ранее доверенного ключевого документа.
	FromAnchor KeySetSource = iota
	// FromLinkPin: единственный ключ, чей sha256[0..12] равен link_pin.
	// Порог фиксирован единицей, доверенного документа не нужно.
	FromLinkPin
	// FromAnchorAndSelf: ротация корня. Кадр проверяется дважды над одним и
	// тем же прообразом, против набора и порога доверенного документа И
	// против набора и порога проверяемого. Оба обязаны пройти.
	FromAnchorAndSelf
)

// AuthRule это одна строка таблицы авторизации.
type AuthRule struct {
	DocType   uint8
	Role      uint64
	KeySet    KeySetSource
	Threshold uint64 // используется только при FromLinkPin
}

// authTable это полная таблица. Строка есть для каждого значения doc_type,
// пережившего шаг P3, поэтому шаг V1 не может дать сбой и кода ошибки не несёт.
var authTable = map[uint8]AuthRule{
	DocKey:       {DocType: DocKey, Role: RoleRoot, KeySet: FromAnchorAndSelf},
	DocCatalog:   {DocType: DocCatalog, Role: RoleOnline, KeySet: FromAnchor},
	DocDirective: {DocType: DocDirective, Role: RoleOnline, KeySet: FromAnchor},
	DocChunk:     {DocType: DocChunk, Role: RoleOnline, KeySet: FromAnchor},
	DocBootstrap: {DocType: DocBootstrap, Role: RoleRoot, KeySet: FromLinkPin, Threshold: 1},
	DocSealed:    {DocType: DocSealed, Role: RoleOnline, KeySet: FromAnchor},
	DocReserve:   {DocType: DocReserve, Role: RoleRoot, KeySet: FromAnchor},
}

// AuthRuleFor возвращает строку таблицы для типа документа. Это шаг V1.
func AuthRuleFor(dt uint8) (AuthRule, bool) {
	r, ok := authTable[dt]
	return r, ok
}

// RoleName возвращает имя роли для сообщений.
func RoleName(role uint64) string {
	switch role {
	case RoleRoot:
		return "root"
	case RoleOnline:
		return "online"
	case RoleTimestamp:
		return "timestamp"
	}
	return fmt.Sprintf("role %d", role)
}

// authorizedSet это разрешённый набор ключей и порог для одной стороны
// проверки. Ключ никогда не выдаётся без своей роли: Role хранится рядом.
type authorizedSet struct {
	Role uint64
	// keys отображает усечённый идентификатор в открытый ключ.
	keys map[string][]byte
	Thr  uint64
	// origin для диагностики: "anchor" или "self" или "link_pin".
	origin string
}

func (a *authorizedSet) contains(kid []byte) bool {
	_, ok := a.keys[string(kid)]
	return ok
}

func (a *authorizedSet) publicKey(kid []byte) ([]byte, bool) {
	pk, ok := a.keys[string(kid)]
	return pk, ok
}

// authorizedSetFrom строит набор из ключевого документа для указанной роли.
// Возвращает false, когда у документа нет такой роли: это единственная форма,
// в которой достижим E_VERIFY_ROLE на шаге V3.
func authorizedSetFrom(kd *KeyDocument, role uint64, origin string) (*authorizedSet, bool) {
	re, ok := kd.Roles[role]
	if !ok {
		return nil, false
	}
	set := &authorizedSet{Role: role, keys: make(map[string][]byte, len(re.KS)), Thr: re.Thr, origin: origin}
	for _, kid := range re.KS {
		pk, ok := kd.publicKey(kid)
		if !ok {
			// Разбор уже это запретил, но набор не строится наполовину.
			return nil, false
		}
		set.keys[string(kid)] = pk
	}
	return set, true
}
