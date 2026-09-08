package auth

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"sync/atomic"
	"testing"
	"time"
)

// signedLikeJWT собирает токен формы header.payload.signature с заданным exp.
// Подпись здесь произвольная: ядро её не проверяет и проверить не может (см.
// jwtExpiry), а тесту нужен именно СРОК.
func signedLikeJWT(t *testing.T, exp time.Time) string {
	t.Helper()
	payload, err := json.Marshal(map[string]any{"sub": "42", "exp": exp.Unix()})
	if err != nil {
		t.Fatalf("сборка payload: %v", err)
	}
	enc := base64.RawURLEncoding.EncodeToString
	return enc([]byte(`{"alg":"HS256","typ":"JWT"}`)) + "." + enc(payload) + ".sig"
}

// Инъекция ОДНОГО протухшего access-токена не имеет права выглядеть как
// авторизация.
//
// Это регресс на поломку, из-за которой телефон, полежавший ночь, переставал
// подключаться: mobile.Configure передавал в ядро только access, tokens.Valid()
// проверял лишь непустоту строки, IsAuthenticated() отвечал «да», и ядро шло в
// панель с мёртвым bearer'ом. Восстановимый 401 превращался в тупик, а
// пользователь видел «api: загрузка узлов подписки для замера: …».
func TestInjectedExpiredAccessWithoutRefreshIsNotAuthenticated(t *testing.T) {
	c := NewPanelClient("https://panel.test", WithStore(NewMemoryStore()))

	if _, err := c.SetTokens("a1", "", time.Now().Add(-time.Minute)); err != nil {
		t.Fatalf("SetTokens: %v", err)
	}
	if c.IsAuthenticated() {
		t.Fatal("протухший access без refresh не является авторизацией")
	}
	if _, err := c.AccessToken(context.Background()); err == nil {
		t.Fatal("ожидалась ошибка: продлевать сессию нечем")
	}
}

// Срок не передали — он всё равно обязан доехать: ядро читает claim exp из
// самого JWT. Без этого достаточно одному мосту забыть аргумент, чтобы
// протухший токен снова считался живым.
func TestInjectedExpiryFallsBackToJWTClaim(t *testing.T) {
	c := NewPanelClient("https://panel.test", WithStore(NewMemoryStore()))

	if _, err := c.SetTokens(signedLikeJWT(t, time.Now().Add(-time.Hour)), "", time.Time{}); err != nil {
		t.Fatalf("SetTokens: %v", err)
	}
	if c.IsAuthenticated() {
		t.Fatal("срок из claim exp не учтён: протухший токен признан живым")
	}

	live := NewPanelClient("https://panel.test", WithStore(NewMemoryStore()))
	if _, err := live.SetTokens(signedLikeJWT(t, time.Now().Add(time.Hour)), "", time.Time{}); err != nil {
		t.Fatalf("SetTokens: %v", err)
	}
	if !live.IsAuthenticated() {
		t.Fatal("живой токен признан протухшим")
	}
}

// Не-JWT (или токен без exp) остаётся «сроком неизвестен» и живым: соврать в эту
// сторону дешевле, чем выкинуть работающую сессию.
func TestInjectedOpaqueAccessStaysValid(t *testing.T) {
	c := NewPanelClient("https://panel.test", WithStore(NewMemoryStore()))
	if _, err := c.SetTokens("opaque-not-a-jwt", "", time.Time{}); err != nil {
		t.Fatalf("SetTokens: %v", err)
	}
	if !c.IsAuthenticated() {
		t.Fatal("токен без читаемого срока не должен считаться протухшим")
	}
}

// С refresh'ем протухший access — это не отказ, а повод обновиться ДО запроса,
// а не после 401. Ровно этот путь и был недостижим: ядру его нечем было
// запустить.
func TestInjectedRefreshRenewsExpiredAccess(t *testing.T) {
	var refreshCalls atomic.Int32
	var sentRefresh string
	rt := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		if req.URL.Path != "/api/v2/app/refresh" {
			t.Fatalf("неожиданный путь %s (ожидалось обновление токена)", req.URL.Path)
		}
		refreshCalls.Add(1)
		var body struct {
			RefreshToken string `json:"refresh_token"`
		}
		if err := json.NewDecoder(req.Body).Decode(&body); err != nil {
			t.Fatalf("разбор тела обновления: %v", err)
		}
		sentRefresh = body.RefreshToken
		return jsonResp(200, `{"access_token":"a2","refresh_token":"r2","expires_in":900}`), nil
	})
	store := NewMemoryStore()
	c := NewPanelClient("https://panel.test", WithHTTPClient(rt), WithStore(store))

	if _, err := c.SetTokens("a1-expired", "r1", time.Now().Add(-time.Minute)); err != nil {
		t.Fatalf("SetTokens: %v", err)
	}
	if !c.IsAuthenticated() {
		t.Fatal("сессия с refresh'ем остаётся пригодной: её есть чем продлить")
	}

	got, err := c.AccessToken(context.Background())
	if err != nil {
		t.Fatalf("AccessToken: %v", err)
	}
	if got != "a2" {
		t.Fatalf("получен токен %q, ожидался обновлённый a2", got)
	}
	if refreshCalls.Load() != 1 {
		t.Fatalf("обновлений: %d, ожидалось ровно 1", refreshCalls.Load())
	}
	if sentRefresh != "r1" {
		t.Fatalf("на обновление ушёл refresh %q, ожидался инъецированный r1", sentRefresh)
	}
	saved, _ := store.Load()
	if saved.RefreshToken != "r2" {
		t.Fatalf("ротированный refresh не сохранён: %+v", saved)
	}
}
