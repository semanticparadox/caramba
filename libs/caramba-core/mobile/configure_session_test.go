package mobile

import (
	"encoding/base64"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// storedTokens — то, что ядро действительно положило в свой файл токенов.
type storedTokens struct {
	AccessToken  string    `json:"access_token"`
	RefreshToken string    `json:"refresh_token"`
	AccessExpiry time.Time `json:"access_expiry"`
}

// clientWithTokenPath повторяет newTestClient, но отдаёт ещё и путь к файлу
// токенов: проверять надо не «вызов не вернул ошибку», а что именно доехало.
func clientWithTokenPath(t *testing.T) (*Client, string) {
	t.Helper()
	dir := t.TempDir()
	tokenPath := filepath.Join(dir, "tokens.json")
	cl, err := NewClient("https://panel.invalid", "", dir, tokenPath)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}
	return cl, tokenPath
}

func readTokens(t *testing.T, path string) storedTokens {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("чтение файла токенов: %v", err)
	}
	var st storedTokens
	if err := json.Unmarshal(raw, &st); err != nil {
		t.Fatalf("разбор файла токенов %q: %v", raw, err)
	}
	return st
}

func jwtWithExp(t *testing.T, exp time.Time) string {
	t.Helper()
	payload, err := json.Marshal(map[string]any{"exp": exp.Unix()})
	if err != nil {
		t.Fatalf("сборка payload: %v", err)
	}
	enc := base64.RawURLEncoding.EncodeToString
	return enc([]byte(`{"alg":"HS256","typ":"JWT"}`)) + "." + enc(payload) + ".sig"
}

// Шов обязан переносить в ядро ВСЮ сессию, а не один её короткоживущий кусок.
//
// Регресс на исходную поломку: Configure звал InjectToken(access, "", 0, sub) —
// refresh терялся ровно здесь, на границе gomobile. Ядро получало 15-минутный
// токен и ничего, чем его продлить; телефон, полежавший час, больше не мог
// загрузить подписку. Проверяется файл токенов, а не отсутствие ошибки: старый
// код тоже возвращал nil.
func TestConfigureCarriesRefreshTokenIntoCore(t *testing.T) {
	cl, tokenPath := clientWithTokenPath(t)

	expiry := time.Now().Add(15 * time.Minute).Truncate(time.Second)
	if err := cl.Configure("", "sub-uuid", "access-1", "refresh-1", expiry.Unix()); err != nil {
		t.Fatalf("Configure: %v", err)
	}

	st := readTokens(t, tokenPath)
	if st.RefreshToken != "refresh-1" {
		t.Fatalf("refresh в ядре %q, ожидался refresh-1: продлевать сессию нечем", st.RefreshToken)
	}
	if st.AccessToken != "access-1" {
		t.Fatalf("access в ядре %q, ожидался access-1", st.AccessToken)
	}
	if !st.AccessExpiry.Equal(expiry) {
		t.Fatalf("срок в ядре %s, ожидался %s", st.AccessExpiry, expiry)
	}
}

// Срок не передали — ядро берёт его из claim exp самого JWT, а не считает
// «неизвестным». Мостов пять, и забыть аргумент может любой.
func TestConfigureDerivesExpiryFromJWT(t *testing.T) {
	cl, tokenPath := clientWithTokenPath(t)

	exp := time.Now().Add(10 * time.Minute).Truncate(time.Second)
	if err := cl.Configure("", "sub-uuid", jwtWithExp(t, exp), "refresh-1", 0); err != nil {
		t.Fatalf("Configure: %v", err)
	}

	st := readTokens(t, tokenPath)
	if !st.AccessExpiry.Equal(exp) {
		t.Fatalf("срок в ядре %s, ожидался разобранный из JWT %s", st.AccessExpiry, exp)
	}
}

// Протухший access без refresh — не авторизация. Status() отдаёт это честно,
// вместо того чтобы отправить ядро в неустранимый 401.
func TestConfigureExpiredAccessOnlyIsNotAuthenticated(t *testing.T) {
	cl, _ := clientWithTokenPath(t)

	expired := jwtWithExp(t, time.Now().Add(-time.Hour))
	if err := cl.Configure("", "", expired, "", 0); err != nil {
		t.Fatalf("Configure: %v", err)
	}

	raw, err := cl.Status()
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	var st struct {
		Authenticated bool `json:"authenticated"`
	}
	if err := json.Unmarshal([]byte(raw), &st); err != nil {
		t.Fatalf("разбор статуса %q: %v", raw, err)
	}
	if st.Authenticated {
		t.Fatal("ядро считает себя авторизованным по протухшему токену без refresh")
	}
}
