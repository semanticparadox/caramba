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
#   scripts/build.sh run -d macos            # run вместо build
#   USE_NATIVE_VPN=false scripts/build.sh apk --debug   # сборка на моке
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

if [[ $# -lt 1 ]]; then
  echo "использование: scripts/build.sh <apk|appbundle|macos|ios|run> [аргументы flutter]" >&2
  exit 2
fi

TARGET="$1"
shift

NATIVE="${USE_NATIVE_VPN:-true}"
EPOCH="$(date +%s)"

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

echo ">> flutter build ${TARGET} (native=${NATIVE}, BUILD_EPOCH=${EPOCH})"
exec flutter build "${TARGET}" "${DEFINES[@]}" "$@"
