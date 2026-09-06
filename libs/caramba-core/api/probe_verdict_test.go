package api

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"testing"
)

// Главный разлом, ради которого вердикты и заведены: одна и та же ошибка «EOF»
// означает разное в зависимости от того, ответил ли адрес.
//
// Это не теория. На боевом флоте узел DE Stealth с чужим uuid даёт «tcp ok
// 118ms, urltest EOF»: адрес жив, ключ не принят. До вердиктов такой узел
// показывался как самый быстрый в списке.
func TestClassifySplitsRejectedKeyFromDeadAddress(t *testing.T) {
	v, _ := classifyProbeFailure(io.EOF, 118)
	if v != ProbeVerdictAuthRejected {
		t.Fatalf("EOF при живом TCP это отказ узла, получено %q", v)
	}
	v, _ = classifyProbeFailure(io.EOF, -1)
	if v != ProbeVerdictPortClosed {
		t.Fatalf("EOF при мёртвом адресе это мёртвый адрес, получено %q", v)
	}
}

// Таймаут делится тем же признаком: при живом адресе это подпись фильтрации
// протокола, при мёртвом — просто мёртвый адрес. Обвинять сеть в первом случае
// и оператора во втором — разные советы человеку.
func TestClassifySplitsTimeoutByReachability(t *testing.T) {
	v, _ := classifyProbeFailure(context.DeadlineExceeded, 90)
	if v != ProbeVerdictTimeout {
		t.Fatalf("таймаут при живом TCP: ожидался %q, получено %q", ProbeVerdictTimeout, v)
	}
	v, _ = classifyProbeFailure(context.DeadlineExceeded, -1)
	if v != ProbeVerdictPortClosed {
		t.Fatalf("таймаут при мёртвом адресе: ожидался %q, получено %q", ProbeVerdictPortClosed, v)
	}
}

// Сертификат — сторона оператора при ЛЮБОМ состоянии TCP: живой адрес не делает
// чужой сертификат виной пользователя. Это ровно тот класс, из-за которого 9 из
// 13 прокси боевой панели мертвы для Clash-клиента.
func TestClassifyTLSIsOperatorSideRegardlessOfTCP(t *testing.T) {
	err := errors.New("tls: failed to verify certificate: x509: certificate signed by unknown authority")
	for _, tcp := range []int{-1, 42} {
		if v, _ := classifyProbeFailure(err, tcp); v != ProbeVerdictTLSUntrusted {
			t.Fatalf("tcpMs=%d: ожидался %q, получено %q", tcp, ProbeVerdictTLSUntrusted, v)
		}
	}
}

// Ошибки самого набора соединения — всегда «адрес не отвечает», и живой TCP их
// не переопределяет: до протокола дело не дошло.
func TestClassifyDialErrorsArePortClosed(t *testing.T) {
	cases := []error{
		fmt.Errorf("dial tcp 10.0.0.1:443: connect: connection refused"),
		fmt.Errorf("dial tcp: lookup nowhere.invalid: no such host"),
		fmt.Errorf("dial tcp 10.0.0.1:443: connect: no route to host"),
	}
	for _, err := range cases {
		if v, _ := classifyProbeFailure(err, -1); v != ProbeVerdictPortClosed {
			t.Fatalf("%v: ожидался %q, получено %q", err, ProbeVerdictPortClosed, v)
		}
	}
}

// Таймаут распознаётся по ТИПУ, а не только по тексту: net.Error с Timeout()
// приходит без слова «timeout» в строке чаще, чем с ним.
func TestClassifyRecognisesNetErrorTimeout(t *testing.T) {
	err := &net.OpError{Op: "read", Err: os.ErrDeadlineExceeded}
	if v, _ := classifyProbeFailure(err, 12); v != ProbeVerdictTimeout {
		t.Fatalf("net.Error.Timeout(): ожидался %q, получено %q", ProbeVerdictTimeout, v)
	}
}

// Незнакомая ошибка при живом адресе — отказ узла, а не таймаут: соединение
// состоялось, и оборвал его тот, кто на той стороне. Тихо считать такое
// «наверное сеть» значит обвинять пользователя в чужой поломке.
func TestClassifyUnknownErrorFallsBackByReachability(t *testing.T) {
	err := errors.New("something entirely new")
	if v, _ := classifyProbeFailure(err, 30); v != ProbeVerdictAuthRejected {
		t.Fatalf("живой адрес: ожидался %q, получено %q", ProbeVerdictAuthRejected, v)
	}
	if v, _ := classifyProbeFailure(err, -1); v != ProbeVerdictPortClosed {
		t.Fatalf("мёртвый адрес: ожидался %q, получено %q", ProbeVerdictPortClosed, v)
	}
}

// Сырой текст ошибки доезжает в detail целиком: он и есть то, что уходит под
// «Подробности», и терять его нельзя.
func TestClassifyKeepsRawDetail(t *testing.T) {
	err := errors.New("EOF while reading vless response header")
	_, detail := classifyProbeFailure(err, 5)
	if detail != err.Error() {
		t.Fatalf("detail потерян: %q", detail)
	}
	if _, d := classifyProbeFailure(nil, 5); d != "" {
		t.Fatalf("успех не должен нести detail, получено %q", d)
	}
}
