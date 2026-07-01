package cli

import (
	"fmt"
	"os"

	"github.com/spf13/cobra"
)

// newConfigCommand — показать путь к собранному mihomo-конфигу.
func newConfigCommand(g *globalFlags) *cobra.Command {
	var pathOnly bool

	cmd := &cobra.Command{
		Use:   "config",
		Short: "Показать путь к собранному mihomo-конфигу",
		Long: "Выводит путь к config.yaml в рабочем каталоге. Файл создаётся при\n" +
			"команде `up`. Флаг --path-only печатает только путь (для скриптов).",
		RunE: func(cmd *cobra.Command, args []string) error {
			path, err := g.configPath()
			if err != nil {
				PrintError(err)
				return err
			}

			if pathOnly {
				fmt.Println(path)
				return nil
			}

			kv("путь", path, 6)
			if _, err := os.Stat(path); err != nil {
				if os.IsNotExist(err) {
					kv("статус", yellow("не создан (выполните `caramba up`)"), 6)
					return nil
				}
				kv("статус", red(err.Error()), 6)
				return nil
			}
			kv("статус", green("существует"), 6)
			return nil
		},
	}
	cmd.Flags().BoolVar(&pathOnly, "path-only", false, "вывести только путь")
	return cmd
}
