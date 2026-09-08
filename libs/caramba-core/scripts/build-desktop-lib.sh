#!/usr/bin/env bash
#
# build-desktop-lib.sh — сборка десктопной разделяемой библиотеки caramba-core
# (cgo c-shared) для встраивания в Flutter через dart:ffi.
#
# В отличие от build-smoke.sh (он собирает CLI-утилиту cmd/caramba-smoke), здесь
# из пакета ffi/ собирается НАТИВНАЯ БИБЛИОТЕКА с C-ABI (CarambaNew/CarambaUp/...),
# которую десктопный плагин caramba_vpn грузит и зовёт через FFI. На десктопе
# mihomo сам поднимает TUN (tunFd=-1), поэтому fd не передаётся; подъём TUN
# требует прав (root/CAP_NET_ADMIN на Linux, админ на Windows). Клиент на macOS
# по умолчанию идёт proxy-режимом (mixed inbound на 127.0.0.1:7890) — там прав
# не требуется.
#
# Требования:
#   - Go-тулчейн и заполненный go.sum: `cd libs/caramba-core && go mod tidy`
#     (тег mihomo тянет большой транзитивный граф — без tidy сборка падает с
#     «missing go.sum entry»);
#   - CGO_ENABLED=1 и системный C-тулчейн (clang/gcc; на Windows mingw-w64),
#     т.к. ядро mihomo (gvisor/sing-tun) требует cgo;
#   - на Windows рядом с libcaramba_core.dll должен лежать wintun.dll (его ставит
#     инсталлятор/раннер плагина).
#
# Использование:
#   scripts/build-desktop-lib.sh              # текущая платформа
#   scripts/build-desktop-lib.sh macos        # то же явно (universal arm64+x86_64)
#   scripts/build-desktop-lib.sh linux
#
# Явная цель нужна CI (.github/workflows/client-desktop.yml вызывает
# `build-desktop-lib.sh <macos|linux>`), чтобы шаг читался и не зависел от того,
# что вернёт `go env GOOS` на раннере.
#
# Артефакт по платформе (расширение выбирает go по GOOS):
#   linux   → build/libcaramba_core.so   (+ build/libcaramba_core.h)
#   darwin  → build/libcaramba_core.dylib (+ build/libcaramba_core.h)
#   windows → build/libcaramba_core.dll   (+ build/libcaramba_core.h)
#             (кросс-сборку Windows-DLL делает отдельный build-windows-lib.sh)
#
# Вендоринг плагином (десктоп):
#   linux   → apps/caramba-client/packages/caramba_vpn/linux/lib/libcaramba_core.so
#   macos   → apps/caramba-client/packages/caramba_vpn/darwin/Libraries/libcaramba_core.dylib
#   windows → apps/caramba-client/packages/caramba_vpn/windows/lib/libcaramba_core.dll (+ wintun.dll)
# Каноничный C-заголовок для dart:ffi: libs/caramba-core/ffi/caramba_core.h
# (cgo также генерирует .h рядом с -o; держите их синхронными).
#
# macOS собирается УНИВЕРСАЛЬНОЙ (arm64 + x86_64, склейка через lipo): .app
# раздаётся DMG-ом без подписи, и на Intel-маке однокомпонентный arm64-dylib
# не загрузился бы вовсе, а Flutter-раннер об этом сообщил бы только в рантайме.
# Срезы задаются CARAMBA_MACOS_ARCHS (по умолчанию «arm64 amd64»); если нужен
# только текущий срез — CARAMBA_MACOS_ARCHS=arm64.
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/build"
PKG="./ffi"
TAGS="mihomo,with_gvisor"
mkdir -p "${OUT}"

HOST_GOOS="$(go env GOOS 2>/dev/null || echo)"
TARGET="${1:-}"
case "${TARGET}" in
  ""|host)          GOOS_TARGET="${HOST_GOOS}" ;;
  macos|darwin|osx) GOOS_TARGET="darwin" ;;
  linux)            GOOS_TARGET="linux" ;;
  windows|win)      GOOS_TARGET="windows" ;;
  *)
    echo "неизвестная цель '${TARGET}'; ожидается macos|linux|windows (или без аргумента)" >&2
    exit 2
    ;;
esac

# Патчи зависимостей готовятся ПОСЛЕ разбора аргументов: на неизвестной цели
# скрипт должен падать сразу, а не после копирования исходников mihomo.
# Патченная копия mihomo + альтернативный go.mod (см. patches/README.md);
# основной go.mod не трогаем.
bash "${ROOT}/scripts/mk-patched-deps.sh" >/dev/null
export GOFLAGS="-modfile=${OUT}/patched.mod"

case "${GOOS_TARGET}" in
  windows) LIB="libcaramba_core.dll" ;;
  darwin)  LIB="libcaramba_core.dylib" ;;
  *)       LIB="libcaramba_core.so" ;;
esac

# Сборка одного среза. Аргументы: GOARCH, путь к выходному файлу.
# Вынесено в функцию, потому что macOS собирается дважды (arm64 + x86_64).
build_slice() {
  local goarch="$1" out_file="$2"
  local -a env_extra=()
  if [[ "${GOOS_TARGET}" == "darwin" ]]; then
    # Минимальная версия ОС как у Flutter-раннера (12.0). Для не-родного среза
    # clang нужно явно указать -arch, иначе он соберёт под архитектуру хоста и
    # линковка c-shared развалится.
    local clang_arch="arm64"
    [[ "${goarch}" == "amd64" ]] && clang_arch="x86_64"
    env_extra+=(
      "CC=clang -arch ${clang_arch}"
      "CXX=clang++ -arch ${clang_arch}"
      "CGO_CFLAGS=${CGO_CFLAGS:-} -mmacosx-version-min=12.0"
      "CGO_LDFLAGS=${CGO_LDFLAGS:-} -mmacosx-version-min=12.0"
    )
  fi
  ( cd "${ROOT}" && env GOOS="${GOOS_TARGET}" GOARCH="${goarch}" CGO_ENABLED=1 \
      "${env_extra[@]}" \
      go build -tags "${TAGS}" -buildmode=c-shared -o "${out_file}" "${PKG}" )
}

if [[ "${GOOS_TARGET}" == "darwin" ]]; then
  ARCHS="${CARAMBA_MACOS_ARCHS:-arm64 amd64}"
  echo ">> go build -tags ${TAGS} -buildmode=c-shared (CGO_ENABLED=1, macOS: ${ARCHS}) → ${OUT}/${LIB}"
  # Каждый срез собирается в свой подкаталог: cgo кладёт сгенерированный
  # заголовок рядом с -o, и без разделения срезы затирали бы заголовок и файлы
  # друг друга.
  SLICES=()
  for arch in ${ARCHS}; do
    slice_dir="${OUT}/macos-${arch}"
    rm -rf "${slice_dir}"; mkdir -p "${slice_dir}"
    echo "   .. срез ${arch}"
    if build_slice "${arch}" "${slice_dir}/${LIB}"; then
      SLICES+=("${slice_dir}/${LIB}")
    else
      # Срез не собрался (обычно нет SDK под чужую архитектуру). Это не повод
      # ронять сборку целиком: arm64 достаточно для Apple Silicon, а честное
      # предупреждение попадёт в лог CI.
      echo "   !! срез ${arch} НЕ собрался — пропускаю (universal-склейки не будет)" >&2
      rm -rf "${slice_dir}"
    fi
  done
  if [[ ${#SLICES[@]} -eq 0 ]]; then
    echo "ни один срез macOS не собрался" >&2
    exit 1
  fi
  # Заголовок cgo одинаков для всех срезов — берём от первого удавшегося.
  cp "$(dirname "${SLICES[0]}")/libcaramba_core.h" "${OUT}/libcaramba_core.h"
  lipo -create "${SLICES[@]}" -output "${OUT}/${LIB}"
  for slice in "${SLICES[@]}"; do rm -rf "$(dirname "${slice}")"; done
  # install name через @rpath, чтобы приложение находило библиотеку в своём
  # bundle (Contents/Frameworks), и ad-hoc подпись после правки: иначе
  # изменённый Mach-O не пройдёт проверку на подписанной системе.
  install_name_tool -id "@rpath/${LIB}" "${OUT}/${LIB}"
  codesign -f -s - "${OUT}/${LIB}" >/dev/null 2>&1 || true
  echo ">> lipo: $(lipo -archs "${OUT}/${LIB}")"
else
  echo ">> go build -tags ${TAGS} -buildmode=c-shared (CGO_ENABLED=1, GOOS=${GOOS_TARGET}) → ${OUT}/${LIB}"
  build_slice "$(go env GOARCH)" "${OUT}/${LIB}"
fi

echo ">> готово: ${OUT}/${LIB} (+ сгенерированный .h рядом)"
echo ">> каноничный заголовок для FFI: ${ROOT}/ffi/caramba_core.h"
# Условие пишется полной формой if, а НЕ как `[[ ... ]] && echo`: под
# `set -e` последняя команда скрипта задаёт его код возврата, и на любой
# не-Windows платформе ложное условие сделало бы успешную сборку неуспешной для
# всякой CI-цепочки через `&&`.
if [[ "${GOOS_TARGET}" == "windows" ]]; then
  echo ">> ВНИМАНИЕ Windows: положите wintun.dll рядом с ${LIB}"
fi
