package subimport

import (
	"fmt"

	"gopkg.in/yaml.v3"
)

// parseClash разбирает clash/mihomo YAML и возвращает список proxy-map'ов из
// секции proxies. Это, по сути, passthrough: панель отдаёт уже валидную форму, и
// субимпорт лишь извлекает proxies, отбрасывая остальные секции (proxy-groups,
// rules, dns) — их заново соберёт profile.AssembleMihomoConfig из локальной
// политики. Прокси без name/type пропускаются.
//
// Нормализация минимальна: гарантируем строковый type/name/server и числовой
// port. amnezia-wg-option, ws-opts, reality-opts и прочие вложенные карты
// сохраняются как есть (yaml.v3 разворачивает их в map[string]any), включая
// строковую форму h1..h4 — если источник уже корректен, мы её не ломаем.
func parseClash(raw []byte) (proxyList, error) {
	var doc struct {
		Proxies []map[string]any `yaml:"proxies"`
	}
	if err := yaml.Unmarshal(raw, &doc); err != nil {
		return nil, fmt.Errorf("subimport: разбор clash YAML: %w", err)
	}
	out := make(proxyList, 0, len(doc.Proxies))
	for _, px := range doc.Proxies {
		name := asString(px["name"])
		typ := asString(px["type"])
		if name == "" || typ == "" {
			continue
		}
		px["name"] = name
		px["type"] = typ
		if s := asString(px["server"]); s != "" {
			px["server"] = s
		}
		if p := asInt(px["port"]); p > 0 {
			px["port"] = p
		}
		normalizeAmneziaOption(px)
		out = append(out, px)
	}
	return out, nil
}

// normalizeAmneziaOption чинит ловушку h1..h4 в amnezia-wg-option: mihomo требует
// h1..h4 СТРОКАМИ, иначе proxy:-декодер молча деградирует до plain WireGuard
// (см. subscription_generator.rs и docs/AMNEZIAWG.md). При passthrough из clash
// YAML yaml.v3 распознаёт "h1: 1" как int, поэтому принудительно приводим к
// строке. jc/jmin/jmax/s1..s4 остаются числами.
func normalizeAmneziaOption(px map[string]any) {
	opt, ok := px["amnezia-wg-option"].(map[string]any)
	if !ok {
		return
	}
	for _, key := range []string{"h1", "h2", "h3", "h4"} {
		if v, ok := opt[key]; ok && v != nil {
			opt[key] = asString(v)
		}
	}
	for _, key := range []string{"jc", "jmin", "jmax", "s1", "s2", "s3", "s4"} {
		if v, ok := opt[key]; ok && v != nil {
			opt[key] = asInt(v)
		}
	}
}
