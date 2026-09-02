#!/usr/bin/env bash
#
# build-smoke.sh — сборка дымовой утилиты caramba-smoke с нативным ядром.
#
# caramba-smoke поднимает ядро в proxy-режиме (mixed-инбаунд SOCKS5+HTTP на
# 127.0.0.1) и проверяет, что трафик реально идёт через подписку. TUN не
# поднимается, поэтому НИКАКИХ прав (root/админ, Network Extension) не нужно —
# это способ доказать соединение до того, как клиент получит привилегии.
#
# Требования (те же, что у build-desktop.sh):
#   - Go-тулчейн и заполненный go.sum: `cd libs/caramba-core && go mod tidy`;
#   - CGO_ENABLED=1 и системный C-тулчейн (clang/gcc; на Windows mingw-w64),
#     т.к. ядро mihomo требует cgo.
#
# Использование:
#   scripts/build-smoke.sh                     # → build/caramba-smoke
#   build/caramba-smoke --sub <URL|файл>       # прогон
#
# Сборка без тега mihomo тоже компилируется (`go build ./cmd/...`), но на
# запуске честно сообщает, что движок — заглушка, и завершается с ошибкой.
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/build"
PKG="./cmd/caramba-smoke"
TAGS="mihomo"
mkdir -p "${OUT}"

BIN="caramba-smoke"
[[ "$(go env GOOS 2>/dev/null || echo)" == "windows" ]] && BIN="caramba-smoke.exe"

echo ">> go build -tags ${TAGS} (CGO_ENABLED=1) → ${OUT}/${BIN}"
( cd "${ROOT}" && CGO_ENABLED=1 go build -tags "${TAGS}" -o "${OUT}/${BIN}" "${PKG}" )

echo ">> готово: ${OUT}/${BIN}"
echo ">> пример: ${OUT}/${BIN} --sub ./my-clash.yaml --format clash --port 7890"
