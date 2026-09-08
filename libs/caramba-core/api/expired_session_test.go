package api

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/app"
	"github.com/semanticparadox/caramba/libs/caramba-core/auth"
)

// Эти тесты воспроизводят ЖАЛОБУ, а не строчку кода: телефон, полежавший
// дольше пятнадцати минут, переставал показывать серверы и отвечал
// «api: загрузка узлов подписки для замера: …». Юнит-тесты рядом
// (auth/session_test.go, mobile/configure_session_test.go) проверяют части шва;
// здесь проверяется то, что видел владелец, — сообщение и его отсутствие.
//
// Почему проверка расколота на два уровня. Ядро собирает свои HTTP-клиенты
// поверх лестницы транспортов (transport.NewDoer), а та требует настоящего TLS
// (инвариант 8, CheckFetchURL) и httptest не принимает — подменить их снаружи
// NewCore не даёт. Поэтому граница «идёт ли ядро в панель вообще» снимается на
// живом api.Core, а «обновляется ли сессия по дороге» — на тех же самых
// auth.PanelClient и app.SubscriptionClient, которые ядро внутри и собирает,
// но с обычным httptest вместо лестницы. Вместе они покрывают путь целиком.

// expiredJWT и liveJWT собирают токен с заданным сроком. Подпись произвольна:
// ядро её не проверяет и проверить не может (нечем), а тесту нужен срок.
func jwtExp(t *testing.T, d time.Duration) string {
	t.Helper()
	payload, err := json.Marshal(map[string]any{"sub": "42", "exp": time.Now().Add(d).Unix()})
	if err != nil {
		t.Fatalf("сборка payload: %v", err)
	}
	enc := base64.RawURLEncoding.EncodeToString
	return enc([]byte(`{"alg":"HS256","typ":"JWT"}`)) + "." + enc(payload) + ".sig"
}

// Протухшая сессия больше не выдаёт себя за авторизацию, и «api: загрузка узлов
// подписки для замера» из-за неё не возникает.
//
// Это ровно тот отказ, который ловил владелец. Ядро с непустым, но мёртвым
// access-токеном считало себя авторизованным (tokens.Valid() смотрел только на
// непустоту строки), probeConfig проходил проверку на входе и шёл в панель с
// заведомо негодным bearer'ом; панель отвечала 401, продлеваться было нечем, и
// пользователь получал сообщение, по которому невозможно догадаться, что надо
// просто войти заново.
//
// Тест намеренно смотрит на ОТСУТСТВИЕ строки, а не на «err == nil»: ядро
// вправе не отдать узлы (их правда неоткуда взять), но не вправе объяснять это
// неудачей загрузки. Панель здесь — panel.invalid, в сеть тест не ходит; в этом
// и суть проверки: со старым поведением ядро туда бы ПОШЛО.
func TestExpiredInjectedSessionDoesNotReportLoadFailure(t *testing.T) {
	dir := t.TempDir()
	core, err := NewCore(Config{
		PanelBaseURL:   "https://panel.invalid",
		SubBaseURL:     "https://panel.invalid",
		WorkDir:        dir,
		TokenStorePath: dir + "/tokens.json",
	})
	if err != nil {
		t.Fatalf("NewCore: %v", err)
	}
	// Ровно то, что делал старый mobile.Configure: один access, ни refresh'а,
	// ни срока. Токен пролежал ночь.
	if err := core.InjectToken(jwtExp(t, -8*time.Hour), "", 0, "sub-uuid"); err != nil {
		t.Fatalf("InjectToken: %v", err)
	}

	rep, perr := core.Probe(context.Background(), 200)
	if perr != nil {
		if strings.Contains(perr.Error(), "загрузка узлов подписки для замера") {
			t.Fatalf("вернулась исходная жалоба владельца: %v", perr)
		}
		t.Fatalf("неожиданная ошибка замера: %v", perr)
	}
	if len(rep.Servers) != 0 {
		t.Fatalf("узлы взяться неоткуда, получено %+v", rep.Servers)
	}
}

// Живой токен по-прежнему ведёт ядро в панель. Без этой половины предыдущий
// тест проходил бы и на ядре, которое разучилось ходить за подпиской совсем.
func TestLiveInjectedSessionStillFetches(t *testing.T) {
	dir := t.TempDir()
	core, err := NewCore(Config{
		PanelBaseURL:   "https://panel.invalid",
		SubBaseURL:     "https://panel.invalid",
		WorkDir:        dir,
		TokenStorePath: dir + "/tokens.json",
	})
	if err != nil {
		t.Fatalf("NewCore: %v", err)
	}
	if err := core.InjectToken(jwtExp(t, time.Hour), "", 0, "sub-uuid"); err != nil {
		t.Fatalf("InjectToken: %v", err)
	}

	if _, perr := core.Probe(context.Background(), 200); perr == nil {
		t.Fatal("с живым токеном ядро обязано пойти в панель и назвать причину неудачи")
	} else if !strings.Contains(perr.Error(), "загрузка узлов подписки для замера") {
		t.Fatalf("ожидалась неудача загрузки, получено: %v", perr)
	}
}

// ГЛАВНОЕ: протухший access при живом refresh — не отказ, а продление.
//
// Проверяется на настоящей паре auth.PanelClient + app.SubscriptionClient (тех
// самых, что собирает NewCore) против поддельной панели. Сильное утверждение
// здесь не «UUID вернулся», а «панель НИ РАЗУ не увидела мёртвый bearer»:
// ядро обновляется ДО запроса, зная срок, а не после 401. 401-путь остаётся
// страховкой на случай, когда срок неизвестен.
func TestExpiredAccessWithRefreshRenewsBeforeRequest(t *testing.T) {
	var refreshes, staleBearer, freshBearer atomic.Int32
	var sentRefresh atomic.Value

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/v2/app/refresh":
			refreshes.Add(1)
			var body struct {
				RefreshToken string `json:"refresh_token"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				t.Errorf("разбор тела обновления: %v", err)
			}
			sentRefresh.Store(body.RefreshToken)
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"access_token":"access-fresh","refresh_token":"refresh-2","expires_in":900}`))
		case "/api/v2/app/subscription":
			switch r.Header.Get("Authorization") {
			case "Bearer access-fresh":
				freshBearer.Add(1)
				w.Header().Set("Content-Type", "application/json")
				_, _ = w.Write([]byte(`{"subscription_uuid":"11111111-2222-3333-4444-555555555555","status":"active"}`))
			default:
				// Сюда попадает мёртвый токен — то, чем заканчивался
				// каждый вызов ядра через четверть часа после входа.
				staleBearer.Add(1)
				w.WriteHeader(http.StatusUnauthorized)
			}
		default:
			t.Errorf("неожиданный путь %s", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer srv.Close()

	store := auth.NewMemoryStore()
	panel := auth.NewPanelClient(srv.URL, auth.WithStore(store), auth.WithHTTPClient(srv.Client()))
	// То, что приложение отдаёт ядру ТЕПЕРЬ: обе половины сессии и срок.
	if _, err := panel.SetTokens("access-stale", "refresh-1", time.Now().Add(-time.Minute)); err != nil {
		t.Fatalf("SetTokens: %v", err)
	}
	if !panel.IsAuthenticated() {
		t.Fatal("сессию есть чем продлить — она пригодна")
	}

	uuid, err := app.NewSubscriptionClient(panel).FetchUUID(context.Background())
	if err != nil {
		t.Fatalf("FetchUUID: %v (продление не сработало)", err)
	}
	if uuid != "11111111-2222-3333-4444-555555555555" {
		t.Fatalf("UUID подписки %q", uuid)
	}
	if staleBearer.Load() != 0 {
		t.Fatalf("панель увидела мёртвый bearer %d раз: обновление опоздало", staleBearer.Load())
	}
	if freshBearer.Load() != 1 {
		t.Fatalf("запросов с обновлённым токеном %d, ожидался 1", freshBearer.Load())
	}
	if refreshes.Load() != 1 {
		t.Fatalf("обновлений %d, ожидалось ровно 1", refreshes.Load())
	}
	if got, _ := sentRefresh.Load().(string); got != "refresh-1" {
		t.Fatalf("на обновление ушёл refresh %q, ожидался инъецированный refresh-1", got)
	}
	// Ротированный refresh обязан пережить продление, иначе следующее
	// обновление уйдёт со старым и панель его отвергнет.
	saved, _ := store.Load()
	if saved.RefreshToken != "refresh-2" {
		t.Fatalf("ротированный refresh не сохранён: %+v", saved)
	}
}

// Без refresh'а протухшая сессия отказывает СРАЗУ и по имени, а не уходит в
// сеть за неустранимым 401. Разница не косметическая: названную причину
// («не выполнен вход») приложение может показать как «войдите заново», а 401 из
// глубины лестницы транспортов доезжал до пользователя как «загрузка узлов…».
func TestExpiredAccessWithoutRefreshNeverReachesPanel(t *testing.T) {
	var hits atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer srv.Close()

	panel := auth.NewPanelClient(srv.URL, auth.WithStore(auth.NewMemoryStore()), auth.WithHTTPClient(srv.Client()))
	if _, err := panel.SetTokens(jwtExp(t, -time.Hour), "", time.Time{}); err != nil {
		t.Fatalf("SetTokens: %v", err)
	}

	_, err := app.NewSubscriptionClient(panel).FetchUUID(context.Background())
	if err == nil {
		t.Fatal("протухшая невосстановимая сессия обязана отказать")
	}
	if !errors.Is(err, auth.ErrNotAuthenticated) {
		t.Fatalf("причина отказа должна называться: %v", err)
	}
	if hits.Load() != 0 {
		t.Fatalf("ядро сходило в панель %d раз с заведомо мёртвым токеном", hits.Load())
	}
}
