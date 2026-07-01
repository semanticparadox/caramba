package cli

import (
	"fmt"
	"os"
	"strings"
)

// ANSI-цвета. Отключаются автоматически, если вывод не в терминал или задан
// NO_COLOR (см. https://no-color.org/).
const (
	cReset  = "\033[0m"
	cBold   = "\033[1m"
	cDim    = "\033[2m"
	cRed    = "\033[31m"
	cGreen  = "\033[32m"
	cYellow = "\033[33m"
	cCyan   = "\033[36m"
	cGray   = "\033[90m"
)

// colorEnabled — кэш решения о раскраске.
var colorEnabled = detectColor()

func detectColor() bool {
	if _, ok := os.LookupEnv("NO_COLOR"); ok {
		return false
	}
	if os.Getenv("TERM") == "dumb" {
		return false
	}
	fi, err := os.Stdout.Stat()
	if err != nil {
		return false
	}
	// Раскрашиваем только если stdout — символьное устройство (терминал).
	return fi.Mode()&os.ModeCharDevice != 0
}

// paint оборачивает текст в ANSI-код, если раскраска включена.
func paint(code, s string) string {
	if !colorEnabled {
		return s
	}
	return code + s + cReset
}

func bold(s string) string   { return paint(cBold, s) }
func dim(s string) string    { return paint(cDim, s) }
func red(s string) string    { return paint(cRed, s) }
func green(s string) string  { return paint(cGreen, s) }
func yellow(s string) string { return paint(cYellow, s) }
func cyan(s string) string   { return paint(cCyan, s) }
func gray(s string) string   { return paint(cGray, s) }

// PrintError печатает ошибку в stderr единым стилем.
func PrintError(err error) {
	fmt.Fprintf(os.Stderr, "%s %s\n", red("✗ ошибка:"), err.Error())
}

// printOK печатает успешное сообщение.
func printOK(format string, a ...any) {
	fmt.Printf("%s %s\n", green("✓"), fmt.Sprintf(format, a...))
}

// printInfo печатает нейтральное сообщение.
func printInfo(format string, a ...any) {
	fmt.Printf("%s %s\n", cyan("•"), fmt.Sprintf(format, a...))
}

// kv печатает строку «ключ: значение» с выравниванием ключа по ширине width.
func kv(key, value string, width int) {
	pad := width - len(key)
	if pad < 0 {
		pad = 0
	}
	fmt.Printf("  %s%s  %s\n", gray(key+":"), strings.Repeat(" ", pad), value)
}
