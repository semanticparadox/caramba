//go:build windows

package cli

// disableEcho на Windows не реализован (stty недоступна) — пароль читается с
// эхом. Возвращает no-op и false. Полноценное скрытие можно добавить позже
// через golang.org/x/term, если потребуется.
func disableEcho() (restore func(), hidden bool) {
	return func() {}, false
}
