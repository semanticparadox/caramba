package transport

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"sync"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

// ErrStoreInconsistent возвращается, когда сохранённое состояние прочиталось
// пустым или несогласованным.
//
// Обнулившееся хранилище неотличимо от отката, поэтому клиент обязан считать
// профиль требующим полной перепроверки из сети, НЕ ИМЕЕТ ПРАВА сбросить
// отметку версии или временной пол в ноль и продолжить, и обязан записать
// событие в диагностику (02-SPEC.md 8.8.3).
var ErrStoreInconsistent = errors.New("transport: сохранённое состояние CSM несогласовано")

// FrameKind это имя файла кадра в каталоге профиля.
type FrameKind string

const (
	FrameKey       FrameKind = "k1"
	FrameCatalog   FrameKind = "c1"
	FrameDirective FrameKind = "m1"
	FrameSealed    FrameKind = "m1s"
	FrameBootstrap FrameKind = "b1"
	FrameReserve   FrameKind = "r1"
)

// StateVersion это версия схемы сохранённого состояния.
const StateVersion = 1

// State это доверенное состояние профиля на диске.
//
// Кадры здесь НЕ лежат: они хранятся отдельными файлами дословно как приняты.
// Испорченный кадр не проходит проверку при загрузке, и именно поэтому кеш
// хранит кадры, а не разобранное состояние.
type State struct {
	Version int `json:"version"`
	// PID это закреплённая идентичность арендатора, hex.
	PID string `json:"pid"`
	// LinkPin это sha256(root_pk)[0..12], hex. Якорь первого доверия.
	LinkPin string `json:"link_pin"`
	// Origin это закреплённый origin регистрации, https://host[:port].
	Origin string `json:"origin"`
	// SubscriptionDomain это единственный хост, на который разрешён переход.
	SubscriptionDomain string `json:"subscription_domain,omitempty"`
	// OperatorName это инертное отображаемое имя оператора.
	OperatorName string `json:"operator_name,omitempty"`
	// PinnedOutOfBand говорит, установлен ли пин вне полосы или в приложении.
	PinnedOutOfBand bool `json:"pinned_out_of_band"`
	// RootChanged истинно, если корневой отпечаток когда-либо менялся.
	RootChanged bool  `json:"root_changed"`
	EnrolledAt  int64 `json:"enrolled_at"`

	// TimeFloor это первичная временная отметка. Монотонна, растёт только на
	// принятой директиве и НИКОГДА не уменьшается.
	TimeFloor int64 `json:"time_floor"`
	// HWM это отметки старшей версии по типу документа. Ключ это десятичный
	// doc_type.
	HWM map[string]uint64 `json:"hwm"`

	// Cap это последняя проверенная битовая маска возможностей оператора.
	Cap uint32 `json:"cap"`
	// CatID это cat_id каталога, названного доверенной директивой.
	CatID string `json:"cat_id,omitempty"`
	// Locator это локатор подписки, 24 символа base32 Crockford.
	Locator string `json:"locator,omitempty"`
	// DTP это отпечаток этого устройства, hex.
	DTP string `json:"dtp,omitempty"`
	// Revoked это персистентный флаг st = 5. Он нагружен: клиент, потерявший
	// его, переподключается по отозванной подписке.
	Revoked bool `json:"revoked"`

	Thr  Thresholds `json:"thresholds"`
	TTL  uint64     `json:"ttl"`
	Jit  uint64     `json:"jit"`
	TTLK uint64     `json:"ttlk"`
	ExpH uint64     `json:"exph"`

	KeyVer       uint64 `json:"key_ver"`
	CatalogVer   uint64 `json:"catalog_ver"`
	DirectiveVer uint64 `json:"directive_ver"`
	KeyIAT       int64  `json:"key_iat"`
	KeyExp       int64  `json:"key_exp"`
	CatalogIAT   int64  `json:"catalog_iat"`
	CatalogExp   int64  `json:"catalog_exp"`
	DirectiveIAT int64  `json:"directive_iat"`
	DirectiveExp int64  `json:"directive_exp"`

	// FetchedAt это момент последней успешной сетевой выборки директивы.
	FetchedAt int64 `json:"fetched_at"`
	// LastRung это ступень, принесшая последний принятый документ. Локально,
	// наружу оператору НЕ отправляется.
	LastRung RungID `json:"last_rung"`

	// LadderOrder и LadderEnabled это выбор пользователя. Он переживает смену
	// каталога: подписанные умолчания его не восстанавливают.
	LadderOrder   []RungID        `json:"ladder_order,omitempty"`
	LadderEnabled map[string]bool `json:"ladder_enabled,omitempty"`
	LadderUserSet map[string]bool `json:"ladder_user_set,omitempty"`
	LadderProxy   string          `json:"ladder_proxy,omitempty"`
}

// Store это дисковое хранилище доверенного состояния одного профиля.
// Каталог 0700, файлы 0600.
type Store struct {
	mu  sync.Mutex
	dir string
	st  State
}

// OpenStore открывает или создаёт хранилище под workDir/csm/<pid>.
// pid передаётся в hex; пустой pid допустим до регистрации и даёт каталог
// csm/pending.
func OpenStore(workDir, pidHex string) (*Store, error) {
	name := pidHex
	if name == "" {
		name = "pending"
	}
	dir := filepath.Join(workDir, "csm", name)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, fmt.Errorf("transport: каталог состояния: %w", err)
	}
	s := &Store{dir: dir}
	if err := s.load(); err != nil {
		return nil, err
	}
	return s, nil
}

// Dir возвращает каталог профиля.
func (s *Store) Dir() string { return s.dir }

func (s *Store) load() error {
	b, err := os.ReadFile(filepath.Join(s.dir, "state.json"))
	if errors.Is(err, os.ErrNotExist) {
		s.st = State{Version: StateVersion, HWM: map[string]uint64{}, Thr: DefaultThresholds()}
		return nil
	}
	if err != nil {
		return fmt.Errorf("transport: чтение состояния: %w", err)
	}
	var st State
	if err := json.Unmarshal(b, &st); err != nil {
		// Испорченный файл это не повод продолжить с нулями: обнулившееся
		// хранилище неотличимо от отката.
		return fmt.Errorf("%w: %v", ErrStoreInconsistent, err)
	}
	if st.Version != StateVersion {
		return fmt.Errorf("%w: версия схемы %d", ErrStoreInconsistent, st.Version)
	}
	if st.HWM == nil {
		st.HWM = map[string]uint64{}
	}
	if st.Thr.RespMax == 0 {
		st.Thr = DefaultThresholds()
	}
	s.st = st
	return nil
}

// State возвращает копию сохранённого состояния.
func (s *Store) State() State {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.st.clone()
}

func (st State) clone() State {
	out := st
	out.HWM = map[string]uint64{}
	for k, v := range st.HWM {
		out.HWM[k] = v
	}
	if st.LadderEnabled != nil {
		out.LadderEnabled = map[string]bool{}
		for k, v := range st.LadderEnabled {
			out.LadderEnabled[k] = v
		}
	}
	if st.LadderUserSet != nil {
		out.LadderUserSet = map[string]bool{}
		for k, v := range st.LadderUserSet {
			out.LadderUserSet[k] = v
		}
	}
	if st.LadderOrder != nil {
		out.LadderOrder = append([]RungID(nil), st.LadderOrder...)
	}
	return out
}

// Update применяет мутацию к состоянию и сохраняет его атомарно.
//
// Временной пол и отметки версии здесь только растут: попытка их понизить это
// откат, и она отвергается на месте, а не превращается в тихую запись.
func (s *Store) Update(fn func(*State)) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	next := s.st.clone()
	if next.HWM == nil {
		next.HWM = map[string]uint64{}
	}
	prevFloor := s.st.TimeFloor
	prevHWM := s.st.HWM
	fn(&next)
	next.Version = StateVersion
	if next.TimeFloor < prevFloor {
		return fmt.Errorf("%w: попытка понизить time_floor с %d до %d", ErrStoreInconsistent, prevFloor, next.TimeFloor)
	}
	for k, old := range prevHWM {
		if nv, ok := next.HWM[k]; !ok || nv < old {
			return fmt.Errorf("%w: попытка понизить hwm[%s] с %d", ErrStoreInconsistent, k, old)
		}
	}
	if err := s.writeLocked(next); err != nil {
		return err
	}
	s.st = next
	return nil
}

func (s *Store) writeLocked(st State) error {
	b, err := json.MarshalIndent(st, "", "  ")
	if err != nil {
		return err
	}
	return writeFile0600(filepath.Join(s.dir, "state.json"), b)
}

// Rebind переносит каталог профиля из временного csm/pending в csm/<pid>
// после регистрации. Один профиль это один каталог: второй каталог означал бы
// вторую отметку версии, а это дыра для отката, а не эшелонированная защита.
func (s *Store) Rebind(workDir, pidHex string) error {
	if pidHex == "" {
		return nil
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	target := filepath.Join(workDir, "csm", pidHex)
	if target == s.dir {
		return nil
	}
	if _, err := os.Stat(target); err == nil {
		// Каталог профиля уже существует: переносить в него нечего, работаем
		// с ним.
		s.dir = target
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(target), 0o700); err != nil {
		return err
	}
	if err := os.Rename(s.dir, target); err != nil {
		return err
	}
	s.dir = target
	return nil
}

// PutFrame сохраняет кадр дословно как принят. Никакой пересборки: испорченный
// кадр обязан провалить проверку при загрузке, а не отличаться от того, что
// пришло.
func (s *Store) PutFrame(kind FrameKind, raw []byte) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return writeFile0600(filepath.Join(s.dir, string(kind)+".frame"), raw)
}

// Frame читает сохранённый кадр.
func (s *Store) Frame(kind FrameKind) ([]byte, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	b, err := os.ReadFile(filepath.Join(s.dir, string(kind)+".frame"))
	if err != nil {
		return nil, false
	}
	return b, true
}

// PutChunk сохраняет фрагмент каталога по индексу.
func (s *Store) PutChunk(catID string, i uint64, raw []byte) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	dir := filepath.Join(s.dir, "chunks", catID)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	return writeFile0600(filepath.Join(dir, strconv.FormatUint(i, 10)+".frame"), raw)
}

// Chunk читает сохранённый фрагмент каталога.
func (s *Store) Chunk(catID string, i uint64) ([]byte, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	b, err := os.ReadFile(filepath.Join(s.dir, "chunks", catID, strconv.FormatUint(i, 10)+".frame"))
	if err != nil {
		return nil, false
	}
	return b, true
}

// writeFile0600 пишет через временный файл и переименование, чтобы обрыв не
// оставил половину состояния.
func writeFile0600(path string, b []byte) error {
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, b, 0o600); err != nil {
		return err
	}
	if err := os.Chmod(tmp, 0o600); err != nil {
		_ = os.Remove(tmp)
		return err
	}
	if err := os.Rename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return err
	}
	return nil
}

// hwmScopeOf сводит doc_type к его области отметки версии.
//
// 02-SPEC.md 4.7 держит 0x03 и 0x06 в ОДНОМ счётчике, привязанном к локатору:
// внешний запечатанный конверт и внутренняя директива несут одну и ту же ver и
// один и тот же локатор, это один документ в двух оболочках. Разведи их по
// разным ключам, и отметка внешнего кадра останется нулевой навсегда, то есть
// у конверта не будет границы отката вовсе.
func hwmScopeOf(docType uint8) uint8 {
	if docType == csm.DocSealed {
		return csm.DocDirective
	}
	return docType
}

// HWMMap превращает сохранённые отметки в форму, которую ждёт csm.TrustState.
func (st State) HWMMap() map[uint8]uint64 {
	out := map[uint8]uint64{}
	for k, v := range st.HWM {
		n, err := strconv.ParseUint(k, 10, 8)
		if err != nil {
			continue
		}
		out[uint8(n)] = v
	}
	// Внешний конверт читает ту же отметку, что и директива внутри него.
	out[csm.DocSealed] = out[csm.DocDirective]
	return out
}

// SetHWM записывает отметку в сохранённую форму.
func (st *State) SetHWM(docType uint8, ver uint64) {
	if st.HWM == nil {
		st.HWM = map[string]uint64{}
	}
	key := strconv.FormatUint(uint64(hwmScopeOf(docType)), 10)
	if cur, ok := st.HWM[key]; ok && cur > ver {
		return
	}
	st.HWM[key] = ver
}

// PIDBytes и LinkPinBytes декодируют hex поля.
func (st State) PIDBytes() []byte     { b, _ := hex.DecodeString(st.PID); return b }
func (st State) LinkPinBytes() []byte { b, _ := hex.DecodeString(st.LinkPin); return b }
func (st State) DTPBytes() []byte     { b, _ := hex.DecodeString(st.DTP); return b }
