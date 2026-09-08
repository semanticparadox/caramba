# Патчи зависимостей

Точечные правки модулей без форка и без vendoring. `scripts/mk-patched-deps.sh`
копирует mihomo из кэша Go в `build/mihomo-src`, накладывает `patches/*.patch`
и пишет `build/patched.mod` с `replace` на эту копию. Сборочные скрипты
(`build-mobile.sh`, `build-desktop-lib.sh`) передают его через
`GOFLAGS=-modfile=build/patched.mod`; основной `go.mod` не меняется, а
`go build`/`go test` без скриптов используют оригинальный mihomo.

- `mihomo-android-package-manager.patch`: `listener/sing_tun/server_android.go`
  не создаёт Android package manager (он читает `/data/system/packages.xml`,
  недоступный без root), если не заданы per-app правила TUN. Без патча
  TUN-листенер падал с permission denied и туннель не поднимался в обычном
  приложении. Per-app split на Android делает VpnService.Builder.
