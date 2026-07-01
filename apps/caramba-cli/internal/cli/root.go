package cli

import (
	"github.com/spf13/cobra"
)

// version — версия CLI; перезаписывается через -ldflags при сборке релиза.
var version = "dev"

// NewRootCommand строит корневую команду exarobot со всеми подкомандами.
func NewRootCommand() *cobra.Command {
	g := &globalFlags{}

	root := &cobra.Command{
		Use:   "exarobot",
		Short: "exarobot — устойчивый к цензуре VPN-клиент (CLI)",
		Long: bold("exarobot") + " — командный клиент exarobot.\n\n" +
			"Поднимает mihomo-туннель по подписке панели. Подходит для\n" +
			"Linux-серверов и desktop без графического интерфейса.\n\n" +
			"Быстрый старт:\n" +
			"  caramba login --email you@example.com\n" +
			"  exarobot up\n" +
			"  exarobot status\n" +
			"  exarobot down",
		Version:       version,
		SilenceUsage:  true,
		SilenceErrors: true,
	}

	// Глобальные флаги конфигурации.
	pf := root.PersistentFlags()
	pf.StringVar(&g.panelURL, "panel", "", "URL панели (env CARAMBA_PANEL_URL)")
	pf.StringVar(&g.subURL, "sub", "", "URL сервиса подписок (env CARAMBA_SUB_URL)")
	pf.StringVar(&g.workDir, "work-dir", "", "рабочий каталог для конфига/кэша (env CARAMBA_WORK_DIR)")
	pf.StringVar(&g.subID, "subscription-id", "", "UUID подписки (env CARAMBA_SUBSCRIPTION_ID)")

	root.AddCommand(
		newLoginCommand(g),
		newLogoutCommand(g),
		newUpCommand(g),
		newDownCommand(g),
		newAutoTuneCommand(g),
		newStatusCommand(g),
		newConfigCommand(g),
		newRoutingCommand(g),
	)

	return root
}
