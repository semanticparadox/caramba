package api

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/semanticparadox/caramba/libs/caramba-core/profile"
	"github.com/semanticparadox/caramba/libs/caramba-core/routing"
	"github.com/semanticparadox/caramba/libs/caramba-core/transport"
)

// Разблокировка загрузки, 02-SPEC.md 8.10.
//
// Ядро до сих пор качало geo-базы прямо с github.com, тянуло rule-set'ы с
// единственного адреса панели и проверяло живость узлов через gstatic.com. В
// РФ заблокировано всё три. Здесь эти три пути переводятся на подписанный
// каталог: адреса берутся из пула зеркал, а содержимое проходит через
// transport.ResourceGuard ДО того, как его увидит mihomo.
//
// Правило, которое здесь важнее остальных: при доверенном каталоге с
// предложенными хешами непроверенная копия не используется НИКОГДА. Ресурс,
// который не сошёлся с подписанным sha256 или которого каталог не назвал, не
// подставляется в конфиг: сборка отказывает, а не откатывается на GitHub.

// Канонические имена geo-баз. Ровно эти имена ядро ищет в своём домашнем
// каталоге (constant/path.go: GeositeName, GeoipName, ASNName и разбор
// geoip.metadb в MMDB()), поэтому под ними же оператор обязан называть
// ресурсы geo в каталоге. Совпадение имён не косметика: файл, положенный под
// другим именем, ядро не найдёт и полезет качать его само.
const (
	geoSiteFile = "GeoSite.dat"
	geoIPFile   = "GeoIP.dat"
	mmdbFile    = "geoip.metadb"
	asnFile     = "ASN.mmdb"
)

// ruleSetDir это подкаталог рабочего каталога, куда ложатся проверенные списки.
const ruleSetDir = "rulesets"

// bootstrapPlan это результат подготовки одной сборки конфига.
type bootstrapPlan struct {
	// boot уходит в profile.Policy.Bootstrap (секция geox-url и цель пробы).
	boot profile.BootstrapConfig
	// pool описывает, откуда пресет берёт свои rule-set'ы.
	pool routing.PoolOptions
	// doh это загрузочные резолверы каталога. Они заменяют компилируемое
	// умолчание, но не пользовательский выбор (profile.ApplyBootstrapDNS).
	doh []string
}

// prepareBootstrap собирает план загрузки под текущий доверенный каталог и
// заранее выкачивает всё, что каталог назвал, сверяя sha256.
//
// wantRuleSets это имена rule-set'ов, которые нужны активному пресету. Лишнего
// не качаем: каталог может называть списки для всех стран сразу.
//
// Ошибка означает отказ собрать конфиг. Это не потеря конфигурации в смысле
// инварианта 16: кешированные документы CSM никуда не деваются, отказывает
// именно попытка подсунуть ядру непроверенные байты.
func (c *Core) prepareBootstrap(ctx context.Context, wantRuleSets []string) (bootstrapPlan, error) {
	c.mu.Lock()
	panelBase := strings.TrimSpace(c.cfg.PanelBaseURL)
	ladder := c.ladder
	c.mu.Unlock()

	plan := bootstrapPlan{pool: routing.PoolOptions{
		// Списки едут через группу-селектор, а не собственным диалером ядра в
		// открытый интернет: ключ proxy: у rule-provider'а именно для этого.
		Proxy: profile.CarambaSelector,
	}}
	if panelBase != "" {
		plan.pool.Bases = []string{panelBase}
	}
	if ladder == nil {
		return plan, nil
	}

	// Пул зеркал сильнее адреса панели: панель это один хост, который цензор
	// блокирует первым, а пул на то и пул.
	if mirrors := ladder.MirrorOrigins(); len(mirrors) > 0 {
		plan.pool.Bases = append(append([]string{}, mirrors...), plan.pool.Bases...)
		// Цель url-test это хост из подписанного пула вместо зашитого
		// gstatic.com, который в РФ заблокирован и объявлял мёртвыми все узлы.
		// Ответ годится любой: группа url-test меряет время, а не код.
		plan.boot.ProbeURL = strings.TrimRight(mirrors[0], "/") + "/"
	}
	// Загрузочный DoH переезжает из зашитых 1.1.1.1 и 8.8.8.8 в каталог.
	plan.doh = ladder.DoHURLs()

	f, err := c.csmFetcher()
	if err != nil {
		// Сборка без CSM: лестницы нет, каталога нет, поведение прежнее.
		return plan, nil
	}
	guard := f.Guard()
	if !guard.Enabled() {
		// Бит 6 снят или каталог не предложил хешей. Загружать rule-set и geo
		// по адресам каталога клиент НЕ ИМЕЕТ ПРАВА (см. transport.
		// ErrResourcesDisabled), поэтому geox-url не трогаем: ядро остаётся
		// на встроенных данных и на своих умолчаниях.
		return plan, nil
	}
	origin := f.Snapshot().Origin
	if strings.TrimSpace(origin) == "" {
		origin = panelBase
	}

	plan.boot.Managed = true
	plan.pool.Verified = true

	// geo-базы. Ресурс, которого каталог не назвал, получает ПУСТОЙ адрес:
	// ядро тогда падает на понятной ошибке загрузки вместо тихого похода на
	// GitHub за неподписанным файлом.
	geo := []struct {
		name string
		dst  *string
	}{
		{geoIPFile, &plan.boot.GeoIPURL},
		{geoSiteFile, &plan.boot.GeoSiteURL},
		{mmdbFile, &plan.boot.MmdbURL},
		{asnFile, &plan.boot.ASNURL},
	}
	for _, g := range geo {
		entry, ok := guard.Entry(g.name)
		if !ok {
			continue
		}
		if err := c.materializeResource(ctx, guard, ladder, origin, g.name, filepath.Join(c.workDir, g.name)); err != nil {
			return bootstrapPlan{}, err
		}
		u, err := mirrorURL(plan.pool.Bases, origin, entry.URL)
		if err != nil {
			return bootstrapPlan{}, fmt.Errorf("api: адрес ресурса %s: %w", g.name, err)
		}
		*g.dst = u
	}

	// rule-set'ы активного пресета.
	for _, name := range wantRuleSets {
		if _, ok := guard.Entry(name); !ok {
			// Каталог такой список не подписывает. Провайдер и правила,
			// которые на него ссылаются, выпадут (PoolOptions.Verified).
			continue
		}
		rel, err := safeRuleSetPath(name)
		if err != nil {
			return bootstrapPlan{}, err
		}
		dst := filepath.Join(c.workDir, filepath.FromSlash(rel))
		if err := os.MkdirAll(filepath.Dir(dst), 0o700); err != nil {
			return bootstrapPlan{}, fmt.Errorf("api: каталог списков: %w", err)
		}
		if err := c.materializeResource(ctx, guard, ladder, origin, name, dst); err != nil {
			return bootstrapPlan{}, err
		}
		if plan.pool.Files == nil {
			plan.pool.Files = map[string]string{}
		}
		plan.pool.Files[name] = rel
	}
	return plan, nil
}

// materializeResource кладёт на диск ровно те байты, чей sha256 совпал с
// подписанным.
//
// Порядок обязателен и он же весь смысл функции: уже лежащий файл сперва
// сверяется с подписанным хешем (диск не доверенная среда, и файл мог быть
// подменён между запусками), и только при несовпадении берётся сеть. Ни одна
// ветка не пишет непроверенные байты и ни одна не оставляет старый файл, если
// он не сошёлся.
func (c *Core) materializeResource(ctx context.Context, guard *transport.ResourceGuard, l *transport.Ladder, origin, name, dst string) error {
	if have, err := os.ReadFile(dst); err == nil {
		if guard.Check(name, have) == nil {
			return nil
		}
		// Файл есть, но он не тот. Удаляем сразу: оставленный на диске он
		// стал бы ровно тем непроверенным откатом, который здесь запрещён.
		_ = os.Remove(dst)
	}
	data, err := guard.Fetch(ctx, l, name, origin)
	if err != nil {
		return fmt.Errorf("api: ресурс %s не подтверждён каталогом: %w", name, err)
	}
	if err := os.WriteFile(dst, data, 0o600); err != nil {
		return fmt.Errorf("api: запись ресурса %s: %w", name, err)
	}
	return nil
}

// mirrorURL строит абсолютный адрес ресурса на первом зеркале пула.
//
// Подписанный путь остаётся путём: 03-WIRE.md 14.3 запрещает абсолютные URL в
// подписанных полях, и ветки «а вдруг это уже URL» здесь нет намеренно.
func mirrorURL(bases []string, origin, path string) (string, error) {
	if !strings.HasPrefix(path, "/") {
		return "", fmt.Errorf("подписанный ресурс обязан быть путём, а не URL")
	}
	base := strings.TrimSpace(origin)
	for _, b := range bases {
		if b = strings.TrimSpace(b); b != "" {
			base = b
			break
		}
	}
	u, err := transport.ResolveAgainst(base, path)
	if err != nil {
		return "", err
	}
	if err := transport.CheckFetchURLString(u); err != nil {
		return "", err
	}
	return u, nil
}

// safeRuleSetPath переводит имя rule-set'а в относительный путь внутри
// рабочего каталога.
//
// Имя приходит из подписанного каталога, но подпись говорит лишь «это написал
// оператор», а не «это безопасно как имя файла». Разделители и точки режем
// здесь, а не надеемся на IsSafePath ядра.
func safeRuleSetPath(name string) (string, error) {
	n := strings.TrimSpace(name)
	if n == "" || n == "." || n == ".." {
		return "", fmt.Errorf("api: недопустимое имя rule-set %q", name)
	}
	for _, r := range n {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9':
		case r == '-', r == '_', r == '.':
		default:
			return "", fmt.Errorf("api: недопустимое имя rule-set %q", name)
		}
	}
	if strings.Contains(n, "..") {
		return "", fmt.Errorf("api: недопустимое имя rule-set %q", name)
	}
	return ruleSetDir + "/" + n, nil
}

// routingForBuild пересобирает маршрутизацию под план загрузки.
//
// Пресет компилируется заново при каждом подъёме, а не один раз в ApplyPreset:
// пул зеркал и набор подтверждённых списков приходят из каталога и меняются
// между запусками, а конфиг, собранный на прошлом каталоге, указывал бы на
// зеркало, которого уже нет.
func (c *Core) routingForBuild(plan bootstrapPlan) *routing.Config {
	c.mu.Lock()
	presetID := c.presetID
	current := c.policy.Routing
	c.mu.Unlock()
	if presetID == "" {
		return current
	}
	preset, ok := routing.PresetByID(presetID)
	if !ok {
		return current
	}
	cfg := preset.BuildWith(plan.pool, profile.CarambaSelector)
	if err := cfg.Validate(); err != nil {
		// Несогласованный набор правил лучше не подсовывать ядру: остаёмся на
		// том, что уже было применено.
		return current
	}
	return &cfg
}

// presetCountry возвращает ISO-код страны пресета (пусто у глобальных).
func presetCountry(presetID string) string {
	preset, ok := routing.PresetByID(presetID)
	if !ok {
		return ""
	}
	return preset.Country
}

// ruleSetNames перечисляет имена rule-set'ов пресета (в порядке объявления).
func ruleSetNames(presetID string) []string {
	preset, ok := routing.PresetByID(presetID)
	if !ok {
		return nil
	}
	out := make([]string, 0, len(preset.Providers))
	for _, p := range preset.Providers {
		out = append(out, p.Name)
	}
	return out
}
