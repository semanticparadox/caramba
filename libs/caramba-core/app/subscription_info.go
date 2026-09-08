// Package app содержит авторизованные клиенты к приложенческому API панели
// caramba (/api/v2/app/*), которым нужен Bearer-токен.
//
// В отличие от пакета subscription (он тянет публичный /sub/{uuid} mihomo-конфиг
// по уже известному UUID), здесь живут вызовы, требующие авторизации через
// auth.PanelClient.DoAuthorized — прежде всего получение UUID подписки текущего
// пользователя после входа.
package app

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
)

// AuthorizedDoer выполняет авторизованный запрос (подставляет Bearer-токен и
// обновляет его при 401). Реализуется auth.PanelClient. Интерфейс введён для
// развязки пакетов и тестируемости.
type AuthorizedDoer interface {
	// BaseURL возвращает корневой URL панели.
	BaseURL() string
	// DoAuthorized выполняет запрос с авторизацией; вызывающий обязан закрыть тело.
	DoAuthorized(req *http.Request) (*http.Response, error)
}

// SubscriptionInfo — разобранный ответ GET /api/v2/app/subscription. Содержит
// только то, что нужно ядру; лишние поля панели игнорируются.
type SubscriptionInfo struct {
	// UUID — subscription_uuid, по которому тянется mihomo-конфиг с /sub/{uuid}.
	UUID string `json:"subscription_uuid"`
	// Status — статус подписки (например "active").
	Status string `json:"status,omitempty"`
	// PlanName — название тарифа.
	PlanName string `json:"plan_name,omitempty"`
	// ClashURL — готовый URL clash/mihomo-конфига (его использует mihomo-ядро).
	ClashURL string `json:"clash_url,omitempty"`
}

// SubscriptionClient получает данные подписки авторизованного пользователя.
type SubscriptionClient struct {
	auth AuthorizedDoer
}

// NewSubscriptionClient создаёт клиент поверх авторизованного исполнителя
// запросов (обычно *auth.PanelClient).
func NewSubscriptionClient(a AuthorizedDoer) *SubscriptionClient {
	return &SubscriptionClient{auth: a}
}

// Fetch запрашивает GET /api/v2/app/subscription и возвращает данные подписки
// текущего пользователя. Эндпоинт защищён JWT — запрос идёт через DoAuthorized.
func (c *SubscriptionClient) Fetch(ctx context.Context) (SubscriptionInfo, error) {
	url := c.auth.BaseURL() + "/api/v2/app/subscription"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return SubscriptionInfo{}, fmt.Errorf("app: создание запроса подписки: %w", err)
	}
	req.Header.Set("Accept", "application/json")

	resp, err := c.auth.DoAuthorized(req)
	if err != nil {
		return SubscriptionInfo{}, fmt.Errorf("app: запрос подписки: %w", err)
	}
	defer func() {
		_, _ = io.Copy(io.Discard, resp.Body)
		_ = resp.Body.Close()
	}()

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return SubscriptionInfo{}, fmt.Errorf("app: чтение ответа подписки: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return SubscriptionInfo{}, fmt.Errorf("app: панель вернула %d по подписке", resp.StatusCode)
	}

	var info SubscriptionInfo
	if err := json.Unmarshal(data, &info); err != nil {
		return SubscriptionInfo{}, fmt.Errorf("app: разбор данных подписки: %w", err)
	}
	if info.UUID == "" {
		return SubscriptionInfo{}, fmt.Errorf("app: панель не вернула subscription_uuid")
	}
	return info, nil
}

// FetchUUID — удобная обёртка: возвращает только UUID подписки.
func (c *SubscriptionClient) FetchUUID(ctx context.Context) (string, error) {
	info, err := c.Fetch(ctx)
	if err != nil {
		return "", err
	}
	return info.UUID, nil
}
