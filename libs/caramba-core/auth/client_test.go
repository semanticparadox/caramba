package auth

import (
	"context"
	"io"
	"net/http"
	"strings"
	"sync/atomic"
	"testing"
)

// roundTripFunc позволяет подменить транспорт в тестах.
type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) Do(req *http.Request) (*http.Response, error) { return f(req) }

func jsonResp(status int, body string) *http.Response {
	return &http.Response{
		StatusCode: status,
		Body:       io.NopCloser(strings.NewReader(body)),
		Header:     http.Header{"Content-Type": []string{"application/json"}},
	}
}

func TestLoginEmailStoresTokens(t *testing.T) {
	store := NewMemoryStore()
	rt := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		// Путь обязан совпадать с app_routes() панели: /api/v2/app/login/email.
		if req.URL.Path != "/api/v2/app/login/email" {
			t.Fatalf("неожиданный путь %s", req.URL.Path)
		}
		return jsonResp(200, `{"access_token":"a1","refresh_token":"r1","expires_in":3600}`), nil
	})
	c := NewPanelClient("https://panel.test", WithHTTPClient(rt), WithStore(store))

	tk, err := c.LoginEmail(context.Background(), "u@e.com", "pw")
	if err != nil {
		t.Fatalf("login: %v", err)
	}
	if tk.AccessToken != "a1" || tk.RefreshToken != "r1" {
		t.Fatalf("неверные токены: %+v", tk)
	}
	saved, _ := store.Load()
	if saved.AccessToken != "a1" {
		t.Fatalf("токены не сохранены: %+v", saved)
	}
	if !c.IsAuthenticated() {
		t.Fatal("ожидалась авторизация")
	}
}

func TestLoginCodeStoresTokensAndSendsCode(t *testing.T) {
	store := NewMemoryStore()
	var gotBody string
	rt := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		if req.URL.Path != "/api/v2/app/login/code" {
			t.Fatalf("неожиданный путь %s", req.URL.Path)
		}
		b, _ := io.ReadAll(req.Body)
		gotBody = string(b)
		return jsonResp(200, `{"access_token":"a1","refresh_token":"r1","expires_in":3600}`), nil
	})
	c := NewPanelClient("https://panel.test", WithHTTPClient(rt), WithStore(store))

	tk, err := c.LoginCode(context.Background(), "654321")
	if err != nil {
		t.Fatalf("login code: %v", err)
	}
	if tk.AccessToken != "a1" || tk.RefreshToken != "r1" {
		t.Fatalf("неверные токены: %+v", tk)
	}
	if !strings.Contains(gotBody, `"code":"654321"`) {
		t.Fatalf("в теле нет кода: %s", gotBody)
	}
	saved, _ := store.Load()
	if saved.AccessToken != "a1" {
		t.Fatalf("токены не сохранены: %+v", saved)
	}
}

func TestLoginCodeUnauthorized(t *testing.T) {
	rt := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		return jsonResp(401, `{"error":"invalid code"}`), nil
	})
	c := NewPanelClient("https://panel.test", WithHTTPClient(rt))
	if _, err := c.LoginCode(context.Background(), "000000"); err == nil {
		t.Fatal("ожидалась ошибка при неверном коде")
	}
}

// TestAuthEndpointPaths фиксирует точные пути запросов аутентификации, чтобы
// рассинхрон с app_routes() панели (/api/v2/app/*) ловился тестом, а не 404 в
// проде.
func TestAuthEndpointPaths(t *testing.T) {
	cases := []struct {
		name     string
		wantPath string
		call     func(c *PanelClient) error
	}{
		{
			name:     "register",
			wantPath: "/api/v2/app/register",
			call: func(c *PanelClient) error {
				_, err := c.RegisterEmail(context.Background(), "u@e.com", "pw")
				return err
			},
		},
		{
			name:     "login_email",
			wantPath: "/api/v2/app/login/email",
			call: func(c *PanelClient) error {
				_, err := c.LoginEmail(context.Background(), "u@e.com", "pw")
				return err
			},
		},
		{
			name:     "login_telegram",
			wantPath: "/api/v2/app/login/telegram",
			call: func(c *PanelClient) error {
				_, err := c.LoginTelegram(context.Background(), TelegramLogin{ID: 1, AuthDate: 1, Hash: "h"})
				return err
			},
		},
		{
			name:     "login_code",
			wantPath: "/api/v2/app/login/code",
			call: func(c *PanelClient) error {
				_, err := c.LoginCode(context.Background(), "123456")
				return err
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var gotPath string
			rt := roundTripFunc(func(req *http.Request) (*http.Response, error) {
				gotPath = req.URL.Path
				return jsonResp(200, `{"access_token":"a1","refresh_token":"r1"}`), nil
			})
			c := NewPanelClient("https://panel.test", WithHTTPClient(rt), WithStore(NewMemoryStore()))
			if err := tc.call(c); err != nil {
				t.Fatalf("%s: %v", tc.name, err)
			}
			if gotPath != tc.wantPath {
				t.Fatalf("%s: путь %q, ожидался %q", tc.name, gotPath, tc.wantPath)
			}
		})
	}
}

func TestLoginUnauthorized(t *testing.T) {
	rt := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		return jsonResp(401, `{"error":"invalid credentials"}`), nil
	})
	c := NewPanelClient("https://panel.test", WithHTTPClient(rt))
	_, err := c.LoginEmail(context.Background(), "u@e.com", "bad")
	if err == nil {
		t.Fatal("ожидалась ошибка")
	}
}

func TestAccessTokenAutoRefreshOn401(t *testing.T) {
	store := NewMemoryStore()
	_ = store.Save(Tokens{AccessToken: "old", RefreshToken: "r1"})

	var refreshed atomic.Int32
	rt := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		switch req.URL.Path {
		case "/api/v2/app/refresh":
			refreshed.Add(1)
			return jsonResp(200, `{"access_token":"new","refresh_token":"r2"}`), nil
		case "/protected":
			auth := req.Header.Get("Authorization")
			if auth == "Bearer old" {
				return jsonResp(401, `{}`), nil
			}
			if auth == "Bearer new" {
				return jsonResp(200, `{"ok":true}`), nil
			}
			t.Fatalf("неожиданный токен: %s", auth)
		}
		t.Fatalf("неожиданный путь %s", req.URL.Path)
		return nil, nil
	})
	c := NewPanelClient("https://panel.test", WithHTTPClient(rt), WithStore(store))

	req, _ := http.NewRequestWithContext(context.Background(), http.MethodGet, "https://panel.test/protected", nil)
	resp, err := c.DoAuthorized(req)
	if err != nil {
		t.Fatalf("DoAuthorized: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		t.Fatalf("ожидался 200 после рефреша, получено %d", resp.StatusCode)
	}
	if refreshed.Load() != 1 {
		t.Fatalf("ожидался ровно 1 рефреш, было %d", refreshed.Load())
	}
}

func TestLogoutClearsStore(t *testing.T) {
	store := NewMemoryStore()
	_ = store.Save(Tokens{AccessToken: "a", RefreshToken: "r"})
	rt := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		return jsonResp(200, `{}`), nil
	})
	c := NewPanelClient("https://panel.test", WithHTTPClient(rt), WithStore(store))
	if err := c.Logout(context.Background()); err != nil {
		t.Fatalf("logout: %v", err)
	}
	saved, _ := store.Load()
	if saved.Valid() {
		t.Fatalf("токены не очищены: %+v", saved)
	}
}
