package autotune

import "testing"

func TestRecommendPicksLowestLatencyDirect(t *testing.T) {
	probes := []ProbeResult{
		{ServerID: "us", Country: "US", LatencyMs: 96, OKProtocols: []string{"Shadowsocks", "AmneziaWG"}},
		{ServerID: "fi", Country: "FI", LatencyMs: 18, OKProtocols: []string{"VLESS-Reality", "AmneziaWG"}},
		{ServerID: "nl", Country: "NL", LatencyMs: 24, OKProtocols: []string{"Hysteria2"}},
	}
	r, err := Recommend(probes, []string{"TR"})
	if err != nil {
		t.Fatal(err)
	}
	if r.ServerID != "fi" {
		t.Errorf("ожидался сервер fi (мин. задержка), получено %q", r.ServerID)
	}
	if r.Protocol != "AmneziaWG" {
		t.Errorf("ожидался самый приоритетный протокол AmneziaWG, получено %q", r.Protocol)
	}
	if r.Relay != "" {
		t.Errorf("при прямом пути relay не нужен, получено %q", r.Relay)
	}
	if r.Stack != DefaultStack {
		t.Errorf("ожидался стек %q, получено %q", DefaultStack, r.Stack)
	}
}

func TestRecommendFallsBackToRelayWhenAllBlocked(t *testing.T) {
	probes := []ProbeResult{
		{ServerID: "nl", Country: "NL", LatencyMs: 0, OKProtocols: nil},
		{ServerID: "fi", Country: "FI", LatencyMs: -1, OKProtocols: []string{}},
	}
	r, err := Recommend(probes, []string{"TR", "KZ"})
	if err != nil {
		t.Fatal(err)
	}
	if r.Relay != "TR" {
		t.Errorf("ожидался relay TR при заблокированном прямом входе, получено %q", r.Relay)
	}
	if r.Protocol != ProtocolPriority[0] {
		t.Errorf("ожидался приоритетный протокол %q, получено %q", ProtocolPriority[0], r.Protocol)
	}
}

func TestRecommendNoRelayCandidatesStillReturns(t *testing.T) {
	probes := []ProbeResult{{ServerID: "nl", Country: "NL", LatencyMs: 0}}
	r, err := Recommend(probes, nil)
	if err != nil {
		t.Fatal(err)
	}
	if r.ServerID != "nl" || r.Relay != "" {
		t.Errorf("ожидался лучший доступный без relay, получено %+v", r)
	}
}

func TestRecommendErrorsOnEmpty(t *testing.T) {
	if _, err := Recommend(nil, nil); err == nil {
		t.Error("ожидалась ошибка ErrNoProbes на пустых измерениях")
	}
}

func TestBestProtocolRespectsPriority(t *testing.T) {
	if got := bestProtocol([]string{"TUIC", "VLESS-Reality", "Shadowsocks"}); got != "VLESS-Reality" {
		t.Errorf("ожидался VLESS-Reality по приоритету, получено %q", got)
	}
	if got := bestProtocol([]string{"Unknown"}); got != "Unknown" {
		t.Errorf("неизвестный протокол должен возвращаться как есть, получено %q", got)
	}
	if got := bestProtocol(nil); got != "" {
		t.Errorf("пустой список должен давать пустую строку, получено %q", got)
	}
}
