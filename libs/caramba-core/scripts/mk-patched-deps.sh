#!/usr/bin/env bash
# Готовит патченную копию mihomo и альтернативный go.mod для сборки:
#   build/mihomo-src        копия модуля из кэша + patches/*.patch
#   build/patched.mod    go.mod + replace на эту копию (и go.sum рядом)
# Сборочные скрипты передают его через GOFLAGS=-modfile=build/patched.mod,
# поэтому основной go.mod не меняется. См. patches/README.md.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/build"
SRC="${OUT}/mihomo-src"
mkdir -p "${OUT}"
# На чистой машине (CI) модуля в кэше ещё нет, и `go list -m` отдаёт пустой
# Dir — дальше `cp` падал с «cannot stat ''». Скачиваем явно, это идемпотентно.
( cd "${ROOT}" && go mod download github.com/metacubex/mihomo )
MOD="$(cd "${ROOT}" && go list -m -f '{{.Dir}}' github.com/metacubex/mihomo)"
if [ -z "${MOD}" ] || [ ! -d "${MOD}" ]; then
  echo "mk-patched-deps: не нашёл исходники github.com/metacubex/mihomo в кэше Go" >&2
  exit 1
fi
rm -rf "${SRC}"
cp -R "${MOD}" "${SRC}"
chmod -R u+w "${SRC}"
for p in "${ROOT}"/patches/*.patch; do
  ( cd "${SRC}" && patch -p1 --silent < "${p}" )
done
cp "${ROOT}/go.mod" "${OUT}/patched.mod"
cp "${ROOT}/go.sum" "${OUT}/patched.sum"
( cd "${ROOT}" && go mod edit -modfile="${OUT}/patched.mod" -replace "github.com/metacubex/mihomo=${SRC}" )
# gomobile (Go 1.24+) требует tool-директиву на gobind.
( cd "${ROOT}" && go mod edit -modfile="${OUT}/patched.mod" -tool golang.org/x/mobile/cmd/gobind )
echo "patched deps: ${SRC} via ${OUT}/patched.mod"
