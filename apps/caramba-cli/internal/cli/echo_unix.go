//go:build !windows

package cli

import (
	"os"
	"os/exec"
)

// disableEcho выключает отображение вводимых символов на unix-терминале через
// `stty -echo`. Возвращает функцию восстановления и признак того, что эхо
// действительно было отключено. Если stdin не терминал или stty недоступна,
// возвращает (no-op, false) — вызывающий код тогда читает пароль с эхом.
func disableEcho() (restore func(), hidden bool) {
	noop := func() {}
	fi, err := os.Stdin.Stat()
	if err != nil || fi.Mode()&os.ModeCharDevice == 0 {
		return noop, false
	}
	if err := sttyEcho(false); err != nil {
		return noop, false
	}
	return func() { _ = sttyEcho(true) }, true
}

func sttyEcho(on bool) error {
	arg := "-echo"
	if on {
		arg = "echo"
	}
	cmd := exec.Command("stty", arg)
	cmd.Stdin = os.Stdin
	return cmd.Run()
}
