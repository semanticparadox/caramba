module github.com/semanticparadox/caramba/libs/caramba-core

go 1.22

require (
	github.com/metacubex/mihomo v1.19.27
	gopkg.in/yaml.v3 v3.0.1
)

// ПРИМЕЧАНИЕ по зависимостям и сборке:
//
//  - github.com/metacubex/mihomo нужен ТОЛЬКО для сборок с build-тегом `mihomo`
//    (engine/engine_mihomo.go, autotune/prober_mihomo.go, api/prober_mihomo.go).
//    Сборка по умолчанию (CLI, тесты, gomobile-фасад без ядра) использует заглушку
//    и mihomo НЕ тянет при компиляции, поэтому собирается и без записей mihomo в
//    go.sum (yaml.v3 — единственная реально импортируемая зависимость).
//
//  - mihomo тянет большой транзитивный граф (sing-tun, sing-box deps, gvisor,
//    quic-go, utls и т.д.) и требует CGO на ряде платформ. go.sum в репозитории
//    СЕЙЧАС НЕПОЛНЫЙ: в нём есть только yaml.v3 (и check.v1), но нет контрольных
//    сумм mihomo и его транзитивных зависимостей. Из-за этого `go build -tags
//    mihomo ./...`, `go mod verify` и `go mod download` падают с «missing go.sum
//    entry for github.com/metacubex/mihomo», пока не выполнен tidy. Запись require
//    одна по себе сборку с тегом НЕ включает.
//
//    Сгенерировать полный граф и go.sum (нужен тулчейн Go + доступ к прокси
//    модулей; в текущем окружении их нет):
//
//        cd libs/caramba-core
//        go mod tidy                      # допишет транзитивные require + go.sum
//        go build ./...                   # сборка по умолчанию (без ядра)
//        go build -tags mihomo ./...      # сборка с нативным ядром (нужен CGO)
//
//    После tidy закоммитить обновлённые go.mod и go.sum. Для CI-джоба `-tags
//    mihomo` потребуется CGO (CGO_ENABLED=1 + C-тулчейн целевой платформы).
//
//  - Режимы сборки, потребляющие ядро:
//      * gomobile bind -tags mihomo пакета mobile/ → AAR/xcframework
//        (scripts/build-mobile.sh);
//      * go build -tags mihomo -buildmode=c-shared пакета ffi/ → разделяемая
//        библиотека libcaramba_core.{so,dylib,dll} + C-заголовок для dart:ffi
//        (scripts/build-desktop-lib.sh);
//      * go build -tags mihomo бинарника apps/caramba-cli (scripts/build-desktop.sh).
//    Все три требуют tidy + CGO. Пакет ffi/ помечен `//go:build cgo`, поэтому в
//    сборку по умолчанию (без cgo/тегов) НЕ попадает и её не ломает.
//
//  - Новые импорты под тегом mihomo НЕ добавляют прямых require: engine/
//    engine_mihomo.go теперь читает github.com/metacubex/mihomo/tunnel/statistic
//    (живая статистика трафика) — это под-пакет уже требуемого mihomo, так что
//    go.mod не меняется (граф добьёт tidy).
