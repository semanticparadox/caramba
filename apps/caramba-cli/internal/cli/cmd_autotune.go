package cli

import (
	"context"
	"encoding/json"
	"os"
	"time"

	"github.com/spf13/cobra"
)

// newAutoTuneCommand — автоподбор сервера/протокола/relay по измерению сети.
//
// Использует Prober по умолчанию (TCP-замер серверов подписки). Честная проверка
// handshake протоколов доступна в сборке с -tags mihomo (там фасад core может
// подменить Prober на autotune.MihomoProber). Команда применяет protocol/relay/
// stack к политике и печатает рекомендацию; туннель не поднимает — выполните
// `caramba up <server>` с рекомендованным ID.
func newAutoTuneCommand(g *globalFlags) *cobra.Command {
	var asJSON bool
	var apply bool

	cmd := &cobra.Command{
		Use:   "autotune",
		Short: "Подобрать лучший сервер/протокол/relay по измерению сети",
		Long: "Измеряет доступность серверов подписки и рекомендует выходной узел,\n" +
			"протокол и при необходимости relay-вход. С флагом --up сразу поднимает\n" +
			"туннель на рекомендованный сервер.",
		RunE: func(cmd *cobra.Command, args []string) error {
			core, err := g.newCore()
			if err != nil {
				PrintError(err)
				return err
			}

			ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
			defer cancel()

			printInfo("измеряю сеть...")
			prober, err := core.NewDefaultProber(ctx)
			if err != nil {
				PrintError(err)
				return err
			}
			rec, err := core.AutoTune(ctx, prober)
			if err != nil {
				PrintError(err)
				return err
			}

			if asJSON {
				enc := json.NewEncoder(os.Stdout)
				enc.SetIndent("", "  ")
				if err := enc.Encode(rec); err != nil {
					PrintError(err)
					return err
				}
			} else {
				printOK("рекомендация готова")
				kv("сервер", rec.ServerID, 10)
				kv("протокол", rec.Protocol, 10)
				kv("стек", rec.Stack, 10)
				if rec.Relay != "" {
					kv("relay", rec.Relay, 10)
				} else {
					kv("relay", "прямой вход", 10)
				}
				kv("причина", rec.Reason, 10)
			}

			if apply {
				printInfo("поднимаю туннель на рекомендованный сервер...")
				res, err := core.Up(ctx, rec.ServerID)
				if err != nil {
					PrintError(err)
					return err
				}
				printOK("туннель запущен (%s)", colorState(res.Engine.State))
				kv("конфиг", res.ConfigPath, 10)
			}
			return nil
		},
	}
	cmd.Flags().BoolVar(&asJSON, "json", false, "вывести рекомендацию в формате JSON")
	cmd.Flags().BoolVar(&apply, "up", false, "сразу поднять туннель на рекомендованный сервер")
	return cmd
}
