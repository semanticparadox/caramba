package cli

import (
	"testing"

	"github.com/semanticparadox/caramba/libs/caramba-core/engine"
)

// TestHumanRate проверяет форматирование мгновенной скорости (байт/с) в
// человекочитаемый вид. Единицы — двоичные (как у humanBytes), с суффиксом /s.
func TestHumanRate(t *testing.T) {
	cases := []struct {
		name string
		bps  int64
		want string
	}{
		{"ноль", 0, "0 B/s"},
		{"байты", 512, "512 B/s"},
		{"килобайты", 1024, "1.00 KiB/s"},
		{"полтора_кило", 1536, "1.50 KiB/s"},
		{"мегабайты", 5 * 1024 * 1024, "5.00 MiB/s"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := humanRate(tc.bps); got != tc.want {
				t.Fatalf("humanRate(%d) = %q, want %q", tc.bps, got, tc.want)
			}
		})
	}
}

// TestFormatTunnelLine проверяет чистое рендеринг одной строки состояния
// фонового режима. Функция детерминирована: без I/O, времени и цвета
// (colorEnabled в тестах выключен, т.к. stdout не терминал), поэтому сравниваем
// строки напрямую.
func TestFormatTunnelLine(t *testing.T) {
	cases := []struct {
		name string
		st   engine.Status
		tr   engine.Traffic
		want string
	}{
		{
			name: "остановлен_нулевой_трафик",
			st:   engine.Status{State: engine.StateStopped},
			tr:   engine.Traffic{},
			want: "stopped  ↓ 0 B/s  ↑ 0 B/s  всего ↓ 0 B ↑ 0 B",
		},
		{
			name: "подключается",
			st:   engine.Status{State: engine.StateStarting},
			tr:   engine.Traffic{},
			want: "starting  ↓ 0 B/s  ↑ 0 B/s  всего ↓ 0 B ↑ 0 B",
		},
		{
			name: "подключён_с_прокси_и_трафиком",
			st:   engine.Status{State: engine.StateConnected, ActiveProxy: "TR-1"},
			tr: engine.Traffic{
				DownBps:   2 * 1024 * 1024,
				UpBps:     256 * 1024,
				DownTotal: 10 * 1024 * 1024,
				UpTotal:   1536 * 1024,
			},
			want: "connected [TR-1]  ↓ 2.00 MiB/s  ↑ 256.00 KiB/s  всего ↓ 10.00 MiB ↑ 1.50 MiB",
		},
		{
			name: "подключён_без_прокси",
			st:   engine.Status{State: engine.StateConnected},
			tr:   engine.Traffic{DownBps: 1024, UpBps: 0, DownTotal: 2048, UpTotal: 512},
			want: "connected  ↓ 1.00 KiB/s  ↑ 0 B/s  всего ↓ 2.00 KiB ↑ 512 B",
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := formatTunnelLine(tc.st, tc.tr); got != tc.want {
				t.Fatalf("formatTunnelLine() =\n  %q\nwant\n  %q", got, tc.want)
			}
		})
	}
}
