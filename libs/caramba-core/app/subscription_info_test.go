package app

import (
	"context"
	"io"
	"net/http"
	"strings"
	"testing"
)

// fakeAuth подменяет авторизованного исполнителя запросов в тестах.
type fakeAuth struct {
	base string
	resp *http.Response
	err  error
	got  *http.Request
}

func (f *fakeAuth) BaseURL() string { return f.base }

func (f *fakeAuth) DoAuthorized(req *http.Request) (*http.Response, error) {
	f.got = req
	if f.err != nil {
		return nil, f.err
	}
	return f.resp, nil
}

func jsonResp(status int, body string) *http.Response {
	return &http.Response{
		StatusCode: status,
		Body:       io.NopCloser(strings.NewReader(body)),
		Header:     http.Header{"Content-Type": []string{"application/json"}},
	}
}

func TestFetchParsesSubscriptionUUID(t *testing.T) {
	fa := &fakeAuth{
		base: "https://panel.test",
		resp: jsonResp(200, `{
			"id": 7,
			"subscription_uuid": "abc-123-uuid",
			"plan_name": "Pro",
			"status": "active",
			"clash_url": "https://panel.test/sub/abc-123-uuid?client=clash"
		}`),
	}
	c := NewSubscriptionClient(fa)

	info, err := c.Fetch(context.Background())
	if err != nil {
		t.Fatalf("fetch: %v", err)
	}
	if info.UUID != "abc-123-uuid" {
		t.Fatalf("UUID = %q, ожидалось abc-123-uuid", info.UUID)
	}
	if info.Status != "active" || info.PlanName != "Pro" {
		t.Fatalf("неверно разобраны поля: %+v", info)
	}
	// Путь обязан совпадать с app_routes() панели.
	if fa.got == nil || fa.got.URL.Path != "/api/v2/app/subscription" {
		t.Fatalf("неожиданный путь запроса: %+v", fa.got)
	}
}

func TestFetchUUIDConvenience(t *testing.T) {
	fa := &fakeAuth{
		base: "https://panel.test",
		resp: jsonResp(200, `{"subscription_uuid":"only-uuid"}`),
	}
	uuid, err := NewSubscriptionClient(fa).FetchUUID(context.Background())
	if err != nil {
		t.Fatalf("fetch uuid: %v", err)
	}
	if uuid != "only-uuid" {
		t.Fatalf("uuid = %q", uuid)
	}
}

func TestFetchErrorsOnMissingUUID(t *testing.T) {
	fa := &fakeAuth{
		base: "https://panel.test",
		resp: jsonResp(200, `{"id":1,"status":"active"}`),
	}
	if _, err := NewSubscriptionClient(fa).Fetch(context.Background()); err == nil {
		t.Fatal("ожидалась ошибка при отсутствии subscription_uuid")
	}
}

func TestFetchErrorsOnNon2xx(t *testing.T) {
	fa := &fakeAuth{
		base: "https://panel.test",
		resp: jsonResp(404, `No subscription found`),
	}
	if _, err := NewSubscriptionClient(fa).Fetch(context.Background()); err == nil {
		t.Fatal("ожидалась ошибка при статусе 404")
	}
}
