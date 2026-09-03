package transport

import (
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/csm"
)

func catalogWith(rs ...csm.Resource) *csm.Catalog {
	return &csm.Catalog{RS: rs}
}

// TestResourceGuardHoldsPreviousSetUntilAnswered: карточка 02-SPEC.md 7.7.1
// обещает, что до ответа пользователя действует ПРЕЖНИЙ набор, и обещание
// держит эта функция.
//
// Хеш связывает байты, но не связывает того, кто выбрал и путь, и хеш
// (04-THREAT-MODEL.md 7.3 шаг 5). Пока страж принимал новый набор
// безоговорочно, новые rule-set и geo-файлы вступали в силу к моменту, когда
// карточка только появлялась на экране, и кнопка "оставить прежние" не
// откатывала ничего.
func TestResourceGuardHoldsPreviousSetUntilAnswered(t *testing.T) {
	const bits = 1 << CapResourceHashes
	old := NewResourceGuard(catalogWith(csm.Resource{Name: "ru-banks", URL: "/rs/a", Hash: make([]byte, 32)}), bits)
	if !old.Enabled() || old.PendingCatalogChange() {
		t.Fatalf("первый набор обязан приниматься молча")
	}

	newHash := make([]byte, 32)
	newHash[0] = 0xAA
	next := NewResourceGuard(catalogWith(csm.Resource{Name: "ru-banks", URL: "/rs/a", Hash: newHash}), bits)
	next.HoldPrevious(old)

	if !next.PendingCatalogChange() {
		t.Fatalf("смена набора не удержана")
	}
	// В СИЛЕ остаётся прежний хеш: байты нового набора не пройдут Check.
	if err := next.Check("ru-banks", []byte("whatever")); err == nil {
		t.Fatalf("удержанный страж пропустил байты")
	}
	if r, ok := next.Entry("ru-banks"); !ok || r.Hash[0] != 0x00 {
		t.Fatalf("действует не прежняя запись: %v %v", ok, r.Hash[0])
	}

	// "Оставить прежние": предложение снято, в силе прежний набор.
	if !next.AnswerCatalogChange(false) {
		t.Fatalf("ответ не принят")
	}
	if next.PendingCatalogChange() {
		t.Fatalf("предложение осталось висеть")
	}
	if r, _ := next.Entry("ru-banks"); r.Hash[0] != 0x00 {
		t.Fatalf("после отказа применён новый набор")
	}
	// Отвечать дважды не ошибка, но и не действие.
	if next.AnswerCatalogChange(true) {
		t.Fatalf("второй ответ принят на пустом месте")
	}
}

// TestResourceGuardAcceptApplature: "принять новые" применяет именно то, что
// было предложено.
func TestResourceGuardAcceptAppliesProposed(t *testing.T) {
	const bits = 1 << CapResourceHashes
	old := NewResourceGuard(catalogWith(csm.Resource{Name: "ru-banks", URL: "/rs/a", Hash: make([]byte, 32)}), bits)
	newHash := make([]byte, 32)
	newHash[0] = 0xAA
	next := NewResourceGuard(catalogWith(csm.Resource{Name: "ru-banks", URL: "/rs/a", Hash: newHash}), bits)
	next.HoldPrevious(old)

	if !next.AnswerCatalogChange(true) {
		t.Fatalf("ответ не принят")
	}
	if r, _ := next.Entry("ru-banks"); r.Hash[0] != 0xAA {
		t.Fatalf("после согласия действует не новый набор")
	}
	if next.PendingCatalogChange() {
		t.Fatalf("предложение осталось висеть")
	}
}

// TestResourceGuardSameSetIsNotAChange: тот же набор не поднимает вопроса, а
// незакрытое предложение переносится, а не теряется.
func TestResourceGuardSameSetIsNotAChange(t *testing.T) {
	const bits = 1 << CapResourceHashes
	cat := catalogWith(csm.Resource{Name: "ru-banks", URL: "/rs/a", Hash: make([]byte, 32)})
	old := NewResourceGuard(cat, bits)
	same := NewResourceGuard(cat, bits)
	same.HoldPrevious(old)
	if same.PendingCatalogChange() {
		t.Fatalf("тот же набор поднял вопрос")
	}

	newHash := make([]byte, 32)
	newHash[0] = 0xAA
	held := NewResourceGuard(catalogWith(csm.Resource{Name: "ru-banks", URL: "/rs/a", Hash: newHash}), bits)
	held.HoldPrevious(same)
	if !held.PendingCatalogChange() {
		t.Fatalf("смена набора не удержана")
	}
	// Тот же каталог пришёл ещё раз, пока пользователь не ответил: вопрос
	// обязан остаться открытым, а не исчезнуть от повторной выборки.
	again := NewResourceGuard(catalogWith(csm.Resource{Name: "ru-banks", URL: "/rs/a", Hash: make([]byte, 32)}), bits)
	again.HoldPrevious(held)
	if !again.PendingCatalogChange() {
		t.Fatalf("незакрытое предложение потеряно повторной выборкой")
	}
}
