package autotune

import (
	"context"
	"net"
	"testing"
	"time"
)

func TestOrderByPriority(t *testing.T) {
	got := orderByPriority([]string{"Shadowsocks", "Unknown", "AmneziaWG", "TUIC"})
	want := []string{"AmneziaWG", "TUIC", "Shadowsocks", "Unknown"}
	if len(got) != len(want) {
		t.Fatalf("длина: ожидалось %d, получено %d (%v)", len(want), len(got), got)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("позиция %d: ожидалось %q, получено %q (%v)", i, want[i], got[i], got)
		}
	}
}

func TestItoa(t *testing.T) {
	cases := map[int]string{0: "0", 7: "7", 443: "443", 65535: "65535", -1: "-1"}
	for in, want := range cases {
		if got := itoa(in); got != want {
			t.Errorf("itoa(%d): ожидалось %q, получено %q", in, want, got)
		}
	}
}

func TestTCPProberEmptyCandidates(t *testing.T) {
	p := NewTCPProber(nil)
	if _, err := p.Probe(context.Background()); err != ErrNoProbes {
		t.Errorf("ожидался ErrNoProbes на пустых кандидатах, получено %v", err)
	}
}

// TestTCPProberReachableAndUnreachable поднимает локальный listener (достижим) и
// добавляет заведомо закрытый порт (недостижим), проверяя классификацию.
func TestTCPProberReachableAndUnreachable(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	defer ln.Close()

	// Принимаем соединения, чтобы DialContext завершался успехом.
	go func() {
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			_ = conn.Close()
		}
	}()

	host, portStr, _ := net.SplitHostPort(ln.Addr().String())
	port := 0
	for _, ch := range portStr {
		port = port*10 + int(ch-'0')
	}

	p := NewTCPProber([]Candidate{
		{ServerID: "live", Country: "FI", Host: host, Port: port, Protocols: []string{"VLESS-Reality"}},
		{ServerID: "dead", Country: "NL", Host: "127.0.0.1", Port: 1, Protocols: []string{"Hysteria2"}},
	})
	p.Timeout = 500 * time.Millisecond

	res, err := p.Probe(context.Background())
	if err != nil {
		t.Fatalf("probe: %v", err)
	}
	if len(res) != 2 {
		t.Fatalf("ожидалось 2 результата, получено %d", len(res))
	}

	byID := map[string]ProbeResult{}
	for _, r := range res {
		byID[r.ServerID] = r
	}

	live := byID["live"]
	if live.LatencyMs <= 0 {
		t.Errorf("достижимый узел должен иметь положительную задержку, получено %d", live.LatencyMs)
	}
	if len(live.OKProtocols) == 0 || live.OKProtocols[0] != "VLESS-Reality" {
		t.Errorf("достижимый узел должен отдавать объявленные протоколы, получено %v", live.OKProtocols)
	}

	dead := byID["dead"]
	if dead.LatencyMs > 0 {
		t.Errorf("закрытый порт должен быть недостижим (LatencyMs<=0), получено %d", dead.LatencyMs)
	}
	if len(dead.OKProtocols) != 0 {
		t.Errorf("недостижимый узел не должен иметь прошедших протоколов, получено %v", dead.OKProtocols)
	}

	// Результаты прободы должны корректно скармливаться Recommend.
	rec, err := Recommend(res, []string{"TR"})
	if err != nil {
		t.Fatalf("recommend: %v", err)
	}
	if rec.ServerID != "live" {
		t.Errorf("Recommend должен выбрать достижимый узел live, получено %q", rec.ServerID)
	}
	if rec.Relay != "" {
		t.Errorf("при достижимом прямом пути relay не нужен, получено %q", rec.Relay)
	}
}
