#!/usr/bin/env bash
#
# ci-android.sh — единственный путь сборки релизного APK Caramba Connect.
#
# Один и тот же скрипт крутится локально и в GitHub Actions
# (.github/workflows/client-android.yml). Так релизный артефакт не зависит от
# того, кто и где нажал кнопку: расхождение шагов «на машине» и «в CI» — самый
# дешёвый способ выпустить APK, который подписан не тем ключом или собран без
# нативного ядра, и заметить это уже после публикации.
#
# Что делает:
#   1. проверяет Android SDK/NDK и JDK 17;
#   2. ставит gomobile той версии, что зафиксирована в go.mod ядра;
#   3. собирает AAR ядра (libs/caramba-core/scripts/build-mobile.sh android);
#   4. вендорит AAR в плагин caramba_vpn (без этого Gradle молча возьмёт старый);
#   5. при наличии секрета раскладывает релизный keystore и key.properties;
#   6. собирает split-per-abi release APK через scripts/build.sh (он же ставит
#      обязательные dart-define'ы USE_NATIVE_VPN/BUILD_EPOCH);
#   7. раскладывает APK под релизными именами и печатает сертификат подписи.
#
# Использование:
#   bash apps/caramba-client/scripts/ci-android.sh
#   bash apps/caramba-client/scripts/ci-android.sh --print-ndk   # версия NDK
#
# Переменные окружения:
#   ANDROID_HOME / ANDROID_SDK_ROOT  — корень Android SDK (обязательно)
#   JAVA_HOME                        — JDK 17 (на macOS определится сам)
#   CARAMBA_NDK_VERSION              — переопределить версию NDK
#   CARAMBA_SKIP_AAR=1               — не пересобирать AAR, если он уже лежит
#                                      (в CI выставляется при попадании в кэш)
#   CARAMBA_GOMOBILE_VERSION         — версия gomobile (по умолчанию из go.mod)
#   ANDROID_KEYSTORE_BASE64 / _PASSWORD / ANDROID_KEY_PASSWORD / _ALIAS
#                                    — релизная подпись; если base64 пуст,
#                                      существующий android/key.properties
#                                      НЕ трогается (локальная машина), а если
#                                      нет и его — Gradle подпишет debug-ключом.
#
# Артефакты (все gitignored):
#   libs/caramba-core/build/exarobot.aar
#   apps/caramba-client/packages/caramba_vpn/android/libs/caramba.aar
#   apps/caramba-client/build/dist/Caramba-Connect-Android-{arm64,armv7}.apk
#   (единая схема имён Caramba-Connect-<OS>-<arch>.<ext>, та же, что ищут
#   apps/caramba-installer и /api/client/app/downloads в панели)
#
set -euo pipefail

# --- пути ---------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${CLIENT_DIR}/../.." && pwd)"
CORE_DIR="${REPO_ROOT}/libs/caramba-core"

AAR_SRC="${CORE_DIR}/build/exarobot.aar"
AAR_DST="${CLIENT_DIR}/packages/caramba_vpn/android/libs/caramba.aar"
APK_DIR="${CLIENT_DIR}/build/app/outputs/flutter-apk"
DIST_DIR="${CLIENT_DIR}/build/dist"

# Версия NDK должна совпадать с flutter.ndkVersion (иначе Gradle скачивает свою
# и шаг установки в CI оказывается бесполезным). Сегодня это константа
# `ndkVersion` из flutter_tools/lib/src/android/gradle_utils.dart.
NDK_VERSION="${CARAMBA_NDK_VERSION:-28.2.13676358}"

# Служебный режим для workflow: он ставит NDK через sdkmanager и не должен
# знать версию отдельно от скрипта.
if [[ "${1:-}" == "--print-ndk" ]]; then
  echo "${NDK_VERSION}"
  exit 0
fi

log() { echo "==> $*"; }
die() { echo "ошибка: $*" >&2; exit 1; }

# --- 1. Android SDK / NDK / JDK ----------------------------------------------
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
[[ -n "${SDK}" ]] || die "ANDROID_HOME (или ANDROID_SDK_ROOT) не задан"
[[ -d "${SDK}" ]] || die "ANDROID_HOME=${SDK} не существует"
export ANDROID_HOME="${SDK}"
export ANDROID_SDK_ROOT="${SDK}"

NDK_DIR="${SDK}/ndk/${NDK_VERSION}"
[[ -d "${NDK_DIR}" ]] || die "NDK ${NDK_VERSION} не найден в ${SDK}/ndk. Поставьте:
  sdkmanager --install \"ndk;${NDK_VERSION}\""
# gomobile ищет NDK по ANDROID_NDK_HOME; без явной переменной он берёт
# произвольный каталог из ndk/ и AAR собирается другим тулчейном, чем APK.
export ANDROID_NDK_HOME="${NDK_DIR}"
export ANDROID_NDK_ROOT="${NDK_DIR}"

if [[ -z "${JAVA_HOME:-}" ]] && [[ -x /usr/libexec/java_home ]]; then
  JAVA_HOME="$(/usr/libexec/java_home -v 17 2>/dev/null || true)"
  export JAVA_HOME
fi
[[ -n "${JAVA_HOME:-}" ]] || die "JAVA_HOME не задан (нужен JDK 17)"
[[ -x "${JAVA_HOME}/bin/java" ]] || die "в JAVA_HOME=${JAVA_HOME} нет bin/java"
JAVA_MAJOR="$("${JAVA_HOME}/bin/java" -version 2>&1 | head -1 | sed -E 's/.*"([0-9]+).*/\1/')"
[[ "${JAVA_MAJOR}" == "17" ]] || echo "предупреждение: JDK ${JAVA_MAJOR}, проект собирается на 17" >&2

command -v go >/dev/null || die "go не найден"
command -v flutter >/dev/null || die "flutter не найден"

log "SDK=${SDK}"
log "NDK=${NDK_DIR}"
log "JDK=${JAVA_HOME} (major ${JAVA_MAJOR})"
log "$(go version)"
log "flutter $(flutter --version 2>/dev/null | head -1)"

# --- 2. gomobile --------------------------------------------------------------
# gobind генерирует привязки по внутреннему API golang.org/x/mobile: если
# установленный gomobile новее модуля из go.mod, bind падает на несовпадении
# сгенерированного кода. Поэтому версия берётся из самого go.mod ядра.
GOBIN_DIR="$(go env GOBIN)"
[[ -n "${GOBIN_DIR}" ]] || GOBIN_DIR="$(go env GOPATH)/bin"
export PATH="${GOBIN_DIR}:${PATH}"

if ! command -v gomobile >/dev/null 2>&1; then
  XMOBILE="${CARAMBA_GOMOBILE_VERSION:-}"
  if [[ -z "${XMOBILE}" ]]; then
    XMOBILE="$(cd "${CORE_DIR}" && go list -m -f '{{.Version}}' golang.org/x/mobile 2>/dev/null || true)"
  fi
  [[ -n "${XMOBILE}" ]] || XMOBILE="latest"
  log "ставлю gomobile/gobind @ ${XMOBILE}"
  go install "golang.org/x/mobile/cmd/gomobile@${XMOBILE}"
  go install "golang.org/x/mobile/cmd/gobind@${XMOBILE}"
  gomobile init
else
  log "gomobile уже установлен: $(command -v gomobile)"
fi

# --- 3. AAR ядра --------------------------------------------------------------
if [[ "${CARAMBA_SKIP_AAR:-0}" == "1" && -s "${AAR_SRC}" ]]; then
  log "AAR из кэша: ${AAR_SRC} ($(du -h "${AAR_SRC}" | cut -f1))"
else
  log "собираю AAR ядра (gomobile bind android, tags mihomo,with_gvisor)"
  bash "${CORE_DIR}/scripts/build-mobile.sh" android
fi
[[ -s "${AAR_SRC}" ]] || die "AAR не собрался: ${AAR_SRC}"

# --- 4. вендоринг AAR в плагин ------------------------------------------------
# Gradle собирается против ТОГО .aar, что лежит в плагине. Пропустить копию —
# значит увезти в релиз ядро предыдущей сборки, не увидев ни одной ошибки.
mkdir -p "$(dirname "${AAR_DST}")"
cp "${AAR_SRC}" "${AAR_DST}"
log "AAR → ${AAR_DST} ($(du -h "${AAR_DST}" | cut -f1))"

# --- 5. релизная подпись ------------------------------------------------------
KEY_PROPS="${CLIENT_DIR}/android/key.properties"
KEYSTORE="${CLIENT_DIR}/android/release.keystore"
if [[ -n "${ANDROID_KEYSTORE_BASE64:-}" ]]; then
  log "раскладываю релизный keystore из секрета"
  umask 077
  printf '%s' "${ANDROID_KEYSTORE_BASE64}" | base64 -d > "${KEYSTORE}"
  [[ -s "${KEYSTORE}" ]] || die "keystore из ANDROID_KEYSTORE_BASE64 пуст"
  # (раньше здесь ждали rootProject.file() и писали относительное имя —
  #  Gradle искал его в android/app/ и падал на validateSigningRelease.)
  {
    # Абсолютный путь: build.gradle.kts резолвит storeFile через file() модуля
    # app/, а не rootProject.file(), и относительное имя искалось в android/app/.
    echo "storeFile=${KEYSTORE}"
    echo "storePassword=${ANDROID_KEYSTORE_PASSWORD:-}"
    echo "keyAlias=${ANDROID_KEY_ALIAS:-caramba-connect}"
    echo "keyPassword=${ANDROID_KEY_PASSWORD:-${ANDROID_KEYSTORE_PASSWORD:-}}"
  } > "${KEY_PROPS}"
  umask 022
elif [[ -f "${KEY_PROPS}" ]]; then
  log "использую существующий android/key.properties (секрет не задан)"
else
  echo "предупреждение: ключа нет — release будет подписан DEBUG-ключом" >&2
fi

# --- 6. сборка APK ------------------------------------------------------------
cd "${CLIENT_DIR}"
log "flutter pub get"
flutter pub get

log "flutter build apk --release --split-per-abi"
bash "${SCRIPT_DIR}/build.sh" apk --release --split-per-abi

# --- 7. релизные имена --------------------------------------------------------
mkdir -p "${DIST_DIR}"
rename_apk() {
  local src="${APK_DIR}/$1" dst="${DIST_DIR}/$2"
  [[ -s "${src}" ]] || die "не найден ${src}"
  cp "${src}" "${dst}"
  echo "    ${dst}  ($(du -h "${dst}" | cut -f1))"
}
log "релизные артефакты:"
rename_apk app-arm64-v8a-release.apk   Caramba-Connect-Android-arm64.apk
rename_apk app-armeabi-v7a-release.apk Caramba-Connect-Android-armv7.apk

# --- 8. подпись в лог ---------------------------------------------------------
# Печатается только открытая часть (subject/fingerprint) — по ней видно, тот же
# это ключ, что у уже установленных у пользователей сборок, или подмена.
APKSIGNER=""
for cand in "${SDK}"/build-tools/*/apksigner; do
  # if/fi, а не `[[ ]] && ...`: при set -e ложное условие на последней итерации
  # цикла роняет весь скрипт уже после того, как APK собраны.
  if [[ -x "${cand}" ]]; then APKSIGNER="${cand}"; fi
done
if [[ -n "${APKSIGNER}" ]]; then
  for apk in Caramba-Connect-Android-arm64.apk Caramba-Connect-Android-armv7.apk; do
    log "apksigner verify ${apk}"
    "${APKSIGNER}" verify --print-certs "${DIST_DIR}/${apk}"
  done
else
  echo "предупреждение: apksigner не найден в ${SDK}/build-tools" >&2
fi

log "готово: ${DIST_DIR}"
