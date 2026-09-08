// Command caramba-smoke — дымовой прогон подключения БЕЗ привилегий.
//
// Утилита доказывает, что подписка реально работает, не поднимая TUN и не
// требуя root: ядро стартует в proxy-режиме (mixed-инбаунд SOCKS5+HTTP на
// 127.0.0.1:<port>), после чего сама утилита ходит в интернет ЧЕРЕЗ этот порт и
// сравнивает выходной IP с прямым. Ровно тот путь, которым десктопный клиент
// подтверждает соединение до того, как получит право поднимать TUN.
//
// Сборка (нужен нативный движок, иначе прогон бессмысленен):
//
//	CGO_ENABLED=1 go build -tags mihomo -o build/caramba-smoke ./cmd/caramba-smoke
//
// То же делает scripts/build-smoke.sh. Сборка без тега mihomo компилируется, но
// на запуске честно сообщает, что движок — заглушка, и завершается с ошибкой,
// вместо ложного «успеха».
//
// Примеры:
//
//	build/caramba-smoke --sub https://panel.example/sub/<uuid>
//	build/caramba-smoke --sub ./my-clash.yaml --format clash --port 7891
//	build/caramba-smoke --sub ./my-clash.yaml --probe --server "NL-1"
//
// Код возврата 0 только если запрос ЧЕРЕЗ локальный прокси действительно
// прошёл.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/semanticparadox/caramba/libs/caramba-core/mobile"
	"github.com/semanticparadox/caramba/libs/caramba-core/subimport"
)

const (
	// userAgent — то, чем утилита представляется панели при выкачивании
	// подписки. Панели часто отдают разный формат в зависимости от UA, поэтому
	// он явный и стабильный.
	userAgent = "CarambaConnect/1.0 (smoke)"

	// ipEchoURL — эхо выходного IP (JSON {"ip":"..."}).
	ipEchoURL = "https://api.ipify.org?format=json"
	// noContentURL — классическая проверка «есть интернет» (ожидается 204).
	noContentURL = "https://www.google.com/generate_204"

	// placeholderPanelURL — api.NewCore требует непустой URL панели, но
	// raw-путь (импортированная подписка) к панели не ходит вовсе. Домен в
	// зарезервированной зоне .invalid гарантирует, что случайного запроса не
	// случится.
	placeholderPanelURL = "https://panel.invalid"
)

// options — разобранные флаги.
type options struct {
	sub     string
	format  string
	server  string
	port    int
	timeout time.Duration
	workdir string
	// probe включает замер задержек до всех узлов подписки ПЕРЕД подъёмом
	// ядра (CarambaProbe/ProbeJSON).
	probe bool
	// probeTimeout — таймаут на один узел замера.
	probeTimeout time.Duration
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintf(os.Stderr, "\nПРОВАЛ: %v\n", err)
		os.Exit(1)
	}
}

// parseFlags разбирает аргументы командной строки.
func parseFlags() (options, error) {
	var o options
	fs := flag.NewFlagSet("caramba-smoke", flag.ContinueOnError)
	fs.StringVar(&o.sub, "sub", "", "URL подписки либо путь к файлу с сырым конфигом (обязателен)")
	fs.StringVar(&o.format, "format", subimport.FormatAuto, "формат подписки: auto|uri|v2ray|clash|singbox")
	fs.StringVar(&o.server, "server", "", "имя узла (поле id метаданных импорта); закрепляется первым в селекторе CARAMBA, пусто — автоматика")
	fs.IntVar(&o.port, "port", 7890, "порт локального mixed-инбаунда (SOCKS5+HTTP)")
	fs.DurationVar(&o.timeout, "timeout", 30*time.Second, "таймаут на выкачивание подписки, ожидание connected и каждую проверку")
	fs.StringVar(&o.workdir, "workdir", "", "рабочий каталог ядра; пусто — временный каталог, удаляемый после прогона")
	fs.BoolVar(&o.probe, "probe", false, "перед подъёмом туннеля промерить задержку до каждого узла и напечатать таблицу")
	fs.DurationVar(&o.probeTimeout, "probe-timeout", 3*time.Second, "таймаут на один узел при --probe")
	fs.Usage = func() {
		fmt.Fprintf(fs.Output(), "caramba-smoke — проверка подписки через локальный прокси, без root.\n\n")
		fmt.Fprintf(fs.Output(), "Использование:\n  caramba-smoke --sub <URL|файл> [флаги]\n\nФлаги:\n")
		fs.PrintDefaults()
		fmt.Fprintf(fs.Output(), "\nКод возврата 0 только если запрос через 127.0.0.1:<port> прошёл.\n")
	}
	if err := fs.Parse(os.Args[1:]); err != nil {
		// flag сам печатает Usage; ErrHelp не считаем ошибкой прогона.
		if errors.Is(err, flag.ErrHelp) {
			os.Exit(0)
		}
		return options{}, err
	}

	if strings.TrimSpace(o.sub) == "" {
		fs.Usage()
		return options{}, errors.New("не задан --sub (URL подписки или путь к файлу)")
	}
	if o.port <= 0 || o.port > 65535 {
		return options{}, fmt.Errorf("некорректный --port %d (ожидается 1..65535)", o.port)
	}
	if o.timeout <= 0 {
		return options{}, fmt.Errorf("некорректный --timeout %s (ожидается положительная длительность)", o.timeout)
	}
	if o.probeTimeout <= 0 {
		return options{}, fmt.Errorf("некорректный --probe-timeout %s (ожидается положительная длительность)", o.probeTimeout)
	}
	o.format = strings.ToLower(strings.TrimSpace(o.format))
	if o.format == "" {
		o.format = subimport.FormatAuto
	}
	switch o.format {
	case subimport.FormatAuto, subimport.FormatURI, subimport.FormatV2ray, subimport.FormatClash, subimport.FormatSingbox:
	default:
		return options{}, fmt.Errorf("неизвестный --format %q (ожидается auto|uri|v2ray|clash|singbox)", o.format)
	}
	return o, nil
}

// run выполняет весь прогон. Любая проблема возвращается ошибкой; паник нет.
func run() error {
	o, err := parseFlags()
	if err != nil {
		return err
	}

	fmt.Printf("== caramba-smoke ==\n")
	fmt.Printf("режим: proxy, mixed-инбаунд 127.0.0.1:%d, таймаут %s\n\n", o.port, o.timeout)

	if !hasMihomoCore {
		return errors.New("бинарник собран БЕЗ тега mihomo: движок — заглушка, реального туннеля не будет. " +
			"Пересоберите: CGO_ENABLED=1 go build -tags mihomo -o build/caramba-smoke ./cmd/caramba-smoke " +
			"(или scripts/build-smoke.sh)")
	}

	// 1. Прямой выход — точка отсчёта для сравнения IP.
	directIP := ""
	fmt.Println("1/7 прямой выход (без прокси)")
	if ip, lat, derr := fetchExitIP(directClient(o.timeout)); derr != nil {
		// Не фатально: прямой выход может быть заблокирован, ради этого всё и затевается.
		fmt.Printf("     не удалось (%v); сравнивать будет не с чем\n", derr)
	} else {
		directIP = ip
		fmt.Printf("     IP %s (%s)\n", ip, lat.Round(time.Millisecond))
	}

	// 2. Сырая подписка: URL или файл.
	fmt.Printf("\n2/7 загрузка подписки: %s\n", o.sub)
	raw, err := loadSubscription(o.sub, o.timeout)
	if err != nil {
		return err
	}
	fmt.Printf("     получено %d байт\n", len(raw))

	// 3. Рабочий каталог.
	workDir := strings.TrimSpace(o.workdir)
	tempDir := ""
	if workDir == "" {
		tempDir, err = os.MkdirTemp("", "caramba-smoke-")
		if err != nil {
			return fmt.Errorf("временный рабочий каталог: %w", err)
		}
		workDir = tempDir
		defer func() { _ = os.RemoveAll(tempDir) }()
	} else if err := os.MkdirAll(workDir, 0o700); err != nil {
		return fmt.Errorf("рабочий каталог %s: %w", workDir, err)
	}

	// Каталог ядра обязан существовать ДО разбора конфига: правила GEOIP/GEOSITE
	// заставляют mihomo искать (и при отсутствии докачивать) geo-базы в нём, а
	// constant.Path.MMDB() отдаёт пустой путь, если каталога нет, и разбор падает
	// с «can't download MMDB: open : no such file or directory».
	homeDir := coreHomeDir(workDir)
	if err := os.MkdirAll(homeDir, 0o700); err != nil {
		return fmt.Errorf("каталог ядра %s: %w", homeDir, err)
	}
	client, err := mobile.NewClient(placeholderPanelURL, "", workDir, filepath.Join(workDir, "tokens.json"))
	if err != nil {
		return fmt.Errorf("создание ядра: %w", err)
	}
	// Порядок важен: api.NewCore сам ставит домашним каталогом ядра свой
	// workDir (чтобы geo-базы качались в него на чистой машине). Здесь мы
	// переопределяем это уже ПОСЛЕ создания ядра: рабочий каталог прогона
	// временный и удаляется, а перекачивать десятки мегабайт каждый запуск
	// незачем.
	setCoreHomeDir(homeDir)

	// 4. Импорт подписки. Формат auto определяется по содержимому, включая
	// base64-список v2ray (subimport сам его декодирует).
	fmt.Printf("\n3/7 импорт подписки (формат %s)\n", o.format)
	metaJSON, err := client.ImportSubscription(string(raw), o.format)
	if err != nil {
		return fmt.Errorf("импорт подписки: %w", err)
	}
	printServers(metaJSON)

	// 5. Замер узлов (опционально) — до подъёма туннеля.
	fmt.Printf("\n4/7 замер узлов (--probe)\n")
	if o.probe {
		probeJSON, perr := client.ProbeJSON(int(o.probeTimeout / time.Millisecond))
		if perr != nil {
			return fmt.Errorf("замер узлов: %w", perr)
		}
		if perr := printProbe(probeJSON); perr != nil {
			return perr
		}
	} else {
		fmt.Printf("     пропущен (добавьте --probe)\n")
	}

	// 6. Proxy-режим и подъём ядра.
	fmt.Printf("\n5/7 запуск ядра в proxy-режиме\n")
	fmt.Printf("     рабочий каталог: %s\n", workDir)
	fmt.Printf("     каталог geo-баз: %s (первый прогон их докачивает)\n", homeDir)
	if err := client.SetTunnelMode("proxy", o.port); err != nil {
		return fmt.Errorf("переключение режима: %w", err)
	}
	upJSON, err := client.Up(o.server)
	if err != nil {
		return fmt.Errorf("подъём туннеля: %w", err)
	}
	// Гасим ядро в любом случае, чтобы не оставить занятый порт.
	defer func() {
		if derr := client.Down(); derr != nil {
			fmt.Fprintf(os.Stderr, "предупреждение: остановка ядра: %v\n", derr)
		}
	}()
	fmt.Printf("     up: %s\n", upJSON)

	waited, err := waitConnected(client, o.timeout)
	if err != nil {
		return err
	}
	fmt.Printf("     стадия connected за %s\n", waited.Round(time.Millisecond))

	if err := waitPort(o.port, o.timeout); err != nil {
		return err
	}
	fmt.Printf("     порт 127.0.0.1:%d принимает соединения\n", o.port)

	// 7. Проверки ЧЕРЕЗ локальный прокси — единственный критерий успеха.
	fmt.Printf("\n6/7 проверка трафика через 127.0.0.1:%d\n", o.port)
	proxied, err := proxyClient(o.port, o.timeout)
	if err != nil {
		return err
	}
	proxyIP, ipLat, err := fetchExitIP(proxied)
	if err != nil {
		return fmt.Errorf("запрос %s через прокси: %w", ipEchoURL, err)
	}
	fmt.Printf("     выходной IP %s (%s)\n", proxyIP, ipLat.Round(time.Millisecond))

	code, ncLat, err := fetchStatus(proxied, noContentURL)
	if err != nil {
		return fmt.Errorf("запрос %s через прокси: %w", noContentURL, err)
	}
	fmt.Printf("     %s → HTTP %d (%s)\n", noContentURL, code, ncLat.Round(time.Millisecond))
	if code != http.StatusNoContent {
		return fmt.Errorf("%s вернул HTTP %d вместо 204", noContentURL, code)
	}

	// 8. Итоговые снимки состояния и счётчиков.
	fmt.Printf("\n7/7 состояние ядра\n")
	if st, serr := client.StatusJSON(); serr == nil {
		fmt.Printf("     status:  %s\n", st)
	} else {
		fmt.Printf("     status:  недоступен (%v)\n", serr)
	}
	if tr, terr := client.TrafficJSON(); terr == nil {
		fmt.Printf("     traffic: %s\n", tr)
	} else {
		fmt.Printf("     traffic: недоступен (%v)\n", terr)
	}

	fmt.Printf("\nУСПЕХ: трафик идёт через 127.0.0.1:%d\n", o.port)
	switch {
	case directIP == "":
		fmt.Printf("  прямой IP: неизвестен, через прокси: %s\n", proxyIP)
	case directIP == proxyIP:
		fmt.Printf("  ВНИМАНИЕ: IP совпал с прямым (%s) — узел, похоже, выпускает через тот же адрес либо сработало правило DIRECT\n", proxyIP)
	default:
		fmt.Printf("  прямой IP: %s, через прокси: %s\n", directIP, proxyIP)
	}
	return nil
}

// coreHomeDir выбирает каталог, в котором ядро держит geo-базы (geoip.metadb,
// GeoSite.dat). Это НЕ рабочий каталог прогона: рабочий может быть временным и
// удаляться, а перекачивать десятки мегабайт каждый запуск незачем. Поэтому
// берём пользовательский кэш и падаем на рабочий каталог, только если кэш
// недоступен.
func coreHomeDir(workDir string) string {
	if cache, err := os.UserCacheDir(); err == nil && strings.TrimSpace(cache) != "" {
		return filepath.Join(cache, "caramba-smoke")
	}
	return filepath.Join(workDir, "core-home")
}

// loadSubscription читает подписку: по http(s) — GET с фирменным User-Agent,
// иначе — из файла. Декодирование base64-списков v2ray делает subimport при
// импорте (detectFormat/tryBase64), отдельного шага здесь не нужно.
func loadSubscription(src string, timeout time.Duration) ([]byte, error) {
	low := strings.ToLower(src)
	if !strings.HasPrefix(low, "http://") && !strings.HasPrefix(low, "https://") {
		data, err := os.ReadFile(src)
		if err != nil {
			return nil, fmt.Errorf("чтение файла подписки: %w", err)
		}
		if len(data) == 0 {
			return nil, fmt.Errorf("файл подписки %s пуст", src)
		}
		return data, nil
	}

	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, src, nil)
	if err != nil {
		return nil, fmt.Errorf("некорректный URL подписки: %w", err)
	}
	req.Header.Set("User-Agent", userAgent)
	resp, err := directClient(timeout).Do(req)
	if err != nil {
		return nil, fmt.Errorf("загрузка подписки: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("подписка вернула HTTP %d", resp.StatusCode)
	}
	// Ограничение сверху защищает от случайной выкачки гигантского тела.
	data, err := io.ReadAll(io.LimitReader(resp.Body, 8<<20))
	if err != nil {
		return nil, fmt.Errorf("чтение тела подписки: %w", err)
	}
	if len(data) == 0 {
		return nil, errors.New("подписка вернула пустое тело")
	}
	return data, nil
}

// printServers печатает список узлов из JSON метаданных импорта. Ошибка разбора
// не фатальна: это лишь диагностика.
func printServers(metaJSON string) {
	var meta struct {
		Servers []struct {
			ID      string `json:"id"`
			Name    string `json:"name"`
			Type    string `json:"type"`
			Server  string `json:"server"`
			Port    int    `json:"port"`
			Country string `json:"country"`
		} `json:"servers"`
	}
	if err := json.Unmarshal([]byte(metaJSON), &meta); err != nil {
		fmt.Printf("     метаданные: %s\n", metaJSON)
		return
	}
	fmt.Printf("     узлов: %d\n", len(meta.Servers))
	for i, s := range meta.Servers {
		if i >= 10 {
			fmt.Printf("       ... и ещё %d\n", len(meta.Servers)-10)
			break
		}
		fmt.Printf("       - id=%s [%s] %s:%d %s\n", s.ID, s.Type, s.Server, s.Port, s.Country)
	}
}

// printProbe печатает таблицу результатов CarambaProbe. Столбец «МС» показывает
// прочерк для узлов, не ответивших за таймаут (latencyMs = -1).
func printProbe(probeJSON string) error {
	var report struct {
		Servers []struct {
			ID        string `json:"id"`
			Type      string `json:"type"`
			Server    string `json:"server"`
			Port      int    `json:"port"`
			Country   string `json:"country"`
			LatencyMs int    `json:"latencyMs"`
		} `json:"servers"`
	}
	if err := json.Unmarshal([]byte(probeJSON), &report); err != nil {
		return fmt.Errorf("разбор результата замера %q: %w", probeJSON, err)
	}
	if len(report.Servers) == 0 {
		fmt.Printf("     узлов для замера нет\n")
		return nil
	}
	fmt.Printf("     %-28s %-10s %-24s %-3s %6s\n", "УЗЕЛ", "ТИП", "АДРЕС", "СТР", "МС")
	alive := 0
	for _, s := range report.Servers {
		lat := "-"
		if s.LatencyMs >= 0 {
			lat = strconv.Itoa(s.LatencyMs)
			alive++
		}
		addr := fmt.Sprintf("%s:%d", s.Server, s.Port)
		fmt.Printf("     %-28s %-10s %-24s %-3s %6s\n", trunc(s.ID, 28), trunc(s.Type, 10), trunc(addr, 24), s.Country, lat)
	}
	fmt.Printf("     ответили %d из %d\n", alive, len(report.Servers))
	return nil
}

// trunc обрезает строку до n рун, чтобы колонки таблицы не разъезжались на
// длинных именах узлов.
func trunc(s string, n int) string {
	r := []rune(s)
	if len(r) <= n {
		return s
	}
	if n <= 1 {
		return string(r[:n])
	}
	return string(r[:n-1]) + "…"
}

// waitConnected опрашивает StatusJSON, пока стадия не станет connected.
// Возвращает потраченное время. Стадия error прекращает ожидание сразу.
func waitConnected(client *mobile.Client, timeout time.Duration) (time.Duration, error) {
	type status struct {
		Stage     string `json:"stage"`
		Detail    string `json:"detail"`
		Mode      string `json:"mode"`
		MixedPort int    `json:"mixedPort"`
	}
	start := time.Now()
	deadline := start.Add(timeout)
	lastStage := ""
	for {
		raw, err := client.StatusJSON()
		if err != nil {
			return time.Since(start), fmt.Errorf("чтение статуса: %w", err)
		}
		var st status
		if uerr := json.Unmarshal([]byte(raw), &st); uerr != nil {
			return time.Since(start), fmt.Errorf("разбор статуса %q: %w", raw, uerr)
		}
		lastStage = st.Stage
		switch st.Stage {
		case "connected":
			return time.Since(start), nil
		case "error":
			return time.Since(start), fmt.Errorf("ядро сообщило об ошибке: %s", st.Detail)
		}
		if time.Now().After(deadline) {
			return time.Since(start), fmt.Errorf("ядро не дошло до стадии connected за %s (последняя стадия %q)", timeout, lastStage)
		}
		time.Sleep(100 * time.Millisecond)
	}
}

// waitPort ждёт, пока mixed-инбаунд начнёт принимать TCP-соединения. Ядро
// поднимает listeners асинхронно после ApplyConfig, поэтому connected ещё не
// означает «порт открыт».
func waitPort(port int, timeout time.Duration) error {
	addr := net.JoinHostPort("127.0.0.1", fmt.Sprint(port))
	deadline := time.Now().Add(timeout)
	var lastErr error
	for {
		conn, err := net.DialTimeout("tcp", addr, 2*time.Second)
		if err == nil {
			_ = conn.Close()
			return nil
		}
		lastErr = err
		if time.Now().After(deadline) {
			return fmt.Errorf("mixed-инбаунд %s не открылся за %s: %w", addr, timeout, lastErr)
		}
		time.Sleep(100 * time.Millisecond)
	}
}

// directClient — HTTP-клиент строго без прокси (в том числе игнорирующий
// переменные окружения HTTP_PROXY), чтобы «прямой» замер был действительно
// прямым.
func directClient(timeout time.Duration) *http.Client {
	return &http.Client{
		Timeout: timeout,
		Transport: &http.Transport{
			Proxy:                 nil,
			DisableKeepAlives:     true,
			TLSHandshakeTimeout:   timeout,
			ResponseHeaderTimeout: timeout,
		},
	}
}

// proxyClient — HTTP-клиент, весь трафик которого идёт в локальный
// mixed-инбаунд ядра. Для https это CONNECT, то есть имя хоста уходит в ядро как
// есть и резолвится уже на выходном узле.
func proxyClient(port int, timeout time.Duration) (*http.Client, error) {
	u, err := url.Parse(fmt.Sprintf("http://127.0.0.1:%d", port))
	if err != nil {
		return nil, fmt.Errorf("адрес локального прокси: %w", err)
	}
	return &http.Client{
		Timeout: timeout,
		Transport: &http.Transport{
			Proxy:                 http.ProxyURL(u),
			DisableKeepAlives:     true,
			TLSHandshakeTimeout:   timeout,
			ResponseHeaderTimeout: timeout,
		},
	}, nil
}

// fetchExitIP спрашивает у эхо-сервиса выходной IP и возвращает его с задержкой
// запроса.
func fetchExitIP(client *http.Client) (string, time.Duration, error) {
	start := time.Now()
	req, err := http.NewRequest(http.MethodGet, ipEchoURL, nil)
	if err != nil {
		return "", 0, fmt.Errorf("некорректный запрос: %w", err)
	}
	req.Header.Set("User-Agent", userAgent)
	resp, err := client.Do(req)
	if err != nil {
		return "", time.Since(start), err
	}
	defer func() { _ = resp.Body.Close() }()
	body, err := io.ReadAll(io.LimitReader(resp.Body, 4<<10))
	if err != nil {
		return "", time.Since(start), fmt.Errorf("чтение ответа: %w", err)
	}
	lat := time.Since(start)
	if resp.StatusCode != http.StatusOK {
		return "", lat, fmt.Errorf("HTTP %d: %s", resp.StatusCode, strings.TrimSpace(string(body)))
	}
	var payload struct {
		IP string `json:"ip"`
	}
	if err := json.Unmarshal(body, &payload); err != nil || strings.TrimSpace(payload.IP) == "" {
		return "", lat, fmt.Errorf("не разобрать ответ %q", strings.TrimSpace(string(body)))
	}
	return payload.IP, lat, nil
}

// fetchStatus делает GET и возвращает только код ответа и задержку.
func fetchStatus(client *http.Client, target string) (int, time.Duration, error) {
	start := time.Now()
	req, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return 0, 0, fmt.Errorf("некорректный запрос: %w", err)
	}
	req.Header.Set("User-Agent", userAgent)
	resp, err := client.Do(req)
	if err != nil {
		return 0, time.Since(start), err
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4<<10))
	return resp.StatusCode, time.Since(start), nil
}
