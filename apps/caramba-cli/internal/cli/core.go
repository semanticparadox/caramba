package cli

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/semanticparadox/caramba/libs/caramba-core/api"
)

// Переменные окружения для конфигурации CLI. PanelBaseURL — единственная
// обязательная (можно переопределить флагом --panel).
const (
	envPanelURL  = "CARAMBA_PANEL_URL"
	envSubURL    = "CARAMBA_SUB_URL"
	envWorkDir   = "CARAMBA_WORK_DIR"
	envTokenPath = "CARAMBA_TOKEN_PATH"
	envSubID     = "CARAMBA_SUBSCRIPTION_ID"

	defaultPanelURL = "https://exarobot.top"
)

// globalFlags — флаги, общие для всех команд (привязываются в root).
type globalFlags struct {
	panelURL string
	subURL   string
	workDir  string
	subID    string
}

// resolve заполняет пустые флаги значениями из окружения и умолчаний.
func (g *globalFlags) resolve() {
	if g.panelURL == "" {
		g.panelURL = envOr(envPanelURL, defaultPanelURL)
	}
	if g.subURL == "" {
		g.subURL = os.Getenv(envSubURL)
	}
	if g.workDir == "" {
		g.workDir = os.Getenv(envWorkDir)
	}
	if g.subID == "" {
		g.subID = os.Getenv(envSubID)
	}
}

// newCore собирает фасад caramba-core из глобальных флагов/окружения и, если
// известен UUID подписки, сразу его привязывает.
func (g *globalFlags) newCore() (*api.Core, error) {
	g.resolve()
	core, err := api.NewCore(api.Config{
		PanelBaseURL:   g.panelURL,
		SubBaseURL:     g.subURL,
		WorkDir:        g.workDir,
		TokenStorePath: os.Getenv(envTokenPath),
	})
	if err != nil {
		return nil, err
	}
	if g.subID != "" {
		core.SetSubscriptionID(g.subID)
	}
	return core, nil
}

// configPath повторяет логику api.NewCore для определения каталога конфига:
// рабочий каталог из флага/окружения либо <UserConfigDir>/caramba, файл
// config.yaml внутри. Используется командой `config`, т.к. фасад не отдаёт путь
// напрямую до запуска Up.
func (g *globalFlags) configPath() (string, error) {
	g.resolve()
	dir := g.workDir
	if dir == "" {
		uc, err := os.UserConfigDir()
		if err != nil {
			return "", fmt.Errorf("каталог конфигурации: %w", err)
		}
		dir = filepath.Join(uc, "caramba")
	}
	return filepath.Join(dir, "config.yaml"), nil
}

func envOr(key, def string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return def
}
