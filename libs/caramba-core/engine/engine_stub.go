//go:build !mihomo

package engine

import (
	"sync"
	"time"
)

// mihomoStub — заглушка движка для сборок без нативного ядра mihomo.
//
// Она отслеживает заявленное состояние, но не поднимает реальный туннель. Это
// позволяет собирать и тестировать CLI и верхние слои (api) без CGO/gomobile.
// Реальная реализация живёт в engine_mihomo.go под build-тегом `mihomo`.
type mihomoStub struct {
	mu             sync.Mutex
	state          State
	configPath     string
	tunFd          int
	connectedSince time.Time
}

// newEngine — фабрика для сборки по умолчанию.
func newEngine() Engine {
	return &mihomoStub{state: StateStopped, tunFd: -1}
}

func (e *mihomoStub) Start(configPath string) error {
	e.mu.Lock()
	defer e.mu.Unlock()
	if configPath == "" {
		return ErrNotImplemented
	}
	// В настоящей сборке здесь происходит:
	//   1) config.Parse(configPath) из mihomo;
	//   2) executor.ApplyConfig(...) / hub.Parse(...) для старта туннеля;
	//   3) (мобильно) передача e.tunFd в tun-listener mihomo.
	// Заглушка лишь фиксирует «подключено».
	e.configPath = configPath
	e.state = StateConnected
	e.connectedSince = time.Now()
	return nil
}

func (e *mihomoStub) Stop() error {
	e.mu.Lock()
	defer e.mu.Unlock()
	// В настоящей сборке: остановка tunnel/inbound listeners mihomo.
	e.state = StateStopped
	e.connectedSince = time.Time{}
	return nil
}

func (e *mihomoStub) Status() (Status, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	st := Status{State: e.state}
	if e.state == StateConnected && !e.connectedSince.IsZero() {
		st.ConnectedSinceMs = e.connectedSince.UnixMilli()
	}
	return st, nil
}

// Traffic в заглушке всегда возвращает нули: без ядра mihomo статистики нет.
// Реальные счётчики отдаёт engine_mihomo.go под build-тегом `mihomo`.
func (e *mihomoStub) Traffic() (Traffic, error) {
	return Traffic{}, nil
}

func (e *mihomoStub) SetTunFd(fd int) error {
	e.mu.Lock()
	defer e.mu.Unlock()
	// В настоящей сборке: пробросить fd в конфиг tun-инбаунда mihomo до Start.
	e.tunFd = fd
	return nil
}
