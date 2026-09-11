#!/usr/bin/env bash
#
# ci-manifest.sh — манифест версии клиента Caramba Connect для одной платформы.
#
# Зачем. До этого шага релиз клиента состоял только из бинарников с
# фиксированными именами: ни панель, ни приложение, ни бот не могли узнать,
# КАКАЯ это версия, не распаковывая файл. Манифест — единственный
# машиночитаемый источник «последняя версия клиента»: панель отдаёт его через
# GET /api/v2/app/version, приложение сравнивает build со своим и показывает
# баннер, бот рассылает «вышла новая версия» ровно один раз на сборку
# (services/client_release_service.rs).
#
# Формат (один файл на платформу, имя Caramba-Connect-<platform>.json):
#   {
#     "platform":     "android" | "windows" | "macos" | "linux",
#     "version":      "1.0.0",              — из pubspec.yaml (X.Y.Z)
#     "build":        109,                  — из pubspec.yaml (+N)
#     "tag":          "client-v1.0.0+109",  — тег релиза
#     "file":         "Caramba-Connect-Setup-x64.exe",  — главный файл
#     "size":         12345678,
#     "sha256":       "…",
#     "published_at": "2026-09-11T12:00:00Z",
#     "files":        [{"arch": "arm64", "file": "…", "size": N, "sha256": "…"}, …]
#   }
# `file`/`size`/`sha256` — главный файл платформы (первый аргумент), по нему
# панель строит download_url. `files` перечисляет ВСЕ файлы: у Android два APK
# по ABI, у Windows инсталлятор и портативный ZIP.
#
# Использование:
#   bash ci-manifest.sh <platform> <out.json> <file> [file…]
#
# Переменные окружения:
#   GITHUB_REF_TYPE/GITHUB_REF_NAME — тег прогона; вне тега тег собирается из
#                                     pubspec как client-v<version>+<build>.
#   CARAMBA_PUBLISHED_AT            — переопределить дату (для тестов).
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

die() { echo "ошибка: ci-manifest: $*" >&2; exit 1; }

PLATFORM="${1:-}"
OUT="${2:-}"
shift 2 || true
[[ -n "${PLATFORM}" && -n "${OUT}" && $# -ge 1 ]] \
  || die "использование: $0 <platform> <out.json> <file> [file…]"
case "${PLATFORM}" in
  android|windows|macos|linux) ;;
  *) die "неизвестная платформа: ${PLATFORM}" ;;
esac

# Версия и сборка — из pubspec.yaml, того же источника, что и тег релиза
# client-v<pubspec> и запись инсталлятора Inno Setup.
VERSION="$(sed -n 's/^version:[[:space:]]*\([0-9][0-9.]*\)+\([0-9][0-9]*\).*/\1/p' "${CLIENT_DIR}/pubspec.yaml")"
BUILD="$(sed -n 's/^version:[[:space:]]*\([0-9][0-9.]*\)+\([0-9][0-9]*\).*/\2/p' "${CLIENT_DIR}/pubspec.yaml")"
[[ -n "${VERSION}" && -n "${BUILD}" ]] || die "не разобрал version: в pubspec.yaml (ожидаю X.Y.Z+N)"

if [[ "${GITHUB_REF_TYPE:-}" == "tag" && -n "${GITHUB_REF_NAME:-}" ]]; then
  TAG="${GITHUB_REF_NAME}"
else
  TAG="client-v${VERSION}+${BUILD}"
fi
PUBLISHED_AT="${CARAMBA_PUBLISHED_AT:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"

# sha256sum есть на Linux и в Git Bash, на macOS — только shasum.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    die "нечем посчитать sha256 (нет sha256sum и shasum)"
  fi
}

# stat различается между GNU и BSD; wc -c одинаков везде.
size_of() { wc -c < "$1" | tr -d ' '; }

# Архитектура по имени файла: контракт имён Caramba-Connect-<OS>-<arch>.<ext>.
arch_of() {
  case "$1" in
    *-arm64.*|*-arm64-*) echo "arm64" ;;
    *-armv7.*|*-armv7-*) echo "armv7" ;;
    *-x64.*|*-x64-*)     echo "x64" ;;
    *)                   echo "" ;;
  esac
}

# Эти строки едут в JSON без экранирования: имена файлов у нас ASCII и без
# кавычек, но хэши/размеры/даты проверяем, чтобы битый JSON не уехал в релиз.
FILES_JSON=""
PRIMARY_NAME="" PRIMARY_SIZE="" PRIMARY_SHA=""
for path in "$@"; do
  [[ -s "${path}" ]] || die "файла нет или он пуст: ${path}"
  name="$(basename "${path}")"
  case "${name}" in
    *\"*|*\\*) die "недопустимое имя файла для JSON: ${name}" ;;
  esac
  size="$(size_of "${path}")"
  sha="$(sha256_of "${path}")"
  [[ "${sha}" =~ ^[0-9a-f]{64}$ ]] || die "не похоже на sha256: ${sha}"
  [[ "${size}" =~ ^[0-9]+$ ]] || die "не похоже на размер: ${size}"
  if [[ -z "${PRIMARY_NAME}" ]]; then
    PRIMARY_NAME="${name}"; PRIMARY_SIZE="${size}"; PRIMARY_SHA="${sha}"
  fi
  entry="{\"arch\":\"$(arch_of "${name}")\",\"file\":\"${name}\",\"size\":${size},\"sha256\":\"${sha}\"}"
  if [[ -n "${FILES_JSON}" ]]; then
    FILES_JSON="${FILES_JSON},${entry}"
  else
    FILES_JSON="${entry}"
  fi
done

mkdir -p "$(dirname "${OUT}")"
cat > "${OUT}" <<EOF
{
  "platform": "${PLATFORM}",
  "version": "${VERSION}",
  "build": ${BUILD},
  "tag": "${TAG}",
  "file": "${PRIMARY_NAME}",
  "size": ${PRIMARY_SIZE},
  "sha256": "${PRIMARY_SHA}",
  "published_at": "${PUBLISHED_AT}",
  "files": [${FILES_JSON}]
}
EOF

# Самопроверка: манифест обязан быть валидным JSON. python есть на всех трёх
# раннерах; без него проверку честно пропускаем.
for py in python3 python; do
  if command -v "${py}" >/dev/null 2>&1; then
    "${py}" -c 'import json,sys; json.load(open(sys.argv[1]))' "${OUT}" </dev/null \
      || die "манифест ${OUT} не разобрался как JSON"
    break
  fi
done

echo "==> манифест: ${OUT} (${PLATFORM} ${VERSION}+${BUILD}, ${TAG}, ${PRIMARY_NAME})"
