package transport

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

// Материал подключения фикстуры. Он собран в одном месте, чтобы проверка
// границы доверия перечисляла ровно те строки, которые ядро обязано оставить
// себе, и чтобы добавление поля в фикстуру требовало добавить его и сюда.
const (
	fxExitHost  = "de1.example.net"
	fxExitSNI   = "www.example.com"
	fxExitSID   = "a1b2c3d4"
	fxRelayHost = "ru1.example.net"
)

var fxExitPBK = bytes.Repeat([]byte{0x7e}, 32)

// fleetFixture это каталог с одним выходом DE, который строит цепочку через
// единственный вход RU. Это минимальная форма, в которой ребро rl вообще
// наблюдаемо: с одним лишь выходом связывать нечего.
func fleetFixture() *csm.Catalog {
	return &csm.Catalog{
		Ex: []csm.Node{{
			ID: "de1", PN: "Germany DE", CC: "DE",
			H: fxExitHost, P: 443,
			PR: 1, NW: 1, SE: 2,
			SNI: fxExitSNI, PBK: fxExitPBK, SID: fxExitSID,
			FP: 1, MTU: 1400,
			RL: "ru1",
		}},
		Re: []csm.Node{{
			ID: "ru1", PN: "Russia RU", CC: "RU",
			H: fxRelayHost, P: 8443,
			PR: 4, NW: 6, SE: 1,
		}},
	}
}

// fleetFetcher собирает выборщик прямо на разобранных документах. Сеть и
// хранилище здесь не участвуют: проверяется проекция, а не выборка.
func fleetFetcher(t *testing.T, cat *csm.Catalog, frame []byte, revoked ...string) *Fetcher {
	t.Helper()
	f, err := NewFetcher(t.TempDir(), "226e8a20f699b964", NewLadder(nil), nil)
	if err != nil {
		t.Fatalf("выборщик: %v", err)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	f.catalog, f.catFrame = cat, frame
	if len(revoked) > 0 {
		f.anchor = &csm.KeyDocument{Rev: csm.Revocation{Nodes: revoked}}
	}
	return f
}

func findNode(in []NodeRef, id string) (NodeRef, bool) {
	for _, n := range in {
		if n.ID == id {
			return n, true
		}
	}
	return NodeRef{}, false
}

// TestSnapshotProjectsFleet проверяет, что флот вообще доезжает до обвязки, и
// доезжает ровно в том виде, в каком его можно нарисовать и нельзя
// использовать для подключения мимо ядра.
func TestSnapshotProjectsFleet(t *testing.T) {
	cases := []struct {
		name    string
		catalog func() *csm.Catalog
		revoked []string
		check   func(t *testing.T, s Snapshot)
	}{
		{
			name:    "выход DE и вход RU проецируются целиком",
			catalog: fleetFixture,
			check: func(t *testing.T, s Snapshot) {
				if len(s.Exits) != 1 || len(s.Relays) != 1 {
					t.Fatalf("ожидались 1 выход и 1 вход, получено %d и %d", len(s.Exits), len(s.Relays))
				}
				ex, re := s.Exits[0], s.Relays[0]
				if ex.ID != "de1" || ex.Name != "Germany DE" || ex.CC != "DE" || ex.Kind != NodeKindExit {
					t.Fatalf("выход спроецирован неверно: %+v", ex)
				}
				if ex.Proto != 1 || ex.ProtoName != "vless" ||
					ex.Network != 1 || ex.NetworkName != "tcp" ||
					ex.Security != 2 || ex.SecurityName != "reality" {
					t.Fatalf("форма протокола выхода спроецирована неверно: %+v", ex)
				}
				if re.ID != "ru1" || re.CC != "RU" || re.Kind != NodeKindRelay {
					t.Fatalf("вход спроецирован неверно: %+v", re)
				}
				if re.ProtoName != "hysteria2" || re.NetworkName != "quic" || re.SecurityName != "tls" {
					t.Fatalf("форма протокола входа спроецирована неверно: %+v", re)
				}
				if !ex.Available || !re.Available {
					t.Fatalf("без отзыва оба узла обязаны быть доступны: %+v %+v", ex, re)
				}
			},
		},
		{
			name:    "ребро rl связывает выход с записью входа",
			catalog: fleetFixture,
			check: func(t *testing.T, s Snapshot) {
				if s.Exits[0].Relay != "ru1" {
					t.Fatalf("ребро rl потеряно: %q", s.Exits[0].Relay)
				}
				// Ребро обязано разрешаться в РАЗДЕЛЕ входов: страна входа
				// живёт там, и вывести её из страны выхода нельзя.
				if _, ok := findNode(s.Relays, s.Exits[0].Relay); !ok {
					t.Fatalf("rl=%q не разрешается ни в один вход: %+v", s.Exits[0].Relay, s.Relays)
				}
				if s.Relays[0].Relay != "" {
					t.Fatalf("у входа не бывает собственного rl: %q", s.Relays[0].Relay)
				}
			},
		},
		{
			name:    "отозванный узел помечен, а не выброшен",
			catalog: fleetFixture,
			revoked: []string{"ru1"},
			check: func(t *testing.T, s Snapshot) {
				re, ok := findNode(s.Relays, "ru1")
				if !ok {
					t.Fatal("отозванный вход пропал из списка; пользователь увидит исчезнувшую страну, а не запрет")
				}
				if re.Available {
					t.Fatalf("отозванный вход обязан быть недоступен: %+v", re)
				}
				if re.Reason != ReasonNodeRevoked {
					t.Fatalf("причина недоступности не названа: %q", re.Reason)
				}
				if ex := s.Exits[0]; !ex.Available || ex.Reason != "" {
					t.Fatalf("отзыв входа не касается выхода: %+v", ex)
				}
			},
		},
		{
			name:    "пустой каталог не выдумывает записи",
			catalog: func() *csm.Catalog { return &csm.Catalog{} },
			check: func(t *testing.T, s Snapshot) {
				if s.Exits != nil || s.Relays != nil {
					t.Fatalf("ожидались пустые списки, получено %+v %+v", s.Exits, s.Relays)
				}
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			f := fleetFetcher(t, tc.catalog(), nil, tc.revoked...)
			tc.check(t, f.Snapshot())
		})
	}
}

// TestSnapshotFleetCarriesNoConnectionMaterial это граница доверия, выраженная
// как утверждение о байтах: всё, чем подключаются, обязано остаться в ядре.
// Утечка сюда лишила бы подписанный каталог смысла — подменить SNI в
// неподписанном слое стало бы нечем поймать.
func TestSnapshotFleetCarriesNoConnectionMaterial(t *testing.T) {
	f := fleetFetcher(t, fleetFixture(), nil)
	raw, err := json.Marshal(f.Snapshot())
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	got := string(raw)
	for _, secret := range []string{
		fxExitHost, fxExitSNI, fxExitSID, fxRelayHost,
		hex.EncodeToString(fxExitPBK),
	} {
		if strings.Contains(got, secret) {
			t.Fatalf("материал подключения %q уехал в снимок", secret)
		}
	}
	// Проверка обязана быть чувствительной: если бы проекция была пустой,
	// предыдущий цикл прошёл бы сам собой.
	if !strings.Contains(got, `"de1"`) || !strings.Contains(got, `"Germany DE"`) {
		t.Fatalf("проекция пуста, значит предыдущая проверка ничего не проверила: %s", got)
	}
}

// TestSnapshotRestoresNodesDroppedByRevocation воспроизводит боевой порядок:
// applyCatalogLocked зовёт csm.DropRevokedNodes, и к моменту снимка записи в
// разобранном каталоге уже нет. Помечать было бы нечего, поэтому проекция
// восстанавливает её разбором того же проверенного кадра.
//
// Кадр берётся из общего корпуса, а не собирается здесь: собранный вручную
// кадр проверял бы разборщик против самого себя.
func TestSnapshotRestoresNodesDroppedByRevocation(t *testing.T) {
	frame, err := os.ReadFile(filepath.Join(corpusRel, "bin", "positive", "c1_typical.bin"))
	if err != nil {
		t.Fatalf("корпус: %v", err)
	}
	_, doc, err := csm.Parse(frame)
	if err != nil {
		t.Fatalf("разбор кадра каталога: %v", err)
	}
	cat, ok := doc.(*csm.Catalog)
	if !ok {
		t.Fatalf("кадр это %T, а не каталог", doc)
	}
	const revokedExit = "n102i3"
	if _, found := findNode(projectNodes(cat.Ex, NodeKindExit, nil), revokedExit); !found {
		t.Fatalf("фикстура корпуса больше не содержит %q; тест потерял предмет", revokedExit)
	}
	before := len(cat.Ex)

	anchor := &csm.KeyDocument{Rev: csm.Revocation{Nodes: []string{revokedExit}}}
	if n := csm.DropRevokedNodes(cat, anchor); n != 1 {
		t.Fatalf("фильтр применения выбросил %d записей, ожидалась 1", n)
	}
	if len(cat.Ex) != before-1 {
		t.Fatalf("фильтр не тронул каталог: было %d, стало %d", before, len(cat.Ex))
	}

	f := fleetFetcher(t, cat, frame, revokedExit)
	s := f.Snapshot()
	if len(s.Exits) != before {
		t.Fatalf("восстановлено %d выходов, ожидалось %d", len(s.Exits), before)
	}
	ref, found := findNode(s.Exits, revokedExit)
	if !found {
		t.Fatal("узел, выброшенный фильтром применения, не восстановлен в проекции")
	}
	if ref.Available || ref.Reason != ReasonNodeRevoked {
		t.Fatalf("восстановленный узел обязан быть помечен отозванным: %+v", ref)
	}
	// Порядок подписи не переставлен: восстановление идёт разбором тех же
	// байт, а не вставкой записи на угаданное место.
	if s.Exits[2].ID != revokedExit {
		t.Fatalf("подписанный порядок нарушен: на позиции 2 %q", s.Exits[2].ID)
	}
}

// TestCapRelayChainingMatchesBitName удерживает константу и словарь битов
// вместе. Разъехавшись, они дали бы фасаду читать чужой бит и объявлять
// цепочку разрешённой там, где оператор её не выдавал.
func TestCapRelayChainingMatchesBitName(t *testing.T) {
	idx := -1
	for i, name := range capBitNames {
		if name == "relay_chaining" {
			idx = i
			break
		}
	}
	if idx < 0 {
		t.Fatal("в capBitNames больше нет бита relay_chaining")
	}
	if want := uint32(1) << uint(idx); CapRelayChaining != want {
		t.Fatalf("CapRelayChaining = %#x, а бит relay_chaining это %#x", CapRelayChaining, want)
	}
}
