package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/profile"
	"github.com/semanticparadox/caramba/libs/caramba-core/subimport"
	"gopkg.in/yaml.v3"
)

// probeConcurrency — сколько узлов меряется одновременно. Ограничение важно и на
// мобильном (лимит дескрипторов, батарея), и на десктопе (подписка на сотню
// узлов иначе откроет сотню соединений разом).
//
// 16, а не 8: по живому замеру боевого флота рабочий узел отвечает за 150-540
// мс, и при восьми слотах проход по тринадцати узлам упирался не в узлы, а в
// очередь. Автоподбор при этом ждёт ВЕСЬ проход, поэтому очередь здесь — прямая
// секунда ожидания человека. Каждый слот в сборке с ядром открывает до двух
// соединений (URL-тест и справочный TCP), то есть потолок дескрипторов — 32.
const probeConcurrency = 16

// defaultProbeTimeout — таймаут на один узел, если приложение не задало свой.
const defaultProbeTimeout = 3 * time.Second

// Цель url-test замера узлов.
//
// Зашитый gstatic.com здесь был той же ошибкой, что и в profile: в РФ он
// заблокирован, и замер честно объявлял мёртвыми ВСЕ узлы, потому что через
// живой узел не отвечал недоступный хост. Цель переезжает на хост из
// подписанного пула зеркал (setProbeTarget вызывается при сборке конфига), а
// зашитое значение остаётся последним средством для клиента без каталога.
var (
	probeTargetMu sync.RWMutex
	probeTarget   = profile.DefaultProbeURL
)

// setProbeTarget задаёт цель замера. Пустая строка возвращает умолчание.
func setProbeTarget(u string) {
	probeTargetMu.Lock()
	defer probeTargetMu.Unlock()
	if u = strings.TrimSpace(u); u == "" {
		probeTarget = profile.DefaultProbeURL
		return
	}
	probeTarget = u
}

// probeTargetURL отдаёт текущую цель замера.
func probeTargetURL() string {
	probeTargetMu.RLock()
	defer probeTargetMu.RUnlock()
	return probeTarget
}

// ProbeServer — результат замера одного узла (контракт ABI v2, CarambaProbe).
//
// ID совпадает с именем прокси в конфиге: его же приложение передаёт в
// Up(serverID), чтобы закрепить узел. LatencyMs = -1 означает «через узел не
// прошёл настоящий запрос» (в отличие от 0, который значил бы «мгновенно»), и
// это НЕ синоним «адрес мёртв»: почему именно, говорит Verdict.
type ProbeServer struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Type      string `json:"type"`
	Server    string `json:"server"`
	Port      int    `json:"port"`
	Country   string `json:"country"`
	LatencyMs int    `json:"latencyMs"`

	// TcpMs — RTT установки TCP-соединения с адресом узла; -1, если адрес не
	// ответил. Справочное число: оно отвечает на вопрос «жив ли адрес», а не
	// «работает ли узел», и само по себе задержкой узла не является.
	TcpMs int `json:"tcpMs"`

	// Verdict — чем кончилась проверка (ProbeVerdict*). Приложение решает по
	// НЕМУ, а не по числу: LatencyMs >= 0 бывает только при ok и tcp_only.
	Verdict string `json:"verdict"`

	// Detail — сырой текст ошибки для «Подробностей». Пусто при ok.
	Detail string `json:"detail,omitempty"`
}

// ProbeReport — полный результат CarambaProbe.
type ProbeReport struct {
	Servers []ProbeServer `json:"servers"`
}

// probeNode — узел из загруженной конфигурации, подготовленный к замеру.
type probeNode struct {
	name   string
	typ    string
	server string
	port   int
	// raw — исходный clash-map прокси. Нужен сборке с ядром: из него строится
	// адаптер для честной проверки через URLTest.
	raw map[string]any
}

// Probe меряет задержку до КАЖДОГО узла текущей загруженной конфигурации, не
// поднимая туннель.
//
// Источник узлов — см. probeConfig: импортированная подписка, иначе кэш
// панельного профиля, иначе загрузка профиля у панели. Если взять узлы неоткуда
// (панель не настроена, вход не выполнен, импорта не было), возвращается пустой
// список — это не ошибка: приложение вправе спросить о замере до импорта.
//
// timeoutMs <= 0 означает таймаут по умолчанию (3с). Замеры идут параллельно, но
// не более probeConcurrency одновременно. Способ замера зависит от сборки:
// TCP-соединение с server:port без тега mihomo и настоящий URL-тест через прокси
// под -tags mihomo (см. probe_default.go / probe_mihomo.go). В обоих случаях
// узел приходит с ВЕРДИКТОМ, и фолбэка «URL-тест не прошёл — покажем TCP» больше
// нет: он был единственной причиной, по которой узел с отвергнутым ключом
// выглядел самым быстрым в списке.
//
// Замер идёт ДО и БЕЗ туннеля и меряет расстояние ЭТОГО устройства до узла —
// это и есть то, чего у приложения не было: панель отдаёт `nodes.last_latency`,
// то есть RTT самого узла до его цели по heartbeat. Мерить сквозь поднятый
// туннель бессмысленно: там остался бы один узел, тот, через который туннель и
// стоит.
func (c *Core) Probe(ctx context.Context, timeoutMs int) (ProbeReport, error) {
	raw, err := c.probeConfig(ctx)
	if err != nil {
		return ProbeReport{}, err
	}

	if len(raw) == 0 {
		return ProbeReport{Servers: []ProbeServer{}}, nil
	}

	nodes, err := parseProbeNodes(raw)
	if err != nil {
		return ProbeReport{}, err
	}
	if len(nodes) == 0 {
		return ProbeReport{Servers: []ProbeServer{}}, nil
	}

	timeout := time.Duration(timeoutMs) * time.Millisecond
	if timeoutMs <= 0 {
		timeout = defaultProbeTimeout
	}

	servers := make([]ProbeServer, len(nodes))
	sem := make(chan struct{}, probeConcurrency)
	var wg sync.WaitGroup

	for i, n := range nodes {
		skipped := skippedOutcome()
		servers[i] = ProbeServer{
			ID:        n.name,
			Name:      n.name,
			Type:      n.typ,
			Server:    n.server,
			Port:      n.port,
			Country:   subimport.CountryFromName(n.name),
			LatencyMs: skipped.latencyMs,
			TcpMs:     skipped.tcpMs,
			// Узел, до которого проход не дошёл, обязан отличаться от узла,
			// который проверили и он не ответил. Раньше оба были «-1».
			Verdict: skipped.verdict,
			Detail:  skipped.detail,
		}
		wg.Add(1)
		go func(idx int, node probeNode) {
			defer wg.Done()
			// Слот захватываем внутри воркера и с оглядкой на ctx, чтобы отмена
			// прерывала пачку между узлами, а не только по таймауту одного
			// соединения. Результат остаётся -1 (недостижим).
			select {
			case sem <- struct{}{}:
			case <-ctx.Done():
				return
			}
			defer func() { <-sem }()
			// Слот мог достаться уже после отмены: select выше выбирает
			// случайно, когда готовы обе ветки. Замер под отменённым
			// контекстом провалится по причине, к узлу не относящейся, и
			// назвать её «порт закрыт» значило бы оболгать живой узел.
			if ctx.Err() != nil {
				return
			}
			out := probeOne(ctx, node, timeout)
			// То же самое, но про отмену ПОСРЕДИ замера. Успех остаётся
			// успехом: если запрос сквозь узел прошёл, он прошёл.
			if ctx.Err() != nil && out.verdict != ProbeVerdictOK {
				return
			}
			servers[idx].LatencyMs = out.latencyMs
			servers[idx].TcpMs = out.tcpMs
			servers[idx].Verdict = out.verdict
			servers[idx].Detail = out.detail
		}(i, n)
	}
	wg.Wait()

	return ProbeReport{Servers: servers}, nil
}

// probeConfig отдаёт конфигурацию, узлы которой надо померить.
//
// Порядок: импортированная подписка → кэш последнего панельного профиля → сама
// панель. Последней ступени раньше не было, и из-за этого замер на ПАНЕЛЬНОМ
// пути был мёртв: конфиг там кэшируется только внутри Up (и NewDefaultProber),
// то есть после подъёма туннеля, — а пользователь спрашивает про задержки
// ИМЕННО до подключения, выбирая, куда подключаться. Ядро, которое приложение
// заводит специально под замер, ни того ни другого не делало и честно
// возвращало пустой список; на автоподборе это и был «Ядро не вернуло ни
// одного узла».
//
// Пустой результат без ошибки означает «узлы взять неоткуда, и это законно»:
// панель не настроена (чистый режим импорта) либо вход не выполнен. Ошибка —
// только настоящая неудача загрузки, и её надо показать, а не выдать за
// «серверов нет».
func (c *Core) probeConfig(ctx context.Context) ([]byte, error) {
	c.mu.Lock()
	raw := c.importedConfig
	if len(raw) == 0 {
		raw = c.lastPanelYAML
	}
	c.mu.Unlock()
	if len(raw) > 0 {
		return raw, nil
	}

	if strings.TrimSpace(c.cfg.PanelBaseURL) == "" || !c.auth.IsAuthenticated() {
		return nil, nil
	}

	// candidates уже умеет всё нужное: лениво подтягивает UUID подписки,
	// загружает профиль и кладёт сырой YAML в кэш — тот самый, который читает
	// эта же функция при следующем замере.
	_, fetched, err := c.candidates(ctx)
	if err != nil {
		return nil, fmt.Errorf("api: загрузка узлов подписки для замера: %w", err)
	}
	return fetched, nil
}

// ProbeJSON — JSON-обёртка над Probe для gomobile/FFI: {"servers":[...]}.
func (c *Core) ProbeJSON(ctx context.Context, timeoutMs int) (string, error) {
	rep, err := c.Probe(ctx, timeoutMs)
	if err != nil {
		return "", err
	}
	b, err := json.Marshal(rep)
	if err != nil {
		return "", fmt.Errorf("api: сериализация результата замера: %w", err)
	}
	return string(b), nil
}

// parseProbeNodes достаёт узлы из clash/mihomo YAML С СОХРАНЕНИЕМ ПОРЯДКА.
//
// Порядок важен: приложение показывает список серверов ровно в том виде, в каком
// его отдал ImportSubscription, и результат замера должен ложиться на тот же
// список. Поэтому здесь не используется subscription.ProxyMaps (он отдаёт map и
// порядок теряет).
func parseProbeNodes(rawYAML []byte) ([]probeNode, error) {
	var doc struct {
		Proxies []map[string]any `yaml:"proxies"`
	}
	if err := yaml.Unmarshal(rawYAML, &doc); err != nil {
		return nil, fmt.Errorf("api: разбор узлов конфигурации: %w", err)
	}
	nodes := make([]probeNode, 0, len(doc.Proxies))
	for _, px := range doc.Proxies {
		name, _ := px["name"].(string)
		if name == "" {
			continue
		}
		typ, _ := px["type"].(string)
		server, _ := px["server"].(string)
		nodes = append(nodes, probeNode{
			name:   name,
			typ:    typ,
			server: server,
			port:   anyToInt(px["port"]),
			raw:    px,
		})
	}
	return nodes, nil
}

// anyToInt приводит значение порта к int: из YAML он приходит int, из JSON —
// float64, из URI-разбора — строкой.
func anyToInt(v any) int {
	switch n := v.(type) {
	case int:
		return n
	case int64:
		return int(n)
	case float64:
		return int(n)
	case string:
		if i, err := strconv.Atoi(n); err == nil {
			return i
		}
	}
	return 0
}

// tcpProbe меряет RTT установки TCP-соединения с узлом. Возвращает -1, если
// соединение не удалось в пределах таймаута.
//
// Это базовый способ замера (сборка без ядра) и одновременно фолбэк для сборки
// с ядром: он не проверяет handshake протокола, но честно отвечает на вопрос
// «жив ли адрес и как далеко он».
func tcpProbe(ctx context.Context, node probeNode, timeout time.Duration) int {
	if node.server == "" || node.port <= 0 {
		return -1
	}
	dctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	var d net.Dialer
	start := time.Now()
	conn, err := d.DialContext(dctx, "tcp", net.JoinHostPort(node.server, strconv.Itoa(node.port)))
	if err != nil {
		return -1
	}
	_ = conn.Close()

	ms := int(time.Since(start) / time.Millisecond)
	if ms <= 0 {
		// Достижим быстрее миллисекунды: 0 в контракте значил бы «мгновенно»,
		// но приложение сортирует по возрастанию и 0 читается как «нет данных».
		ms = 1
	}
	return ms
}
