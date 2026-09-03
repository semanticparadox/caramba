package csm

import (
	"bytes"
	"crypto/ecdh"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
)

// Харнесс общего корпуса CSM/1. Корпус лежит в дереве клиента и НЕ копируется
// сюда: один корпус обслуживает все три реализации, и копия немедленно
// разошлась бы с оригиналом.
//
// Пропуск вектора это провал: харнесс, который что-то пропускает, перестаёт
// быть merge gate.
const corpusRel = "../../../apps/caramba-client/docs/protocol/05-TEST-VECTORS"

// ------------------------------------------------------------ модель vectors.json

type corpusCtx struct {
	Anchor      string            `json:"anchor"`
	PinnedPID   string            `json:"pinned_pid"`
	LinkPin     string            `json:"link_pin"`
	Now         int64             `json:"now"`
	TimeFloor   int64             `json:"time_floor"`
	Nonce       string            `json:"expected_nonce"`
	DeviceDTP   string            `json:"device_dtp"`
	DeviceAgree string            `json:"device_agreement_sk"`
	HWM         map[string]uint64 `json:"hwm"`
	StoredFrame string            `json:"stored_frame"`
	BoundCat    string            `json:"bound_cat"`
	BoundTier   *uint64           `json:"bound_tier"`
	Note        string            `json:"note"`
}

type corpusVec struct {
	ID      string     `json:"id"`
	Group   string     `json:"group"`
	File    string     `json:"file"`
	DocType int        `json:"doc_type"`
	Bytes   int        `json:"bytes"`
	SHA256  string     `json:"sha256"`
	Verdict string     `json:"verdict"`
	Code    string     `json:"code"`
	Step    string     `json:"step"`
	Context string     `json:"context"`
	Over    *corpusCtx `json:"context_override"`
	Note    string     `json:"note"`
}

type corpusArmor struct {
	ID      string   `json:"id"`
	File    string   `json:"file"`
	Frames  int      `json:"frames"`
	Stream  int      `json:"stream_bytes"`
	Chunks  int      `json:"chunks"`
	BID     string   `json:"bid"`
	Verdict string   `json:"verdict"`
	Code    string   `json:"code"`
	Carries []string `json:"carries"`
	Note    string   `json:"note"`
}

type corpusKeyVec struct {
	ID        string `json:"id"`
	PublicKey string `json:"public_key"`
	Verdict   string `json:"verdict"`
	Clause    string `json:"clause"`
	Note      string `json:"note"`
}

type corpusSigVec struct {
	ID        string `json:"id"`
	PublicKey string `json:"public_key"`
	Message   string `json:"message_hex"`
	Signature string `json:"signature"`
	Verdict   string `json:"verdict"`
	Clause    string `json:"clause"`
	Note      string `json:"note"`
}

type rfc9180Vector struct {
	Mode      int    `json:"mode"`
	KemID     int    `json:"kem_id"`
	KdfID     int    `json:"kdf_id"`
	AeadID    int    `json:"aead_id"`
	Info      string `json:"info"`
	SkRm      string `json:"skRm"`
	PkRm      string `json:"pkRm"`
	Enc       string `json:"enc"`
	Shared    string `json:"shared_secret"`
	KSC       string `json:"key_schedule_context"`
	Secret    string `json:"secret"`
	Key       string `json:"key"`
	BaseNonce string `json:"base_nonce"`
	Exporter  string `json:"exporter_secret"`
	Seq0PT    string `json:"seq0_pt"`
	Seq0AAD   string `json:"seq0_aad"`
	Seq0Nonce string `json:"seq0_nonce"`
	Seq0CT    string `json:"seq0_ct"`
	Seq1AAD   string `json:"seq1_aad"`
	Seq1Nonce string `json:"seq1_nonce"`
	Seq1CT    string `json:"seq1_ct"`
}

type corpusHPKE struct {
	AADFixture  string         `json:"aad_fixture"`
	Info        string         `json:"info"`
	InfoASCII   string         `json:"info_ascii"`
	RecipientPK string         `json:"recipient_pk"`
	RecipientSK string         `json:"recipient_sk"`
	RKV         uint64         `json:"rkv"`
	Suite       map[string]any `json:"suite"`
	RFC9180     rfc9180Vector  `json:"rfc9180_key_schedule_vector"`
}

type corpusDerivation struct {
	Name   string `json:"name"`
	Input  string `json:"input"`
	Output string `json:"output"`
	Rule   string `json:"rule"`
}

type publishedDigest struct {
	Document string `json:"document"`
	File     string `json:"file"`
	Bytes    int    `json:"bytes"`
	Expected string `json:"expected_by_wire_15"`
	Actual   string `json:"actual"`
	Match    bool   `json:"match"`
}

type corpusFile struct {
	CorpusVersion int                  `json:"corpus_version"`
	FixtureKeys   map[string]string    `json:"fixture_keys"`
	Contexts      map[string]corpusCtx `json:"contexts"`
	Digests       []publishedDigest    `json:"published_digest_check"`
	ArmorCheck    map[string]any       `json:"published_armor_check"`
	Vectors       []corpusVec          `json:"vectors"`
	Armor         []corpusArmor        `json:"armor"`
	KeyIngest     []corpusKeyVec       `json:"ed25519_public_key_ingest"`
	SigVectors    []corpusSigVec       `json:"ed25519_signature"`
	HPKE          corpusHPKE           `json:"hpke"`
	Derivations   []corpusDerivation   `json:"derivations"`
	Counts        map[string]int       `json:"counts"`
	Registry      map[string][]string  `json:"error_code_registry"`
}

var (
	corpusOnce sync.Once
	corpusData *corpusFile
	corpusErr  error
)

func loadCorpus(t *testing.T) *corpusFile {
	t.Helper()
	corpusOnce.Do(func() {
		b, err := os.ReadFile(filepath.Join(corpusRel, "vectors.json"))
		if err != nil {
			corpusErr = err
			return
		}
		var c corpusFile
		if err := json.Unmarshal(b, &c); err != nil {
			corpusErr = err
			return
		}
		corpusData = &c
	})
	if corpusErr != nil {
		t.Fatalf("csm: cannot load the shared corpus at %s: %v", corpusRel, corpusErr)
	}
	return corpusData
}

func mustHex(t *testing.T, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatalf("bad hex %q: %v", s, err)
	}
	return b
}

func corpusRead(t *testing.T, rel string) []byte {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(corpusRel, rel))
	if err != nil {
		t.Fatalf("cannot read fixture %s: %v", rel, err)
	}
	return b
}

// ------------------------------------------------------------ построение состояния

func buildTrustState(t *testing.T, c *corpusFile, v corpusVec) *TrustState {
	t.Helper()
	base, ok := c.Contexts[v.Context]
	if !ok {
		t.Fatalf("vector %s names context %q which does not exist", v.ID, v.Context)
	}

	// context_override заменяет названные поля базового контекста.
	hwm := map[string]uint64{}
	for k, val := range base.HWM {
		hwm[k] = val
	}
	storedFrame := base.StoredFrame
	boundCat := base.BoundCat
	boundTier := base.BoundTier
	if v.Over != nil {
		for k, val := range v.Over.HWM {
			hwm[k] = val
		}
		if v.Over.StoredFrame != "" {
			storedFrame = v.Over.StoredFrame
		}
		if v.Over.Anchor != "" {
			base.Anchor = v.Over.Anchor
		}
		if v.Over.Now != 0 {
			base.Now = v.Over.Now
		}
		if v.Over.TimeFloor != 0 {
			base.TimeFloor = v.Over.TimeFloor
		}
		if v.Over.BoundCat != "" {
			boundCat = v.Over.BoundCat
		}
		if v.Over.BoundTier != nil {
			boundTier = v.Over.BoundTier
		}
	}

	st := &TrustState{
		PinnedPID:    mustHex(t, base.PinnedPID),
		Now:          base.Now,
		ClockTrusted: true,
		TimeFloor:    base.TimeFloor,
		HWM:          map[uint8]uint64{},
	}
	if base.LinkPin != "" {
		pin, err := Base32CrockfordDecode(base.LinkPin)
		if err != nil {
			t.Fatalf("context %s: bad link_pin: %v", v.Context, err)
		}
		st.LinkPin = pin
	}
	if base.Nonce != "" {
		st.ExpectedNonce = mustHex(t, base.Nonce)
	}
	if base.DeviceDTP != "" {
		st.DeviceDTP = mustHex(t, base.DeviceDTP)
	}
	if base.DeviceAgree != "" {
		st.AgreementKeys = map[uint64][]byte{1: mustHex(t, base.DeviceAgree)}
	}
	for k, val := range hwm {
		n, err := strconv.ParseUint(k, 10, 8)
		if err != nil {
			t.Fatalf("context %s: bad hwm key %q", v.Context, k)
		}
		st.HWM[uint8(n)] = val
	}
	if base.Anchor != "" {
		kd, _, err := ParseKeyDocument(corpusRead(t, base.Anchor))
		if err != nil {
			t.Fatalf("context %s: anchor %s does not parse: %v", v.Context, base.Anchor, err)
		}
		st.Anchor = kd
	}
	if storedFrame != "" {
		st.StoredFrame = corpusRead(t, storedFrame)
	}
	// cat и tier доверенной директивы приходят полями контекста, а не из
	// текста заметки: шаги V14a и V14b читают их из ранее доверенного
	// документа, и харнесс обязан подставлять их так же явно.
	if boundCat != "" {
		sum := sha256.Sum256(corpusRead(t, boundCat))
		st.BoundCatHash = sum[:]
	}
	st.BoundTier = boundTier
	return st
}

// ------------------------------------------------------------ сами тесты

func TestCorpusVectors(t *testing.T) {
	c := loadCorpus(t)
	if len(c.Vectors) != c.Counts["vectors"] {
		t.Fatalf("corpus declares %d vectors, the array holds %d", c.Counts["vectors"], len(c.Vectors))
	}

	exercised := 0
	for _, v := range c.Vectors {
		v := v
		t.Run(v.ID, func(t *testing.T) {
			raw := corpusRead(t, v.File)
			if len(raw) != v.Bytes {
				t.Fatalf("%s: file is %d bytes, vectors.json says %d", v.File, len(raw), v.Bytes)
			}
			sum := sha256.Sum256(raw)
			if got := hex.EncodeToString(sum[:]); got != v.SHA256 {
				t.Fatalf("%s: sha256 %s, vectors.json says %s", v.File, got, v.SHA256)
			}

			st := buildTrustState(t, c, v)
			// Проверка идёт по копии, отданной как есть: пакет не имеет права
			// изменить входной буфер.
			input := append([]byte(nil), raw...)
			_, err := Verify(input, st)
			if !bytes.Equal(input, raw) {
				t.Fatalf("%s: Verify mutated its input buffer", v.ID)
			}

			switch v.Verdict {
			case "accept":
				if err != nil {
					t.Fatalf("%s: expected accept, got %v (step %s)\nnote: %s",
						v.ID, err, StepOf(err), v.Note)
				}
			case "reject":
				if err == nil {
					t.Fatalf("%s: expected reject with %s, got accept\nnote: %s", v.ID, v.Code, v.Note)
				}
				got := CodeOf(err)
				if string(got) != v.Code {
					t.Fatalf("%s: expected %s at %s, got %s at %s (%v)\nnote: %s",
						v.ID, v.Code, v.Step, got, StepOf(err), err, v.Note)
				}
			default:
				t.Fatalf("%s: unknown verdict %q", v.ID, v.Verdict)
			}
		})
		exercised++
	}
	if exercised != len(c.Vectors) {
		t.Fatalf("exercised %d of %d vectors; a skipped entry is a failure", exercised, len(c.Vectors))
	}
}

func TestCorpusArmor(t *testing.T) {
	c := loadCorpus(t)
	if len(c.Armor) != c.Counts["armor"] {
		t.Fatalf("corpus declares %d armor sets, the array holds %d", c.Counts["armor"], len(c.Armor))
	}
	for _, a := range c.Armor {
		a := a
		t.Run(a.ID, func(t *testing.T) {
			text := string(corpusRead(t, a.File))
			stream, err := ArmorDecodeText(text)
			switch a.Verdict {
			case "accept":
				if err != nil {
					t.Fatalf("%s: expected accept, got %v\nnote: %s", a.ID, err, a.Note)
				}
				if len(stream) != a.Stream {
					t.Fatalf("%s: decoded %d bytes, corpus says %d", a.ID, len(stream), a.Stream)
				}
				if got := BundleID(stream); got != a.BID {
					t.Fatalf("%s: bid %s, corpus says %s", a.ID, got, a.BID)
				}
				frames, err := SplitFrameStream(stream)
				if err != nil {
					t.Fatalf("%s: frame stream is not walkable: %v", a.ID, err)
				}
				if len(frames) != a.Frames {
					t.Fatalf("%s: walked %d frames, corpus says %d", a.ID, len(frames), a.Frames)
				}
				// Каждый кадр потока обязан разобраться сам по себе.
				for i, f := range frames {
					if _, _, err := Parse(f); err != nil {
						t.Fatalf("%s: frame %d does not parse: %v", a.ID, i, err)
					}
				}
				// Число строк совпадает с объявленным.
				lines, err := ArmorEncode(stream)
				if err != nil {
					t.Fatalf("%s: re-encode failed: %v", a.ID, err)
				}
				if len(lines) != a.Chunks {
					t.Fatalf("%s: re-encoded into %d chunks, corpus says %d", a.ID, len(lines), a.Chunks)
				}
			case "reject":
				if err == nil {
					t.Fatalf("%s: expected reject with %s, got accept\nnote: %s", a.ID, a.Code, a.Note)
				}
				if got := string(CodeOf(err)); got != a.Code {
					t.Fatalf("%s: expected %s, got %s (%v)", a.ID, a.Code, got, err)
				}
			default:
				t.Fatalf("%s: unknown verdict %q", a.ID, a.Verdict)
			}
		})
	}
}

func TestCorpusArmorPublishedLine(t *testing.T) {
	c := loadCorpus(t)
	want, _ := c.ArmorCheck["expected"].(string)
	if want == "" {
		t.Fatal("published_armor_check carries no expected line")
	}
	blob := corpusRead(t, "bin/positive/b1_wire_8_5.bin")
	lines, err := ArmorEncode(blob)
	if err != nil {
		t.Fatalf("armor encode: %v", err)
	}
	if len(lines) != 1 {
		t.Fatalf("the 8.5 blob must be one chunk, got %d", len(lines))
	}
	if lines[0] != want {
		t.Fatalf("armored line does not reproduce 03-WIRE.md 10.5\n got %s\nwant %s", lines[0], want)
	}
}

func TestCorpusPublishedDigests(t *testing.T) {
	c := loadCorpus(t)
	for _, d := range c.Digests {
		raw := corpusRead(t, d.File)
		if len(raw) != d.Bytes {
			t.Fatalf("%s: %d bytes, corpus says %d", d.File, len(raw), d.Bytes)
		}
		sum := sha256.Sum256(raw)
		if got := hex.EncodeToString(sum[:]); got != d.Expected {
			t.Fatalf("%s: sha256 %s, 03-WIRE.md 15 says %s", d.Document, got, d.Expected)
		}
		if !d.Match {
			t.Fatalf("%s: the corpus itself reports a digest mismatch", d.Document)
		}
	}
}

func TestCorpusEd25519KeyIngest(t *testing.T) {
	c := loadCorpus(t)
	if len(c.KeyIngest) != c.Counts["ed25519_public_key_ingest"] {
		t.Fatalf("corpus declares %d key vectors, the array holds %d",
			c.Counts["ed25519_public_key_ingest"], len(c.KeyIngest))
	}
	for _, k := range c.KeyIngest {
		k := k
		t.Run(k.ID, func(t *testing.T) {
			err := CheckPublicKey(mustHex(t, k.PublicKey))
			if k.Verdict == "accept" && err != nil {
				t.Fatalf("%s: expected accept, got %v\nnote: %s", k.ID, err, k.Note)
			}
			if k.Verdict == "reject" && err == nil {
				t.Fatalf("%s: expected reject (%s), got accept\nnote: %s", k.ID, k.Clause, k.Note)
			}
		})
	}
}

func TestCorpusEd25519Signatures(t *testing.T) {
	c := loadCorpus(t)
	if len(c.SigVectors) != c.Counts["ed25519_signature"] {
		t.Fatalf("corpus declares %d signature vectors, the array holds %d",
			c.Counts["ed25519_signature"], len(c.SigVectors))
	}
	for _, s := range c.SigVectors {
		s := s
		t.Run(s.ID, func(t *testing.T) {
			pk := mustHex(t, s.PublicKey)
			// Каждый открытый ключ этих векторов обязан пройти приём: они
			// проверяют уравнение подписи, а не отсев ключей.
			if err := CheckPublicKey(pk); err != nil {
				t.Fatalf("%s: public key fails ingest, which is not what this vector tests: %v", s.ID, err)
			}
			ok := VerifySignature(pk, mustHex(t, s.Message), mustHex(t, s.Signature))
			want := s.Verdict == "accept"
			if ok != want {
				t.Fatalf("%s: VerifySignature returned %v, expected %v (%s)\nnote: %s",
					s.ID, ok, want, s.Clause, s.Note)
			}
		})
	}
}

func TestCorpusHPKEKeySchedule(t *testing.T) {
	c := loadCorpus(t)
	v := c.HPKE.RFC9180

	if v.KemID != int(HPKEKemID) || v.KdfID != int(HPKEKdfID) || v.AeadID != int(HPKEAeadID) || v.Mode != int(HPKEModeBase) {
		t.Fatalf("the RFC 9180 vector is not the CSM/1 suite")
	}
	dh, ownPub, err := ecdhP256(mustHex(t, v.SkRm), mustHex(t, v.Enc))
	if err != nil {
		t.Fatalf("ecdh: %v", err)
	}
	shared, err := dhkemDecap(dh, ownPub, mustHex(t, v.Enc))
	if err != nil {
		t.Fatalf("dhkem decap: %v", err)
	}
	if got := hex.EncodeToString(shared); got != v.Shared {
		t.Fatalf("shared_secret %s, RFC 9180 A.5 says %s", got, v.Shared)
	}
	key, baseNonce, exporter, ksc := hpkeKeySchedule(shared, mustHex(t, v.Info))
	if got := hex.EncodeToString(ksc); got != v.KSC {
		t.Fatalf("key_schedule_context %s, RFC says %s", got, v.KSC)
	}
	if got := hex.EncodeToString(key); got != v.Key {
		t.Fatalf("key %s, RFC says %s", got, v.Key)
	}
	if got := hex.EncodeToString(baseNonce); got != v.BaseNonce {
		t.Fatalf("base_nonce %s, RFC says %s", got, v.BaseNonce)
	}
	if got := hex.EncodeToString(exporter); got != v.Exporter {
		t.Fatalf("exporter_secret %s, RFC says %s", got, v.Exporter)
	}

	// Открыть оба сообщения RFC, чтобы проверить и ChaCha20-Poly1305, и
	// наложение номера сообщения на base_nonce.
	n0 := hpkeNonce(baseNonce, 0)
	if got := hex.EncodeToString(n0); got != v.Seq0Nonce {
		t.Fatalf("seq0 nonce %s, RFC says %s", got, v.Seq0Nonce)
	}
	pt0, err := chachaPolyOpen(key, n0, mustHex(t, v.Seq0AAD), mustHex(t, v.Seq0CT))
	if err != nil {
		t.Fatalf("seq0 open: %v", err)
	}
	if got := hex.EncodeToString(pt0); got != v.Seq0PT {
		t.Fatalf("seq0 plaintext %s, RFC says %s", got, v.Seq0PT)
	}
	n1 := hpkeNonce(baseNonce, 1)
	if got := hex.EncodeToString(n1); got != v.Seq1Nonce {
		t.Fatalf("seq1 nonce %s, RFC says %s", got, v.Seq1Nonce)
	}
	pt1, err := chachaPolyOpen(key, n1, mustHex(t, v.Seq1AAD), mustHex(t, v.Seq1CT))
	if err != nil {
		t.Fatalf("seq1 open: %v", err)
	}
	if got := hex.EncodeToString(pt1); got != v.Seq0PT {
		t.Fatalf("seq1 plaintext %s, RFC says %s", got, v.Seq0PT)
	}

	// Запечатывание восстанавливает тот же шифртекст: реализация AEAD
	// симметрична и не только проверяет тег.
	if got := hex.EncodeToString(chachaPolySeal(key, n0, mustHex(t, v.Seq0AAD), pt0)); got != v.Seq0CT {
		t.Fatalf("re-sealed seq0 %s, RFC says %s", got, v.Seq0CT)
	}
}

func TestCorpusSealAAD(t *testing.T) {
	c := loadCorpus(t)
	pid := mustHex(t, c.FixtureKeys["pid"])
	dtp := mustHex(t, c.FixtureKeys["dtp"])
	got := hex.EncodeToString(SealAAD(pid, dtp, 412))
	if got != c.HPKE.AADFixture {
		t.Fatalf("aad %s, corpus says %s", got, c.HPKE.AADFixture)
	}
	if hex.EncodeToString([]byte(HPKEInfo)) != c.HPKE.Info {
		t.Fatalf("info string does not match the corpus")
	}
}

func TestCorpusDerivations(t *testing.T) {
	c := loadCorpus(t)
	fk := c.FixtureKeys
	rootPub := mustHex(t, fk["root_public"])
	onlinePub := mustHex(t, fk["online_public"])

	got := map[string]string{}
	got["pid"] = hex.EncodeToString(PIDOf(rootPub))
	got["kid_root"] = hex.EncodeToString(KeyIDOf(rootPub))
	got["kid_online"] = hex.EncodeToString(KeyIDOf(onlinePub))
	got["link_pin"] = LinkPin(rootPub)
	got["loc"] = LocatorOf(mustHex(t, fk["loc_hmac_secret"]), fk["subscription_uuid_for_loc"], 1)
	dtp := sha256.Sum256([]byte("csm1-doc-example-device-spki"))
	got["dtp"] = hex.EncodeToString(dtp[:16])

	// Код зачисления: link_pin[0..8] плюс 12 символов секрета, пятью группами
	// по четыре через дефис (02-SPEC.md 9.2).
	secret := sha256.Sum256([]byte("csm1-doc-example-enroll-secret"))
	code := LinkPin(rootPub)[:8] + Base32CrockfordEncode(secret[:8])[:12]
	var grouped []string
	for i := 0; i < len(code); i += 4 {
		grouped = append(grouped, code[i:i+4])
	}
	got["enrollment_code"] = strings.Join(grouped, "-")

	priv, err := ecdh.P256().NewPrivateKey(mustHex(t, fk["device_agreement_sk"]))
	if err != nil {
		t.Fatalf("device agreement key: %v", err)
	}
	got["device_agreement_pk"] = hex.EncodeToString(priv.PublicKey().Bytes())

	raw16 := make([]byte, 16)
	for i := range raw16 {
		raw16[i] = byte(i)
	}
	got["crockford_roundtrip"] = Base32CrockfordEncode(raw16)

	seen := 0
	for _, d := range c.Derivations {
		have, ok := got[d.Name]
		if !ok {
			t.Fatalf("derivation %s is not covered by this harness", d.Name)
		}
		if have != d.Output {
			t.Fatalf("derivation %s: got %s, corpus says %s (%s)", d.Name, have, d.Output, d.Rule)
		}
		seen++
	}
	if seen != c.Counts["derivations"] {
		t.Fatalf("checked %d derivations, corpus declares %d", seen, c.Counts["derivations"])
	}

	// Обратное декодирование обязано вернуть исходные байты.
	back, err := Base32CrockfordDecode(got["crockford_roundtrip"])
	if err != nil || !bytes.Equal(back, raw16) {
		t.Fatalf("crockford round trip failed: %v", err)
	}
}

func TestCorpusErrorCodeRegistry(t *testing.T) {
	c := loadCorpus(t)
	have := map[string]bool{}
	for _, code := range AllCodes() {
		have[string(code)] = true
	}
	total := 0
	for family, codes := range c.Registry {
		for _, code := range codes {
			if !have[code] {
				t.Fatalf("registry family %s names %s, which this package does not define", family, code)
			}
			total++
		}
	}
	if total != len(AllCodes()) {
		t.Fatalf("package defines %d codes, the registry names %d", len(AllCodes()), total)
	}
	// Ни один вектор не имеет права нести код вне реестра.
	for _, v := range c.Vectors {
		if v.Code != "" && !have[v.Code] {
			t.Fatalf("vector %s carries code %s, which is outside the registry", v.ID, v.Code)
		}
	}
}

// TestCorpusChunkReassembly собирает каталог из его фрагментов и проверяет
// собранный кадр целиком, как требует 03-WIRE.md 8.4.
func TestCorpusChunkReassembly(t *testing.T) {
	c := loadCorpus(t)
	groups := map[string][]corpusVec{}
	for _, v := range c.Vectors {
		if v.DocType != int(DocChunk) || v.Verdict != "accept" {
			continue
		}
		base := v.ID[:strings.LastIndex(v.ID, "-")]
		groups[base] = append(groups[base], v)
	}
	if len(groups) == 0 {
		t.Fatal("the corpus carries no chunk fixtures")
	}
	for base, vs := range groups {
		base, vs := base, vs
		t.Run(base, func(t *testing.T) {
			st := buildTrustState(t, c, vs[0])
			frames := make([][]byte, 0, len(vs))
			for _, v := range vs {
				frames = append(frames, corpusRead(t, v.File))
			}
			res, err := VerifyCatalogFromChunks(frames, st)
			if err != nil {
				t.Fatalf("%s: reassembly and re-verification failed: %v", base, err)
			}
			cat, ok := res.Doc.(*Catalog)
			if !ok {
				t.Fatalf("%s: reassembled document is %s, expected a catalog", base, DocTypeName(res.Frame.DocType))
			}
			if len(cat.Ex) == 0 {
				t.Fatalf("%s: reassembled catalog carries no exits", base)
			}
			// Собранные байты обязаны совпасть с опубликованным каталогом.
			var want string
			switch base {
			case "pos-c1c-min":
				want = "bin/positive/c1_min.bin"
			case "pos-c1c-typ":
				want = "bin/positive/c1_typical.bin"
			case "pos-c1c-max":
				want = "bin/positive/c1_max.bin"
			}
			if want != "" && !bytes.Equal(res.Frame.Raw, corpusRead(t, want)) {
				t.Fatalf("%s: reassembled frame is not byte-identical to %s", base, want)
			}
		})
	}
}

// TestCorpusSealedInner проверяет, что внутренняя директива запечатанного
// вектора восстанавливается и разбирается полностью.
func TestCorpusSealedInner(t *testing.T) {
	c := loadCorpus(t)
	found := 0
	for _, v := range c.Vectors {
		if v.DocType != int(DocSealed) || v.Verdict != "accept" {
			continue
		}
		found++
		st := buildTrustState(t, c, v)
		res, err := Verify(corpusRead(t, v.File), st)
		if err != nil {
			t.Fatalf("%s: %v", v.ID, err)
		}
		if res.Inner == nil {
			t.Fatalf("%s: the inner directive was not verified", v.ID)
		}
		dir, ok := res.Inner.Doc.(*Directive)
		if !ok {
			t.Fatalf("%s: the recovered inner document is not a directive", v.ID)
		}
		if !bytes.Equal(dir.Nonce, st.ExpectedNonce) {
			t.Fatalf("%s: the inner nonce is not the outstanding one", v.ID)
		}
		if !bytes.Equal(res.InnerFrame, corpusRead(t, "bin/positive/m1_min.bin")) {
			t.Fatalf("%s: the recovered plaintext is not the published m1_min frame", v.ID)
		}
	}
	if found == 0 {
		t.Fatal("the corpus carries no accepted sealed directive")
	}
}

// TestCorpusCoverage проверяет, что каждый код реестра действительно
// встречается среди отрицательных векторов, а харнесс их все прогоняет.
func TestCorpusCoverage(t *testing.T) {
	c := loadCorpus(t)
	byGroup := map[string]int{}
	for _, v := range c.Vectors {
		byGroup[v.Group]++
	}
	for _, g := range []string{"positive", "negative", "reference"} {
		if byGroup[g] != c.Counts[g] {
			t.Fatalf("group %s: %d vectors, corpus declares %d", g, byGroup[g], c.Counts[g])
		}
	}
	seen := map[string]int{}
	for _, v := range c.Vectors {
		if v.Code != "" {
			seen[v.Code]++
		}
	}
	// Коды семейства seal и verify обязаны быть покрыты; armor покрыт
	// отдельным набором.
	for _, code := range AllCodes() {
		if code == EParseFraming {
			continue // покрыт и кадрами, и армированными наборами
		}
		if seen[string(code)] == 0 {
			t.Fatalf("code %s is named by no vector; the corpus claims full coverage", code)
		}
	}
	fmt.Fprintf(os.Stderr, "csm corpus: %d vectors, %d armor sets, %d key vectors, %d signature vectors\n",
		len(c.Vectors), len(c.Armor), len(c.KeyIngest), len(c.SigVectors))
}
