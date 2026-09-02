//go:build mihomo

// Реальный Prober поверх ядра mihomo: для каждого кандидата строит прокси-адаптер
// нужного типа и проверяет его URLTest'ом (настоящий handshake + HTTP-проба через
// прокси). Это даёт честный ответ на два вопроса автоподбора: достижим ли узел и
// какие протоколы реально проходят сквозь DPI с текущего клиента.
//
// Собирается только с тегом `mihomo`. Без тега используется TCPProber из
// prober.go (быстрая TCP-достижимость без проверки handshake).
package autotune

import (
	"context"
	"sync"
	"time"

	"github.com/metacubex/mihomo/adapter"
	"github.com/metacubex/mihomo/common/utils"
)

// probeURL — лёгкий эндпойнт без тела (HTTP 204), стандартный для url-test в
// clash/mihomo. Успешный ответ через прокси = протокол реально проходит.
const probeURL = "https://www.gstatic.com/generate_204"

// MihomoProber — реальный Prober. Заполняется тем же списком кандидатов, что и
// TCPProber, но дополнительно проверяет handshake каждого объявленного протокола
// через построенный прокси-адаптер mihomo.
//
// ProxyConfigs (необязательно) задаёт сырые clash-map'ы прокси по ServerID и
// протоколу: ключ — ServerID, значение — map[protocol]map[string]any (так, как
// прокси описан в конфиге панели). Если для кандидата есть конфиг конкретного
// протокола — он проверяется URLTest'ом; иначе протокол пропускается.
type MihomoProber struct {
	Candidates []Candidate
	// ProxyConfigs: ServerID -> (имя протокола -> сырой clash-конфиг прокси).
	// Эти map'ы берутся из секции proxies конфига подписки (по одному на
	// объявленный для узла протокол).
	ProxyConfigs map[string]map[string]map[string]any
	// Timeout — таймаут на одну проверку. 0 → 5s.
	Timeout time.Duration
	// Concurrency — число параллельных проверок. 0 → 6.
	Concurrency int
}

// NewMihomoProber собирает реальный Prober.
func NewMihomoProber(candidates []Candidate, proxyConfigs map[string]map[string]map[string]any) *MihomoProber {
	return &MihomoProber{Candidates: candidates, ProxyConfigs: proxyConfigs}
}

// Probe реализует Prober: для каждого узла проверяет handshake всех объявленных
// протоколов через mihomo URLTest и возвращает минимальную задержку и список
// реально прошедших протоколов.
func (p *MihomoProber) Probe(ctx context.Context) ([]ProbeResult, error) {
	if len(p.Candidates) == 0 {
		return nil, ErrNoProbes
	}
	timeout := p.Timeout
	if timeout <= 0 {
		timeout = 5 * time.Second
	}
	conc := p.Concurrency
	if conc <= 0 {
		conc = 6
	}

	results := make([]ProbeResult, len(p.Candidates))
	sem := make(chan struct{}, conc)
	var wg sync.WaitGroup

	for i, cand := range p.Candidates {
		wg.Add(1)
		go func(idx int, c Candidate) {
			defer wg.Done()
			// Захват слота — внутри воркера и с оглядкой на ctx, чтобы отмена
			// прерывала пачку между пробами, а не только по таймауту URLTest'а.
			select {
			case sem <- struct{}{}:
			case <-ctx.Done():
				results[idx] = ProbeResult{ServerID: c.ServerID, Country: c.Country, LatencyMs: -1}
				return
			}
			defer func() { <-sem }()
			results[idx] = p.probeOne(ctx, c, timeout)
		}(i, cand)
	}
	wg.Wait()

	return results, nil
}

// expectStatus — диапазон допустимых HTTP-статусов пробы (любой 2xx).
var expectStatus, _ = utils.NewUnsignedRanges[uint16]("200-299")

// probeOne проверяет один узел: каждый объявленный протокол — отдельным
// URLTest'ом через собранный прокси-адаптер. LatencyMs = минимум по прошедшим.
func (p *MihomoProber) probeOne(parent context.Context, c Candidate, timeout time.Duration) ProbeResult {
	res := ProbeResult{ServerID: c.ServerID, Country: c.Country, LatencyMs: -1}

	cfgByProto := p.ProxyConfigs[c.ServerID]
	protos := c.Protocols
	if len(protos) == 0 {
		protos = ProtocolPriority
	}

	best := -1
	var ok []string
	for _, proto := range protos {
		raw, has := cfgByProto[proto]
		if !has || raw == nil {
			// Нет сырого конфига прокси для этого протокола — проверить нечем.
			continue
		}
		px, err := adapter.ParseProxy(raw)
		if err != nil || px == nil {
			continue
		}
		ctx, cancel := context.WithTimeout(parent, timeout)
		delay, err := px.URLTest(ctx, probeURL, expectStatus)
		cancel()
		if err != nil || delay == 0 {
			continue // handshake не прошёл / отрезан DPI
		}
		ok = append(ok, proto)
		d := int(delay)
		if best < 0 || d < best {
			best = d
		}
	}

	if len(ok) > 0 {
		res.LatencyMs = best
		res.OKProtocols = orderByPriority(ok)
	}
	return res
}
