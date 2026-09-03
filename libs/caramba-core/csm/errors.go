package csm

import "fmt"

// Code это код причины из реестра 03-WIRE.md 6.6. Три реализации обязаны
// возвращать один и тот же код на одном и том же кадре: согласия в том, что
// кадр отвергнут, недостаточно, потому что два верификатора, падающие по
// разным причинам, это способ спрятать настоящее расхождение.
type Code string

// Разбор, шаги P1..P12 (03-WIRE.md 6.1).
const (
	EParseShort     Code = "E_PARSE_SHORT"
	EParseMagic     Code = "E_PARSE_MAGIC"
	EParseDocType   Code = "E_PARSE_DOCTYPE"
	EParseLen       Code = "E_PARSE_LEN"
	EParseNsigs     Code = "E_PARSE_NSIGS"
	EParseFraming   Code = "E_PARSE_FRAMING"
	EParseSlotOrder Code = "E_PARSE_SLOTORDER"
	EParseCBOR      Code = "E_PARSE_CBOR"
	EParseEnvelope  Code = "E_PARSE_ENVELOPE"
	EParseField     Code = "E_PARSE_FIELD"
)

// Проверка, шаги V1..V14 (03-WIRE.md 6.2).
const (
	EVerifyRole         Code = "E_VERIFY_ROLE"
	EVerifyNoAnchor     Code = "E_VERIFY_NOANCHOR"
	EVerifyUnauthorized Code = "E_VERIFY_UNAUTHORIZED"
	EVerifyRevoked      Code = "E_VERIFY_REVOKED"
	EVerifySig          Code = "E_VERIFY_SIG"
	EVerifyThreshold    Code = "E_VERIFY_THRESHOLD"
	EVerifyPID          Code = "E_VERIFY_PID"
	EVerifyVersion      Code = "E_VERIFY_VERSION"
	EVerifyRotation     Code = "E_VERIFY_ROTATION"
	EVerifyIAT          Code = "E_VERIFY_IAT"
	EVerifyExpired      Code = "E_VERIFY_EXPIRED"
	EVerifyNonce        Code = "E_VERIFY_NONCE"
	EVerifyDevice       Code = "E_VERIFY_DEVICE"
	EVerifyCatHash      Code = "E_VERIFY_CATHASH"
)

// Распечатывание, шаги 3..6 раздела 9.4.
const (
	ESealRecipient Code = "E_SEAL_RECIPIENT"
	ESealSuite     Code = "E_SEAL_SUITE"
	ESealOpen      Code = "E_SEAL_OPEN"
)

// AllCodes возвращает полный реестр 03-WIRE.md 6.6 в порядке документа.
// Харнесс корпуса сверяет с ним таблицу error_code_registry.
func AllCodes() []Code {
	return []Code{
		EParseShort, EParseMagic, EParseDocType, EParseLen, EParseNsigs,
		EParseFraming, EParseSlotOrder, EParseCBOR, EParseEnvelope, EParseField,
		EVerifyRole, EVerifyNoAnchor, EVerifyUnauthorized, EVerifyRevoked,
		EVerifySig, EVerifyThreshold, EVerifyPID, EVerifyVersion,
		EVerifyRotation, EVerifyIAT, EVerifyExpired, EVerifyNonce,
		EVerifyDevice, EVerifyCatHash,
		ESealRecipient, ESealSuite, ESealOpen,
	}
}

// Class это слой, на котором отказ произошёл.
type Class uint8

const (
	// ClassParse: решается целиком по входным байтам. Правильная реакция это
	// выбросить байты и считать, что ступень лестницы вернула ничего.
	// Пользователю НЕ показывается как заявление о подделке: подавляющая
	// причина это captive portal, зеркало с ошибкой или обрезанный ответ.
	ClassParse Class = iota
	// ClassVerify: требует доверенного документа, закреплённого pid, отметки
	// версии, часов или выданного nonce. Это событие безопасности.
	ClassVerify
	// ClassSeal: слой HPKE вокруг директивы, раздел 9.
	ClassSeal
)

// Error несёт код причины из реестра и шаг, который его решил.
// Шаг диагностический, нормативен только Code.
type Error struct {
	Code   Code
	Step   string
	Detail string
}

func (e *Error) Error() string {
	if e.Detail == "" {
		return fmt.Sprintf("csm: %s at %s", e.Code, e.Step)
	}
	return fmt.Sprintf("csm: %s at %s: %s", e.Code, e.Step, e.Detail)
}

// Class сообщает слой отказа.
func (e *Error) Class() Class {
	switch e.Code {
	case EParseShort, EParseMagic, EParseDocType, EParseLen, EParseNsigs,
		EParseFraming, EParseSlotOrder, EParseCBOR, EParseEnvelope, EParseField:
		return ClassParse
	case ESealRecipient, ESealSuite, ESealOpen:
		return ClassSeal
	default:
		return ClassVerify
	}
}

// IsParse истинно, когда отказ решён без ключевого материала и без состояния.
func (e *Error) IsParse() bool { return e.Class() == ClassParse }

// IsVerify истинно для отказа проверки, то есть события безопасности.
func (e *Error) IsVerify() bool { return e.Class() == ClassVerify }

// IsSeal истинно для отказа распечатывания HPKE.
func (e *Error) IsSeal() bool { return e.Class() == ClassSeal }

func errf(code Code, step, format string, args ...any) *Error {
	return &Error{Code: code, Step: step, Detail: fmt.Sprintf(format, args...)}
}

// CodeOf извлекает код причины из ошибки пакета. Для чужой ошибки
// возвращает пустую строку.
func CodeOf(err error) Code {
	if e, ok := err.(*Error); ok {
		return e.Code
	}
	return ""
}

// StepOf извлекает диагностический шаг.
func StepOf(err error) string {
	if e, ok := err.(*Error); ok {
		return e.Step
	}
	return ""
}
