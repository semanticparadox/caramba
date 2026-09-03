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
