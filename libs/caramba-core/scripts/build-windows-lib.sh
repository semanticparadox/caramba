#!/usr/bin/env bash
#
# build-windows-lib.sh — сборка Windows-DLL ядра caramba-core (cgo c-shared)
# для десктопного Flutter-плагина caramba_vpn.
#
# Зачем отдельный скрипт, а не ветка в build-desktop-lib.sh. Тот собирает под
# ТЕКУЩУЮ платформу (`go env GOOS`) и не умеет кросс-компиляцию: под Windows
# нужен чужой C-тулчейн (mingw-w64), своё имя артефакта и вендоринг wintun.dll.
# Держать это отдельно дешевле, чем ветвить общий скрипт: здесь один и тот же
# код работает и на CI-раннере windows-latest (нативный gcc), и на Маке
# (x86_64-w64-mingw32-gcc), поэтому локальная проверка совпадает с CI.
#
# Имя артефакта — libcaramba_core.dll (без вариантов). Плагин грузит его
# LoadLibraryW(L"libcaramba_core.dll") (packages/caramba_vpn/windows/
# caramba_core_ffi.h), CMake бандлит его по этому же имени, dart:ffi-путь ищет
# его же (lib/src/ffi/library_lookup.dart). Историческое имя caramba_core.dll
# (без префикса lib) больше нигде не используется.
#
# Требования:
#   - Go-тулчейн и заполненный go.sum (`cd libs/caramba-core && go mod tidy`):
#     тег mihomo тянет большой транзитивный граф;
#   - CGO_ENABLED=1 и C-тулчейн под Windows:
#       * кросс-сборка (macOS/Linux): `brew install mingw-w64` /
#         `apt-get install gcc-mingw-w64-x86-64` → x86_64-w64-mingw32-gcc;
#       * нативно на Windows (CI): gcc из mingw-w64, который уже стоит на
#         образе windows-latest.
#   - Патченый mihomo (scripts/mk-patched-deps.sh) — без патча TUN не стартует.
#
# Использование:
#   scripts/build-windows-lib.sh                 # amd64, вендоринг в плагин
#   scripts/build-windows-lib.sh --arch amd64    # то же явно
#   scripts/build-windows-lib.sh --no-vendor     # только build/, не копировать
#
# Артефакты (оба gitignored):
#   libs/caramba-core/build/libcaramba_core.dll (+ libcaramba_core.h рядом)
#   apps/caramba-client/packages/caramba_vpn/windows/lib/libcaramba_core.dll
#
# wintun.dll этим скриптом НЕ качается: он не собирается, а скачивается с
# проверкой контрольной суммы — см. scripts/fetch-wintun.sh.
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "${ROOT}/../.." && pwd)"
OUT="${ROOT}/build"
PKG="./ffi"
TAGS="mihomo,with_gvisor"
LIB="libcaramba_core.dll"
PLUGIN_LIB_DIR="${REPO}/apps/caramba-client/packages/caramba_vpn/windows/lib"

ARCH="amd64"
VENDOR=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) ARCH="${2:-}"; shift 2 ;;
    --no-vendor) VENDOR=0; shift ;;
    -h|--help) sed -n '2,40p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "неизвестный аргумент: $1" >&2; exit 2 ;;
  esac
done

case "${ARCH}" in
  amd64|arm64) ;;
  *) echo "поддерживаются только --arch amd64|arm64 (дано: ${ARCH})" >&2; exit 2 ;;
esac

# Выбор C-компилятора. На самой Windows кросс-префикса нет — там штатный gcc,
# и подставлять CC не нужно (пустой CC = дефолт cgo).
HOST_OS="$(go env GOOS)"
if [[ "${HOST_OS}" == "windows" ]]; then
  CC_BIN="${CC:-}"
else
  case "${ARCH}" in
    amd64) CC_BIN="${CC:-x86_64-w64-mingw32-gcc}" ;;
    arm64) CC_BIN="${CC:-aarch64-w64-mingw32-gcc}" ;;
  esac
  if ! command -v "${CC_BIN}" >/dev/null 2>&1; then
    echo "нет кросс-компилятора ${CC_BIN}." >&2
    echo "macOS: brew install mingw-w64   |   Debian/Ubuntu: apt-get install gcc-mingw-w64" >&2
    exit 1
  fi
fi

mkdir -p "${OUT}"
# Патчи зависимостей: патченная копия mihomo + альтернативный go.mod
# (см. patches/README.md); основной go.mod не трогаем.
bash "${ROOT}/scripts/mk-patched-deps.sh" >/dev/null
export GOFLAGS="-modfile=${OUT}/patched.mod"

echo ">> go build GOOS=windows GOARCH=${ARCH} -tags ${TAGS} -buildmode=c-shared → ${OUT}/${LIB}"
(
  cd "${ROOT}"
  export GOOS=windows GOARCH="${ARCH}" CGO_ENABLED=1
  if [[ -n "${CC_BIN}" ]]; then export CC="${CC_BIN}"; fi
  go build -tags "${TAGS}" -buildmode=c-shared -o "${OUT}/${LIB}" "${PKG}"
)

echo ">> готово: ${OUT}/${LIB} (+ сгенерированный .h рядом)"
echo ">> каноничный заголовок для FFI: ${ROOT}/ffi/caramba_core.h"

if [[ "${VENDOR}" -eq 1 ]]; then
  # Вендоринг сразу в плагин: `flutter build windows` бандлит DLL из
  # windows/lib/ (см. caramba_vpn_bundled_libraries в его CMakeLists.txt).
  # Каталог может отсутствовать на чистом клоне — его содержимое gitignored.
  mkdir -p "${PLUGIN_LIB_DIR}"
  cp "${OUT}/${LIB}" "${PLUGIN_LIB_DIR}/${LIB}"
  echo ">> вендоринг: ${PLUGIN_LIB_DIR}/${LIB}"
fi

echo ">> напоминание: рядом с ${LIB} нужен wintun.dll — scripts/fetch-wintun.sh"
