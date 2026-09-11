#!/usr/bin/env bash
#
# build.sh — обёртка над `flutter build`/`flutter run`, которая подставляет
# обязательные dart-define'ы. Собирать клиент напрямую можно, но тогда легко
# забыть BUILD_EPOCH, и CSM-энроллмент откажет на проверке правдоподобия часов
# (lib/data/models/csm_enrollment.dart: kCsmBuildEpoch == 0 → часы «не заданы»,
# первое доверие не устанавливается). Это безопасное направление отказа, но
# выглядит как баг, поэтому подстановка автоматизирована здесь.
#
# BUILD_EPOCH — момент сборки в секундах Unix. Окно правдоподобия часов
# отсчитывается от него, поэтому он должен быть настоящим временем сборки,
# а не константой в репозитории.
#
# Использование:
#   scripts/build.sh apk --debug
#   scripts/build.sh macos --release
#   scripts/build.sh macos-dmg               # release + неподписанный DMG
#   scripts/build.sh run -d macos            # run вместо build
#   USE_NATIVE_VPN=false scripts/build.sh apk --debug   # сборка на моке
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

if [[ $# -lt 1 ]]; then
  echo "использование: scripts/build.sh <apk|appbundle|macos|macos-dmg|ios|linux|windows|run> [аргументы flutter]" >&2
  exit 2
fi

TARGET="$1"
shift

NATIVE="${USE_NATIVE_VPN:-true}"
EPOCH="$(date +%s)"

# Экспорт, а не только dart-define: --dart-define виден исключительно Dart-коду,
# а решение «нативная сборка или mock» нужно ещё и нативной стороне — podspec
# плагина на Darwin читает USE_NATIVE_VPN из окружения во время `pod install`,
# чтобы выбрать между -DCARAMBA_CORE_REQUIRED и mock-сборкой. Без экспорта
# podspec видел бы «не задано» даже когда здесь явно передан false.
export USE_NATIVE_VPN="${NATIVE}"

DEFINES=(
  --dart-define=USE_NATIVE_VPN="${NATIVE}"
  --dart-define=BUILD_EPOCH="${EPOCH}"
)

# CARAMBA_API_BASE переопределяет панель тенанта №1 для сборок под другого
# оператора; пустое значение оставляет дефолт из lib/data/api_client.dart.
if [[ -n "${CARAMBA_API_BASE:-}" ]]; then
  DEFINES+=(--dart-define=CARAMBA_API_BASE="${CARAMBA_API_BASE}")
fi

if [[ "${TARGET}" == "run" ]]; then
  echo ">> flutter run (native=${NATIVE}, BUILD_EPOCH=${EPOCH})"
  exec flutter run "${DEFINES[@]}" "$@"
fi

# macos-dmg — релизная сборка плюс упаковка в неподписанный DMG. Отдельная цель,
# а не отдельный скрипт: и CI (client-desktop.yml), и локальная проверка должны
# получать БАЙТ-В-БАЙТ одинаковую последовательность шагов, иначе «у меня
# собиралось» опять расходится с релизом.
#
# Подписи нет (сертификата Apple Developer у проекта нет), поэтому Gatekeeper на
# чужой машине потребует «Открыть всё равно»; это осознанное состояние раунда.
if [[ "${TARGET}" == "macos-dmg" ]]; then
  echo ">> flutter build macos --release (native=${NATIVE}, BUILD_EPOCH=${EPOCH})"
  flutter build macos --release "${DEFINES[@]}" "$@"

  APP_DIR="${ROOT}/build/macos/Build/Products/Release"
  APP="$(/usr/bin/find "${APP_DIR}" -maxdepth 1 -name '*.app' -print -quit)"
  if [[ -z "${APP}" ]]; then
    echo "не найден .app в ${APP_DIR}" >&2
    exit 1
  fi

  # Имя файла по единой схеме ассетов релиза (Caramba-Connect-<OS>-<arch>.<ext>);
  # то же имя ищут apps/caramba-installer и /api/client/app/downloads в панели.
  DMG="${ROOT}/build/Caramba-Connect-macOS-arm64.dmg"
  STAGE="$(mktemp -d)"
  # ловушка на выходе: временный каталог со стомегабайтным .app не должен
  # оставаться в /var/folders, если hdiutil упадёт.
  trap 'rm -rf "${STAGE}"' EXIT
  cp -R "${APP}" "${STAGE}/"
  # Ссылка на /Applications — привычный жест «перетащи сюда»; без неё DMG
  # выглядит как папка с непонятным файлом.
  ln -s /Applications "${STAGE}/Applications"

  rm -f "${DMG}"
  echo ">> hdiutil create → ${DMG}"
  hdiutil create -volname "Caramba Connect" -srcfolder "${STAGE}" \
    -ov -format UDZO "${DMG}" >/dev/null
  echo ">> готово: ${DMG} ($(du -h "${DMG}" | cut -f1))"
  exit 0
fi

echo ">> flutter build ${TARGET} (native=${NATIVE}, BUILD_EPOCH=${EPOCH})"
exec flutter build "${TARGET}" "${DEFINES[@]}" "$@"
