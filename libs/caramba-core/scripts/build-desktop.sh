#!/usr/bin/env bash
#
# build-desktop.sh — сборка CLI/десктопного бинарника caramba с нативным ядром.
#
# На десктопе (Linux/macOS/Windows) mihomo сам поднимает TUN-интерфейс, поэтому
# SetTunFd не нужен — достаточно собрать с тегом `mihomo`. Без тега получится
# заглушка движка (для разработки/тестов без ядра).
#
# Замечания:
#   - Тег `mihomo` подключает engine_mihomo.go и prober_mihomo.go.
#   - Нативное ядро mihomo требует CGO на ряде платформ (gvisor/sing-tun),
#     поэтому CGO_ENABLED=1; нужен системный C-тулчейн (clang/gcc, на Windows
#     mingw-w64).
#   - Поднятие TUN на десктопе требует прав (root/CAP_NET_ADMIN на Linux,
#     админ на Windows). Запускать бинарник соответствующе.
#
# Использование:
#   scripts/build-desktop.sh                 # текущая платформа → build/caramba
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# CLI живёт в соседнем модуле apps/caramba-cli; собираем его main с ядром.
CLI_DIR="$(cd "${ROOT}/../../apps/caramba-cli" && pwd)"
OUT="${ROOT}/build"
mkdir -p "${OUT}"

BIN="caramba"
[[ "$(go env GOOS 2>/dev/null || echo)" == "windows" ]] && BIN="caramba.exe"

echo ">> go build -tags mihomo (CGO_ENABLED=1) → ${OUT}/${BIN}"
( cd "${CLI_DIR}" && CGO_ENABLED=1 go build -tags mihomo -o "${OUT}/${BIN}" ./ )
echo ">> готово: ${OUT}/${BIN}"
echo ">> сборка без ядра (заглушка, без CGO): go build -o ${OUT}/${BIN} ./ в ${CLI_DIR}"
