// Этот файл содержит кандидатов на измерение и Prober по умолчанию (быстрый
// TCP-замер), доступный в любой сборке — без нативного ядра mihomo. Он не умеет
// проверять handshake протоколов (нет криптослоя), поэтому помечает достижимые
// узлы «универсальным» набором протоколов из приоритета. Реальная проверка
// handshake живёт в prober_mihomo.go под build-тегом `mihomo`.

package autotune

import (
	"context"
	"net"
	"sort"
	"sync"
	"time"
)

// Candidate — выходной узел-кандидат для измерения. Заполняется из метаданных
// подписки (subscription.Server): адрес/порт берутся из конфига панели.
type Candidate struct {
	// ServerID — стабильный идентификатор узла (имя из конфига mihomo или UUID).
	ServerID string
	// Country — ISO-2 страны выходного узла (если известна), для relay-логики.
	Country string
	// Host — адрес узла (домен или IP) для TCP-замера.
	Host string
	// Port — порт узла.
	Port int
	// Protocols — протоколы, которые объявлены для этого узла панелью. Default
	// Prober использует их как «кандидаты на успех» при достижимости порта;
	// реальный mihomo-Prober проверяет каждый handshake'ом.
	Protocols []string
}

// addr возвращает host:port для net.Dial.
func (c Candidate) addr() string {
	return net.JoinHostPort(c.Host, itoa(c.Port))
}

// TCPProber — Prober по умолчанию: меряет TCP-достижимость порта каждого
// кандидата и RTT соединения. Доступен во всех сборках, поэтому AutoTune всегда
// вызываем (даже в CLI/тестах без ядра). Конкурентен и ограничен таймаутом.
type TCPProber struct {
	// Candidates — узлы для измерения.
	Candidates []Candidate
	// Timeout — таймаут на одно соединение. 0 → 3s.
	Timeout time.Duration
	// Concurrency — число параллельных замеров. 0 → 8.
	Concurrency int
}

// NewTCPProber собирает Prober по умолчанию из списка кандидатов.
func NewTCPProber(candidates []Candidate) *TCPProber {
	return &TCPProber{Candidates: candidates}
}

// Probe реализует интерфейс Prober: конкурентно меряет TCP-RTT до каждого
// кандидата. Достижимым считается узел, к которому удалось открыть TCP-соединение
// в пределах таймаута; набор «прошедших» протоколов для него = объявленные
// панелью протоколы (точная проверка handshake требует ядра, см. mihomo-Prober).
func (p *TCPProber) Probe(ctx context.Context) ([]ProbeResult, error) {
	if len(p.Candidates) == 0 {
		return nil, ErrNoProbes
	}
	timeout := p.Timeout
	if timeout <= 0 {
		timeout = 3 * time.Second
	}
	conc := p.Concurrency
	if conc <= 0 {
		conc = 8
	}

	results := make([]ProbeResult, len(p.Candidates))
	sem := make(chan struct{}, conc)
	var wg sync.WaitGroup

	for i, cand := range p.Candidates {
		wg.Add(1)
		go func(idx int, c Candidate) {
			defer wg.Done()
			// Захват слота — внутри воркера и с оглядкой на ctx, чтобы отмена
			// прерывала пачку между пробами, а не только по таймауту dial'а.
			select {
			case sem <- struct{}{}:
			case <-ctx.Done():
				return // results[idx] остаётся нулевым (LatencyMs 0 → недостижим)
			}
			defer func() { <-sem }()
			results[idx] = tcpProbeOne(ctx, c, timeout)
		}(i, cand)
	}
	wg.Wait()

	return results, nil
}

// tcpProbeOne открывает TCP-соединение и возвращает измерение для одного узла.
func tcpProbeOne(ctx context.Context, c Candidate, timeout time.Duration) ProbeResult {
	res := ProbeResult{ServerID: c.ServerID, Country: c.Country, LatencyMs: -1}

	dctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	var d net.Dialer
	start := time.Now()
	conn, err := d.DialContext(dctx, "tcp", c.addr())
	if err != nil {
		return res // недостижим: LatencyMs остаётся -1
	}
	_ = conn.Close()

	rtt := int(time.Since(start) / time.Millisecond)
	if rtt <= 0 {
		rtt = 1 // достижим быстрее 1мс — нормализуем, чтобы пройти reachable()
	}
	res.LatencyMs = rtt
	// Без ядра нельзя проверить handshake — считаем «прошедшими» протоколы,
	// объявленные панелью для этого узла. Если их нет — берём весь приоритет,
	// чтобы достижимый узел не выпал из выбора.
	if len(c.Protocols) > 0 {
		res.OKProtocols = orderByPriority(c.Protocols)
	} else {
		res.OKProtocols = append([]string(nil), ProtocolPriority...)
	}
	return res
}

// orderByPriority возвращает протоколы в порядке ProtocolPriority (известные
// первыми), сохраняя неизвестные в конце в исходном порядке.
func orderByPriority(protos []string) []string {
	rank := make(map[string]int, len(ProtocolPriority))
	for i, p := range ProtocolPriority {
		rank[p] = i
	}
	out := append([]string(nil), protos...)
	sort.SliceStable(out, func(i, j int) bool {
		ri, oki := rank[out[i]]
		rj, okj := rank[out[j]]
		if oki && okj {
			return ri < rj
		}
		if oki != okj {
			return oki // известные раньше неизвестных
		}
		return false
	})
	return out
}

// itoa — маленький helper, чтобы не тянуть strconv ради одного порта.
func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	neg := n < 0
	if neg {
		n = -n
	}
	var buf [20]byte
	i := len(buf)
	for n > 0 {
		i--
		buf[i] = byte('0' + n%10)
		n /= 10
	}
	if neg {
		i--
		buf[i] = '-'
	}
	return string(buf[i:])
}
