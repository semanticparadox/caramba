//go:build !mihomo

package main

// hasMihomoCore сообщает, что бинарник собран БЕЗ нативного ядра: движок —
// заглушка (engine/engine_stub.go), туннель не поднимается. Утилита в такой
// сборке компилируется (чтобы `go build ./...` и `go vet ./...` покрывали её код
// без CGO), но на запуске объясняет это и завершается с ошибкой вместо ложного
// «успеха». Разбор флагов и --help при этом работают.
const hasMihomoCore = false

// setCoreHomeDir без ядра ничего не делает: geo-базы читает только mihomo.
func setCoreHomeDir(string) {}
