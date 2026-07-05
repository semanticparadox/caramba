package cli

import (
	"context"

	"github.com/spf13/cobra"
)

// ВНИМАНИЕ: каждая команда CLI создаёт новый api.Core через g.newCore(), а
// сборка по умолчанию использует заглушку движка (mihomoStub) с состоянием
// только в памяти процесса. Поэтому `down`/`status` в отдельных запусках НЕ
// видят туннель, поднятый предыдущим `up`. Сквозной жизненный цикл заработает
// лишь с реальным движком mihomo (OS-уровень TUN/процесс) или фоновым демоном —
// см. TODO на типе api.Core.

// newUpCommand — поднять туннель. Необязательный аргумент [server] закрепляет
// конкретный выходной узел (node_id в запросе подписки).
func newUpCommand(g *globalFlags) *cobra.Command {
	var preset string
	var relay string
	var protocol string
	var bypassDomains []string
	var apps []string
	var appMode string
	var foreground bool
	cmd := &cobra.Command{
		Use:   "up [server]",
		Short: "Поднять VPN-туннель",
		Long: "Загружает подписку, собирает mihomo-конфиг и запускает туннель.\n" +
			"Необязательный аргумент [server] — ID выходного узла для закрепления.\n" +
			"Флаг --preset выбирает режим маршрутизации (см. `exarobot routing list`).",
		Args: cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			core, err := g.newCore()
			if err != nil {
				PrintError(err)
				return err
			}
			if preset != "" {
				if err := core.ApplyPreset(preset); err != nil {
					PrintError(err)
					return err
				}
				printInfo("режим маршрутизации: %s", preset)
			}
			if relay != "" {
				core.SetRelay(relay)
				printInfo("relay (вход через): %s", relay)
			}
			if protocol != "" {
				core.SetProtocol(protocol)
				printInfo("протокол: %s", protocol)
			}
			if len(bypassDomains) > 0 || len(apps) > 0 {
				core.SetSplitTunnel(bypassDomains, appMode, apps)
				if len(apps) > 0 {
					mode := appMode
					if mode == "" {
						mode = "bypass"
					}
					printInfo("split-tunnel: %s для %d приложений", mode, len(apps))
				}
				if len(bypassDomains) > 0 {
					printInfo("байпас доменов: %d", len(bypassDomains))
				}
			}
			server := ""
			if len(args) == 1 {
				server = args[0]
			}

			printInfo("поднимаю туннель...")
			res, err := core.Up(context.Background(), server)
			if err != nil {
				PrintError(err)
				return err
			}
			printOK("туннель запущен (%s)", colorState(res.Engine.State))
			kv("конфиг", res.ConfigPath, 8)
			if res.Engine.ActiveProxy != "" {
				kv("прокси", res.Engine.ActiveProxy, 8)
			}

			// Фоновый режим удерживает процесс открытым: Up возвращается сразу, а
			// одноразовый CLI-процесс, завершившись, разрушил бы OS-уровневый utun
			// вместе с собой (см. комментарий вверху файла). --foreground держит
			// туннель живым и опрашивает движок ~1 Гц до Ctrl-C.
			if foreground {
				return runForeground(core)
			}
			return nil
		},
	}
	cmd.Flags().StringVar(&preset, "preset", "", "режим маршрутизации (напр. ru-smart, telegram-only)")
	cmd.Flags().StringVar(&relay, "relay", "", "вход через страну (relay), ISO-2 напр. TR, KZ, FI")
	cmd.Flags().StringVar(&protocol, "protocol", "", "протокол (AmneziaWG, VLESS-Reality, Hysteria2, TUIC, Shadowsocks)")
	cmd.Flags().StringSliceVar(&bypassDomains, "bypass-domain", nil, "домен мимо туннеля (DIRECT), можно повторять")
	cmd.Flags().StringSliceVar(&apps, "app", nil, "приложение/процесс для split-tunnel, можно повторять")
	cmd.Flags().StringVar(&appMode, "app-mode", "bypass", "режим --app: bypass (эти мимо туннеля) или allow (только эти в туннель)")
	cmd.Flags().BoolVarP(&foreground, "foreground", "f", false, "не завершаться: держать туннель открытым и опрашивать состояние до Ctrl-C")
	return cmd
}

// newDownCommand — остановить туннель.
func newDownCommand(g *globalFlags) *cobra.Command {
	return &cobra.Command{
		Use:   "down",
		Short: "Остановить VPN-туннель",
		RunE: func(cmd *cobra.Command, args []string) error {
			core, err := g.newCore()
			if err != nil {
				PrintError(err)
				return err
			}
			if err := core.Down(); err != nil {
				PrintError(err)
				return err
			}
			printOK("туннель остановлен")
			return nil
		},
	}
}
