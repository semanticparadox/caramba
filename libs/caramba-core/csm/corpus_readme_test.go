package csm

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// README корпуса это документация merge gate: третья реализация (подписант и
// проверяющий на Rust) читает именно его, а не vectors.json построчно. Когда
// README расходится с корпусом, шлюз описан неверно, и расхождение не видно
// никому: тесты сходятся с vectors.json, а человек сверяется с README.
//
// Тест держит README и vectors.json в одном счёте. Дешевле сверять числа, чем
// объяснять, почему в README 138 векторов, а на диске 143.
func TestCorpusReadmeCountsMatch(t *testing.T) {
	c := loadCorpus(t)

	path := filepath.Join(corpusRel, "README.md")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("не читается %s: %v", path, err)
	}
	readme := string(raw)

	// Сколько чего в корпусе на самом деле.
	var negative int
	for _, v := range c.Vectors {
		if v.Group == "negative" {
			negative++
		}
	}

	checks := []struct {
		what string
		want string
	}{
		{"число векторов в разделе 4", fmt.Sprintf("| `vectors` | %d frame fixtures. |", len(c.Vectors))},
		{"заголовок раздела 6.2", fmt.Sprintf("### 6.2 Negative frames, %d", negative)},
		{"строка bin/negative в разделе 1", fmt.Sprintf("negative/   %d files", negative)},
		{"ожидаемый вывод генератора", fmt.Sprintf("%d positive, %d negative", c.Counts["positive"], negative)},
	}
	for _, ch := range checks {
		if !strings.Contains(readme, ch.want) {
			t.Errorf("README разошёлся с корпусом (%s): ожидалась строка %q", ch.what, ch.want)
		}
	}

	// Раздел 5 обязан описывать bound_cat и bound_tier как ПОЛЯ контекста.
	// Пока их там нет, реализация, написанная по README, выковыривает cat и
	// tier из прозаической заметки вектора и проходит V14a/V14b случайно.
	for _, field := range []string{"`bound_cat`", "`bound_tier`"} {
		if !strings.Contains(readme, field) {
			t.Errorf("README не описывает поле контекста %s: харнессу придётся читать note", field)
		}
	}
}
