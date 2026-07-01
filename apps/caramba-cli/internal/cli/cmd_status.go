package cli

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/engine"
	"github.com/semanticparadox/caramba/libs/caramba-core/subscription"
	"github.com/spf13/cobra"
)

// newStatusCommand — показать состояние авторизации, движка и подписки.
func newStatusCommand(g *globalFlags) *cobra.Command {
	var asJSON bool

	cmd := &cobra.Command{
		Use:   "status",
		Short: "Показать состояние подключения и подписки",
		RunE: func(cmd *cobra.Command, args []string) error {
			core, err := g.newCore()
			if err != nil {
				PrintError(err)
				return err
			}

			ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
			defer cancel()
			res, err := core.Status(ctx)
			if err != nil {
				PrintError(err)
				return err
			}

			if asJSON {
				enc := json.NewEncoder(os.Stdout)
				enc.SetIndent("", "  ")
				if err := enc.Encode(res); err != nil {
					PrintError(err)
					return err
				}
				return nil
			}

			printStatus(res.Authenticated, res.Engine, res.Subscription)
			return nil
		},
	}
	cmd.Flags().BoolVar(&asJSON, "json", false, "вывести в формате JSON")
	return cmd
}

// printStatus печатает человекочитаемую таблицу состояния.
func printStatus(authed bool, st engine.Status, sub *subscription.Metadata) {
	fmt.Println(bold("Состояние exarobot"))
	fmt.Println(gray("─────────────────"))

	const w = 12
	authStr := red("нет")
	if authed {
		authStr = green("да")
	}
	kv("авторизация", authStr, w)
	kv("туннель", colorState(st.State), w)
	if st.ActiveProxy != "" {
		kv("прокси", st.ActiveProxy, w)
	}
	if st.Detail != "" {
		kv("детали", dim(st.Detail), w)
	}

	if sub == nil {
		return
	}
	fmt.Println()
	fmt.Println(bold("Подписка"))
	fmt.Println(gray("────────"))
	if sub.Title != "" {
		kv("тариф", sub.Title, w)
	}
	used := humanBytes(sub.Traffic.Used())
	if sub.Traffic.Total > 0 {
		kv("трафик", fmt.Sprintf("%s / %s", used, humanBytes(sub.Traffic.Total)), w)
	} else {
		kv("трафик", fmt.Sprintf("%s (безлимит)", used), w)
	}
	if !sub.Expiry.IsZero() {
		left := time.Until(sub.Expiry)
		exp := sub.Expiry.Local().Format("2006-01-02 15:04")
		if left > 0 {
			kv("истекает", fmt.Sprintf("%s (через %s)", exp, humanDuration(left)), w)
		} else {
			kv("истекает", red(exp+" (истекла)"), w)
		}
	}
	if n := len(sub.Servers); n > 0 {
		kv("серверов", fmt.Sprintf("%d", n), w)
	}
}

// colorState раскрашивает состояние движка.
func colorState(s engine.State) string {
	switch s {
	case engine.StateConnected:
		return green(string(s))
	case engine.StateStarting:
		return yellow(string(s))
	case engine.StateError:
		return red(string(s))
	default:
		return gray(string(s))
	}
}

// humanBytes форматирует байты в человекочитаемый вид (двоичные единицы).
func humanBytes(b int64) string {
	const unit = 1024
	if b < unit {
		return fmt.Sprintf("%d B", b)
	}
	div, exp := int64(unit), 0
	for n := b / unit; n >= unit; n /= unit {
		div *= unit
		exp++
	}
	return fmt.Sprintf("%.2f %ciB", float64(b)/float64(div), "KMGTPE"[exp])
}

// humanDuration форматирует длительность в краткий вид (дни/часы).
func humanDuration(d time.Duration) string {
	if d >= 24*time.Hour {
		days := int(d.Hours()) / 24
		hours := int(d.Hours()) % 24
		if hours > 0 {
			return fmt.Sprintf("%dд %dч", days, hours)
		}
		return fmt.Sprintf("%dд", days)
	}
	if d >= time.Hour {
		return fmt.Sprintf("%dч %dм", int(d.Hours()), int(d.Minutes())%60)
	}
	return fmt.Sprintf("%dм", int(d.Minutes()))
}
