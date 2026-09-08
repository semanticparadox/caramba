#!/usr/bin/env bash
# Готовит патченную копию mihomo и альтернативный go.mod для сборки:
#   build/mihomo-src        копия модуля из кэша + patches/*.patch
#   build/patched.mod    go.mod + replace на эту копию (и go.sum рядом)
# Сборочные скрипты передают его через GOFLAGS=-modfile=build/patched.mod,
# поэтому основной go.mod не меняется. См. patches/README.md.
#
# Патчи накладываются `git apply`, а НЕ утилитой patch. Причины ровно две, и обе
# про зависания в CI:
#   - patch есть не везде: в Git Bash на образе windows-latest его может не
#     оказаться, а git есть по определению — на нём приехал чекаут;
#   - patch при неудачном применении переходит в ДИАЛОГ («File to patch:»,
#     «Skip this patch? [y]») и читает stdin. В GitHub Actions stdin шага не
#     закрыт, поэтому такой вопрос выглядит не как ошибка, а как тишина до
#     ручной отмены прогона.
# У `git apply` диалогов нет вовсе: не применилось — ненулевой код и текст.
# Поведение на macOS/Linux не меняется: тот же -p1 относительно build/mihomo-src.
set -euo pipefail

ts()  { date '+%H:%M:%S'; }
log() { echo "mk-patched-deps [$(ts)] $*"; }
die() { echo "mk-patched-deps: $*" >&2; exit 1; }

# Ни одна из вызываемых здесь программ не должна уметь спросить подтверждение.
export GIT_TERMINAL_PROMPT=0
export GIT_ASKPASS="${GIT_ASKPASS:-echo}"
export GCM_INTERACTIVE=never

command -v go  >/dev/null 2>&1 || die "go не найден в PATH"
command -v git >/dev/null 2>&1 || die "git не найден в PATH (нужен для git apply)"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/build"
SRC="${OUT}/mihomo-src"
mkdir -p "${OUT}"

# На чистой машине (CI) модуля в кэше ещё нет, и `go list -m` отдаёт пустой
# Dir — дальше `cp` падал с «cannot stat ''». Скачиваем явно, это идемпотентно.
# Шаг молчаливый и на холодном кэше идёт минутами, поэтому обрамлён метками
# времени: иначе в логе прогона он неотличим от зависания.
log "go mod download github.com/metacubex/mihomo (на холодном кэше это минуты без вывода)"
# Разветвлено явно, а не через ${VAR:+-x}: пустой аргумент после разбиения
# слов доезжает до go как пустая строка, а массив с set -u ломается на bash 3.2
# (штатный /bin/bash в macOS).
if [ -n "${CARAMBA_VERBOSE:-}" ]; then
  ( cd "${ROOT}" && go mod download -x github.com/metacubex/mihomo ) </dev/null
else
  ( cd "${ROOT}" && go mod download github.com/metacubex/mihomo ) </dev/null
fi
log "go mod download: готово"

MOD="$(cd "${ROOT}" && go list -m -f '{{.Dir}}' github.com/metacubex/mihomo)"
if [ -z "${MOD}" ] || [ ! -d "${MOD}" ]; then
  die "не нашёл исходники github.com/metacubex/mihomo в кэше Go"
fi
log "исходники mihomo: ${MOD}"

rm -rf "${SRC}"
cp -R "${MOD}" "${SRC}"
chmod -R u+w "${SRC}"
log "копия модуля: ${SRC}"

# Патч применяется к копии в SRC. cd внутрь, а не --directory: путь SRC
# абсолютный, а --directory принимает только относительный префикс (и требует
# --unsafe-paths). Каталог лежит внутри рабочего дерева этого репозитория
# (build/ игнорируется), git apply это переживает — пути он считает от cwd.
apply_patch() {
  local patch_file="$1" work="$1"
  log "патч: $(basename "${patch_file}")"
  # .gitattributes держит в репозитории LF, но клон с чужой конфигурацией
  # (core.autocrlf=true) может принести CR, а для git apply это «patch does not
  # apply» без внятной причины. Нормализуем КОПИЮ, оригинал не трогаем.
  if grep -q $'\r' "${patch_file}"; then
    work="${OUT}/$(basename "${patch_file}").lf"
    tr -d '\r' < "${patch_file}" > "${work}"
    log "  в патче были CR — применяю нормализованную копию: ${work}"
  fi
  # --verbose печатает «Applied patch ... cleanly» — это и признак жизни шага,
  # и доказательство, что патч действительно лёг, а не был тихо пропущен.
  ( cd "${SRC}" && git apply -p1 --whitespace=nowarn --verbose "${work}" ) </dev/null \
    || die "патч $(basename "${patch_file}") не применился к ${SRC}"
}

# nullglob: без него на пустом каталоге цикл получил бы литерал «*.patch»
# и упал внутри git apply с невнятным «No such file or directory».
shopt -s nullglob
patches=( "${ROOT}"/patches/*.patch )
shopt -u nullglob
[ "${#patches[@]}" -gt 0 ] || die "в ${ROOT}/patches нет ни одного *.patch — без патча TUN не стартует"
for p in "${patches[@]}"; do
  apply_patch "${p}"
done

cp "${ROOT}/go.mod" "${OUT}/patched.mod"
cp "${ROOT}/go.sum" "${OUT}/patched.sum"
( cd "${ROOT}" && go mod edit -modfile="${OUT}/patched.mod" -replace "github.com/metacubex/mihomo=${SRC}" ) </dev/null
# gomobile (Go 1.24+) требует tool-директиву на gobind.
( cd "${ROOT}" && go mod edit -modfile="${OUT}/patched.mod" -tool golang.org/x/mobile/cmd/gobind ) </dev/null
log "patched deps: ${SRC} via ${OUT}/patched.mod"
