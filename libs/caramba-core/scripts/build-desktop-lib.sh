#!/usr/bin/env bash
#
# build-desktop-lib.sh — сборка десктопной разделяемой библиотеки caramba-core
# (cgo c-shared) для встраивания в Flutter через dart:ffi.
#
# В отличие от build-desktop.sh (он собирает CLI-бинарник caramba), здесь из
# пакета ffi/ собирается НАТИВНАЯ БИБЛИОТЕКА с C-ABI (CarambaNew/CarambaUp/...),
# которую десктопный плагин caramba_vpn грузит и зовёт через FFI. На десктопе
# mihomo сам поднимает TUN (tunFd=-1), поэтому fd не передаётся; подъём TUN
# требует прав (root/CAP_NET_ADMIN на Linux, админ на Windows).
#
# Требования:
#   - Go-тулчейн и заполненный go.sum: `cd libs/caramba-core && go mod tidy`
#     (тег mihomo тянет большой транзитивный граф — без tidy сборка падает с
#     «missing go.sum entry»);
#   - CGO_ENABLED=1 и системный C-тулчейн (clang/gcc; на Windows mingw-w64),
#     т.к. ядро mihomo (gvisor/sing-tun) требует cgo;
#   - на Windows рядом с caramba_core.dll должен лежать wintun.dll (его ставит
#     инсталлятор/раннер плагина).
#
# Использование:
#   scripts/build-desktop-lib.sh            # текущая платформа
#
# Артефакт по платформе (расширение выбирает go по GOOS):
#   linux   → build/libcaramba_core.so   (+ build/libcaramba_core.h)
#   darwin  → build/libcaramba_core.dylib (+ build/libcaramba_core.h)
#   windows → build/caramba_core.dll      (+ build/caramba_core.h)
#
# Вендоринг плагином (десктоп):
#   linux   → apps/caramba-client/linux/caramba_vpn/libcaramba_core.so
#   macos   → apps/caramba-client/macos/caramba_vpn/libcaramba_core.dylib
#   windows → apps/caramba-client/windows/caramba_vpn/caramba_core.dll (+ wintun.dll)
# Каноничный C-заголовок для dart:ffi: libs/caramba-core/ffi/caramba_core.h
# (cgo также генерирует .h рядом с -o; держите их синхронными).
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/build"
PKG="./ffi"
TAGS="mihomo,with_gvisor"
mkdir -p "${OUT}"
# Патчи зависимостей: патченная копия mihomo + альтернативный go.mod
# (см. patches/README.md); основной go.mod не трогаем.
bash "${ROOT}/scripts/mk-patched-deps.sh" >/dev/null
export GOFLAGS="-modfile=${OUT}/patched.mod"

GOOS="$(go env GOOS 2>/dev/null || echo)"
case "${GOOS}" in
  windows) LIB="caramba_core.dll" ;;
  darwin)  LIB="libcaramba_core.dylib" ;;
  *)       LIB="libcaramba_core.so" ;;
esac

echo ">> go build -tags ${TAGS} -buildmode=c-shared (CGO_ENABLED=1) → ${OUT}/${LIB}"
# macOS: минимальная версия ОС как у Flutter-раннера (12.0).
if [[ "${GOOS}" == "darwin" ]]; then
  export CGO_CFLAGS="${CGO_CFLAGS:-} -mmacosx-version-min=12.0"
  export CGO_LDFLAGS="${CGO_LDFLAGS:-} -mmacosx-version-min=12.0"
fi
( cd "${ROOT}" && CGO_ENABLED=1 go build \
    -tags "${TAGS}" \
    -buildmode=c-shared \
    -o "${OUT}/${LIB}" \
    "${PKG}" )

# macOS: install name через @rpath, чтобы приложение находило библиотеку в
# своём bundle (Contents/Frameworks), и подпись ad-hoc после правки.
if [[ "${GOOS}" == "darwin" ]]; then
  install_name_tool -id "@rpath/${LIB}" "${OUT}/${LIB}"
  codesign -f -s - "${OUT}/${LIB}" >/dev/null 2>&1 || true
fi
echo ">> готово: ${OUT}/${LIB} (+ сгенерированный .h рядом)"
echo ">> каноничный заголовок для FFI: ${ROOT}/ffi/caramba_core.h"
# Условие пишется полной формой if, а НЕ как `[[ ... ]] && echo`: под
# `set -e` последняя команда скрипта задаёт его код возврата, и на любой
# не-Windows платформе ложное условие сделало бы успешную сборку неуспешной для
# всякой CI-цепочки через `&&`.
if [[ "${GOOS}" == "windows" ]]; then
  echo ">> ВНИМАНИЕ Windows: положите wintun.dll рядом с ${LIB}"
fi
