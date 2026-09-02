//go:build mihomo

// Реальная реализация движка поверх ядра mihomo (clash.meta). Собирается только
// с build-тегом `mihomo` и требует зависимости github.com/metacubex/mihomo в
// go.mod. Без тега используется лёгкая заглушка (engine_stub.go), чтобы CLI и
// верхние слои собирались без CGO/нативного ядра.
package engine

import (
	"fmt"
	"sync"
	"time"

	"github.com/metacubex/mihomo/hub/executor"
	"github.com/metacubex/mihomo/tunnel"
	"github.com/metacubex/mihomo/tunnel/statistic"
	"github.com/semanticparadox/caramba/libs/caramba-core/profile"
)

// mihomoEngine — реальная реализация Engine поверх mihomo.
//
// Движок держит один глобальный экземпляр ядра mihomo (executor оперирует
// глобальным состоянием), поэтому в рамках процесса должен существовать ровно
// один активный mihomoEngine. Для долгоживущих процессов (Flutter-приложение,
// будущий демон) это естественно; тонкий CLI создаёт его на одну команду.
type mihomoEngine struct {
	mu         sync.Mutex
	state      State
	detail     string
	configPath string
	// tunFd — файловый дескриптор TUN, переданный платформой (Android
	// VpnService / iOS NetworkExtension). -1 означает «не задан»: на десктопе
	// mihomo поднимает TUN сам.
	tunFd int
	// connectedSince — момент успешного перехода в connected. Нулевое значение —
	// не подключено. Используется для ConnectedSinceMs в Status.
	connectedSince time.Time
}

// newEngine — фабрика для сборки с нативным ядром.
func newEngine() Engine {
	return &mihomoEngine{state: StateStopped, tunFd: -1}
}

// Start разбирает конфигурацию по пути и применяет её к ядру mihomo. Если задан
// tunFd (мобильные платформы), он прописывается в TUN-инбаунд до применения.
func (e *mihomoEngine) Start(configPath string) error {
	e.mu.Lock()
	defer e.mu.Unlock()
	if configPath == "" {
		return fmt.Errorf("engine: пустой путь конфигурации")
	}

	e.state = StateStarting
	e.detail = ""

	// ParseWithPath читает файл и собирает *config.Config (валидирует proxies,
	// proxy-groups, rules, rule-providers и т.д.).
	cfg, err := executor.ParseWithPath(configPath)
	if err != nil {
		e.state = StateError
		e.detail = err.Error()
		return fmt.Errorf("engine: разбор конфигурации mihomo: %w", err)
	}

	// Проброс TUN fd, выданного платформой. На десктопе tunFd == -1 и секция
	// TUN из конфига используется как есть (ядро само создаёт устройство).
	if e.tunFd >= 0 {
		if cfg.General == nil {
			e.state = StateError
			e.detail = "конфигурация без секции general"
			return fmt.Errorf("engine: конфигурация без секции general")
		}
		// Включаем TUN-инбаунд и подменяем источник на готовый дескриптор.
		cfg.General.Tun.Enable = true
		cfg.General.Tun.FileDescriptor = e.tunFd
		// На мобильных платформах системные маршруты ставит сама ОС (VpnService/
		// NEPacketTunnelProvider), поэтому ядру auto-route не нужен.
		cfg.General.Tun.AutoRoute = false
		cfg.General.Tun.AutoDetectInterface = false
	}

	// force=true — полностью переинициализировать ядро под новый конфиг
	// (inbound-listeners, прокси, правила, DNS, TUN).
	executor.ApplyConfig(cfg, true)

	// Туннель работает в режиме rule. Режим форсируется намеренно: список rules
	// целиком формирует profile-сборка (split-tunnel, kill-switch, пресеты), и
	// она же владеет финальным MATCH — поэтому конфиг панели не должен переключать
	// движок в global/direct в обход политики клиента.
	tunnel.SetMode(tunnel.Rule)

	e.configPath = configPath
	e.state = StateConnected
	e.connectedSince = time.Now()
	return nil
}

// Stop останавливает все listeners и сбрасывает состояние ядра.
func (e *mihomoEngine) Stop() error {
	e.mu.Lock()
	defer e.mu.Unlock()
	// Shutdown закрывает inbound-listeners (включая TUN) и сбрасывает провайдеры.
	executor.Shutdown()
	e.state = StateStopped
	e.detail = ""
	e.connectedSince = time.Time{}
	// Сбрасываем накопленные счётчики статистики, чтобы новая сессия считалась
	// с нуля (менеджер mihomo глобальный и переживает рестарт туннеля).
	statistic.DefaultManager.ResetStatistic()
	return nil
}

// Status возвращает текущее состояние и активный прокси основной группы.
func (e *mihomoEngine) Status() (Status, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	st := Status{State: e.state, Detail: e.detail}
	if e.state == StateConnected {
		st.ActiveProxy = activeProxyName()
		if !e.connectedSince.IsZero() {
			st.ConnectedSinceMs = e.connectedSince.UnixMilli()
		}
	}
	return st, nil
}

// Traffic снимает мгновенную скорость и накопленные счётчики с глобального
// менеджера статистики mihomo. Когда туннель не поднят — нули.
//
// statistic.DefaultManager.Now() отдаёт скорость за последнюю секунду
// (up, down), а Snapshot() — суммарные DownloadTotal/UploadTotal. Менеджер
// глобальный (на процесс), поэтому Stop сбрасывает его через ResetStatistic.
func (e *mihomoEngine) Traffic() (Traffic, error) {
	e.mu.Lock()
	connected := e.state == StateConnected
	e.mu.Unlock()
	if !connected {
		return Traffic{}, nil
	}
	up, down := statistic.DefaultManager.Now()
	snap := statistic.DefaultManager.Snapshot()
	return Traffic{
		DownBps:   int64(down),
		UpBps:     int64(up),
		DownTotal: snap.DownloadTotal,
		UpTotal:   snap.UploadTotal,
	}, nil
}

// SetTunFd сохраняет файловый дескриптор TUN для проброса в ядро при Start.
// Вызывать до Up/Start. Передача -1 возвращает поведение «ядро поднимает TUN
// само» (десктоп).
func (e *mihomoEngine) SetTunFd(fd int) error {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.tunFd = fd
	return nil
}

// activeProxyName возвращает имя прокси, выбранного в основной группе-селекторе
// CARAMBA. Если группы нет (или это не selector) — пустая строка.
//
// Имя основной группы берём из единого источника profile.CarambaSelector (тот же
// пакет формирует селектор при сборке конфига), чтобы переименование панелью не
// рассинхронизировало движок и сборку. Импорт безопасен: profile не зависит от
// engine, цикла нет (а файл и так собирается только под -tags mihomo).
func activeProxyName() string {
	mainGroup := profile.CarambaSelector
	proxies := tunnel.Proxies()
	p, ok := proxies[mainGroup]
	if !ok || p == nil {
		return ""
	}
	// C.Proxy у групп-селекторов отдаёт текущий выбор через Now().
	type nower interface{ Now() string }
	if n, ok := p.(nower); ok {
		return n.Now()
	}
	return ""
}
