#!/usr/bin/env bash
#
# fetch-wintun.sh — скачивает подписанный wintun.dll и кладёт его туда, где его
# ждёт Windows-сборка Flutter-клиента.
#
# Зачем. wintun — юзермодный TUN-драйвер, который mihomo открывает, чтобы
# создать туннельный адаптер на Windows. Он НЕ собирается из исходников в нашей
# цепочке: нужна подпись WHQL, поэтому берётся официальный релизный zip с
# wintun.net. Двоичный файл gitignored (packages/caramba_vpn/.gitignore:
# windows/lib/*.dll), значит его надо получать явным шагом — и локально, и в CI.
#
# Целостность. Скачанный архив сверяется с ЗАШИТОЙ ниже SHA-256 (опубликована на
# wintun.net рядом со ссылкой и перепроверена при написании скрипта). Это
# единственная защита: мы вкладываем чужой бинарник в свой инсталлятор, и молча
# проглотить подменённый архив нельзя. Обновление версии = смена и URL, и суммы.
#
# Использование:
#   scripts/fetch-wintun.sh                 # amd64 → плагин
#   scripts/fetch-wintun.sh --arch arm64
#   scripts/fetch-wintun.sh --dest /путь/куда/положить
#
# Артефакт (gitignored):
#   apps/caramba-client/packages/caramba_vpn/windows/lib/wintun.dll
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "${ROOT}/../.." && pwd)"

WINTUN_VERSION="0.14.1"
WINTUN_URL="https://www.wintun.net/builds/wintun-${WINTUN_VERSION}.zip"
# SHA-256 архива wintun-0.14.1.zip (источник: https://www.wintun.net/).
WINTUN_SHA256="07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51"

ARCH="amd64"
DEST="${REPO}/apps/caramba-client/packages/caramba_vpn/windows/lib"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) ARCH="${2:-}"; shift 2 ;;
    --dest) DEST="${2:-}"; shift 2 ;;
    -h|--help) sed -n '2,30p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "неизвестный аргумент: $1" >&2; exit 2 ;;
  esac
done

case "${ARCH}" in
  amd64|arm64|x86|arm) ;;
  *) echo "в архиве есть только amd64|arm64|x86|arm (дано: ${ARCH})" >&2; exit 2 ;;
esac

# sha256: на macOS это shasum, на Linux/Git-Bash — sha256sum. Проверяем оба,
# чтобы скрипт был один для локальной машины и для CI-раннера windows-latest.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "нет ни sha256sum, ни shasum — проверить архив нечем" >&2
    exit 1
  fi
}

# unzip есть не везде (например, в голом Git Bash на windows-раннере), поэтому
# запасной путь — модуль zipfile из python3.
extract_dll() {
  local zip="$1" member="$2" out="$3"
  if command -v unzip >/dev/null 2>&1; then
    unzip -p "${zip}" "${member}" > "${out}"
  elif command -v python3 >/dev/null 2>&1; then
    python3 - "${zip}" "${member}" "${out}" <<'PY'
import sys, zipfile
zip_path, member, out = sys.argv[1:4]
with zipfile.ZipFile(zip_path) as z, open(out, "wb") as fh:
    fh.write(z.read(member))
PY
  else
    echo "нет ни unzip, ни python3 — распаковать архив нечем" >&2
    exit 1
  fi
}

TMP="$(mktemp -d)"
# Временный каталог убирается в любом случае: и на успехе, и на провале
# проверки суммы (иначе битый архив останется на диске и переживёт запуск).
trap 'rm -rf "${TMP}"' EXIT

echo ">> скачиваю ${WINTUN_URL}"
curl -fsSL -o "${TMP}/wintun.zip" "${WINTUN_URL}"

GOT="$(sha256_of "${TMP}/wintun.zip")"
if [[ "${GOT}" != "${WINTUN_SHA256}" ]]; then
  echo "SHA-256 не совпала!" >&2
  echo "  ожидалось: ${WINTUN_SHA256}" >&2
  echo "  получено:  ${GOT}" >&2
  exit 1
fi
echo ">> SHA-256 совпала: ${GOT}"

mkdir -p "${DEST}"
# Пишем во временный файл и только потом переносим на место: прерванная
# распаковка не должна оставить обрезанный wintun.dll, который сборка молча
# упакует в дистрибутив.
extract_dll "${TMP}/wintun.zip" "wintun/bin/${ARCH}/wintun.dll" "${TMP}/wintun.dll"
mv "${TMP}/wintun.dll" "${DEST}/wintun.dll"
chmod 644 "${DEST}/wintun.dll"

echo ">> готово: ${DEST}/wintun.dll (wintun ${WINTUN_VERSION}, ${ARCH})"
