// Command caramba — тонкий CLI поверх caramba-core для Linux/desktop-сценария
// использования VPN. Все команды (login/logout/up/down/status/config) делегируют
// в фасад github.com/semanticparadox/caramba/libs/caramba-core/api; здесь —
// только разбор флагов и оформление вывода.
package main

import (
	"fmt"
	"os"

	"github.com/semanticparadox/caramba/apps/caramba-cli/internal/cli"
)

func main() {
	if err := cli.NewRootCommand().Execute(); err != nil {
		// cobra уже печатает ошибку использования; здесь дублируем код выхода,
		// чтобы скрипты могли его распознать. Текст ошибки команды печатается
		// внутри RunE через cli.PrintError, поэтому тут только код возврата.
		fmt.Fprintln(os.Stderr)
		os.Exit(1)
	}
}
