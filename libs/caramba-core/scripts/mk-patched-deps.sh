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
MOD="$(cd "${ROOT}" && go list -m -f '{{.Dir}}' github.com/metacubex/mihomo)"
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
