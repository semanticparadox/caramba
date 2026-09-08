//go:build mihomo

// Выбор Prober'а для сборки С нативным ядром (-tags mihomo). Здесь доступна
// честная проверка handshake: для каждого кандидата строится прокси-адаптер
// mihomo нужного типа и проверяется URLTest'ом (настоящий handshake + HTTP-проба
// сквозь прокси). Это даёт автоподбору правдивый ответ, какие протоколы реально
// проходят через DPI с текущего клиента.
package api

import (
	"github.com/semanticparadox/caramba/libs/caramba-core/autotune"
	"github.com/semanticparadox/caramba/libs/caramba-core/subscription"
)

// newProber возвращает реальный Prober на ядре mihomo. ProxyConfigs строятся из
// сырого YAML подписки: для каждого узла (ServerID = имя прокси) собирается карта
// «дружелюбное имя протокола → сырой clash-конфиг прокси». MihomoProber берёт из
// неё конфиг под каждый объявленный протокол и проверяет его handshake'ом.
func newProber(cands []autotune.Candidate, rawYAML []byte) autotune.Prober {
	return autotune.NewMihomoProber(cands, proxyConfigsFromYAML(rawYAML))
}

// proxyConfigsFromYAML строит карту ServerID -> (дружелюбное имя протокола ->
// сырой clash-конфиг прокси) для MihomoProber.
//
// Извлечение сырых map'ов из YAML вынесено в subscription.ProxyMaps (чистый,
// без build-тега, тестируемый без ядра). Здесь к ним применяется то же
// отображение clash-тип → дружелюбное имя (friendlyProtocol), что и для
// кандидатов, чтобы ключи протокола совпадали с Candidate.Protocols. На ошибке
// разбора возвращается nil — MihomoProber тогда пропустит проверку handshake
// (узлы останутся без сырого конфига), что безопасно деградирует.
func proxyConfigsFromYAML(rawYAML []byte) map[string]map[string]map[string]any {
	byType, err := subscription.ProxyMaps(rawYAML)
	if err != nil {
		return nil
	}
	out := make(map[string]map[string]map[string]any, len(byType))
	for name, protos := range byType {
		byProto := make(map[string]map[string]any, len(protos))
		for clashType, raw := range protos {
			byProto[friendlyProtocol(clashType)] = raw
		}
		out[name] = byProto
	}
	return out
}
