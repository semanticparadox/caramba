//go:build !mihomo

package api

// setCoreHomeDir в сборке без нативного ядра ничего не делает: geo-базы
// (geoip.metadb, GeoSite.dat) читает только mihomo, а сам пакет api остаётся
// свободным от зависимости на ядро — иначе `go build ./...` и `go test ./...`
// без CGO перестали бы работать.
func setCoreHomeDir(string) {}
