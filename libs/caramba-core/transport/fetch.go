package transport

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

// ClientCap это битовое поле возможностей, СКОМПИЛИРОВАННОЕ в этот клиент.
// Оно не настраивается, не выбирается по сети и не хранится: бит, который
// клиент не реализует, равен нулю, и пересечение с оператором даёт ноль, что
// и есть безопасное направление (02-SPEC.md 6.2).
//
// Бит 10 (проброс variant) включён, потому что subscription.FetchOptions
// теперь несёт Variant; до этого изменения он был бы ложью о клиенте.
const ClientCap uint32 = 1<<0 | 1<<1 | 1<<2 | 1<<3 | 1<<4 | 1<<5 | 1<<6 |
	1<<7 | 1<<8 | 1<<9 | 1<<10 | 1<<11

// BuildEpoch это Unix-секунда сборки. Нужна для классификации часов на первом
// запуске: при отсутствии доверенных часов ЕДИНСТВЕННОЕ, что ограничивает
// повтор старого ключевого документа, это правдоподобность часов устройства.
// Значение переопределяется линковкой: -X ...transport.buildEpochRaw=<unix>.
var buildEpochRaw = "1767225600" // 2026-01-01T00:00:00Z

// BuildEpoch возвращает разобранную отметку сборки.
func BuildEpoch() int64 {
	v, err := strconv.ParseInt(strings.TrimSpace(buildEpochRaw), 10, 64)
	if err != nil || v <= 0 {
		return 1767225600
	}
	return v
}

// ClockPlausibleWindow это десять лет: окно, внутри которого часы устройства
// на регистрации считаются правдоподобными.
const ClockPlausibleWindow int64 = 315360000

// Отказы выборки.
var (
	// ErrNotEnrolled означает, что профиль ещё не закрепил корневой ключ.
	ErrNotEnrolled = errors.New("transport: профиль не зарегистрирован")
	// ErrClockImplausible это отказ ЗАВЕРШИТЬ регистрацию: часы устройства
	// вне окна правдоподобности. Регистрироваться вслепую нельзя, и часы
	// устройства клиент не выставляет сам.
	ErrClockImplausible = errors.New("transport: часы устройства неправдоподобны, регистрация отклонена")
	// ErrPinMismatchRoot это несовпадение корневого пина при первом доверии.
	// Жёсткая ошибка без пути "всё равно продолжить".
	ErrPinMismatchRoot = errors.New("transport: корневой ключ не совпал с продиктованным пином")
	// ErrCatalogRefused означает, что каталог отвергнут клиентским зажимом,
	// а не подписью.
	ErrCatalogRefused = errors.New("transport: каталог отвергнут клиентским правилом")
	// ErrRevoked означает персистентный st = 5.
	ErrRevoked = errors.New("transport: подписка отозвана")
)

// Fetcher это путь выборки и проверки документов CSM/1 одного профиля.
//
// Инвариант 16 живёт здесь и он абсолютный: истёкший документ по-прежнему
// подключает. Сетевой отказ НЕ очищает кеш, НЕ гасит туннель и НЕ понижает
// доверенное состояние; он только запрещает принимать новые настройки и новый
// статус.
type Fetcher struct {
	mu      sync.Mutex
	workDir string
	store   *Store
	ladder  *Ladder
	keys    DeviceKeys
	now     func() time.Time
	monoRef time.Time
	wallRef int64

	// clockTrusted живёт ТОЛЬКО в памяти и пересчитывается на каждом запуске:
	// сохранённый флаг пережил бы смену часов, а он существует именно чтобы её
	// заметить.
	clockTrusted  bool
	clockEstimate int64
	// estimateAt это монотонная точка, в которой была снята clockEstimate.
	estimateAt time.Time
	// clockChanged взводится, когда доверие часам снималось: обвязка обязана
	// уметь сказать пользователю, что часы устройства переводили.
	clockChanged bool

	anchor      *csm.KeyDocument
	anchorFrame []byte
	catalog     *csm.Catalog
	catFrame    []byte
	directive   *csm.Directive
	dirFrame    []byte
	guard       *ResourceGuard

	nonce   []byte
	nonceAt time.Time

	// revokedNodesDropped и fleetEmpty это то, что обвязка обязана показать
	// после применения rev.nodes.
	revokedNodesDropped int
	fleetEmpty          bool
}

// NewFetcher создаёт выборщик над рабочим каталогом. pidHex может быть пуст до
// регистрации.
func NewFetcher(workDir, pidHex string, l *Ladder, keys DeviceKeys) (*Fetcher, error) {
	st, err := OpenStore(workDir, pidHex)
	if err != nil {
		return nil, err
	}
	if keys == nil {
		keys, err = NewSoftwareDeviceKeys(st.Dir())
		if err != nil {
			return nil, err
		}
	}
	f := &Fetcher{workDir: workDir, store: st, ladder: l, keys: keys, now: time.Now}
	f.monoRef = time.Now()
	f.wallRef = f.monoRef.Unix()
	l.SetCache(f)
	return f, nil
}

// SetClock подменяет часы. Только для тестов.
func (f *Fetcher) SetClock(fn func() time.Time) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.now = fn
}

// Store и Ladder отдают части выборщика вызывающему.
func (f *Fetcher) Store() *Store    { return f.store }
func (f *Fetcher) Ladder() *Ladder  { return f.ladder }
func (f *Fetcher) Keys() DeviceKeys { return f.keys }

// Lookup реализует CacheSource: ступень R0 отдаёт последний хороший кадр для
// пути запроса. Сети это не стоит и это всегда первая ступень.
func (f *Fetcher) Lookup(req *http.Request) ([]byte, bool) {
	if req == nil || req.URL == nil || req.Method != http.MethodGet {
		return nil, false
	}
	p := req.URL.Path
	switch {
	case p == "/sub/k1":
		return f.store.Frame(FrameKey)
	case strings.HasPrefix(p, "/sub/m1/"):
		return f.store.Frame(FrameSealed)
	case strings.HasPrefix(p, "/sub/r1/"):
		return f.store.Frame(FrameReserve)
	case strings.HasPrefix(p, "/sub/c1/"):
		catID, idx, ok := parseChunkPath(p)
		if !ok {
			return nil, false
		}
		return f.store.Chunk(catID, idx)
	}
	return nil, false
}

func parseChunkPath(p string) (string, uint64, bool) {
	parts := strings.Split(strings.Trim(p, "/"), "/")
	if len(parts) != 4 || parts[0] != "sub" || parts[1] != "c1" {
		return "", 0, false
	}
	i, err := strconv.ParseUint(parts[3], 10, 64)
	if err != nil {
		return "", 0, false
	}
	// cat_id уходит в filepath.Join под каталогом профиля, поэтому набор
	// символов проверяется ДО того, как строка станет частью пути. Значение
	// приходит от управляющего слоя, а не от оператора, так что это упрочнение,
	// а не закрытие дыры; упрочнение стоит одной проверки.
	if !csm.ValidCatalogID(parts[2]) {
		return "", 0, false
	}
	return parts[2], i, true
}

// trustLocked собирает состояние доверия для проверки одного документа.
//
// Роли, пороги и список отзыва читаются ИЗ ЯКОРЯ, то есть из ранее доверенного
// документа, и никогда из проверяемого.
func (f *Fetcher) trustLocked(st State) *csm.TrustState {
	f.reviewClockLocked()
	ts := &csm.TrustState{
		PinnedPID:    st.PIDBytes(),
		LinkPin:      st.LinkPinBytes(),
		Anchor:       f.anchor,
		Now:          f.nowTrustedLocked(),
		ClockTrusted: f.clockTrusted,
		TimeFloor:    st.TimeFloor,
		HWM:          st.HWMMap(),
		DeviceDTP:    st.DTPBytes(),
	}
	if f.keys != nil {
		gens := []uint64{}
		for g := uint64(1); g <= f.keys.Generation(); g++ {
			gens = append(gens, g)
		}
		ts.AgreementKeys = AgreementKeyMap(f.keys, gens)
	}
	return ts
}

// reviewClockLocked реализует последний пункт 02-SPEC.md 5.5: доверие часам
// СНИМАЕТСЯ, когда стенные часы ушли назад больше чем на 300 секунд
// относительно монотонного смещения. Это сигнал сброса к заводским настройкам
// и ручного перевода часов, и без него один перевод стрелок назад делает V12
// проходимым для сколь угодно старых документов.
func (f *Fetcher) reviewClockLocked() {
	if !f.clockTrusted {
		return
	}
	elapsed := int64(time.Since(f.monoRef) / time.Second)
	drift := (f.now().Unix() - f.wallRef) - elapsed
	if drift < -int64(csm.SkewSeconds) {
		f.clockTrusted = false
		f.clockChanged = true
		// Опорные точки переустанавливаются, иначе один прыжок назад
		// докладывался бы на каждом последующем вызове.
		f.monoRef = time.Now()
		f.wallRef = f.now().Unix()
	}
}

// nowTrustedLocked отдаёт время для шагов свежести. Пока часам доверяют, это
// оценка из последней директивы плюс прошедшее МОНОТОННОЕ время, а не стенные
// часы: 02-SPEC.md 5.5 описывает именно её, и она не прыгает вместе с
// пользовательской настройкой даты.
func (f *Fetcher) nowTrustedLocked() int64 {
	if f.clockTrusted && f.clockEstimate > 0 {
		return f.clockEstimate + int64(time.Since(f.estimateAt)/time.Second)
	}
	return f.now().Unix()
}

// ClockChanged сообщает, что доверие часам снималось в этом запуске.
func (f *Fetcher) ClockChanged() bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.clockChanged
}

// LoadCached поднимает доверенное состояние с диска.
//
// Кадры перепроверяются при загрузке: кеш хранит кадры, а не разобранное
// состояние, ровно чтобы подделанный кадр провалился здесь, а не был принят на
// веру. Ошибка загрузки НЕ обнуляет состояние: обнулившееся хранилище
// неотличимо от отката.
func (f *Fetcher) LoadCached() error {
	f.mu.Lock()
	defer f.mu.Unlock()
	st := f.store.State()
	if st.PID == "" {
		return ErrNotEnrolled
	}

	keyFrame, ok := f.store.Frame(FrameKey)
	if !ok {
		return fmt.Errorf("%w: ключевой документ не сохранён", ErrStoreInconsistent)
	}
	// Ключевой документ это якорь, и предыдущего якоря для него нет: он
	// проверяется против закреплённого link_pin по правилу первого доверия.
	//
	// Отметка версии подставляется НАСТОЯЩАЯ. Ранее здесь стояло ver - 1,
	// чтобы обойти шаг V10, который применялся к каждому 0x01 без условия; это
	// заодно выключало и V9, то есть откатанный на диске якорь не ловился при
	// загрузке. V10 теперь выполняется только при растущей версии
	// (03-WIRE.md 6.3 и 7.3), поэтому обход больше не нужен, а V9 работает.
	ts := f.trustLocked(st)
	ts.Anchor = nil
	ts.StoredFrame = keyFrame
	res, err := csm.Verify(keyFrame, ts)
	if err != nil {
		return err
	}
	kd, _ := res.Doc.(*csm.KeyDocument)
	if kd == nil {
		return fmt.Errorf("%w: сохранённый кадр не ключевой документ", ErrStoreInconsistent)
	}
	f.anchor, f.anchorFrame = kd, keyFrame

	// Директива поднимается ПЕРВОЙ: её tier и cat нужны каталогу для шагов
	// V14a и V14b, а читать их из самого каталога нельзя.
	if df, ok := f.store.Frame(FrameDirective); ok {
		ts := f.trustLocked(st)
		ts.StoredFrame = df
		// Кадр уже был принят под своим nonce; повторить его на перезагрузке
		// нечем. Флаг снимает ТОЛЬКО V13 и только при побайтовом совпадении
		// с сохранённым кадром, который здесь и подставлен.
		ts.CachedReplay = true
		r, err := csm.Verify(df, ts)
		switch {
		case err != nil:
			// Подделанный кадр в кеше это событие безопасности, а не пустое
			// место: 02-SPEC.md 8.8.3 требует его записать, иначе он
			// неотличим от отсутствующего файла.
			f.ladder.RecordCacheFailure("directive", err)
		default:
			if d, ok := r.Doc.(*csm.Directive); ok {
				f.directive, f.dirFrame = d, df
			}
		}
	}
	if cf, ok := f.store.Frame(FrameCatalog); ok {
		ts := f.trustLocked(st)
		ts.StoredFrame = cf
		if f.directive != nil {
			ts.BoundCatHash = f.directive.Cat
			tier := f.directive.Tier
			ts.BoundTier = &tier
		}
		r, err := csm.Verify(cf, ts)
		switch {
		case err != nil:
			f.ladder.RecordCacheFailure("catalog", err)
		default:
			if cat, ok := r.Doc.(*csm.Catalog); ok {
				f.catalog, f.catFrame = cat, cf
			}
		}
	}
	f.applyCatalogLocked(st)
	return nil
}

// EffectiveCap возвращает пересечение оператора и клиента.
//
// Оператором считается cap САМОЙ СВЕЖЕЙ проверенной и неистёкшей директивы;
// cap каталога используется, только пока такой директивы нет. Четыре бита,
// утверждающие наличие содержимого каталога (0, 4, 5, 6), при отсутствующем
// или пустом массиве считаются нулём: это утверждение о байтах, а не
// переопределение политики, и выдать возможность оно не может.
func (f *Fetcher) EffectiveCap() uint32 {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.effectiveCapLocked()
}

func (f *Fetcher) effectiveCapLocked() uint32 {
	var op uint32
	// То же время, что и у шагов свежести: пока часам доверяют, это оценка из
	// последней директивы плюс монотонное время, а не стенные часы.
	f.reviewClockLocked()
	now := f.nowTrustedLocked()
	if f.directive != nil && (!f.clockTrusted || now <= int64(f.directive.Env.Exp)+int64(csm.SkewSeconds)) {
		op = binary.BigEndian.Uint32(f.directive.Cap[:])
	} else if f.catalog != nil {
		op = binary.BigEndian.Uint32(f.catalog.Cap[:])
	} else {
		return 0
	}
	eff := op & ClientCap
	// Карве-аут по содержимому.
	if f.catalog == nil {
		return eff &^ ((1 << 0) | (1 << 4) | (1 << 5) | (1 << 6))
	}
	if len(f.catalog.Ex) == 0 {
		eff &^= 1 << 0
	}
	if len(f.catalog.Mir) == 0 {
		eff &^= 1 << 4
	}
	if len(f.catalog.DoH) == 0 {
		eff &^= 1 << 5
	}
	if len(f.catalog.RS) == 0 && len(f.catalog.Geo) == 0 {
		eff &^= 1 << 6
	}
	return eff
}

// applyCatalogLocked переносит доверенный каталог в лестницу и страж ресурсов.
func (f *Fetcher) applyCatalogLocked(st State) {
	if f.catalog == nil {
		return
	}
	// Отзыв узлов применяется ЗДЕСЬ, до того как каталог станет пригодным:
	// 03-WIRE.md 8.1 ключ 12 требует, чтобы изъятый узел выпадал и на
	// кешированном каталоге, при полностью отсутствующей сети. Фильтр
	// идемпотентен, поэтому повторный вызов ничего не меняет.
	if n := csm.DropRevokedNodes(f.catalog, f.anchor); n > 0 {
		f.revokedNodesDropped += n
		f.ladder.RecordCacheNote("rev.nodes",
			fmt.Sprintf("%d node(s) named in rev.nodes dropped from the catalog", n))
	}
	// Пустой ex после фильтрации это флот, которым клиент пользоваться не
	// может, а не флот с нулём выходов. Отказ закрытый и его видно в обвязке.
	f.fleetEmpty = len(f.catalog.Ex) == 0
	capBits := f.effectiveCapLocked()
	thr, err := ClampThresholds(f.catalog.Thr)
	if err == nil {
		f.ladder.SetThresholds(thr)
	}
	_ = f.ladder.ApplyCatalog(f.catalog, capBits)
	f.guard = NewResourceGuard(f.catalog, capBits)
	if len(st.LadderOrder) > 0 {
		_ = f.ladder.SetOrder(st.LadderOrder)
	}
	for name, on := range st.LadderEnabled {
		if r, err := strconv.ParseUint(name, 10, 8); err == nil {
			_ = f.ladder.SetEnabled(RungID(r), on)
		}
	}
	if st.LadderProxy != "" {
		f.ladder.SetProxy(st.LadderProxy)
	}
}

// RevokedNodesDropped и FleetEmpty отдают итог применения rev.nodes.
func (f *Fetcher) RevokedNodesDropped() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.revokedNodesDropped
}

func (f *Fetcher) FleetEmpty() bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.fleetEmpty
}

// Guard возвращает страж ресурсов доверенного каталога.
func (f *Fetcher) Guard() *ResourceGuard {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.guard == nil {
		return NewResourceGuard(nil, 0)
	}
	return f.guard
}

// newNonce выпускает новый nonce и запоминает его. Одновременно у профиля
// выдан ровно один nonce; выпуск нового бросает предыдущий.
func (f *Fetcher) newNonce() ([]byte, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return nil, err
	}
	f.nonce = b
	f.nonceAt = f.now()
	return b, nil
}

// nonceValid проверяет, что выданный nonce ещё жив (300 секунд).
func (f *Fetcher) nonceValid() bool {
	return len(f.nonce) == 16 && f.now().Sub(f.nonceAt) <= 300*time.Second
}

// ---------------------------------------------------------------- регистрация

// EnrollOptions это вход регистрации.
type EnrollOptions struct {
	// Origin это origin регистрации. Игнорируется, когда задан Blob: origin
	// берётся из подписанного blob.
	Origin string
	// Code это код регистрации, 20 символов, дефисы косметические.
	Code string
	// LinkPin это продиктованный пин, 20 символов base32 Crockford.
	LinkPin string
	// Blob это кадр 0x05 или армированный поток. Необязателен.
	Blob []byte
	// SubscriptionDomain разрешает единственный переход.
	SubscriptionDomain string
	// AccountJWT непуст для второго и последующих устройств: тогда
	// используется маршрут enroll/device, а код не нужен.
	AccountJWT string
}

// Enroll выполняет поток 02-SPEC.md 9.5.
func (f *Fetcher) Enroll(ctx context.Context, opt EnrollOptions) error {
	f.mu.Lock()
	defer f.mu.Unlock()

	origin := opt.Origin
	code := opt.Code
	linkPin := strings.TrimSpace(opt.LinkPin)
	var pinBytes []byte
	if linkPin != "" {
		b, err := csm.Base32CrockfordDecode(linkPin)
		if err != nil || len(b) != csm.KeyIDTruncLen {
			return fmt.Errorf("%w: link_pin %q", ErrPinMismatchRoot, linkPin)
		}
		pinBytes = b
	}

	// Часы. Классификация делается ОДИН РАЗ, на регистрации, и отказ здесь это
	// отказ регистрации, а не тихий проход вслепую.
	nowUnix := f.now().Unix()
	be := BuildEpoch()
	if nowUnix < be || nowUnix > be+ClockPlausibleWindow {
		return fmt.Errorf("%w: часы %d вне окна [%d, %d]", ErrClockImplausible, nowUnix, be, be+ClockPlausibleWindow)
	}
	plausible := true

	var blobDoc *csm.BootstrapBlob
	if len(opt.Blob) > 0 {
		raw := opt.Blob
		if looksArmored(raw) {
			stream, err := csm.ArmorDecodeText(string(raw))
			if err != nil {
				return err
			}
			frames, err := csm.SplitFrameStream(stream)
			if err != nil {
				return err
			}
			if len(frames) == 0 {
				return fmt.Errorf("%w: пустой армированный поток", ErrPinMismatchRoot)
			}
			raw = frames[0]
		}
		ts := &csm.TrustState{LinkPin: pinBytes, Now: nowUnix, ClockTrusted: plausible, HWM: map[uint8]uint64{}}
		res, err := csm.Verify(raw, ts)
		if err != nil {
			return err
		}
		b, ok := res.Doc.(*csm.BootstrapBlob)
		if !ok {
			return fmt.Errorf("%w: кадр не bootstrap blob", ErrPinMismatchRoot)
		}
		// sha256(rk)[0..12] обязан совпасть с продиктованным пином. Blob, чей
		// rk не совпал, отвергается жёстко, и пути "всё равно продолжить" нет.
		if pinBytes != nil && !bytes.Equal(csm.KeyIDOf(b.RK), pinBytes) {
			return ErrPinMismatchRoot
		}
		if pinBytes == nil {
			pinBytes = csm.KeyIDOf(b.RK)
		}
		blobDoc = b
		origin = b.Org
		if code == "" {
			code = b.Code
		}
		f.ladder.SetReservePool(b.Mir)
		_ = f.store.PutFrame(FrameBootstrap, raw)
	}

	normOrigin, err := NormalizeOrigin(origin)
	if err != nil {
		return err
	}
	if len(pinBytes) != csm.KeyIDTruncLen {
		return fmt.Errorf("%w: пин не задан", ErrPinMismatchRoot)
	}

	// Шаг 3 и 4: ключевой документ по лестнице, первое доверие по link_pin.
	keyFrame, err := f.fetchKeyDocument(ctx, normOrigin, opt.SubscriptionDomain, pinBytes, nowUnix, plausible)
	if err != nil {
		return err
	}
	ts := &csm.TrustState{LinkPin: pinBytes, Now: nowUnix, ClockTrusted: plausible, HWM: map[uint8]uint64{}}
	res, err := csm.Verify(keyFrame, ts)
	if err != nil {
		return err
	}
	kd := res.Doc.(*csm.KeyDocument)
	f.anchor, f.anchorFrame = kd, keyFrame

	pid := kd.Env.PID
	name := ""
	if blobDoc != nil {
		name = blobDoc.Name
	}

	// Шаг 5: pid закрепляется, time_floor берётся из iat этого документа.
	if err := f.store.Update(func(s *State) {
		s.PID = hex.EncodeToString(pid)
		s.LinkPin = hex.EncodeToString(pinBytes)
		s.Origin = normOrigin
		s.SubscriptionDomain = opt.SubscriptionDomain
		s.OperatorName = name
		s.PinnedOutOfBand = blobDoc != nil
		s.EnrolledAt = nowUnix
		if int64(kd.Env.IAT) > s.TimeFloor {
			s.TimeFloor = int64(kd.Env.IAT)
		}
		s.SetHWM(csm.DocKey, kd.Env.Ver)
		s.KeyVer, s.KeyIAT, s.KeyExp = kd.Env.Ver, int64(kd.Env.IAT), int64(kd.Env.Exp)
	}); err != nil {
		return err
	}
	if err := f.store.PutFrame(FrameKey, keyFrame); err != nil {
		return err
	}

	// Шаги 6 и 7: ключи устройства и тело регистрации под X-CSM-Proof.
	spki, err := f.keys.SigningSPKI()
	if err != nil {
		return err
	}
	dtp := Thumbprint(spki)
	agree, err := f.keys.AgreementPublic()
	if err != nil {
		return err
	}
	nonce, err := f.newNonce()
	if err != nil {
		return err
	}
	pairs := []CBORPair{
		{Key: 1, Val: CBORUint(1)},
		{Key: 3, Val: CBORBstr(nonce)},
		{Key: 4, Val: CBORBstr(spki)},
		{Key: 5, Val: CBORBstr(agree)},
		{Key: 6, Val: CBORUint(uint64(f.keys.Tier()))},
		{Key: 7, Val: CBORUint(f.keys.Generation())},
	}
	path := PathEnrollDevice
	if opt.AccountJWT == "" {
		path = PathEnrollCode
		pairs = append(pairs, CBORPair{Key: 2, Val: CBORTstr(normalizeCode(code))})
	}
	body, err := EncodeCBOR(CBORMap(pairs...))
	if err != nil {
		return err
	}
	sig, err := f.keys.Sign(WriteProofPreImage(http.MethodPost, path, body))
	if err != nil {
		return err
	}

	if err := f.store.Update(func(s *State) { s.DTP = hex.EncodeToString(dtp) }); err != nil {
		return err
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, normOrigin+path, bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/cbor")
	req.Header.Set("X-CSM-Proof", ProofHeader(sig))
	req.ContentLength = int64(len(body))
	if opt.AccountJWT != "" {
		req.Header.Set("Authorization", "Bearer "+opt.AccountJWT)
	}

	st := f.store.State()
	var sealedFrame []byte
	resp, err := f.ladder.Do(ctx, req, DoOptions{
		Origin: normOrigin, SubscriptionDomain: opt.SubscriptionDomain, Force: true,
		// R2 и R3 отсутствуют намеренно: тело регистрации второго устройства
		// идёт под Authorization аккаунта, а подменяющая хост ступень отдала
		// бы этот токен хосту из пула зеркал. rewriteRequest ловит это ещё
		// раз, на границе запроса; здесь ступени просто не тратятся.
		AllowRungs: []RungID{R1Direct, R4Tunnel, R5Proxy},
		Verify: func(b []byte) error {
			ts := f.trustLocked(st)
			ts.Now, ts.ClockTrusted = nowUnix, plausible
			ts.ExpectedNonce, ts.DeviceDTP = nonce, dtp
			ts.Anchor = kd
			_, verr := csm.Verify(b, ts)
			return verr
		},
	})
	if err != nil {
		return err
	}
	sealedFrame = resp.Body
	if err := f.acceptDirectiveLocked(ctx, sealedFrame, nonce, dtp, resp.Rung); err != nil {
		return err
	}
	// Профиль переезжает из csm/pending в csm/<pid>: один профиль это один
	// каталог и одна отметка версии.
	if err := f.store.Rebind(f.workDir, hex.EncodeToString(pid)); err != nil {
		return err
	}
	if sk, ok := f.keys.(*SoftwareDeviceKeys); ok {
		if err := sk.Relocate(f.store.Dir()); err != nil {
			return err
		}
	}
	return nil
}

// normalizeCode снимает косметические дефисы кода регистрации.
func normalizeCode(s string) string {
	return strings.ToUpper(strings.ReplaceAll(strings.TrimSpace(s), "-", ""))
}

func looksArmored(b []byte) bool {
	return bytes.HasPrefix(bytes.TrimSpace(b), []byte("CARCAP1"))
}

// fetchKeyDocument берёт GET /sub/k1 по лестнице.
func (f *Fetcher) fetchKeyDocument(ctx context.Context, origin, subDomain string, pin []byte, now int64, clockOK bool) ([]byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, origin+"/sub/k1", nil)
	if err != nil {
		return nil, err
	}
	resp, err := f.ladder.Do(ctx, req, DoOptions{
		Origin: origin, SubscriptionDomain: subDomain, Force: true,
		Verify: func(b []byte) error {
			ts := &csm.TrustState{LinkPin: pin, Now: now, ClockTrusted: clockOK, HWM: map[uint8]uint64{}}
			_, verr := csm.Verify(b, ts)
			return verr
		},
	})
	if err != nil {
		return nil, err
	}
	return resp.Body, nil
}

// ---------------------------------------------------------------- обновление

// Refresh выполняет один цикл: директива, при необходимости каталог, при
// необходимости ключевой документ.
//
// Сетевой отказ здесь НЕ отменяет доверенное состояние. Ровно в этом смысл
// инварианта 16: профиль остаётся на кешированных документах и продолжает
// подключать, а отказ виден в диагностике как возраст конфигурации и её
// источник.
func (f *Fetcher) Refresh(ctx context.Context) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	st := f.store.State()
	if st.PID == "" || f.anchor == nil {
		return ErrNotEnrolled
	}
	if st.Locator == "" {
		return fmt.Errorf("%w: локатор неизвестен", ErrNotEnrolled)
	}

	nonce, err := f.newNonce()
	if err != nil {
		return err
	}
	dtp := st.DTPBytes()
	q := fmt.Sprintf("?n=%s&v=%d&d=%s",
		csm.Base32CrockfordEncode(nonce), st.DirectiveVer, csm.Base32CrockfordEncode(dtp))
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, st.Origin+"/sub/m1/"+st.Locator+q, nil)
	if err != nil {
		return err
	}

	// 02-SPEC.md 10.1 правило 1: при снятом бите 1 директиву нельзя брать ни с
	// какой ступени, кроме R1. Незапечатанная директива на зеркале отдала бы
	// ему отпечаток устройства, статус, выбор и счётчики трафика открытым
	// текстом.
	allow := []RungID(nil)
	if f.effectiveCapLocked()&(1<<1) == 0 {
		allow = []RungID{R0Cached, R1Direct}
	}

	opt := DoOptions{
		Origin: st.Origin, SubscriptionDomain: st.SubscriptionDomain, AllowRungs: allow,
		Verify: func(b []byte) error {
			ts := f.trustLocked(st)
			ts.ExpectedNonce, ts.DeviceDTP = nonce, dtp
			_, verr := csm.Verify(b, ts)
			return verr
		},
	}
	resp, err := f.ladder.Do(ctx, req, opt)
	if err != nil {
		return err
	}
	if resp.FromCache {
		// Кешированный кадр уже принят; повторно поднимать отметки не надо.
		return nil
	}
	return f.acceptDirectiveLocked(ctx, resp.Body, nonce, dtp, resp.Rung)
}

// acceptDirectiveLocked принимает проверенную директиву и подтягивает каталог.
func (f *Fetcher) acceptDirectiveLocked(ctx context.Context, frame, nonce, dtp []byte, rung RungID) error {
	st := f.store.State()
	ts := f.trustLocked(st)
	ts.ExpectedNonce, ts.DeviceDTP = nonce, dtp
	res, err := csm.Verify(frame, ts)
	if err != nil {
		return err
	}
	inner := res
	if res.Inner != nil {
		inner = res.Inner
	}
	dir, ok := inner.Doc.(*csm.Directive)
	if !ok {
		return fmt.Errorf("%w: восстановленный документ не директива", ErrStoreInconsistent)
	}

	// Часы становятся доверенными на первой проверенной директиве. Вместе с
	// оценкой запоминается монотонная точка, относительно которой она растёт,
	// и стенная опора, по которой замечается перевод часов назад.
	f.clockTrusted = true
	f.clockEstimate = int64(dir.Env.IAT)
	f.estimateAt = time.Now()
	f.monoRef = f.estimateAt
	f.wallRef = f.now().Unix()
	f.directive = dir
	if res.Inner != nil {
		f.dirFrame = res.InnerFrame
	} else {
		f.dirFrame = frame
	}

	if err := f.store.PutFrame(FrameSealed, frame); err != nil {
		return err
	}
	if err := f.store.PutFrame(FrameDirective, f.dirFrame); err != nil {
		return err
	}
	if err := f.store.Update(func(s *State) {
		if int64(dir.Env.IAT) > s.TimeFloor {
			s.TimeFloor = int64(dir.Env.IAT)
		}
		s.SetHWM(csm.DocDirective, dir.Env.Ver)
		s.DirectiveVer, s.DirectiveIAT, s.DirectiveExp = dir.Env.Ver, int64(dir.Env.IAT), int64(dir.Env.Exp)
		s.Cap = binary.BigEndian.Uint32(dir.Cap[:])
		s.FetchedAt = f.now().Unix()
		s.LastRung = rung
		if dir.Loc != "" {
			s.Locator = dir.Loc
		}
		if len(dir.Cat) == 32 {
			s.CatID = csm.CatalogID(dir.Cat)
		}
		if dir.St == 5 {
			s.Revoked = true
		}
		s.TTL, s.Jit = ClampTTL(dir.TTL, 0)
		s.ExpH = ClampExpH(dir.ExpH, 0)
	}); err != nil {
		return err
	}

	// Каталог тянем, только если директива назвала chash, которого у нас нет.
	if len(dir.Cat) == 32 && (f.catFrame == nil || !bytes.Equal(csm.CatalogHash(f.catFrame), dir.Cat)) {
		if err := f.fetchCatalogLocked(ctx, dir); err != nil {
			// Каталог не пришёл: директива уже принята, и на кешированном
			// каталоге профиль продолжает работать.
			return err
		}
	}
	f.applyCatalogLocked(f.store.State())
	return nil
}

// fetchCatalogLocked тянет фрагменты каталога и собирает его.
func (f *Fetcher) fetchCatalogLocked(ctx context.Context, dir *csm.Directive) error {
	st := f.store.State()
	catID := csm.CatalogID(dir.Cat)
	cn := dir.CN
	if cn == 0 {
		cn = 1
	}
	frames := make([][]byte, 0, cn)
	for i := uint64(0); i < cn; i++ {
		u := fmt.Sprintf("%s/sub/c1/%s/%d", st.Origin, catID, i)
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, u, nil)
		if err != nil {
			return err
		}
		req.Header.Set("X-CSM-Loc", st.Locator)
		idx := i
		resp, err := f.ladder.Do(ctx, req, DoOptions{
			Origin: st.Origin, SubscriptionDomain: st.SubscriptionDomain,
			// Фрагмент каталога живёт под своим потолком, а не под resp_max.
			MaxBody: csm.ChunkRespMax,
			Force:   true,
			Verify: func(b []byte) error {
				ts := f.trustLocked(st)
				cs := csm.NewChunkSetFor(cn)
				_, verr := cs.Add(b, ts)
				return verr
			},
		})
		if err != nil {
			return err
		}
		frames = append(frames, resp.Body)
		if err := f.store.PutChunk(catID, idx, resp.Body); err != nil {
			return err
		}
	}
	ts := f.trustLocked(st)
	ts.BoundCatHash = dir.Cat
	tier := dir.Tier
	ts.BoundTier = &tier
	res, err := csm.VerifyCatalogFromChunksCN(frames, cn, ts)
	if err != nil {
		return err
	}
	cat, ok := res.Doc.(*csm.Catalog)
	if !ok {
		return fmt.Errorf("%w: собранный документ не каталог", ErrStoreInconsistent)
	}
	// Зажим resp_max это отказ КАТАЛОГА, а не понижение значения: инвариант 5
	// фиксирует 4096, и поле может только опускать.
	thr, err := ClampThresholds(cat.Thr)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrCatalogRefused, err)
	}
	f.catalog, f.catFrame = cat, res.Frame.Raw
	f.ladder.SetThresholds(thr)
	if err := f.store.PutFrame(FrameCatalog, res.Frame.Raw); err != nil {
		return err
	}
	return f.store.Update(func(s *State) {
		s.SetHWM(csm.DocCatalog, cat.Env.Ver)
		s.CatalogVer, s.CatalogIAT, s.CatalogExp = cat.Env.Ver, int64(cat.Env.IAT), int64(cat.Env.Exp)
		s.CatID = catID
		s.Thr = thr
	})
}

// RefreshKeyDocument проходит цепочку ключевых документов от текущей версии.
func (f *Fetcher) RefreshKeyDocument(ctx context.Context) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	st := f.store.State()
	if st.PID == "" || f.anchor == nil {
		return ErrNotEnrolled
	}
	u := fmt.Sprintf("%s/sub/k1?since=%d", st.Origin, st.KeyVer)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, u, nil)
	if err != nil {
		return err
	}
	resp, err := f.ladder.Do(ctx, req, DoOptions{
		Origin: st.Origin, SubscriptionDomain: st.SubscriptionDomain,
		Verify: func(b []byte) error {
			frames, ferr := csm.SplitFrameStream(b)
			if ferr != nil {
				return ferr
			}
			if len(frames) == 0 {
				return fmt.Errorf("%w: пустой поток", ErrStoreInconsistent)
			}
			return nil
		},
	})
	if err != nil {
		return err
	}
	frames, err := csm.SplitFrameStream(resp.Body)
	if err != nil {
		return err
	}
	// Версии идут подряд и не пропускаются. Короткий поток это не ошибка:
	// повторяем запрос с новым since выше по стеку.
	for _, fr := range frames {
		ts := f.trustLocked(f.store.State())
		res, verr := csm.Verify(fr, ts)
		if verr != nil {
			return verr
		}
		kd, ok := res.Doc.(*csm.KeyDocument)
		if !ok {
			return fmt.Errorf("%w: кадр не ключевой документ", ErrStoreInconsistent)
		}
		newPin := ""
		if re, ok := kd.Roles[csm.RoleRoot]; ok && len(re.KS) == 1 {
			newPin = hex.EncodeToString(re.KS[0])
		}
		if err := f.store.PutFrame(FrameKey, fr); err != nil {
			return err
		}
		if err := f.store.Update(func(s *State) {
			s.SetHWM(csm.DocKey, kd.Env.Ver)
			s.KeyVer, s.KeyIAT, s.KeyExp = kd.Env.Ver, int64(kd.Env.IAT), int64(kd.Env.Exp)
			if newPin != "" && newPin != s.LinkPin {
				s.LinkPin = newPin
				s.RootChanged = true
			}
		}); err != nil {
			return err
		}
		// Якорь в памяти обновляется ПОСЛЕ обеих записей: иначе отказ диска
		// оставил бы процесс с якорем впереди сохранённого состояния до конца
		// своей жизни.
		f.anchor, f.anchorFrame = kd, fr
	}
	return nil
}

// ---------------------------------------------------------------- запись настроек

// SettingsWrite это запрос на изменение настроек.
type SettingsWrite struct {
	// Want это карта pol: ключ поля к значению. Отсутствующий ключ означает
	// "без изменений", текст "default" означает сброс к умолчанию оператора.
	Want map[uint64]string
	// Sel это карта sel.
	Sel map[uint64]string
	// AccountJWT обязателен: маршрут живёт в защищённой группе.
	AccountJWT string
}

// RequestSettings отправляет PUT /api/v2/app/preferences и принимает
// подписанный и запечатанный ответ как новую директиву.
//
// Изменение проходит по той же лестнице, что и всё остальное: управляющий слой
// на Dart своих сокетов к оператору не открывает.
func (f *Fetcher) RequestSettings(ctx context.Context, w SettingsWrite) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	st := f.store.State()
	if st.PID == "" || f.anchor == nil {
		return ErrNotEnrolled
	}
	if f.effectiveCapLocked()&(1<<3) == 0 {
		return fmt.Errorf("transport: запись настроек не предлагается оператором")
	}
	nonce, err := f.newNonce()
	if err != nil {
		return err
	}
	dtp := st.DTPBytes()
	wantPairs := make([]CBORPair, 0, len(w.Want))
	for k, v := range w.Want {
		wantPairs = append(wantPairs, CBORPair{Key: k, Val: CBORTstr(v)})
	}
	selPairs := make([]CBORPair, 0, len(w.Sel))
	for k, v := range w.Sel {
		selPairs = append(selPairs, CBORPair{Key: k, Val: CBORTstr(v)})
	}
	pairs := []CBORPair{
		{Key: 1, Val: CBORUint(1)},
		{Key: 2, Val: CBORBstr(nonce)},
		{Key: 3, Val: CBORBstr(dtp)},
		{Key: 4, Val: CBORMap(wantPairs...)},
	}
	if len(selPairs) > 0 {
		pairs = append(pairs, CBORPair{Key: 5, Val: CBORMap(selPairs...)})
	}
	body, err := EncodeCBOR(CBORMap(pairs...))
	if err != nil {
		return err
	}
	sig, err := f.keys.Sign(WriteProofPreImage(http.MethodPut, PathPreferences, body))
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, st.Origin+PathPreferences, bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/cbor")
	req.Header.Set("X-CSM-Proof", ProofHeader(sig))
	req.Header.Set("If-Match", strconv.FormatUint(st.DirectiveVer, 10))
	req.ContentLength = int64(len(body))
	if w.AccountJWT != "" {
		req.Header.Set("Authorization", "Bearer "+w.AccountJWT)
	}
	resp, err := f.ladder.Do(ctx, req, DoOptions{
		Origin: st.Origin, SubscriptionDomain: st.SubscriptionDomain, Force: true,
		// Запись настроек всегда несёт токен аккаунта: маршрут живёт в
		// защищённой группе. Ступени, подменяющие хост, поэтому исключены.
		AllowRungs: []RungID{R1Direct, R4Tunnel, R5Proxy},
		Verify: func(b []byte) error {
			ts := f.trustLocked(st)
			ts.ExpectedNonce, ts.DeviceDTP = nonce, dtp
			_, verr := csm.Verify(b, ts)
			return verr
		},
	})
	if err != nil {
		return err
	}
	return f.acceptDirectiveLocked(ctx, resp.Body, nonce, dtp, resp.Rung)
}
