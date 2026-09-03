package csm

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"testing"
)

// Модульные тесты для того, до чего общий корпус не дотягивается: равенство
// версий при побайтовом совпадении, недоверенные часы, истёкший якорь, шаг
// V14b, сборка кадра, пересборка фрагментов, армированный читатель и
// декодер base32.

// baseState собирает состояние по образцу контекста default корпуса, но без
// чтения vectors.json: значения взяты из 03-WIRE.md раздел 15.
func baseState(t *testing.T) *TrustState {
	t.Helper()
	anchor, _, err := ParseKeyDocument(corpusRead(t, "bin/positive/k1_min.bin"))
	if err != nil {
		t.Fatalf("anchor: %v", err)
	}
	pin, err := Base32CrockfordDecode("49Q8M87PK6WP9QXG3T30")
	if err != nil {
		t.Fatalf("link_pin: %v", err)
	}
	nonce, _ := hex.DecodeString("a3f10c94b27e5d6188ff20419c73ae05")
	dtp, _ := hex.DecodeString("4f0f22569564aab09a2d1a75c132d955")
	pid, _ := hex.DecodeString("226e8a20f699b964")
	agree, _ := hex.DecodeString("a801c3a8f0bbc69e012c248320808ccf24e4928065cedfbcfd22032e9c954138")
	return &TrustState{
		PinnedPID:     pid,
		LinkPin:       pin,
		Anchor:        anchor,
		Now:           1788307500,
		ClockTrusted:  true,
		TimeFloor:     1788307200,
		HWM:           map[uint8]uint64{1: 1, 2: 6, 3: 411, 4: 6, 5: 0, 8: 0},
		ExpectedNonce: nonce,
		DeviceDTP:     dtp,
		AgreementKeys: map[uint64][]byte{1: agree},
	}
}

// ------------------------------------------------------------------ V9

func TestVersionEqualAcceptsIdenticalFrame(t *testing.T) {
	raw := corpusRead(t, "bin/positive/c1_min.bin")

	st := baseState(t)
	st.HWM[DocCatalog] = 7 // ver каталога c1_min
	st.StoredFrame = append([]byte(nil), raw...)
	if _, err := Verify(raw, st); err != nil {
		t.Fatalf("ver == hwm with a byte-identical stored frame must be accepted: %v", err)
	}

	// Без сохранённого кадра равенство отвергается.
	st2 := baseState(t)
	st2.HWM[DocCatalog] = 7
	_, err := Verify(raw, st2)
	if CodeOf(err) != EVerifyVersion {
		t.Fatalf("ver == hwm without a stored frame must be E_VERIFY_VERSION, got %v", err)
	}

	// Ниже отметки отвергается всегда.
	st3 := baseState(t)
	st3.HWM[DocCatalog] = 8
	_, err = Verify(raw, st3)
	if CodeOf(err) != EVerifyVersion {
		t.Fatalf("ver < hwm must be E_VERIFY_VERSION, got %v", err)
	}
}

// ------------------------------------------------------------------ V11, V12

func TestClockNotTrustedMakesSkewAndExpiryInert(t *testing.T) {
	raw := corpusRead(t, "bin/positive/m1_min.bin")

	// Часы уехали далеко за срок жизни и якоря, и документа. Пока
	// clock_trusted ложно, оба шага пропускаются, и проверка проходит: это
	// заодно доказывает, что V12 применяется к проверяемому документу, а не к
	// якорю, чей exp здесь тоже давно позади.
	st := baseState(t)
	st.ClockTrusted = false
	st.Now = 1788307200 + 5_000_000
	if _, err := Verify(raw, st); err != nil {
		t.Fatalf("with an untrusted clock V11 skew and V12 must be inert: %v", err)
	}

	// С доверенными часами тот же документ истёк.
	st.ClockTrusted = true
	_, err := Verify(raw, st)
	if CodeOf(err) != EVerifyExpired {
		t.Fatalf("expected E_VERIFY_EXPIRED with a trusted clock, got %v", err)
	}
}

func TestExpiredAnchorStillAuthorizes(t *testing.T) {
	// Якорь k1_min истекает через 604800 секунд после iat. Ставим часы за
	// этот момент и снимаем доверие к ним, чтобы проверяемый документ не
	// падал на своём собственном V12. Успех означает, что срок якоря не
	// консультируется вовсе.
	st := baseState(t)
	st.ClockTrusted = false
	st.Now = int64(st.Anchor.Env.Exp) + 100_000
	if _, err := Verify(corpusRead(t, "bin/positive/c1_min.bin"), st); err != nil {
		t.Fatalf("an expired trusted key document must remain a valid authorization anchor: %v", err)
	}
}

func TestTimeFloorRejectsALongDeadDocument(t *testing.T) {
	st := baseState(t)
	st.TimeFloor = 1788307200 + 10_000_000
	st.ClockTrusted = false
	_, err := Verify(corpusRead(t, "bin/positive/m1_min.bin"), st)
	if CodeOf(err) != EVerifyIAT {
		t.Fatalf("a document dead long before the floor must be E_VERIFY_IAT, got %v", err)
	}
}

// ------------------------------------------------------------------ V14b

func TestTierHashBindingV14b(t *testing.T) {
	// k1_typical публикует tiers {1: chash(c1_min), 2: chash(c1_max)}.
	anchor, _, err := ParseKeyDocument(corpusRead(t, "bin/positive/k1_typical.bin"))
	if err != nil {
		t.Fatalf("anchor: %v", err)
	}
	if len(anchor.Tiers) == 0 {
		t.Fatal("k1_typical is expected to publish tier hashes")
	}

	tier1 := uint64(1)
	st := baseState(t)
	st.Anchor = anchor
	st.HWM[DocCatalog] = 0
	st.BoundTier = &tier1

	// Каталог тарифа 1, чей chash и есть опубликованный: принимается.
	if _, err := Verify(corpusRead(t, "bin/positive/c1_min.bin"), st); err != nil {
		t.Fatalf("the root-anchored catalog must be accepted: %v", err)
	}
	// Другой каталог под тем же тарифом директивы: V14b отвергает. Это и есть
	// та подмена, ради которой шаг существует: собственный tier каталога здесь
	// не читается вовсе, поэтому подставить в него неякорённое значение и
	// проскочить проверку нельзя.
	_, err = Verify(corpusRead(t, "bin/positive/c1_typical.bin"), st)
	if CodeOf(err) != EVerifyCatHash {
		t.Fatalf("a catalog whose chash is not the published tiers entry must be E_VERIFY_CATHASH, got %v", err)
	}

	// Якорь публикует tiers, а тариф директивы не назван: принять каталог
	// нельзя, потому что проверять его не против чего.
	stNoTier := baseState(t)
	stNoTier.Anchor = anchor
	stNoTier.HWM[DocCatalog] = 0
	if _, err := Verify(corpusRead(t, "bin/positive/c1_min.bin"), stNoTier); CodeOf(err) != EVerifyCatHash {
		t.Fatalf("a catalog verified against a tiers-publishing anchor with no bound tier must be refused, got %v", err)
	}

	// Без tiers у якоря шаг вырождается в утверждение обвязки, а не в отказ.
	st2 := baseState(t)
	st2.HWM[DocCatalog] = 0
	if _, err := Verify(corpusRead(t, "bin/positive/c1_typical.bin"), st2); err != nil {
		t.Fatalf("an anchor without tiers must not refuse a catalog: %v", err)
	}
	cat := mustCatalog(t, corpusRead(t, "bin/positive/c1_typical.bin"), st2)
	if FleetRootAnchored(cat.Tier, st2.Anchor) {
		t.Fatal("a fleet with no published tier hash must not report as root anchored")
	}
	if !FleetRootAnchored(1, anchor) {
		t.Fatal("a fleet with a published tier hash must report as root anchored")
	}
}

func mustCatalog(t *testing.T, raw []byte, st *TrustState) *Catalog {
	t.Helper()
	res, err := Verify(raw, st)
	if err != nil {
		t.Fatalf("verify: %v", err)
	}
	cat, ok := res.Doc.(*Catalog)
	if !ok {
		t.Fatal("document is not a catalog")
	}
	return cat
}

// ------------------------------------------------------------------ V8

func TestPinnedPIDMismatch(t *testing.T) {
	st := baseState(t)
	st.PinnedPID = []byte{9, 9, 9, 9, 9, 9, 9, 9}
	_, err := Verify(corpusRead(t, "bin/positive/m1_min.bin"), st)
	if CodeOf(err) != EVerifyPID {
		t.Fatalf("expected E_VERIFY_PID, got %v", err)
	}
}

// ------------------------------------------------------------------ кадр

func TestFrameExactLengthRule(t *testing.T) {
	raw := corpusRead(t, "bin/positive/m1_min.bin")
	if len(raw) != 228 {
		t.Fatalf("m1_min is %d bytes, 03-WIRE.md 1.1 says 228", len(raw))
	}
	f, err := ParseFrame(raw)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if len(f.PreImage) != 7+144 {
		t.Fatalf("pre-image is %d bytes, 7 + payload_len is %d", len(f.PreImage), 7+144)
	}
	if !bytes.Equal(f.PreImage, raw[:151]) {
		t.Fatal("the pre-image must be the first 7 + payload_len bytes as transmitted")
	}
	if got := SigningPreImage(DocDirective, f.Payload); !bytes.Equal(got, f.PreImage) {
		t.Fatal("SigningPreImage disagrees with the parsed pre-image")
	}

	// Один добавленный байт это отказ, а не игнорируемый хвост.
	if _, err := ParseFrame(append(append([]byte(nil), raw...), 0)); CodeOf(err) != EParseFraming {
		t.Fatalf("a trailing byte must be E_PARSE_FRAMING, got %v", err)
	}
	// Один снятый байт тоже.
	if _, err := ParseFrame(raw[:len(raw)-1]); CodeOf(err) != EParseFraming {
		t.Fatalf("a truncated frame must be E_PARSE_FRAMING, got %v", err)
	}
}

func TestBuildFrameSortsSlotsAndRefusesDuplicates(t *testing.T) {
	raw := corpusRead(t, "bin/positive/c1_dual_sig.bin")
	f, err := ParseFrame(raw)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	// Слоты в файле уже отсортированы; подаём их наоборот и требуем тот же кадр.
	rev := []SigSlot{f.Sigs[1], f.Sigs[0]}
	out, err := BuildFrame(f.DocType, f.Payload, rev)
	if err != nil {
		t.Fatalf("build: %v", err)
	}
	if !bytes.Equal(out, raw) {
		t.Fatal("BuildFrame must emit slots sorted by ascending keyid_trunc")
	}
	if _, err := BuildFrame(f.DocType, f.Payload, []SigSlot{f.Sigs[0], f.Sigs[0]}); err == nil {
		t.Fatal("two slots with the same keyid_trunc must be refused")
	}
	if _, err := BuildFrame(0x07, f.Payload, f.Sigs); err == nil {
		t.Fatal("a reserved doc_type must not be emittable")
	}
}

func TestFrameStreamWalk(t *testing.T) {
	a := corpusRead(t, "bin/positive/k1_min.bin")
	b := corpusRead(t, "bin/positive/c1_min.bin")
	stream := append(append([]byte(nil), a...), b...)

	frames, err := SplitFrameStream(stream)
	if err != nil {
		t.Fatalf("walk: %v", err)
	}
	if len(frames) != 2 || !bytes.Equal(frames[0], a) || !bytes.Equal(frames[1], b) {
		t.Fatal("the stream must split back into its two frames")
	}
	if _, err := SplitFrameStream(append(stream, 0x00)); CodeOf(err) != EParseFraming {
		t.Fatal("a byte left over after the last frame must be E_PARSE_FRAMING")
	}
	if _, err := SplitFrameStream(stream[:len(stream)-1]); CodeOf(err) != EParseFraming {
		t.Fatal("a truncated final frame must be E_PARSE_FRAMING")
	}
}

// ------------------------------------------------------------------ CBOR

func TestCBORProfileRejectsBeforeAllocating(t *testing.T) {
	cases := []struct {
		name    string
		payload []byte
	}{
		{"array head above MAX_ARRAY_ITEMS", []byte{0xa1, 0x01, 0x9a, 0x00, 0x01, 0x00, 0x00}},
		{"map head above MAX_MAP_PAIRS", []byte{0xb8, 0x41}},
		{"bstr head above MAX_BSTR_BYTES", []byte{0xa1, 0x01, 0x5a, 0x00, 0x10, 0x00, 0x00}},
		{"tstr head above MAX_TSTR_BYTES", []byte{0xa1, 0x01, 0x79, 0x01, 0x01}},
		{"array longer than the payload", []byte{0xa1, 0x01, 0x98, 0x40}},
	}
	for _, c := range cases {
		if _, err := decodeCBORPayload(c.payload); CodeOf(err) != EParseCBOR {
			t.Fatalf("%s: expected E_PARSE_CBOR, got %v", c.name, err)
		}
	}
}

func TestCBORProfileStructuralRules(t *testing.T) {
	cases := []struct {
		name    string
		payload []byte
	}{
		{"top level array", []byte{0x81, 0x01}},
		{"trailing byte", []byte{0xa1, 0x01, 0x01, 0x00}},
		{"indefinite map", []byte{0xbf, 0x01, 0x01, 0xff}},
		{"tag", []byte{0xa1, 0x01, 0xc2, 0x41, 0x01}},
		{"float", []byte{0xa1, 0x01, 0xf9, 0x42, 0x00}},
		{"null", []byte{0xa1, 0x01, 0xf6}},
		{"undefined", []byte{0xa1, 0x01, 0xf7}},
		{"simple 32", []byte{0xa1, 0x01, 0xf8, 0x20}},
		{"negative int", []byte{0xa1, 0x01, 0x20}},
		{"non-minimal uint", []byte{0xa1, 0x01, 0x18, 0x01}},
		{"non-minimal length", []byte{0xa1, 0x01, 0x58, 0x08, 0, 0, 0, 0, 0, 0, 0, 0}},
		{"text key", []byte{0xa1, 0x62, 0x73, 0x74, 0x01}},
		{"key zero", []byte{0xa1, 0x00, 0x01}},
		{"key 1024", []byte{0xa1, 0x19, 0x04, 0x00, 0x01}},
		{"unsorted keys", []byte{0xa2, 0x02, 0x01, 0x01, 0x01}},
		{"duplicate keys", []byte{0xa2, 0x01, 0x01, 0x01, 0x01}},
		{"invalid utf8", []byte{0xa1, 0x01, 0x61, 0xff}},
	}
	for _, c := range cases {
		if _, err := decodeCBORPayload(c.payload); CodeOf(err) != EParseCBOR {
			t.Fatalf("%s: expected E_PARSE_CBOR, got %v", c.name, err)
		}
	}

	// Глубина 6 допустима, 7 нет.
	ok := []byte{0xa1, 0x01, 0x81, 0x81, 0x81, 0x81, 0x01}
	if _, err := decodeCBORPayload(ok); err != nil {
		t.Fatalf("depth 6 must be accepted: %v", err)
	}
	deep := []byte{0xa1, 0x01, 0x81, 0x81, 0x81, 0x81, 0x81, 0x01}
	if _, err := decodeCBORPayload(deep); CodeOf(err) != EParseCBOR {
		t.Fatalf("depth 7 must be E_PARSE_CBOR, got %v", err)
	}

	// Некритический неизвестный ключ проходит декодер и игнорируется полем.
	nc := []byte{0xa2, 0x01, 0x01, 0x18, 0x40, 0x01}
	v, err := decodeCBORPayload(nc)
	if err != nil {
		t.Fatalf("a non-critical key must decode: %v", err)
	}
	if !v.Has(64) {
		t.Fatal("the non-critical key should be present in the decoded value")
	}
}

func TestCBORMaxUint(t *testing.T) {
	// 2^53 - 1 проходит, 2^53 нет.
	okp := []byte{0xa1, 0x01, 0x1b, 0x00, 0x1f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff}
	if _, err := decodeCBORPayload(okp); err != nil {
		t.Fatalf("MAX_UINT must decode: %v", err)
	}
	over := []byte{0xa1, 0x01, 0x1b, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00}
	if _, err := decodeCBORPayload(over); CodeOf(err) != EParseCBOR {
		t.Fatalf("2^53 must be E_PARSE_CBOR, got %v", err)
	}
}

// ------------------------------------------------------------------ авторизация

func TestAuthorizationTableCoversEveryEmittableDocType(t *testing.T) {
	for dt := range docTypeDefined {
		rule, ok := AuthRuleFor(dt)
		if !ok {
			t.Fatalf("doc_type 0x%02x survives P3 but has no authorization row; V1 must be unable to fail", dt)
		}
		if rule.Role != RoleRoot && rule.Role != RoleOnline {
			t.Fatalf("doc_type 0x%02x resolves to %s, which is not a v1 role", dt, RoleName(rule.Role))
		}
	}
	// Роль не выдаётся отдельно от ключа: набор всегда несёт свою роль.
	anchor, _, err := ParseKeyDocument(corpusRead(t, "bin/positive/k1_min.bin"))
	if err != nil {
		t.Fatalf("anchor: %v", err)
	}
	set, ok := authorizedSetFrom(anchor, RoleOnline, "anchor")
	if !ok || set.Role != RoleOnline {
		t.Fatal("an authorized key set must carry the role it was resolved for")
	}
	if _, ok := authorizedSetFrom(anchor, RoleTimestamp, "anchor"); ok {
		t.Fatal("a role the anchor does not publish must not resolve")
	}
}

func TestNoKeyIsReturnedWithoutItsRole(t *testing.T) {
	anchor, _, err := ParseKeyDocument(corpusRead(t, "bin/positive/k1_min.bin"))
	if err != nil {
		t.Fatalf("anchor: %v", err)
	}
	if anchor.KeyCount() != 2 {
		t.Fatalf("k1_min carries %d keys, expected 2", anchor.KeyCount())
	}
	rootKID := anchor.Roles[RoleRoot].KS[0]
	onlineKID := anchor.Roles[RoleOnline].KS[0]

	if _, ok := anchor.AuthorizedKey(RoleRoot, rootKID); !ok {
		t.Fatal("the root key must be reachable under role root")
	}
	// Тот же ключ под чужой ролью не выдаётся, хотя он присутствует в keys.
	// Это ровно та подмена, которую таблица ролей существует, чтобы закрыть.
	if _, ok := anchor.AuthorizedKey(RoleOnline, rootKID); ok {
		t.Fatal("the root key must not be reachable under role online")
	}
	if _, ok := anchor.AuthorizedKey(RoleRoot, onlineKID); ok {
		t.Fatal("the online key must not be reachable under role root")
	}
	if _, ok := anchor.AuthorizedKey(RoleTimestamp, rootKID); ok {
		t.Fatal("a role the document does not publish must yield nothing")
	}
}

// ------------------------------------------------------------------ фрагменты

func TestChunkSetRefusesDisagreeingMembers(t *testing.T) {
	st := baseState(t)
	cs := NewChunkSet()
	if _, err := cs.Add(corpusRead(t, "bin/positive/c1c_typ_0.bin"), st); err != nil {
		t.Fatalf("first chunk: %v", err)
	}
	if cs.Complete() {
		t.Fatal("a two chunk set is not complete after one chunk")
	}
	if got := cs.Missing(); len(got) != 1 || got[0] != 1 {
		t.Fatalf("Missing should report chunk 1, got %v", got)
	}
	// Фрагмент другого каталога обязан быть отвергнут, а не подмешан.
	if _, err := cs.Add(corpusRead(t, "bin/positive/c1c_max_0.bin"), st); CodeOf(err) != EParseField {
		t.Fatalf("a chunk of another catalog must be refused, got %v", err)
	}
	if _, err := cs.Reassemble(); err == nil {
		t.Fatal("an incomplete set must not reassemble")
	}
	if _, err := cs.Add(corpusRead(t, "bin/positive/c1c_typ_1.bin"), st); err != nil {
		t.Fatalf("second chunk: %v", err)
	}
	frame, err := cs.Reassemble()
	if err != nil {
		t.Fatalf("reassemble: %v", err)
	}
	if !bytes.Equal(frame, corpusRead(t, "bin/positive/c1_typical.bin")) {
		t.Fatal("the reassembled frame must be byte-identical to the published catalog")
	}
	sum := sha256.Sum256(frame)
	if CatalogID(sum[:]) == "" {
		t.Fatal("cat_id must be derivable from the reassembled frame")
	}
}

// ------------------------------------------------------------------ армирование

func TestArmorRoundTripAndReaderRules(t *testing.T) {
	stream := append(append([]byte(nil), corpusRead(t, "bin/positive/k1_min.bin")...),
		corpusRead(t, "bin/positive/c1_typical.bin")...)
	lines, err := ArmorEncode(stream)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	if len(lines) < 3 {
		t.Fatalf("expected several chunks, got %d", len(lines))
	}

	// Порядок строк не важен.
	shuffled := make([]string, len(lines))
	for i := range lines {
		shuffled[len(lines)-1-i] = lines[i]
	}
	back, err := ArmorDecode(shuffled)
	if err != nil {
		t.Fatalf("out of order decode: %v", err)
	}
	if !bytes.Equal(back, stream) {
		t.Fatal("the armored round trip must reproduce the stream")
	}

	// Повторное сканирование идентичной строки допустимо.
	dup := append(append([]string{}, lines...), lines[0])
	if _, err := ArmorDecode(dup); err != nil {
		t.Fatalf("an identical duplicate scan must be accepted: %v", err)
	}

	// Пропущенная порядковая, расхождение по bid, по n и по чужому алфавиту.
	if _, err := ArmorDecode(lines[1:]); CodeOf(err) != EParseFraming {
		t.Fatal("a missing ordinal must be E_PARSE_FRAMING")
	}
	mixed := append([]string{}, lines...)
	mixed[1] = "CARCAP1.AAAAAAAA" + mixed[1][16:]
	if _, err := ArmorDecode(mixed); CodeOf(err) != EParseFraming {
		t.Fatal("a mixed bid must be E_PARSE_FRAMING")
	}
	badChar := append([]string{}, lines...)
	badChar[0] = badChar[0][:len(badChar[0])-1] + "U"
	if _, err := ArmorDecode(badChar); CodeOf(err) != EParseFraming {
		t.Fatal("a character outside the Crockford alphabet must be E_PARSE_FRAMING")
	}
	// Префикс сравнивается без учёта регистра.
	lower := append([]string{}, lines...)
	for i := range lower {
		lower[i] = strings.Replace(lower[i], "CARCAP1", "carcap1", 1)
	}
	if _, err := ArmorDecode(lower); err != nil {
		t.Fatalf("the CARCAP1 prefix is compared case-insensitively: %v", err)
	}
}

// ------------------------------------------------------------------ base32

func TestCrockfordAlphabetRules(t *testing.T) {
	raw := []byte{0x00, 0x11, 0x22, 0x33, 0x44}
	enc := Base32CrockfordEncode(raw)
	if len(enc) != 8 {
		t.Fatalf("5 bytes must encode to 8 characters, got %d", len(enc))
	}
	back, err := Base32CrockfordDecode(enc)
	if err != nil || !bytes.Equal(back, raw) {
		t.Fatalf("round trip failed: %v", err)
	}
	// I, i, L и l дают 1, O и o дают 0, дефис игнорируется.
	a, err := Base32CrockfordDecode("10-10")
	if err != nil {
		t.Fatalf("hyphens must be ignored: %v", err)
	}
	b, err := Base32CrockfordDecode("IOlo")
	if err != nil {
		t.Fatalf("I, O, l and o must map: %v", err)
	}
	if !bytes.Equal(a, b) {
		t.Fatal("IOlo must decode to the same bytes as 1010")
	}
	// U вне алфавита.
	if _, err := Base32CrockfordDecode("UUUU"); err == nil {
		t.Fatal("U must be rejected")
	}
	// Ненулевые хвостовые биты заполнения отвергаются.
	pin := "49Q8M87PK6WP9QXG3T30"
	if _, err := Base32CrockfordDecode(pin); err != nil {
		t.Fatalf("the fixture link_pin must decode: %v", err)
	}
	if _, err := Base32CrockfordDecode(pin[:len(pin)-1] + "1"); err == nil {
		t.Fatal("non-zero trailing pad bits must be rejected")
	}
}

// ------------------------------------------------------------------ HPKE

func TestSealRefusesTheWrongAgreementKey(t *testing.T) {
	raw := corpusRead(t, "bin/positive/m1s_min.bin")

	st := baseState(t)
	if _, err := Verify(raw, st); err != nil {
		t.Fatalf("the sealed fixture must open: %v", err)
	}

	// Тот же кадр под чужим ключом согласования: тег AEAD не сходится.
	other, _ := hex.DecodeString("a3ad342bf784735c7d33e112bdacbad1602bde3211433c5b0a727beed2b52fb3")
	st.AgreementKeys = map[uint64][]byte{1: other}
	if _, err := Verify(raw, st); CodeOf(err) != ESealOpen {
		t.Fatalf("a wrong agreement key must be E_SEAL_OPEN, got %v", err)
	}

	// Нет ключа поколения rkv вовсе.
	st.AgreementKeys = map[uint64][]byte{}
	if _, err := Verify(raw, st); CodeOf(err) != ESealRecipient {
		t.Fatalf("a missing generation must be E_SEAL_RECIPIENT, got %v", err)
	}
}

// ------------------------------------------------------------------ поля

func TestHostnameAndPathRules(t *testing.T) {
	goodHosts := []string{"m1.example-cdn.net", "a", "de1.exa-nodes.net"}
	badHosts := []string{"M1.Example-Cdn.net", "-lead.example.net", "trail-.example.net",
		"example.net.", "", "user@example.net", "example.net:443", "пример.рф", strings.Repeat("a", 65)}
	for _, h := range goodHosts {
		if !validHostname(h) {
			t.Fatalf("hostname %q must be accepted", h)
		}
	}
	for _, h := range badHosts {
		if validHostname(h) {
			t.Fatalf("hostname %q must be rejected", h)
		}
	}

	if !validIPLiteral("198.51.100.7") || !validIPLiteral("2001:db8::1") {
		t.Fatal("canonical IP literals must be accepted")
	}
	if validIPLiteral("2001:DB8::1") || validIPLiteral("198.051.100.7") {
		t.Fatal("non-canonical IP literals must be rejected")
	}

	goodPaths := []string{"/dns-query", "/rulesets/ru-direct.srs", "/a/b?c=d"}
	badPaths := []string{"//evil.example.net/dns", "/a/../dns-query", "/dns%2fquery", "/dns%2Fquery",
		"dns-query", "https://x/y", "/a b", "/a\\b", "/a@b/../c", "/%zz"}
	for _, p := range goodPaths {
		if !validPathOnly(p, 128) {
			t.Fatalf("path %q must be accepted", p)
		}
	}
	for _, p := range badPaths {
		if validPathOnly(p, 128) {
			t.Fatalf("path %q must be rejected", p)
		}
	}

	if !validOrigin("https://panel.example.net") || !validOrigin("https://panel.example.net:8443") {
		t.Fatal("https origins must be accepted")
	}
	for _, o := range []string{"http://panel.example.net", "https://panel.example.net/x",
		"https://panel.example.net:0", "panel.example.net", "https://"} {
		if validOrigin(o) {
			t.Fatalf("origin %q must be rejected", o)
		}
	}
}

func TestNodeAndRouteIDCharsets(t *testing.T) {
	if !validNodeID("n17i3") || !validNodeID("A_b-9") {
		t.Fatal("the node id charset is [0-9A-Za-z_-]")
	}
	for _, id := range []string{"n17 3", "", "default", strings.Repeat("a", 25), "n/17"} {
		if validNodeID(id) {
			t.Fatalf("node id %q must be rejected", id)
		}
	}
	if !validRouteID("bypass-ru") || validRouteID("Bypass") || validRouteID("") {
		t.Fatal("the route id charset is [a-z0-9-]")
	}
}

// ------------------------------------------------------------------ производные

func TestLocatorDerivation(t *testing.T) {
	secret, _ := hex.DecodeString("15bc4454d394e20a38fd6a2c29b898e48eee36ab5cf46e7d7f8286f45427d756")
	got := LocatorOf(secret, "9f3c1d02-5b8e-4a17-9d44-0e7a6c11b3f8", 1)
	if got != "EA3B8SKCY6VBWASE7AM1X48Y" {
		t.Fatalf("loc %s, 03-WIRE.md 15 says EA3B8SKCY6VBWASE7AM1X48Y", got)
	}
	if len(got) != 24 {
		t.Fatalf("loc must be 24 characters, got %d", len(got))
	}
	// Смена поколения меняет локатор, и только его.
	if LocatorOf(secret, "9f3c1d02-5b8e-4a17-9d44-0e7a6c11b3f8", 2) == got {
		t.Fatal("a new generation must produce a new locator")
	}
}

func TestErrorClassSplit(t *testing.T) {
	_, err := ParseFrame([]byte("nope"))
	e, ok := err.(*Error)
	if !ok || !e.IsParse() || e.IsVerify() {
		t.Fatalf("a short frame must be a parse failure, got %v", err)
	}
	st := baseState(t)
	st.PinnedPID = []byte{1, 2, 3, 4, 5, 6, 7, 8}
	_, err = Verify(corpusRead(t, "bin/positive/m1_min.bin"), st)
	e, ok = err.(*Error)
	if !ok || !e.IsVerify() || e.IsParse() {
		t.Fatalf("a pid mismatch must be a verification failure, got %v", err)
	}
}

// ------------------------------------------------------- rev.nodes и V13/V10

func TestDropRevokedNodes(t *testing.T) {
	// k1_typical публикует rev.nodes = {n99i1, n98i2}.
	anchor, _, err := ParseKeyDocument(corpusRead(t, "bin/positive/k1_typical.bin"))
	if err != nil {
		t.Fatalf("anchor: %v", err)
	}
	if len(anchor.Rev.Nodes) == 0 {
		t.Fatal("k1_typical is expected to publish rev.nodes")
	}

	cat := &Catalog{
		Ex: []Node{{ID: "n17i3"}, {ID: anchor.Rev.Nodes[0]}, {ID: "n18i1"}},
		Re: []Node{{ID: anchor.Rev.Nodes[1]}},
	}
	if n := DropRevokedNodes(cat, anchor); n != 2 {
		t.Fatalf("expected 2 revoked entries dropped, got %d", n)
	}
	if len(cat.Ex) != 2 || len(cat.Re) != 0 {
		t.Fatalf("filtering left ex=%d re=%d", len(cat.Ex), len(cat.Re))
	}
	for _, n := range cat.Ex {
		if n.ID == anchor.Rev.Nodes[0] {
			t.Fatal("a seized node survived the filter")
		}
	}
	// Идемпотентность: фильтр запускается на каждой загрузке кеша.
	if n := DropRevokedNodes(cat, anchor); n != 0 {
		t.Fatalf("second pass dropped %d entries, the filter is not idempotent", n)
	}
}

func TestDirectiveWithoutOutstandingNonceIsRefused(t *testing.T) {
	frame := corpusRead(t, "bin/positive/m1_min.bin")
	st := baseState(t)
	st.HWM[DocDirective] = 0
	if _, err := Verify(frame, st); err != nil {
		t.Fatalf("the corpus directive must verify with its nonce: %v", err)
	}
	// Без выданного nonce директива отвергается, а не проверяется частично.
	st2 := baseState(t)
	st2.HWM[DocDirective] = 0
	st2.ExpectedNonce = nil
	if _, err := Verify(frame, st2); CodeOf(err) != EVerifyNonce {
		t.Fatalf("a directive verified with no outstanding nonce must be E_VERIFY_NONCE, got %v", err)
	}
	// Кроме одного случая: перечитывание уже принятого кадра, объявленное
	// вызывающим и подтверждённое побайтовым совпадением.
	st3 := baseState(t)
	st3.HWM[DocDirective] = 0
	st3.ExpectedNonce = nil
	st3.CachedReplay = true
	st3.StoredFrame = frame
	if _, err := Verify(frame, st3); err != nil {
		t.Fatalf("a byte-identical cached re-read must be accepted: %v", err)
	}
	// И флаг не работает на другом кадре.
	st4 := baseState(t)
	st4.HWM[DocDirective] = 0
	st4.ExpectedNonce = nil
	st4.CachedReplay = true
	st4.StoredFrame = corpusRead(t, "bin/positive/m1_typical.bin")
	if _, err := Verify(frame, st4); CodeOf(err) != EVerifyNonce {
		t.Fatalf("CachedReplay must not apply to a frame that is not the stored one, got %v", err)
	}
}

func TestKeyDocumentRereadSkipsRotation(t *testing.T) {
	frame := corpusRead(t, "bin/positive/k1_min.bin")
	kd, _, err := ParseKeyDocument(frame)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	// Перечитывание кешированного якоря: ver == hwm, кадр тот же. V9 принимает
	// по побайтовому совпадению, V10 не выполняется, порог проверяется.
	st := baseState(t)
	st.Anchor = nil
	st.HWM[DocKey] = kd.Env.Ver
	st.StoredFrame = frame
	if _, err := Verify(frame, st); err != nil {
		t.Fatalf("a byte-identical re-read of the trusted anchor must be accepted: %v", err)
	}
	// А откат кадра на той же версии по-прежнему ловится на V9, что и было
	// потеряно, пока вызывающий подставлял ver - 1.
	st2 := baseState(t)
	st2.Anchor = nil
	st2.HWM[DocKey] = kd.Env.Ver
	st2.StoredFrame = corpusRead(t, "bin/positive/k1_typical.bin")
	if _, err := Verify(frame, st2); CodeOf(err) != EVerifyVersion {
		t.Fatalf("ver == hwm with different stored bytes must be E_VERIFY_VERSION, got %v", err)
	}
}

// TestSealOpensThroughAnAgreementSource доказывает, что запечатанная директива
// открывается держателем ключа, который скаляр НЕ отдаёт.
//
// Это единственный путь, доступный ключу в Secure Enclave или StrongBox:
// оба выполняют ECDH и никогда не выпускают закрытый ключ наружу, а
// 02-SPEC.md 9.4 требует держать ключ согласования именно там. Без этого теста
// аппаратный уровень был бы объявлен и неисполним.
func TestSealOpensThroughAnAgreementSource(t *testing.T) {
	raw := corpusRead(t, "bin/positive/m1s_min.bin")

	st := baseState(t)
	sk := st.AgreementKeys[1]
	// Карта скаляров ПУСТА: ровно так выглядит состояние на аппаратном
	// носителе ключа.
	st.AgreementKeys = map[uint64][]byte{}
	st.Agreement = testAgreement{sk: sk}
	if _, err := Verify(raw, st); err != nil {
		t.Fatalf("запечатанная директива обязана открыться через источник согласования: %v", err)
	}

	// Источник, не знающий поколения, это шаг 5, а не отказ AEAD.
	st2 := baseState(t)
	st2.AgreementKeys = map[uint64][]byte{}
	st2.Agreement = testAgreement{}
	if _, err := Verify(raw, st2); CodeOf(err) != ESealRecipient {
		t.Fatalf("отсутствие поколения обязано быть E_SEAL_RECIPIENT, получено %v", err)
	}

	// Источник побеждает карту: карта с ВЕРНЫМ скаляром не спасает источник,
	// который отдаёт чужой ECDH.
	other, _ := hex.DecodeString("a3ad342bf784735c7d33e112bdacbad1602bde3211433c5b0a727beed2b52fb3")
	st3 := baseState(t)
	st3.Agreement = testAgreement{sk: other}
	if _, err := Verify(raw, st3); CodeOf(err) != ESealOpen {
		t.Fatalf("источник обязан побеждать карту скаляров, получено %v", err)
	}
}

// testAgreement это держатель ключа, который выполняет ECDH сам. Пустой sk
// означает "поколения нет".
type testAgreement struct{ sk []byte }

func (a testAgreement) Agree(rkv uint64, peer []byte) ([]byte, []byte, error) {
	if len(a.sk) == 0 || rkv != 1 {
		return nil, nil, ErrNoAgreementGeneration
	}
	return ecdhP256(a.sk, peer)
}
