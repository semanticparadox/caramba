package transport

import (
	"sort"
	"time"
)

// RungID это идентификатор ступени, 02-SPEC.md 8.1 и 03-WIRE.md 5 (rung).
type RungID uint8

const (
	// R0Cached читает последние хорошие проверенные документы с диска.
	// Всегда включена, выключить нельзя, всегда первая в любом законном
	// порядке, сети не стоит.
	R0Cached RungID = 0
	// R1Direct это один запрос к закреплённому origin регистрации.
	R1Direct RungID = 1
	// R2Mirrors это пул mir из каталога плюс резервный пул из /sub/r1/{loc}.
	R2Mirrors RungID = 2
	// R3DoH резолвит хост зеркала через запись doh и подключается к литеральному
	// адресу с явным SNI.
	R3DoH RungID = 3
	// R4Tunnel идёт через собственный туннель приложения.
	R4Tunnel RungID = 4
	// R5Proxy это введённый пользователем SOCKS5 или HTTP прокси. Только для
	// выборки манифеста и конфигурации, НИКОГДА для трафика туннеля.
	R5Proxy RungID = 5
	// R6OutOfBand это QR, файл или вставка в армированной форме 03-WIRE.md 10.
	// Всегда включена, выключить нельзя, никогда не автоматическая.
	R6OutOfBand RungID = 6
	// R7Onion это onion-фронт. Компонента в этом репозитории нет, поэтому
	// ступень скомпилирована, видна и выключена с причиной. Она НЕ спрятана и
	// НЕ пропущена молча: инвариант 17.
	R7Onion RungID = 7
)

// MaxRung это наибольший скомпилированный идентификатор ступени.
const MaxRung = R7Onion

// AllRungs перечисляет все скомпилированные ступени в возрастающем порядке.
// Это то, что показывается на одном экране, целиком.
func AllRungs() []RungID {
	return []RungID{R0Cached, R1Direct, R2Mirrors, R3DoH, R4Tunnel, R5Proxy, R6OutOfBand, R7Onion}
}

// Name возвращает короткое машинное имя ступени.
func (r RungID) Name() string {
	switch r {
	case R0Cached:
		return "cached"
	case R1Direct:
		return "direct"
	case R2Mirrors:
		return "mirrors"
	case R3DoH:
		return "doh"
	case R4Tunnel:
		return "tunnel"
	case R5Proxy:
		return "proxy"
	case R6OutOfBand:
		return "out_of_band"
	case R7Onion:
		return "onion"
	default:
		return "unknown"
	}
}

// Mandatory сообщает, что ступень выключить нельзя (02-SPEC.md 8.3):
// ступени 0 и 6 обязаны присутствовать в включённом наборе.
func (r RungID) Mandatory() bool { return r == R0Cached || r == R6OutOfBand }

// Network сообщает, открывает ли ступень сокет. R0 читает диск, R6 требует
// действия человека, R7 не реализована; ни одна из них не участвует в
// автоматическом сетевом цикле выборки.
func (r RungID) Network() bool {
	switch r {
	case R1Direct, R2Mirrors, R3DoH, R4Tunnel, R5Proxy:
		return true
	default:
		return false
	}
}

// Attempts возвращает число попыток на цикл для ступени, 02-SPEC.md 8.4.
func (r RungID) Attempts() int {
	switch r {
	case R0Cached, R1Direct, R4Tunnel, R5Proxy:
		return 1
	case R2Mirrors:
		return 3
	case R3DoH:
		return 2
	default:
		// R6 автоматических попыток не имеет по построению, R7 не реализована.
		return 0
	}
}

// Timeout возвращает таймаут одной попытки, 02-SPEC.md 8.5.
func (r RungID) Timeout() time.Duration {
	switch r {
	case R4Tunnel, R5Proxy:
		return AttemptTimeoutR4R5
	default:
		return AttemptTimeoutR1R2R3
	}
}

// Reason это причина, по которой ступень выключена или недоступна. Словарь
// закрытый, 02-SPEC.md 8.1.
type Reason string

const (
	// ReasonNone: ступень включена и доступна.
	ReasonNone Reason = ""
	// ReasonUserDisabled: пользователь её выключил.
	ReasonUserDisabled Reason = "user_disabled"
	// ReasonNotOffered: бит возможностей или данные каталога отсутствуют.
	ReasonNotOffered Reason = "not_offered_by_operator"
	// ReasonPlatformUnsupported: эта сборка на этой ОС так не умеет.
	ReasonPlatformUnsupported Reason = "platform_unsupported"
	// ReasonNotConfigured: R5 без введённого прокси, R3 без записи doh.
	ReasonNotConfigured Reason = "not_configured"
	// ReasonAppVersionUnsupported: оператор её предлагает, эта сборка не
	// реализует.
	ReasonAppVersionUnsupported Reason = "app_version_unsupported"
)

// Outcome это исход одной попытки. Отдельный код разбора или проверки лежит в
// Attempt.Code и берётся из реестра 03-WIRE.md 6.6.
type Outcome string

const (
	// OutcomeOK: ступень вернула документ, который прошёл проверку.
	OutcomeOK Outcome = "ok"
	// OutcomeNetwork: соединение не состоялось, оборвалось или истёк таймаут.
	OutcomeNetwork Outcome = "network"
	// OutcomeHTTP: ответ пришёл, но код состояния не 200 и не 304.
	OutcomeHTTP Outcome = "http"
	// OutcomeRefused: ответ отвергнут транспортом до разбора кадра
	// (Content-Encoding, превышение resp_max, кросс-origin редирект, http://).
	OutcomeRefused Outcome = "refused"
	// OutcomeParse: байты не являются кадром CSM/1. Ладдер идёт дальше, и
	// пользователю это НЕ показывается как заявление о подделке.
	OutcomeParse Outcome = "parse"
	// OutcomeVerify: кадр не проверился против ключей этого оператора. Это
	// событие безопасности, и оно не приравнивается к пустому ответу.
	OutcomeVerify Outcome = "verify"
	// OutcomeBudget: бюджет соединения не позволяет отправить запрос.
	OutcomeBudget Outcome = "budget"
)

// Attempt это одна запись истории попыток. История локальная, никогда не
// выгружается и является сырьём для экрана "что это приложение отправляет"
// (02-SPEC.md 8.8, инвариант 17).
type Attempt struct {
	Rung RungID `json:"rung"`
	// Host это хост или непрозрачная метка зеркала.
	Host    string    `json:"host,omitempty"`
	Start   time.Time `json:"start"`
	Millis  int64     `json:"millis"`
	Outcome Outcome   `json:"outcome"`
	// Code это код 03-WIRE.md 6.6, когда он был.
	Code string `json:"code,omitempty"`
	// Status это код состояния HTTP, когда ответ пришёл.
	Status int    `json:"status,omitempty"`
	Detail string `json:"detail,omitempty"`
}

// AttemptHistoryMax это глубина истории на профиль, 02-SPEC.md 8.8.
const AttemptHistoryMax = 200

// RungState это то, что видит пользователь про одну ступень.
type RungState struct {
	Rung    RungID `json:"rung"`
	Name    string `json:"name"`
	Order   int    `json:"order"`
	Enabled bool   `json:"enabled"`
	// Reason непуста ровно тогда, когда Enabled ложно.
	Reason Reason `json:"reason,omitempty"`
	// UserSet сообщает, что пользователь трогал эту ступень: с этого момента
	// подписанные умолчания её не восстанавливают (02-SPEC.md 8.3).
	UserSet bool `json:"user_set"`
	// Last это последняя попытка по этой ступени, если она была.
	Last *Attempt `json:"last,omitempty"`
}

// sortRungStates упорядочивает по эффективному порядку, затем по номеру.
func sortRungStates(s []RungState) {
	sort.SliceStable(s, func(i, j int) bool {
		if s[i].Order != s[j].Order {
			return s[i].Order < s[j].Order
		}
		return s[i].Rung < s[j].Rung
	})
}
