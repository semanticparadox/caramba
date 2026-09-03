package mobile

import (
	"os"
	"strings"
	"testing"
)

// Директива `tool golang.org/x/mobile/cmd/gobind` в go.mod это единственное,
// что удерживает golang.org/x/mobile в графе модуля: gobind не импортируется
// ни одним пакетом этого модуля, поэтому без директивы `go mod tidy` выкинет
// x/mobile из go.mod и go.sum, и `gomobile bind` пакета mobile/ перестанет
// разрешать gobind. Ломается вся нативная привязка: и Android-AAR, и
// iOS-xcframework. Комментарий в go.mod это объясняет, а тест это стережёт.
func TestGoModPinsGobindTool(t *testing.T) {
	raw, err := os.ReadFile("../go.mod")
	if err != nil {
		t.Fatalf("не читается go.mod: %v", err)
	}
	text := string(raw)

	var found bool
	for _, line := range strings.Split(text, "\n") {
		if strings.TrimSpace(line) == "tool golang.org/x/mobile/cmd/gobind" {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("в go.mod нет директивы `tool golang.org/x/mobile/cmd/gobind`: " +
			"gomobile bind останется без gobind, привязка Android/iOS не соберётся")
	}

	// Сама зависимость обязана остаться в require-блоке: директива tool без
	// require не пинует версию, и сборка привязки перестаёт быть
	// воспроизводимой.
	if !strings.Contains(text, "golang.org/x/mobile v") {
		t.Fatal("в go.mod нет require golang.org/x/mobile: версия gobind не зафиксирована")
	}

	// Комментарий рядом с require-блоком когда-то утверждал, что go.sum
	// неполный и что сборка с тегом mihomo падает до `go mod tidy`. Оба
	// утверждения неверны, и вернуться они не должны.
	for _, stale := range []string{
		"go.sum в репозитории",
		"СЕЙЧАС НЕПОЛНЫЙ",
		"missing go.sum entry",
	} {
		if strings.Contains(text, stale) {
			t.Fatalf("в go.mod вернулось устаревшее утверждение %q: "+
				"go.sum полный, сборка с тегом mihomo проходит без tidy", stale)
		}
	}
}
