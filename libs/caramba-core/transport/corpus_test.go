package transport

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

// Харнесс общего корпуса CSM/1 для транспортного слоя.
//
// Корпус лежит в дереве клиента и НЕ копируется сюда: один корпус обслуживает
// все три реализации, и копия немедленно разошлась бы с оригиналом. Раздел
// transport файла vectors.json это восемь утверждений именно об этом слое, и
// каждое обязано быть проверено. Неизвестный идентификатор в корпусе это
// провал харнесса, а не пропуск: харнесс, который что-то пропускает,
// перестаёт быть merge gate.
const corpusRel = "../../../apps/caramba-client/docs/protocol/05-TEST-VECTORS"

type transportVector struct {
	ID      string `json:"id"`
	Subject string `json:"subject"`
	Verdict string `json:"verdict"`
	Code    string `json:"code"`
	Rule    string `json:"rule"`
}

type corpusContext struct {
	Anchor    string            `json:"anchor"`
	PinnedPID string            `json:"pinned_pid"`
	LinkPin   string            `json:"link_pin"`
	Now       int64             `json:"now"`
	TimeFloor int64             `json:"time_floor"`
	Nonce     string            `json:"expected_nonce"`
	DeviceDTP string            `json:"device_dtp"`
	AgreeSK   string            `json:"device_agreement_sk"`
	HWM       map[string]uint64 `json:"hwm"`
}

type corpusFile struct {
	Transport []transportVector        `json:"transport"`
	Contexts  map[string]corpusContext `json:"contexts"`
}

func loadCorpus(t *testing.T) corpusFile {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(corpusRel, "vectors.json"))
	if err != nil {
		t.Fatalf("корпус не читается: %v", err)
	}
	var c corpusFile
	if err := json.Unmarshal(b, &c); err != nil {
		t.Fatalf("корпус не разбирается: %v", err)
	}
	if len(c.Transport) == 0 {
		t.Fatal("в корпусе нет раздела transport")
	}
	return c
}

func fixture(t *testing.T, rel string) []byte {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(corpusRel, rel))
	if err != nil {
		t.Fatalf("фикстура %s: %v", rel, err)
	}
	return b
}

func mustHex(t *testing.T, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatalf("hex %q: %v", s, err)
	}
	return b
}

// TestCorpusTransport проходит раздел transport корпуса целиком.
func TestCorpusTransport(t *testing.T) {
	c := loadCorpus(t)
	seen := map[string]bool{}
	for _, v := range c.Transport {
		v := v
		t.Run(v.ID, func(t *testing.T) {
			seen[v.ID] = true
			if v.Verdict != "reject" {
				t.Fatalf("вектор %s ожидает вердикт %q, харнесс знает только reject", v.ID, v.Verdict)
			}
			switch v.ID {
			case "tr-content-encoding":
				h := http.Header{}
				h.Set("Content-Encoding", "gzip")
				resp := &http.Response{StatusCode: 200, Header: h, Body: io.NopCloser(strings.NewReader("x"))}
				if _, _, _, err := readCSMResponse(resp, DefaultRespMax, nil, "", true); !errors.Is(err, ErrContentEncoding) {
					t.Fatalf("%s: ответ с Content-Encoding принят: %v", v.ID, err)
				}

			case "tr-body-over-resp-max":
				body := bytes.Repeat([]byte{0x41}, 4097)
				resp := &http.Response{StatusCode: 200, Header: http.Header{}, Body: io.NopCloser(bytes.NewReader(body))}
				if _, _, _, err := readCSMResponse(resp, DefaultRespMax, nil, "", true); !errors.Is(err, ErrBodyTooLarge) {
					t.Fatalf("%s: тело 4097 байт принято: %v", v.ID, err)
				}

			case "tr-chunk-body-over-cap":
				body := bytes.Repeat([]byte{0x41}, 3585)
				resp := &http.Response{StatusCode: 200, Header: http.Header{}, Body: io.NopCloser(bytes.NewReader(body))}
				if _, _, _, err := readCSMResponse(resp, csm.ChunkRespMax, nil, "", true); !errors.Is(err, ErrBodyTooLarge) {
					t.Fatalf("%s: фрагмент 3585 байт принят: %v", v.ID, err)
				}

			case "tr-payload-len-over-cap":
				// Заголовок заявляет payload_len 49153. Отказ обязан наступить
				// НА ЗАГОЛОВКЕ, до выделения памяти по заявлению отвечающего:
				// на вход подаётся только 7 байт головы.
				raw := append([]byte{'C', 'S', 'M', '1', csm.DocDirective, 0xc0, 0x01, 0x01},
					bytes.Repeat([]byte{0x00}, 76)...)
				_, _, err := csm.Parse(raw)
				assertCode(t, v, err)

			case "tr-armor-stream-over-cap":
				stream := bytes.Repeat([]byte{0x00}, 65537)
				_, err := csm.SplitFrameStream(stream)
				assertCode(t, v, err)

			case "tr-armor-chunk-count":
				line := "CARCAP1 XXXXXXXX 1/107 " + strings.Repeat("A", 8)
				if _, err := csm.ArmorDecode([]string{line}); err == nil {
					t.Fatalf("%s: армированный набор с n = 107 принят", v.ID)
				}

			case "tr-redirect-cross-origin":
				from, _ := url.Parse("https://panel.example.net/sub/k1")
				to, _ := url.Parse("https://other.example.org/sub/k1")
				if err := CheckRedirect(from, to, 0, "sub.example.net"); !errors.Is(err, ErrRedirectRefused) {
					t.Fatalf("%s: переход на чужой хост принят: %v", v.ID, err)
				}

			case "tr-http-scheme":
				for _, u := range []string{
					"http://panel.example.net/sub/k1",
					"http://panel.example.net/rulesets/direct.yaml",
					"http://panel.example.net/geo/geoip.dat",
				} {
					if err := CheckFetchURLString(u); !errors.Is(err, ErrSchemeNotTLS) {
						t.Fatalf("%s: %s принят: %v", v.ID, u, err)
					}
				}
				// Единственное не-TLS исключение.
				if err := CheckFetchURLString("http://vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd.onion/sub/k1"); err != nil {
					t.Fatalf("%s: onion по http отвергнут: %v", v.ID, err)
				}

			default:
				t.Fatalf("вектор %s не покрыт харнессом: пропуск делает корпус не merge gate", v.ID)
			}
		})
	}
	for _, v := range c.Transport {
		if !seen[v.ID] {
			t.Fatalf("вектор %s не выполнялся", v.ID)
		}
	}
}

// assertCode сверяет код причины из реестра 03-WIRE.md 6.6, когда корпус его
// называет. Согласия в том, что вход отвергнут, недостаточно: два верификатора,
// падающие по разным причинам, это способ спрятать настоящее расхождение.
func assertCode(t *testing.T, v transportVector, err error) {
	t.Helper()
	if err == nil {
		t.Fatalf("%s: вход принят, ожидался отказ", v.ID)
	}
	if v.Code == "" {
		return
	}
	if got := string(csm.CodeOf(err)); got != v.Code {
		t.Fatalf("%s: код %q, ожидался %q (%v)", v.ID, got, v.Code, err)
	}
}

// seedStore готовит хранилище по контексту корпуса.
func seedStore(t *testing.T, ctx corpusContext, hwmOverride map[string]uint64) (*Store, string) {
	t.Helper()
	dir := t.TempDir()
	st, err := OpenStore(dir, ctx.PinnedPID)
	if err != nil {
		t.Fatalf("хранилище: %v", err)
	}
	pin, err := csm.Base32CrockfordDecode(ctx.LinkPin)
	if err != nil {
		t.Fatalf("link_pin: %v", err)
	}
	hwm := ctx.HWM
	if hwmOverride != nil {
		hwm = hwmOverride
	}
	if err := st.Update(func(s *State) {
		s.PID = ctx.PinnedPID
		s.LinkPin = hex.EncodeToString(pin)
		s.Origin = "https://panel.example.net"
		s.TimeFloor = ctx.TimeFloor
		s.DTP = ctx.DeviceDTP
		s.HWM = map[string]uint64{}
		for k, v := range hwm {
			s.HWM[k] = v
		}
	}); err != nil {
		t.Fatalf("посев состояния: %v", err)
	}
	return st, dir
}

// TestCorpusCachedAnchorLoads: сохранённый ключевой документ перепроверяется
// при загрузке против закреплённого link_pin, а не принимается на веру.
func TestCorpusCachedAnchorLoads(t *testing.T) {
	c := loadCorpus(t)
	ctx := c.Contexts["default"]
	st, dir := seedStore(t, ctx, nil)
	anchor := fixture(t, "bin/positive/k1_min.bin")
	if err := st.PutFrame(FrameKey, anchor); err != nil {
		t.Fatalf("сохранение якоря: %v", err)
	}

	l := NewLadder(newFakeExchange())
	f, err := NewFetcher(dir, ctx.PinnedPID, l, nil)
	if err != nil {
		t.Fatalf("выборщик: %v", err)
	}
	f.SetClock(func() time.Time { return time.Unix(ctx.Now, 0) })
	if err := f.LoadCached(); err != nil {
		t.Fatalf("загрузка кеша: %v", err)
	}
	snap := f.Snapshot()
	if !snap.Enrolled || !snap.Key.Present {
		t.Fatalf("якорь не поднялся: %+v", snap.Key)
	}
	if snap.RootFingerprint == "" {
		t.Fatal("отпечаток корня пуст, инвариант 18 не выполнен")
	}

	// Подделанный кадр обязан провалить проверку ПРИ ЗАГРУЗКЕ. Кеш хранит
	// кадры, а не разобранное состояние, ровно ради этого.
	bad := append([]byte(nil), anchor...)
	bad[len(bad)-1] ^= 0xff
	if err := st.PutFrame(FrameKey, bad); err != nil {
		t.Fatalf("сохранение подделки: %v", err)
	}
	f2, err := NewFetcher(dir, ctx.PinnedPID, NewLadder(newFakeExchange()), nil)
	if err != nil {
		t.Fatalf("выборщик: %v", err)
	}
	f2.SetClock(func() time.Time { return time.Unix(ctx.Now, 0) })
	err = f2.LoadCached()
	if err == nil {
		t.Fatal("подделанный кешированный кадр принят при загрузке")
	}
	if code := csm.CodeOf(err); code != csm.EVerifySig {
		t.Fatalf("код %q, ожидался %q", code, csm.EVerifySig)
	}
}

// TestCorpusCatalogChunksAndHashMismatch: фрагменты каталога собираются и
// проверяются целиком, а несовпадение cat из директивы это отказ каталога.
func TestCorpusCatalogChunksAndHashMismatch(t *testing.T) {
	c := loadCorpus(t)
	ctx := c.Contexts["default"]
	anchorFrame := fixture(t, "bin/positive/k1_min.bin")
	anchor, _, err := csm.ParseKeyDocument(anchorFrame)
	if err != nil {
		t.Fatalf("якорь: %v", err)
	}
	chunks := [][]byte{
		fixture(t, "bin/positive/c1c_typ_0.bin"),
		fixture(t, "bin/positive/c1c_typ_1.bin"),
	}
	catFrame := fixture(t, "bin/positive/c1_typical.bin")
	chash := csm.CatalogHash(catFrame)

	base := func() *csm.TrustState {
		return &csm.TrustState{
			PinnedPID:    mustHex(t, ctx.PinnedPID),
			Anchor:       anchor,
			Now:          ctx.Now,
			ClockTrusted: true,
			TimeFloor:    ctx.TimeFloor,
			HWM:          map[uint8]uint64{1: 1, 2: 0, 3: 411, 4: 0},
		}
	}

	ts := base()
	ts.BoundCatHash = chash
	res, err := csm.VerifyCatalogFromChunks(chunks, ts)
	if err != nil {
		t.Fatalf("сборка каталога из фрагментов: %v", err)
	}
	cat, ok := res.Doc.(*csm.Catalog)
	if !ok {
		t.Fatal("собранный документ не каталог")
	}
	if !bytes.Equal(res.Frame.Raw, catFrame) {
		t.Fatal("собранные байты не совпали с эталонным кадром каталога")
	}

	// Инвариант 12 на уровне каталога: cat из директивы не совпал, каталог
	// отвергается, а не подгоняется.
	ts = base()
	wrong := append([]byte(nil), chash...)
	wrong[0] ^= 0xff
	ts.BoundCatHash = wrong
	_, err = csm.VerifyCatalogFromChunks(chunks, ts)
	if code := csm.CodeOf(err); code != csm.EVerifyCatHash {
		t.Fatalf("код %q, ожидался %q (%v)", code, csm.EVerifyCatHash, err)
	}

	// Пороги каталога проходят зажим, а не применяются как есть.
	thr, err := ClampThresholds(cat.Thr)
	if err != nil {
		t.Fatalf("зажим порогов каталога: %v", err)
	}
	if thr.RespMax > DefaultRespMax || thr.ConnBytes > ConnBytesCeiling || thr.ConnPackets > ConnPacketsCeiling {
		t.Fatalf("зажатые пороги вне потолков: %+v", thr)
	}
}

// TestCorpusStaleButLive: каталог, выпущенный за 20 дней до временного пола и
// живой ещё 10 дней, ПРИНИМАЕТСЯ. Это тот вектор, который различает V11 в
// буквальном чтении 03-WIRE.md и его исправленную форму из 02-SPEC.md.
func TestCorpusStaleButLive(t *testing.T) {
	c := loadCorpus(t)
	ctx := c.Contexts["default"]
	anchorFrame := fixture(t, "bin/positive/k1_min.bin")
	anchor, _, err := csm.ParseKeyDocument(anchorFrame)
	if err != nil {
		t.Fatalf("якорь: %v", err)
	}
	stale := fixture(t, "bin/positive/c1_stale_but_live.bin")

	ex := newFakeExchange()
	ex.bodies[R1Direct] = stale
	l := NewLadder(ex)
	l.SetRandSource(5)

	var verifyErr error
	resp, err := l.Do(context.Background(), newRequest(t, "https://panel.example.net/sub/c1/x/0"), DoOptions{
		Origin: "https://panel.example.net",
		Verify: func(b []byte) error {
			ts := &csm.TrustState{
				PinnedPID:    mustHex(t, ctx.PinnedPID),
				Anchor:       anchor,
				Now:          ctx.Now,
				ClockTrusted: true,
				TimeFloor:    ctx.TimeFloor,
				HWM:          map[uint8]uint64{2: 0},
			}
			_, verifyErr = csm.Verify(b, ts)
			return verifyErr
		},
	})
	if err != nil {
		t.Fatalf("устаревший но живой каталог отвергнут: %v (%v)", err, verifyErr)
	}
	if resp.Rung != R1Direct {
		t.Fatalf("ступень %v", resp.Rung)
	}
}

// TestCorpusStaleCachedStillConnects: сеть недоступна целиком, и профиль
// обязан продолжать работать на последних хороших документах. Инвариант 16,
// абсолютный: истёкший документ по-прежнему подключает.
func TestCorpusStaleCachedStillConnects(t *testing.T) {
	c := loadCorpus(t)
	ctx := c.Contexts["default"]
	st, dir := seedStore(t, ctx, nil)
	if err := st.PutFrame(FrameKey, fixture(t, "bin/positive/k1_min.bin")); err != nil {
		t.Fatalf("якорь: %v", err)
	}

	ex := newFakeExchange() // ни одна ступень не отвечает
	l := NewLadder(ex)
	l.SetRandSource(9)
	f, err := NewFetcher(dir, ctx.PinnedPID, l, nil)
	if err != nil {
		t.Fatalf("выборщик: %v", err)
	}
	// Часы уходят далеко вперёд: документы истекли по любому счёту.
	f.SetClock(func() time.Time { return time.Unix(ctx.Now+40*86400, 0) })
	l.SetClock(func() time.Time { return time.Unix(ctx.Now+40*86400, 0) })
	if err := f.LoadCached(); err != nil {
		t.Fatalf("загрузка кеша: %v", err)
	}

	// Обновление проваливается.
	if err := f.Refresh(context.Background()); err == nil {
		t.Fatal("обновление без сети прошло")
	}
	// И доверенное состояние ОСТАЛОСЬ на месте.
	snap := f.Snapshot()
	if !snap.Enrolled || !snap.Key.Present {
		t.Fatal("сетевой отказ снёс доверенное состояние, инвариант 16 нарушен")
	}
	if !snap.Stale {
		t.Fatal("состояние не помечено как работа на кеше, инвариант 21 не выполнен")
	}
	if snap.Source == "" {
		t.Fatal("источник конфигурации не назван, инвариант 21 не выполнен")
	}
	// Ступень R0 продолжает отдавать сохранённый кадр.
	body, ok := f.Lookup(newRequest(t, "https://panel.example.net/sub/k1"))
	if !ok || len(body) == 0 {
		t.Fatal("ступень R0 перестала отдавать последний хороший документ")
	}
}

// TestResourceHashRefusal: инвариант 12. Файл, чей sha256 не совпал с
// подписанным каталогом, не применяется.
func TestResourceHashRefusal(t *testing.T) {
	c := loadCorpus(t)
	ctx := c.Contexts["default"]
	anchorFrame := fixture(t, "bin/positive/k1_min.bin")
	anchor, _, err := csm.ParseKeyDocument(anchorFrame)
	if err != nil {
		t.Fatalf("якорь: %v", err)
	}
	catFrame := fixture(t, "bin/positive/c1_typical.bin")
	res, err := csm.Verify(catFrame, &csm.TrustState{
		PinnedPID:    mustHex(t, ctx.PinnedPID),
		Anchor:       anchor,
		Now:          ctx.Now,
		ClockTrusted: true,
		TimeFloor:    ctx.TimeFloor,
		HWM:          map[uint8]uint64{2: 0},
	})
	if err != nil {
		t.Fatalf("каталог: %v", err)
	}
	cat := res.Doc.(*csm.Catalog)
	if len(cat.RS) == 0 {
		t.Fatal("в типичном каталоге нет записей rs")
	}
	g := NewResourceGuard(cat, 1<<CapResourceHashes)
	if !g.Enabled() {
		t.Fatal("страж ресурсов выключен при выставленном бите 6")
	}
	name := cat.RS[0].Name
	if err := g.Check(name, []byte("не тот файл")); !errors.Is(err, ErrResourceHash) {
		t.Fatalf("файл с чужим sha256 принят: %v", err)
	}
	if err := g.Check("никому не известный", []byte("x")); !errors.Is(err, ErrResourceUnknown) {
		t.Fatalf("ресурс вне каталога принят: %v", err)
	}
	// Снятый бит 6 запрещает загрузку целиком: отказ загружать это безопасное
	// направление, потому что каталог без хешей не может удовлетворить
	// инвариант 12.
	off := NewResourceGuard(cat, 0)
	if err := off.Check(name, []byte("x")); !errors.Is(err, ErrResourcesDisabled) {
		t.Fatalf("при снятом бите 6 загрузка разрешена: %v", err)
	}
}
