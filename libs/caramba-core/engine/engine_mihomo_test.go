//go:build mihomo

package engine

import "testing"

// TestShouldForceFindProcessOff проверяет, что принудительное выключение
// поиска процесса mihomo (см. Start и комментарий у shouldForceFindProcessOff)
// срабатывает только на Android. Раньше единый гейт `tunFd >= 0` гасил
// PROCESS-NAME-правила и на macOS/iOS — эта регрессия и есть повод для теста.
func TestShouldForceFindProcessOff(t *testing.T) {
	cases := []struct {
		goos string
		want bool
	}{
		{goos: "android", want: true},
		{goos: "darwin", want: false},
		{goos: "ios", want: false},
		{goos: "windows", want: false},
		{goos: "linux", want: false},
		{goos: "", want: false},
	}

	for _, tc := range cases {
		t.Run(tc.goos, func(t *testing.T) {
			got := shouldForceFindProcessOff(tc.goos)
			if got != tc.want {
				t.Errorf("shouldForceFindProcessOff(%q) = %v, want %v", tc.goos, got, tc.want)
			}
		})
	}
}
