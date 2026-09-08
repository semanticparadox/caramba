package auth

import (
	"encoding/base64"
	"encoding/json"
	"strings"
	"time"
)

// jwtExpiry достаёт claim exp из access-токена, НЕ проверяя подпись. Нулевое
// значение означает «срок из токена не читается» (не JWT, нет claim, мусор).
//
// Подпись здесь проверять нечем и незачем. Ключ панели ядру неизвестен, а нужен
// нам не факт подлинности, а СРОК — ответ на вопрос «идти с этим токеном в сеть
// или сначала обновиться». Подделанный срок ничего не открывает: с ним запрос
// просто уйдёт и вернётся с 401, то есть ровно туда же, куда ведёт честный
// протухший токен.
//
// Функция существует как страховка от забывчивости мостов. Срок обязан доезжать
// до ядра явным аргументом (InjectToken/Configure), но мостов пять, и ровно один
// из них — mobile.Configure — годами передавал 0 и пустой refresh, из-за чего
// 15-минутный access превращался в «авторизован навсегда». Пока claim exp лежит
// в самом токене, ядру незачем зависеть от того, вспомнил ли о нём очередной
// слой: аргумент главнее, эта функция — сеть под ним.
func jwtExpiry(token string) time.Time {
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 3 {
		return time.Time{}
	}
	// JWT кодируется base64url БЕЗ выравнивания '=' (RFC 7515 §2).
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return time.Time{}
	}
	var claims struct {
		Exp int64 `json:"exp"`
	}
	if err := json.Unmarshal(payload, &claims); err != nil || claims.Exp <= 0 {
		return time.Time{}
	}
	return time.Unix(claims.Exp, 0)
}
