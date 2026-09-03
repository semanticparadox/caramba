package transport

import (
	"errors"
	"fmt"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

// Константы бюджета и таймаутов. 03-WIRE.md 11.1 и 02-SPEC.md 8.5.
const (
	// HandshakeDebitBytes это фиксированный дебет за каждое новое TCP
	// соединение: клиент не может переносимо посчитать байты собственного
	// рукопожатия TLS, поэтому протокол списывает их авансом. Значение
	// предварительное, 03-WIRE.md 11.1.
	HandshakeDebitBytes uint64 = 2816
	// HandshakeDebitPackets это тот же дебет в пакетах данных.
	HandshakeDebitPackets uint64 = 8
	// PacketMTUAssumed это MTU, по которому байты пересчитываются в пакеты.
	PacketMTUAssumed uint64 = 1400

	// ConnBytesCeiling это скомпилированный потолок thr.conn_bytes. Подписанное
	// значение связывает, только если оно НИЖЕ (02-SPEC.md 8.6.1).
	ConnBytesCeiling uint64 = 15360
	// ConnPacketsCeiling это скомпилированный потолок thr.conn_packets.
	ConnPacketsCeiling uint64 = 25

	// DefaultConnBytes, DefaultConnPackets и DefaultRespMax действуют, пока ни
	// один каталог не был проверен (02-SPEC.md 8.6).
	DefaultConnBytes   uint64 = 8192
	DefaultConnPackets uint64 = 22
	DefaultRespMax     uint64 = 4096

	// ExphFloor это нижняя граница окна offline-грации оператора, секунды.
	ExphFloor uint64 = 86400
	// TTLFloor это нижняя граница периода опроса, секунды. Именно 900, а не
	// 1800: 06-MIGRATION.md 4.3 опирается на ускорение до 900 для аварийного
	// отката, и пол в 1800 сделал бы его молча неработающим.
	TTLFloor uint64 = 900
	// TTLMinJitterPercent применяется независимо от подписанного jit.
	TTLMinJitterPercent uint64 = 10
	// DefaultTTL и DefaultTTLK это умолчания периодов, 02-SPEC.md 5.6.
	DefaultTTL  uint64 = 7200
	DefaultTTLK uint64 = 21600
)

// Таймауты одной попытки и цикла, 02-SPEC.md 8.5.
const (
	TCPConnectTimeout    = 5 * time.Second
	TLSHandshakeTimeout  = 5 * time.Second
	AttemptTimeoutR1R2R3 = 12 * time.Second
	AttemptTimeoutR4R5   = 20 * time.Second
	CycleBudget          = 90 * time.Second
)

// Backoff между неуспешными циклами, 02-SPEC.md 8.7.
const (
	BackoffInitial = 30 * time.Second
	BackoffMax     = 3600 * time.Second
	// BackoffJitterPercent это разброс в обе стороны.
	BackoffJitterPercent = 20
)

// ErrRespMaxTooHigh это отказ каталога: thr.resp_max выше 4096 нарушает
// инвариант 5 и арифметику разбиения 03-WIRE.md 11.3. Поле может только
// понижать значение, поэтому это отказ каталога, а не зажим.
var ErrRespMaxTooHigh = errors.New("transport: thr.resp_max выше 4096, каталог отвергнут")

// ErrBudgetExceeded возвращается, когда запрос нельзя начать даже на свежем
// соединении при текущих порогах.
var ErrBudgetExceeded = errors.New("transport: бюджет соединения не допускает запрос")

// Thresholds это действующие пороги размеров после зажима. Подписанные поля
// приходят из доверенного каталога; поля этой структуры уже безопасны.
type Thresholds struct {
	ConnBytes   uint64 `json:"conn_bytes"`
	ConnPackets uint64 `json:"conn_packets"`
	RespMax     uint64 `json:"resp_max"`
}

// DefaultThresholds возвращает пороги до того, как хоть один каталог был
// проверен.
func DefaultThresholds() Thresholds {
	return Thresholds{ConnBytes: DefaultConnBytes, ConnPackets: DefaultConnPackets, RespMax: DefaultRespMax}
}

// ClampThresholds применяет 02-SPEC.md 8.6.1 к подписанным порогам.
//
// Общее правило, которое стоит помнить, если таблица устареет: подписанное
// поле, которое может только ухудшить положение клиента, зажимается на
// клиенте; там, где поле может и то и другое, побеждает БОЛЕЕ БЕЗОПАСНОЕ из
// подписанного значения и собственного потолка.
func ClampThresholds(signed csm.Thresholds) (Thresholds, error) {
	out := DefaultThresholds()

	// resp_max: инвариант 5 фиксирует 4096, поле может только понижать.
	// Значение выше это отказ КАТАЛОГА, а не зажим.
	if signed.RespMax != 0 {
		if signed.RespMax > DefaultRespMax {
			return Thresholds{}, fmt.Errorf("%w: %d", ErrRespMaxTooHigh, signed.RespMax)
		}
		out.RespMax = signed.RespMax
	}
	if signed.ConnBytes != 0 {
		out.ConnBytes = minU64(signed.ConnBytes, ConnBytesCeiling)
	}
	if signed.ConnPackets != 0 {
		out.ConnPackets = minU64(signed.ConnPackets, ConnPacketsCeiling)
	}
	return out, nil
}

// ClampExpH применяет пол окна грации. userWindow больше нуля означает, что
// пользователь явно задал более короткое окно в настройках, и тогда его выбор
// побеждает пол.
func ClampExpH(signed, userWindow uint64) uint64 {
	if userWindow > 0 {
		return userWindow
	}
	if signed < ExphFloor {
		return ExphFloor
	}
	return signed
}

// ClampTTL применяет пол периода опроса и минимальный разброс.
func ClampTTL(signedTTL, signedJit uint64) (ttl, jit uint64) {
	ttl = signedTTL
	if ttl == 0 {
		ttl = DefaultTTL
	}
	if ttl < TTLFloor {
		ttl = TTLFloor
	}
	jit = signedJit
	if jit < TTLMinJitterPercent {
		jit = TTLMinJitterPercent
	}
	return ttl, jit
}

// packetsFor считает пакеты для одного сообщения: ceil(bytes / MTU), минимум 1.
func packetsFor(b uint64) uint64 {
	if b == 0 {
		return 1
	}
	n := (b + PacketMTUAssumed - 1) / PacketMTUAssumed
	if n == 0 {
		n = 1
	}
	return n
}

// ConnBudget это учёт байтов и пакетов одного TCP соединения. Соединение
// открывается уже с списанным рукопожатием (03-WIRE.md 11.2).
type ConnBudget struct {
	thr     Thresholds
	bytes   uint64
	packets uint64
	// requests это число запросов, отправленных по этому соединению. Нужно
	// только для диагностики.
	requests int
}

// NewConnBudget открывает учёт нового соединения и сразу списывает дебет
// рукопожатия.
func NewConnBudget(thr Thresholds) *ConnBudget {
	return &ConnBudget{thr: thr, bytes: HandshakeDebitBytes, packets: HandshakeDebitPackets}
}

// Bytes и Packets отдают текущие накопленные значения.
func (b *ConnBudget) Bytes() uint64   { return b.bytes }
func (b *ConnBudget) Packets() uint64 { return b.packets }
func (b *ConnBudget) Requests() int   { return b.requests }

// CanSend проверяет правило 03-WIRE.md 11.2: запрос нельзя начинать, если
// проектируемая сумма ПОСЛЕ него и его максимально возможного ответа
// (thr.resp_max) превысит порог. Проекция идёт по максимуму, а не по факту,
// поэтому на умолчаниях вторая выборка директивы получает своё соединение.
func (b *ConnBudget) CanSend(requestBytes uint64) bool {
	projBytes := b.bytes + requestBytes + b.thr.RespMax
	if projBytes > b.thr.ConnBytes {
		return false
	}
	projPackets := b.packets + packetsFor(requestBytes) + packetsFor(b.thr.RespMax)
	return projPackets <= b.thr.ConnPackets
}

// ChargeRequest списывает строку запроса и заголовки.
func (b *ConnBudget) ChargeRequest(n uint64) {
	b.bytes += n
	b.packets += packetsFor(n)
	b.requests++
}

// ChargeResponse списывает строку ответа, заголовки и тело.
func (b *ConnBudget) ChargeResponse(n uint64) {
	b.bytes += n
	b.packets += packetsFor(n)
}

// Exhausted сообщает, что соединение обязано быть закрыто: любой из порогов
// достигнут. Клиент закрывает сам, а не ждёт пира.
func (b *ConnBudget) Exhausted() bool {
	return b.bytes >= b.thr.ConnBytes || b.packets >= b.thr.ConnPackets
}

func minU64(a, b uint64) uint64 {
	if a < b {
		return a
	}
	return b
}
