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
	v, _ := classifyProbeFailure(io.EOF, 118, false)
	if v != ProbeVerdictAuthRejected {
		t.Fatalf("EOF при живом TCP это отказ узла, получено %q", v)
	}
	v, _ = classifyProbeFailure(io.EOF, -1, false)
	if v != ProbeVerdictPortClosed {
		t.Fatalf("EOF при мёртвом адресе это мёртвый адрес, получено %q", v)
	}
}

// Таймаут делится тем же признаком: при живом адресе это подпись фильтрации
// протокола, при мёртвом — просто мёртвый адрес. Обвинять сеть в первом случае
// и оператора во втором — разные советы человеку.
func TestClassifySplitsTimeoutByReachability(t *testing.T) {
	v, _ := classifyProbeFailure(context.DeadlineExceeded, 90, false)
	if v != ProbeVerdictTimeout {
		t.Fatalf("таймаут при живом TCP: ожидался %q, получено %q", ProbeVerdictTimeout, v)
	}
	v, _ = classifyProbeFailure(context.DeadlineExceeded, -1, false)
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
		if v, _ := classifyProbeFailure(err, tcp, false); v != ProbeVerdictTLSUntrusted {
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
		if v, _ := classifyProbeFailure(err, -1, false); v != ProbeVerdictPortClosed {
			t.Fatalf("%v: ожидался %q, получено %q", err, ProbeVerdictPortClosed, v)
		}
	}
}

// Таймаут распознаётся по ТИПУ, а не только по тексту: net.Error с Timeout()
// приходит без слова «timeout» в строке чаще, чем с ним.
func TestClassifyRecognisesNetErrorTimeout(t *testing.T) {
	err := &net.OpError{Op: "read", Err: os.ErrDeadlineExceeded}
	if v, _ := classifyProbeFailure(err, 12, false); v != ProbeVerdictTimeout {
		t.Fatalf("net.Error.Timeout(): ожидался %q, получено %q", ProbeVerdictTimeout, v)
	}
}

// Незнакомая ошибка при живом адресе — отказ узла, а не таймаут: соединение
// состоялось, и оборвал его тот, кто на той стороне. Тихо считать такое
// «наверное сеть» значит обвинять пользователя в чужой поломке.
func TestClassifyUnknownErrorFallsBackByReachability(t *testing.T) {
	err := errors.New("something entirely new")
	if v, _ := classifyProbeFailure(err, 30, false); v != ProbeVerdictAuthRejected {
		t.Fatalf("живой адрес: ожидался %q, получено %q", ProbeVerdictAuthRejected, v)
	}
	if v, _ := classifyProbeFailure(err, -1, false); v != ProbeVerdictPortClosed {
		t.Fatalf("мёртвый адрес: ожидался %q, получено %q", ProbeVerdictPortClosed, v)
	}
}

// Сырой текст ошибки доезжает в detail целиком: он и есть то, что уходит под
// «Подробности», и терять его нельзя.
func TestClassifyKeepsRawDetail(t *testing.T) {
	err := errors.New("EOF while reading vless response header")
	_, detail := classifyProbeFailure(err, 5, false)
	if detail != err.Error() {
		t.Fatalf("detail потерян: %q", detail)
	}
	if _, d := classifyProbeFailure(nil, 5, false); d != "" {
		t.Fatalf("успех не должен нести detail, получено %q", d)
	}
}

// Разлом, из-за которого владелец увидел «Не проходит: адрес не отвечает» у
// живого TUIC: у UDP-семейства TCP-проба возвращает -1 ВСЕГДА, потому что на
// том порту TCP никто не слушает. Пока -1 считался доказательством, любой отказ
// QUIC становился «мёртвым адресом» — и это был единственный текст, который
// экран мог показать для TUIC и Hysteria2.
//
// Снято с устройства: TUIC отвечает «Application error 0x0 (remote)» — узел
// ответил и отверг ключ. Правильный вердикт auth_rejected, а не port_closed.
func TestClassifyUDPFamilyNeverBlamesTheAddressOnTCPSilence(t *testing.T) {
	quicReject := errors.New("Application error 0x0 (remote)")
	if v, _ := classifyProbeFailure(quicReject, -1, true); v != ProbeVerdictAuthRejected {
		t.Fatalf("TUIC с чужим ключом: ожидался %q, получено %q", ProbeVerdictAuthRejected, v)
	}
	// Тот же текст на TCP-семействе при мёртвом адресе остаётся мёртвым
	// адресом: там молчание TCP — настоящее свидетельство.
	if v, _ := classifyProbeFailure(quicReject, -1, false); v != ProbeVerdictAuthRejected {
		// «Application error» — прикладной отказ, он важнее отсутствия TCP:
		// такую строку не выдаёт закрытый порт.
		t.Fatalf("прикладной отказ: ожидался %q, получено %q", ProbeVerdictAuthRejected, v)
	}
}

// Таймаут QUIC при молчащем TCP — это таймаут, а не закрытый порт. Разница в
// совете человеку: «протокол режут» против «сервера нет».
func TestClassifyUDPTimeoutStaysTimeout(t *testing.T) {
	err := errors.New("timeout: no recent network activity")
	if v, _ := classifyProbeFailure(err, -1, true); v != ProbeVerdictTimeout {
		t.Fatalf("UDP-таймаут: ожидался %q, получено %q", ProbeVerdictTimeout, v)
	}
	if v, _ := classifyProbeFailure(err, -1, false); v != ProbeVerdictPortClosed {
		t.Fatalf("TCP-таймаут при мёртвом адресе: ожидался %q, получено %q", ProbeVerdictPortClosed, v)
	}
}

// Настоящий отказ набора (ICMP port unreachable доезжает до quic-go словами
// «connection refused») остаётся закрытым портом и для UDP: здесь свидетельство
// есть, и оно прямое.
func TestClassifyUDPDialRefusalIsStillPortClosed(t *testing.T) {
	err := errors.New("dial udp 10.0.0.1:443: connect: connection refused")
	if v, _ := classifyProbeFailure(err, -1, true); v != ProbeVerdictPortClosed {
		t.Fatalf("UDP connection refused: ожидался %q, получено %q", ProbeVerdictPortClosed, v)
	}
}

// Незнакомая ошибка у UDP-семейства не имеет права стать «адрес не отвечает»:
// доказательства мёртвого адреса не существует.
func TestClassifyUDPUnknownErrorIsNotPortClosed(t *testing.T) {
	if v, _ := classifyProbeFailure(errors.New("something entirely new"), -1, true); v == ProbeVerdictPortClosed {
		t.Fatalf("UDP с неизвестной ошибкой не должен обвинять адрес, получено %q", v)
	}
}

// Семейства перечислены явно: ошибка в этом списке возвращает всю ветку UDP в
// «адрес не отвечает», и заметить это на экране почти невозможно.
func TestUDPFamilyMembership(t *testing.T) {
	for _, typ := range []string{"hysteria2", "TUIC", " wireguard ", "hysteria"} {
		if !isUDPProxyType(typ) {
			t.Errorf("%q обязан считаться UDP-семейством", typ)
		}
	}
	for _, typ := range []string{"vless", "vmess", "trojan", "ss", "http", ""} {
		if isUDPProxyType(typ) {
			t.Errorf("%q не UDP-семейство", typ)
		}
	}
}

// Naive измерять нечем, и это надо СКАЗАТЬ, а не выяснять соединением: ядро
// адаптера для него не строит ни при каких полях.
func TestNaiveIsUnbuildableAndSaysSoWithoutDialling(t *testing.T) {
	if coreCanBuildProxyType("naive") {
		t.Fatal("naive не должен считаться строимым типом")
	}
	for _, typ := range []string{"vless", "hysteria2", "ss"} {
		if !coreCanBuildProxyType(typ) {
			t.Errorf("%q ядро строит, а тип объявлен нестроимым", typ)
		}
	}
	out := unsupportedOutcome("naive")
	if out.verdict != ProbeVerdictUnsupported || out.latencyMs != -1 || out.tcpMs != -1 {
		t.Fatalf("итог для naive: %+v", out)
	}
	if out.detail == "" {
		t.Fatal("итог без detail не объяснит человеку причину")
	}
}
