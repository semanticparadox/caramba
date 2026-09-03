package transport

import (
	"encoding/binary"
	"encoding/hex"
	"strings"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

// DocState это состояние проверки одного документа: версия, выпуск, срок,
// отпечаток подписавшего, результат проверки и разобранные поля. Инвариант 19
// требует, чтобы это было видно пользователю, поэтому оно уезжает наружу
// целиком, а не сокращённо.
type DocState struct {
	Present bool     `json:"present"`
	Ver     uint64   `json:"ver"`
	IAT     int64    `json:"iat"`
	Exp     int64    `json:"exp"`
	Expired bool     `json:"expired"`
	Signers []string `json:"signers,omitempty"`
	Role    uint64   `json:"role,omitempty"`
	Bytes   int      `json:"bytes,omitempty"`
}

// ResourceRef это одна запись rs или geo доверенного каталога, спроецированная
// для клиента: вид, имя и подписанный sha256 в hex.
//
// Проекция нужна снаружи, потому что 02-SPEC.md 7.7.1 делает изменение набора
// ресурсов сужением защиты: хеш связывает байты, но НЕ связывает того, кто
// выбрал и путь, и хеш. Клиент обязан заметить смену набора и спросить
// пользователя, и заметить он её может только сравнив то, что видит сейчас, с
// тем, что видел раньше.
type ResourceRef struct {
	// Kind это "rs" либо "geo".
	Kind string `json:"kind"`
	Name string `json:"name"`
	// Hash это подписанный sha256 в hex.
	Hash string `json:"hash"`
}

// RouteRules это список rule-set одного маршрута каталога, ro[].rs. Его смена
// это третья строка 02-SPEC.md 7.7.1.
type RouteRules struct {
	ID string   `json:"id"`
	RS []string `json:"rs"`
}

// Значения NodeRef.Kind. Разделение ex и re подписано (03-WIRE.md 8.2 ключи 3
// и 4), поэтому вид узла не выводится из формы записи, а переносится как есть.
const (
	NodeKindExit  = "exit"
	NodeKindRelay = "relay"
)

// ReasonNodeRevoked это машинная причина недоступности узла: его идентификатор
// назван в rev.nodes доверенного ключевого документа.
const ReasonNodeRevoked = "revoked"

// CapRelayChaining это бит relay_chaining, capBitNames[2] (02-SPEC.md 6.3).
// Константа существует, чтобы фасад не писал 1<<2 руками; согласованность с
// capBitNames закреплена тестом.
const CapRelayChaining uint32 = 1 << 2

// NodeRef это проекция одной записи ex или re доверенного каталога: ровно то,
// чем рисуется выбор страны и протокола, и НИЧЕГО из того, чем подключаются.
//
// Границу проводит именно этот тип. Ядро держит запись узла целиком — h, p,
// sni, pbk, sid, fp, alp, hop, obf, ssm, mtu (03-WIRE.md 8.2.1) — и собирает
// исходящее соединение само. Наружу уезжают идентификатор, ярлык, страна и
// форма протокола. Рисующей стороне материал подключения не нужен, а его
// появление там означало бы, что содержимое подписанного каталога живёт ещё и
// в неподписанном слое: подмена одного SNI в слое Dart после этого не ловится
// ничем, потому что проверять там нечего.
type NodeRef struct {
	ID string `json:"id"`
	// Name это pn, инертный ярлык оператора. Он приходит подписанным и
	// показывается как есть.
	Name string `json:"name"`
	// CC это ISO 3166-1 alpha-2 в верхнем регистре.
	CC string `json:"cc"`
	// Kind это NodeKindExit либо NodeKindRelay.
	Kind string `json:"kind"`

	// Proto, Network и Security это закрытые словари pr, nw и se
	// (03-WIRE.md 5). Рядом с числом едет имя из того же словаря: ярлык
	// обвязке нужен, а своя таблица на стороне Dart разошлась бы с этой при
	// первом же расширении словаря, и разошлась бы молча.
	Proto        uint64 `json:"proto"`
	ProtoName    string `json:"proto_name,omitempty"`
	Network      uint64 `json:"network"`
	NetworkName  string `json:"network_name,omitempty"`
	Security     uint64 `json:"security"`
	SecurityName string `json:"security_name,omitempty"`

	// Relay это rl: идентификатор записи re, через которую этот выход строит
	// цепочку (03-WIRE.md 8.2.1 ключ 22). Пусто у входов и у выходов без
	// цепочки. Это ребро, по которому обвязка связывает две проекции: страну
	// входа она обязана взять из Relays, а не угадать по стране выхода.
	Relay string `json:"relay,omitempty"`

	// Available ложно, когда идентификатор назван в rev.nodes доверенного
	// ключевого документа. Отозванный узел остаётся в списке помеченным, а не
	// исчезает из него: список, укоротившийся молча, неотличим от списка,
	// который укоротил сам оператор, и пользователь в первом случае ищет
	// пропавшую страну, а во втором ждёт её возвращения.
	Available bool `json:"available"`
	// Reason называет причину недоступности машинным словом (сейчас только
	// ReasonNodeRevoked). Пусто, когда Available истинно.
	Reason string `json:"reason,omitempty"`
}

// Имена закрытых словарей 03-WIRE.md 5. Отсутствующее значение даёт пустое
// имя, а не выдуманное: число уже уехало в Proto/Network/Security, и обвязка
// покажет его, если словарь на этой сборке ещё не знает такого варианта.
var (
	protoNames = map[uint64]string{
		1: "vless", 2: "vmess", 3: "trojan", 4: "hysteria2",
		5: "tuic", 6: "shadowsocks", 7: "naive", 8: "wireguard",
	}
	networkNames = map[uint64]string{
		1: "tcp", 2: "ws", 3: "grpc", 4: "httpupgrade", 5: "xhttp", 6: "quic",
	}
	securityNames = map[uint64]string{0: "none", 1: "tls", 2: "reality"}
)

// Snapshot это полное состояние профиля для клиента.
type Snapshot struct {
	Enrolled bool   `json:"enrolled"`
	PID      string `json:"pid,omitempty"`
	Origin   string `json:"origin,omitempty"`
	// Operator это отображаемое имя оператора, инертный текст.
	Operator string `json:"operator,omitempty"`
	// RootFingerprint это пин корня группами по четыре, инвариант 18.
	RootFingerprint string `json:"root_fingerprint,omitempty"`
	PinnedOutOfBand bool   `json:"pinned_out_of_band"`
	RootChanged     bool   `json:"root_changed"`
	EnrolledAt      int64  `json:"enrolled_at,omitempty"`

	Key       DocState `json:"key"`
	Catalog   DocState `json:"catalog"`
	Directive DocState `json:"directive"`

	// CapOperator это битовая маска оператора, CapClient скомпилированная,
	// CapEffective их пересечение с карве-аутом по содержимому.
	CapOperator  uint32   `json:"cap_operator"`
	CapClient    uint32   `json:"cap_client"`
	CapEffective uint32   `json:"cap_effective"`
	CapBits      []string `json:"cap_bits,omitempty"`

	// Stale истинно, когда клиент работает на кешированных документах.
	// AgeSeconds и Source это инвариант 21.
	Stale      bool   `json:"stale"`
	AgeSeconds int64  `json:"age_seconds"`
	Source     string `json:"source"`

	// FleetRootAnchored ложно, когда в доверенном ключевом документе нет
	// записи tiers для тарифа директивы. Это пониженное сдерживание, и оно
	// обязано быть названо в интерфейсе, а не молча пропущено.
	FleetRootAnchored bool `json:"fleet_root_anchored"`
	// ClockTrusted ложно, пока не проверена ни одна директива.
	ClockTrusted bool `json:"clock_trusted"`
	// ClockChanged истинно, когда доверие часам снималось: стенные часы ушли
	// назад больше чем на 300 секунд относительно монотонного смещения
	// (02-SPEC.md 5.5).
	ClockChanged bool  `json:"clock_changed"`
	TimeFloor    int64 `json:"time_floor"`

	// RevokedNodesDropped это число записей ex и re, выброшенных по rev.nodes
	// доверенного ключевого документа. FleetEmpty истинно, когда после
	// фильтрации не осталось ни одного выхода: это флот, которым пользоваться
	// нельзя, и говорить об этом обязан интерфейс.
	RevokedNodesDropped int  `json:"revoked_nodes_dropped,omitempty"`
	FleetEmpty          bool `json:"fleet_empty,omitempty"`

	Status  uint64 `json:"status"`
	Reason  uint64 `json:"reason"`
	Revoked bool   `json:"revoked"`
	Locator string `json:"locator,omitempty"`
	CatID   string `json:"cat_id,omitempty"`

	Thresholds Thresholds `json:"thresholds"`
	TTL        uint64     `json:"ttl"`
	Jit        uint64     `json:"jit"`
	ExpH       uint64     `json:"exph"`

	// Selection это авторитетный выбор из директивы.
	Exit    string `json:"exit,omitempty"`
	Relay   string `json:"relay,omitempty"`
	Preset  string `json:"preset,omitempty"`
	Variant uint64 `json:"variant,omitempty"`

	// Resources и Routes это сырьё для карточки 02-SPEC.md 7.7.1. Оба поля
	// это ПРОЕКЦИЯ доверенного каталога, а не решение: сравнивает наборы и
	// поднимает карточку клиент.
	Resources []ResourceRef `json:"resources,omitempty"`
	Routes    []RouteRules  `json:"routes,omitempty"`

	// ResourcesHeld истинно, когда пришедший каталог назвал ДРУГОЙ набор
	// ресурсов и ядро удерживает прежний до ответа пользователя. Пока это
	// поле истинно, действуют записи ResourcesInForce, а не Resources, и
	// экран, обещающий "пока вы не ответите, действует прежний набор", говорит
	// правду ровно благодаря этому.
	ResourcesHeld bool `json:"resources_held,omitempty"`
	// ResourcesInForce это имена набора, который применяется СЕЙЧАС. Заполнено
	// только при удержании: в обычном случае это и есть Resources.
	ResourcesInForce []string `json:"resources_in_force,omitempty"`

	// Exits и Relays это флот доверенного каталога, ex и re, в порядке
	// подписи. Без них выбор страны нарисовать нечем: остальной снимок несёт
	// ЧЕТЫРЕ строки авторитетного выбора (Exit, Relay, Preset, Variant), но
	// ни одной строки о том, из чего этот выбор сделан, поэтому обвязка
	// показывала бы выбранное значение и не могла бы предложить ни одного
	// другого.
	//
	// Порядок не пересортирован: он подписан, и любая сортировка здесь
	// означала бы, что пользователь видит не тот список, который подписал
	// оператор. Сортировать по алфавиту или по стране это дело того, кто
	// рисует.
	Exits  []NodeRef `json:"exits,omitempty"`
	Relays []NodeRef `json:"relays,omitempty"`

	Rungs []RungState `json:"rungs"`
}

// capBitNames перечисляет назначения битов, 02-SPEC.md 6.3.
var capBitNames = []string{
	"per_node_material", "sealed_directives", "relay_chaining", "settings_write",
	"mirror_pool", "doh_endpoints", "resource_hashes", "deprecation_channel",
	"onboarding_grant", "device_enrollment", "variant_forwarding", "port_hopping",
}

// Snapshot собирает читаемое состояние профиля.
func (f *Fetcher) Snapshot() Snapshot {
	f.mu.Lock()
	defer f.mu.Unlock()
	st := f.store.State()
	f.reviewClockLocked()
	now := f.nowTrustedLocked()

	s := Snapshot{
		Enrolled:        st.PID != "",
		PID:             st.PID,
		Origin:          st.Origin,
		Operator:        st.OperatorName,
		PinnedOutOfBand: st.PinnedOutOfBand,
		RootChanged:     st.RootChanged,
		EnrolledAt:      st.EnrolledAt,
		CapClient:       ClientCap,
		ClockTrusted:    f.clockTrusted,
		TimeFloor:       st.TimeFloor,
		Revoked:         st.Revoked,
		Locator:         st.Locator,
		CatID:           st.CatID,
		Thresholds:      f.ladder.Thresholds(),
		TTL:             st.TTL,
		Jit:             st.Jit,
		ExpH:            st.ExpH,
		Rungs:           f.ladder.State(),
	}
	if st.LinkPin != "" {
		s.RootFingerprint = groupFour(strings.ToUpper(csm.Base32CrockfordEncode(st.LinkPinBytes())))
	}
	if f.anchor != nil {
		s.Key = DocState{
			Present: true, Ver: f.anchor.Env.Ver,
			IAT: int64(f.anchor.Env.IAT), Exp: int64(f.anchor.Env.Exp),
			Expired: f.clockTrusted && now > int64(f.anchor.Env.Exp)+int64(csm.SkewSeconds),
			Role:    csm.RoleRoot, Bytes: len(f.anchorFrame),
		}
	}
	if f.catalog != nil {
		s.Resources = catalogResources(f.catalog)
		s.Routes = catalogRoutes(f.catalog)
		s.Exits, s.Relays = catalogFleet(f.catalog, f.catFrame, f.anchor)
		if f.guard.PendingCatalogChange() {
			s.ResourcesHeld = true
			s.ResourcesInForce = f.guard.Names()
		}
		s.Catalog = DocState{
			Present: true, Ver: f.catalog.Env.Ver,
			IAT: int64(f.catalog.Env.IAT), Exp: int64(f.catalog.Env.Exp),
			Expired: f.clockTrusted && now > int64(f.catalog.Env.Exp)+int64(csm.SkewSeconds),
			Role:    csm.RoleOnline, Bytes: len(f.catFrame),
		}
	}
	if f.directive != nil {
		d := f.directive
		s.Directive = DocState{
			Present: true, Ver: d.Env.Ver,
			IAT: int64(d.Env.IAT), Exp: int64(d.Env.Exp),
			Expired: f.clockTrusted && now > int64(d.Env.Exp)+int64(csm.SkewSeconds),
			Role:    csm.RoleOnline, Bytes: len(f.dirFrame),
		}
		s.Status, s.Reason = d.St, d.RC
		s.CapOperator = binary.BigEndian.Uint32(d.Cap[:])
		if d.Sel != nil {
			s.Exit, s.Relay, s.Preset, s.Variant = d.Sel.Exit, d.Sel.Relay, d.Sel.Preset, d.Sel.Variant
		}
	} else if f.catalog != nil {
		s.CapOperator = binary.BigEndian.Uint32(f.catalog.Cap[:])
	}
	s.CapEffective = f.effectiveCapLocked()
	for i, name := range capBitNames {
		if s.CapEffective&(1<<uint(i)) != 0 {
			s.CapBits = append(s.CapBits, name)
		}
	}
	// Тариф берётся из доверенной директивы, а не из каталога: строку tiers
	// не имеет права выбирать проверяемый документ (03-WIRE.md 6.2 V14b).
	s.ClockChanged = f.clockChanged
	s.RevokedNodesDropped = f.revokedNodesDropped
	s.FleetEmpty = f.fleetEmpty
	if f.catalog != nil && f.anchor != nil && f.directive != nil {
		s.FleetRootAnchored = csm.FleetRootAnchored(f.directive.Tier, f.anchor)
	}

	// Возраст конфигурации и её источник, инвариант 21. Истёкшая директива
	// НЕ отключает пользователя: она только не даёт принять новый статус, и
	// именно это здесь и показывается.
	if st.FetchedAt > 0 {
		s.AgeSeconds = now - st.FetchedAt
	}
	s.Stale = s.Directive.Expired || st.FetchedAt == 0 ||
		(st.TTL > 0 && s.AgeSeconds > int64(st.TTL)*2)
	s.Source = st.LastRung.Name()
	if st.FetchedAt == 0 {
		s.Source = R0Cached.Name()
	}
	return s
}

// catalogResources проецирует rs и geo каталога в порядке подписи. Порядок
// сохраняется как есть: он подписан, а пересортировка здесь означала бы, что
// клиент сравнивает не то, что подписал оператор.
func catalogResources(cat *csm.Catalog) []ResourceRef {
	if cat == nil {
		return nil
	}
	out := make([]ResourceRef, 0, len(cat.RS)+len(cat.Geo))
	for _, r := range cat.RS {
		out = append(out, ResourceRef{Kind: "rs", Name: r.Name, Hash: hex.EncodeToString(r.Hash)})
	}
	for _, r := range cat.Geo {
		out = append(out, ResourceRef{Kind: "geo", Name: r.Name, Hash: hex.EncodeToString(r.Hash)})
	}
	return out
}

// catalogRoutes проецирует ro[].rs каждого маршрута.
func catalogRoutes(cat *csm.Catalog) []RouteRules {
	if cat == nil {
		return nil
	}
	out := make([]RouteRules, 0, len(cat.Ro))
	for _, r := range cat.Ro {
		rs := make([]string, len(r.RS))
		copy(rs, r.RS)
		out = append(out, RouteRules{ID: r.ID, RS: rs})
	}
	return out
}

// catalogFleet проецирует ex и re доверенного каталога и помечает отозванные
// узлы вместо того, чтобы их прятать.
//
// Тонкость, из-за которой одной пометки мало. К моменту снимка отозванные
// записи уже физически выброшены из разобранного каталога: applyCatalogLocked
// зовёт csm.DropRevokedNodes, и это правильно — 03-WIRE.md 8.1 ключ 12
// требует, чтобы изъятый узел выпал из ПРИМЕНЕНИЯ даже на кешированном
// каталоге без сети. Но выпасть из применения и исчезнуть с экрана это разные
// вещи, и второе здесь не подразумевается: пользователю нужно увидеть, что
// страна пропала не сама.
//
// Поэтому записи восстанавливаются разбором того же кадра, который уже был
// проверен. Подпись не перепроверяется (её проверили, когда кадр приняли),
// диск не читается, и ветка выполняется ТОЛЬКО когда rev.nodes назвал
// идентификатор, которого в отфильтрованном каталоге нет. На каталоге без
// отзывов — а это обычный каталог — не делается ничего лишнего.
//
// Отказ разбора не выдумывает записи: тогда проекция остаётся такой, какой её
// сделал отфильтрованный каталог, и отозванный узел просто отсутствует. Это
// хуже, но это не ложь.
func catalogFleet(cat *csm.Catalog, frame []byte, anchor *csm.KeyDocument) (exits, relays []NodeRef) {
	if cat == nil {
		return nil, nil
	}
	revoked := revokedNodeSet(anchor)
	exits = projectNodes(cat.Ex, NodeKindExit, revoked)
	relays = projectNodes(cat.Re, NodeKindRelay, revoked)
	if len(revoked) == 0 || len(frame) == 0 {
		return exits, relays
	}
	if !anyRevokedMissing(revoked, exits, relays) {
		return exits, relays
	}
	_, doc, err := csm.Parse(frame)
	if err != nil {
		return exits, relays
	}
	full, ok := doc.(*csm.Catalog)
	if !ok {
		return exits, relays
	}
	// Полный каталог это надмножество отфильтрованного, разобранное из тех же
	// байт, поэтому пересборка целиком сохраняет подписанный порядок точнее,
	// чем вставка недостающих записей на угаданные места.
	return projectNodes(full.Ex, NodeKindExit, revoked), projectNodes(full.Re, NodeKindRelay, revoked)
}

// revokedNodeSet собирает rev.nodes доверенного ключевого документа.
func revokedNodeSet(anchor *csm.KeyDocument) map[string]bool {
	if anchor == nil || len(anchor.Rev.Nodes) == 0 {
		return nil
	}
	m := make(map[string]bool, len(anchor.Rev.Nodes))
	for _, id := range anchor.Rev.Nodes {
		m[id] = true
	}
	return m
}

// anyRevokedMissing истинно, когда хотя бы один отозванный идентификатор не
// представлен в проекции: значит, его уже выбросил фильтр применения.
func anyRevokedMissing(revoked map[string]bool, sets ...[]NodeRef) bool {
	present := make(map[string]bool, len(revoked))
	for _, set := range sets {
		for _, n := range set {
			if revoked[n.ID] {
				present[n.ID] = true
			}
		}
	}
	return len(present) < len(revoked)
}

// projectNodes проецирует срез записей узлов в порядке подписи.
func projectNodes(in []csm.Node, kind string, revoked map[string]bool) []NodeRef {
	if len(in) == 0 {
		return nil
	}
	out := make([]NodeRef, 0, len(in))
	for _, n := range in {
		ref := NodeRef{
			ID: n.ID, Name: n.PN, CC: n.CC, Kind: kind,
			Proto: n.PR, ProtoName: protoNames[n.PR],
			Network: n.NW, NetworkName: networkNames[n.NW],
			Security: n.SE, SecurityName: securityNames[n.SE],
			Relay:     n.RL,
			Available: !revoked[n.ID],
		}
		if !ref.Available {
			ref.Reason = ReasonNodeRevoked
		}
		out = append(out, ref)
	}
	return out
}

// groupFour разбивает отпечаток на группы по четыре символа.
func groupFour(s string) string {
	var b strings.Builder
	for i, r := range s {
		if i > 0 && i%4 == 0 {
			b.WriteByte(' ')
		}
		b.WriteRune(r)
	}
	return b.String()
}

// FingerprintHex отдаёт пин в hex для мест, где нужен машинный вид.
func FingerprintHex(pin []byte) string { return hex.EncodeToString(pin) }
