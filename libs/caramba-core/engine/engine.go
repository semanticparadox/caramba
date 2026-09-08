// Package engine описывает абстракцию VPN-движка и связку с ядром mihomo.
//
// Интерфейс Engine скрывает детали ядра от верхних слоёв (api, CLI, Flutter).
// Реальная реализация поверх mihomo (clash-lib) подключается через build-теги:
//   - по умолчанию собирается лёгкий stub (mihomoStub), позволяющий собирать
//     CLI/тесты без нативного ядра;
//   - сборка с тегом `mihomo` (см. engine_mihomo.go) импортирует
//     github.com/metacubex/mihomo и поднимает настоящий туннель.
package engine

import "fmt"

// State — состояние движка.
type State string

const (
	StateStopped   State = "stopped"
	StateStarting  State = "starting"
	StateConnected State = "connected"
	StateError     State = "error"
)

// Status — снимок состояния движка для UI/CLI.
type Status struct {
	State State  `json:"state"`
	// Detail — дополнительное описание (например, текст ошибки).
	Detail string `json:"detail,omitempty"`
	// ActiveProxy — имя выбранного прокси в основной группе, если известно.
	ActiveProxy string `json:"active_proxy,omitempty"`
	// ConnectedSinceMs — момент перехода в connected (unix-миллисекунды). 0 —
	// если не подключено. Нативный слой отсчитывает от него аптайм.
	ConnectedSinceMs int64 `json:"connected_since_ms,omitempty"`
}

// Traffic — мгновенная скорость и накопленные счётчики туннеля, снятые с
// статистики ядра mihomo. Поля совпадают по смыслу с контрактом traffic-канала
// нативного слоя (downBps/upBps/downTotal/upTotal).
type Traffic struct {
	// DownBps — скорость скачивания, байт/с (мгновенная).
	DownBps int64 `json:"down_bps"`
	// UpBps — скорость отдачи, байт/с (мгновенная).
	UpBps int64 `json:"up_bps"`
	// DownTotal — всего скачано за сессию, байт.
	DownTotal int64 `json:"down_total"`
	// UpTotal — всего отдано за сессию, байт.
	UpTotal int64 `json:"up_total"`
}

// Engine — управление жизненным циклом VPN-туннеля на базе mihomo.
type Engine interface {
	// Start запускает туннель из конфигурационного файла (путь к mihomo YAML).
	Start(configPath string) error
	// Stop останавливает туннель.
	Stop() error
	// Status возвращает текущее состояние.
	Status() (Status, error)
	// Traffic возвращает мгновенную скорость и накопленные счётчики туннеля.
	// Когда движок не подключён — нули. Нативный слой опрашивает ~1 Гц.
	Traffic() (Traffic, error)
	// SetTunFd передаёт ядру файловый дескриптор TUN, созданный платформой
	// (Android VpnService / iOS NetworkExtension). На десктопе, где TUN
	// поднимает само ядро, вызывать не требуется.
	SetTunFd(fd int) error
}

// ErrNotImplemented возвращается stub-движком там, где требуется нативное ядро.
var ErrNotImplemented = fmt.Errorf("engine: нативное ядро mihomo не встроено в эту сборку (соберите с -tags mihomo)")

// New возвращает реализацию движка для текущей сборки. Конкретная фабрика
// определяется build-тегами (newEngine).
func New() Engine {
	return newEngine()
}
