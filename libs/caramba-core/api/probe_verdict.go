package api

import (
	"context"
	"errors"
	"net"
	"strings"
)

// Вердикт замера одного узла: ЧЕМ кончилась проверка, а не только «сколько мс».
//
// Голое число задержки отвечало на вопрос «жив ли адрес» и выдавало этот ответ
// за «работает ли узел». Разница между ними — вся разница между рабочим флотом
// и мёртвым: узел с отозванным ключом принимает TCP за 118 мс и отвергает
// handshake, и до этих вердиктов он был неотличим от самого быстрого рабочего.
//
// Классификатор живёт ЗДЕСЬ, а не в libs/caramba-shared: тот крейт на Rust и
// принадлежит панели, а классы отказа URLTest нужны только клиенту. И он без
// build-тега, чтобы его можно было проверить тестом в сборке без ядра.
const (
	// ProbeVerdictOK — единственный вердикт, означающий «через узел прошёл
	// настоящий запрос». Только он даёт LatencyMs >= 0.
	ProbeVerdictOK = "ok"

	// ProbeVerdictTLSUntrusted — TLS не сложился: самоподписанный или чужой
	// сертификат, несовпадение имени. Это сторона ОПЕРАТОРА: пользователь
	// такое не чинит.
	ProbeVerdictTLSUntrusted = "tls_untrusted"

	// ProbeVerdictAuthRejected — адрес отвечает, а узел разрывает соединение
	// после handshake: ключ подписки узлом не принят. Проверено вживую на
	// боевом флоте: DE Stealth с чужим uuid даёт «tcp ok 118ms, urltest EOF».
	ProbeVerdictAuthRejected = "auth_rejected"

	// ProbeVerdictPortClosed — адрес не отвечает вовсе (отказ соединения, нет
	// маршрута, имя не разрешается). До протокола дело не дошло.
	ProbeVerdictPortClosed = "port_closed"

	// ProbeVerdictTimeout — TCP до узла есть, а запрос сквозь него не
	// уложился в отведённое время. Типичная подпись DPI, который режет
	// протокол, а не адрес.
	ProbeVerdictTimeout = "timeout"

	// ProbeVerdictUnsupported — ядро не собрало адаптер из полей прокси
	// (незнакомый тип). Про сам узел это не говорит НИЧЕГО, поэтому и не
	// «сломан».
	ProbeVerdictUnsupported = "unsupported"

	// ProbeVerdictTCPOnly — сборка без нативного ядра: проверен только адрес,
	// handshake проверять нечем. Число задержки здесь настоящее, но отвечает
	// на более слабый вопрос, и UI обязан это сказать.
	ProbeVerdictTCPOnly = "tcp_only"

	// ProbeVerdictSkipped — до узла проход не дошёл (контекст отменён,
	// истёк потолок волны). Не «мёртв», а «не проверяли».
	ProbeVerdictSkipped = "skipped"
)

// probeOutcome — полный итог замера одного узла.
type probeOutcome struct {
	// latencyMs — задержка НАСТОЯЩЕГО запроса сквозь узел; -1 при любом
	// вердикте, кроме ok (и tcp_only, где иного измерения не существует).
	latencyMs int
	// tcpMs — справочный RTT установки TCP; -1, если адрес не ответил.
	tcpMs int
	// verdict — один из ProbeVerdict* выше.
	verdict string
	// detail — сырой текст ошибки для «Подробностей»; человеческий текст
	// строит приложение по вердикту, а не по этой строке.
	detail string
}

// skippedOutcome — узел, до которого проход не дошёл.
func skippedOutcome() probeOutcome {
	return probeOutcome{
		latencyMs: -1,
		tcpMs:     -1,
		verdict:   ProbeVerdictSkipped,
		detail:    "the sweep was cancelled before this node was reached",
	}
}

// classifyProbeFailure называет КЛАСС провала URL-теста.
//
// tcpMs — результат независимой TCP-проверки того же адреса (-1, если адрес не
// ответил). Он и есть та вторая точка, без которой отличить «узел отверг ключ»
// от «до узла не достучаться» нечем: обе ошибки выглядят как «не получилось».
//
// tcpBlind говорит, что TCP-свидетеля не было вовсе и молчание tcpMs ничего не
// доказывает. Так у UDP-семейств (hysteria2/tuic/wireguard): на их порту TCP
// никто не слушает, и раньше ровно отсюда бралось «адрес не отвечает» у
// отвечающего адреса — любой отказ QUIC превращался в port_closed, потому что
// tcpMs был -1 всегда. Так же у выходов, которые набираются через релей: прямой
// TCP до них не лежит ни на одном настоящем пути. В обоих случаях достижимость
// считается НЕИЗВЕСТНОЙ, и ни одна ветка не имеет права объявить адрес мёртвым
// по одному лишь tcpMs.
//
// Порядок веток от конкретного к общему. Неизвестная ошибка при живом TCP
// считается отказом узла, а не таймаутом: соединение состоялось и было
// прервано чем-то на той стороне.
func classifyProbeFailure(err error, tcpMs int, tcpBlind bool) (string, string) {
	if err == nil {
		return ProbeVerdictOK, ""
	}
	detail := err.Error()
	low := strings.ToLower(detail)
	// «Адрес мёртв» — вывод из ОТСУТСТВИЯ ответа по TCP. Он допустим только
	// там, где TCP вообще был свидетелем.
	deadAddress := !tcpBlind && tcpMs < 0

	switch {
	case containsAny(low, "x509", "certificate", "tls: ", "handshake failure", "unknown authority", "bad certificate"):
		// Сертификат — всегда сторона оператора, независимо от TCP.
		return ProbeVerdictTLSUntrusted, detail

	case containsAny(low, "no such host", "server misbehaving", "name resolution"):
		return ProbeVerdictPortClosed, detail

	case containsAny(low, "connection refused", "no route to host", "network is unreachable", "host is unreachable", "connect: "):
		return ProbeVerdictPortClosed, detail

	case containsAny(low, "application error", "crypto_error", "crypto error", "authentication", "unauthorized", "token"):
		// Подпись отказа QUIC-семейств: узел ОТВЕТИЛ и закрыл сессию
		// прикладным кодом. Живой TUIC с чужим паролем даёт ровно это —
		// «Application error 0x0 (remote)», — и до этой ветки такой узел
		// объявлялся мёртвым адресом.
		return ProbeVerdictAuthRejected, detail

	case isTimeoutErr(err) || containsAny(low, "deadline exceeded", "i/o timeout", "timeout", "no recent network activity"):
		// Таймаут при живом (или неизвестном) адресе — почти всегда
		// фильтрация протокола. Таймаут при доказанно мёртвом адресе это
		// просто мёртвый адрес.
		if deadAddress {
			return ProbeVerdictPortClosed, detail
		}
		return ProbeVerdictTimeout, detail

	case containsAny(low, "eof", "connection reset", "broken pipe", "closed by"):
		if deadAddress {
			return ProbeVerdictPortClosed, detail
		}
		return ProbeVerdictAuthRejected, detail
	}

	if deadAddress {
		return ProbeVerdictPortClosed, detail
	}
	return ProbeVerdictAuthRejected, detail
}

// isTimeoutErr отвечает на вопрос по типу ошибки, а не по её тексту: у
// context.DeadlineExceeded и net.Error текст бывает разным, а смысл один.
func isTimeoutErr(err error) bool {
	if errors.Is(err, context.DeadlineExceeded) {
		return true
	}
	var ne net.Error
	if errors.As(err, &ne) {
		return ne.Timeout()
	}
	return false
}

func containsAny(haystack string, needles ...string) bool {
	for _, n := range needles {
		if strings.Contains(haystack, n) {
			return true
		}
	}
	return false
}
