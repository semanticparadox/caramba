package cli

import (
	"fmt"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/api"
	"github.com/semanticparadox/caramba/libs/caramba-core/engine"
)

// foregroundTick — период опроса движка в фоновом режиме (~1 Гц). Совпадает с
// частотой, на которой нативный слой опрашивает traffic/status-каналы.
const foregroundTick = time.Second

// humanRate форматирует мгновенную скорость (байт/с) в человекочитаемый вид с
// двоичными единицами (как humanBytes) и суффиксом «/s». Чистая функция.
func humanRate(bps int64) string {
	return humanBytes(bps) + "/s"
}

// formatTunnelLine рендерит одну компактную строку состояния туннеля для
// фонового режима: состояние (+прокси, если известен), мгновенные скорости и
// накопленные счётчики. Чистая и детерминированная — без I/O, времени и
// сигналов, поэтому её легко покрыть таблицей тестов. Раскраска состояния
// делегируется colorState (в не-терминале она сама собой отключается).
func formatTunnelLine(st engine.Status, tr engine.Traffic) string {
	state := colorState(st.State)
	if st.ActiveProxy != "" {
		state = fmt.Sprintf("%s [%s]", state, st.ActiveProxy)
	}
	return fmt.Sprintf(
		"%s  ↓ %s  ↑ %s  всего ↓ %s ↑ %s",
		state,
		humanRate(tr.DownBps),
		humanRate(tr.UpBps),
		humanBytes(tr.DownTotal),
		humanBytes(tr.UpTotal),
	)
}

// runForeground удерживает процесс открытым после успешного Up: раз в секунду
// печатает строку состояния (formatTunnelLine) и ждёт SIGINT/SIGTERM. По сигналу
// гасит туннель (core.Down) и печатает строку разрыва, возвращая nil (выход 0).
//
// Это решает проблему одноразового CLI: api.Core.Up возвращает управление сразу,
// а без удержания процесс завершается и OS-уровневый utun разрушается вместе с
// ним (см. комментарий в cmd_tunnel.go). Флаг --foreground держит туннель живым,
// пока пользователь не нажмёт Ctrl-C.
func runForeground(core *api.Core) error {
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, os.Interrupt, syscall.SIGTERM)
	defer signal.Stop(sig)

	printInfo("фоновый режим: Ctrl-C для остановки")

	// Печатаем первую строку сразу, не дожидаясь первого тика.
	printTunnelTick(core)

	ticker := time.NewTicker(foregroundTick)
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			printTunnelTick(core)
		case <-sig:
			// Пустая строка, чтобы отделить вывод от ^C в терминале.
			fmt.Println()
			if err := core.Down(); err != nil {
				PrintError(err)
				return err
			}
			printOK("туннель остановлен")
			return nil
		}
	}
}

// printTunnelTick снимает состояние и трафик движка и печатает одну строку.
// Ошибки опроса не фатальны для цикла: показываем деградированную строку и
// продолжаем удерживать процесс (движок мог ещё подниматься).
func printTunnelTick(core *api.Core) {
	st, err := core.EngineStatus()
	if err != nil {
		st = engine.Status{State: engine.StateError, Detail: err.Error()}
	}
	tr, err := core.Traffic()
	if err != nil {
		tr = engine.Traffic{}
	}
	fmt.Println(formatTunnelLine(st, tr))
}
