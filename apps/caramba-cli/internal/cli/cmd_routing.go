package cli

import (
	"fmt"

	"github.com/spf13/cobra"
)

// newRoutingCommand — управление режимами «умной» маршрутизации (пресетами).
func newRoutingCommand(g *globalFlags) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "routing",
		Short: "Режимы умной маршрутизации (пресеты правил)",
		Long: "Показывает доступные пресеты маршрутизации. Применить пресет можно\n" +
			"при подъёме туннеля: `caramba up --preset <id>`.",
	}
	cmd.AddCommand(newRoutingListCommand(g))
	return cmd
}

// newRoutingListCommand — вывести список доступных пресетов.
func newRoutingListCommand(g *globalFlags) *cobra.Command {
	var country string
	c := &cobra.Command{
		Use:   "list",
		Short: "Список пресетов маршрутизации",
		RunE: func(cmd *cobra.Command, args []string) error {
			core, err := g.newCore()
			if err != nil {
				PrintError(err)
				return err
			}
			presets := core.ListPresets(country)
			if len(presets) == 0 {
				printInfo("пресеты не найдены")
				return nil
			}
			fmt.Println(bold("Доступные режимы маршрутизации:"))
			fmt.Println()
			for _, p := range presets {
				tag := p.Emoji
				if p.Country != "" {
					tag += " " + p.Country
				}
				fmt.Printf("  %s  %s  %s\n", cyan(fmt.Sprintf("%-14s", p.ID)), tag, bold(p.Name))
				fmt.Printf("  %s\n\n", gray(p.Description))
			}
			fmt.Println(dim("Применить:  caramba up --preset <id>"))
			return nil
		},
	}
	c.Flags().StringVar(&country, "country", "", "показать релевантные стране пресеты первыми (ISO, напр. RU)")
	return c
}
