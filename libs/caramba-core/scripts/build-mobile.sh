#!/usr/bin/env bash
#
# build-mobile.sh — сборка нативных привязок caramba-core через gomobile bind.
#
# Собирает gomobile-фасад (libs/caramba-core/mobile) с нативным ядром mihomo для
# Android (AAR) и iOS (xcframework). Требует установленного Go-тулчейна, gomobile,
# и платформенных SDK (Android NDK / Xcode). В CI/окружении без тулчейна скрипт
# не запускается — это инструкция к локальной/CI-сборке.
#
# Использование:
#   scripts/build-mobile.sh android        # → build/exarobot.aar
#   scripts/build-mobile.sh ios            # → build/exarobot.xcframework
#   scripts/build-mobile.sh all            # обе цели
#
# Подготовка (один раз):
#   go install golang.org/x/mobile/cmd/gomobile@latest
#   go install golang.org/x/mobile/cmd/gobind@latest
#   gomobile init
#
# ВНИМАНИЕ: перед сборкой с тегом mihomo выполните `cd libs/caramba-core &&
# go mod tidy` (заполняет go.sum транзитивным графом mihomo); без этого bind
# падает с «missing go.sum entry». Нужен CGO (gvisor/sing-tun) — gomobile его
# включает сам для android/ios.
#
# Экспортируемая поверхность mobile.Client, которую потребляет нативный плагин
# (канал com.caramba/vpn): NewClient, Configure(panelURL,subscriptionID,
# accessToken), SetTunFd, Up→JSON, Down, StatusJSON→{stage,detail?,
# connectedSinceMs}, TrafficJSON→{downBps,upBps,downTotal,upTotal}, плюс
# Login*/SetProtocol/SetRelay/ApplyPreset/SetSplitTunnel/ListPresets/AutoTune.
#
# Куда вендорится артефакт:
#   android → apps/caramba-client/android/caramba_vpn/libs/exarobot.aar
#   ios     → apps/caramba-client/ios/Frameworks/exarobot.xcframework
#
set -euo pipefail

# Каталог модуля caramba-core (родитель scripts/).
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/build"
PKG="github.com/semanticparadox/caramba/libs/caramba-core/mobile"

# Тег mihomo подключает нативное ядро (engine_mihomo.go, prober_mihomo.go).
TAGS="mihomo,with_gvisor"

mkdir -p "${OUT}"
# Патчи зависимостей: патченная копия mihomo + альтернативный go.mod
# (см. patches/README.md). gomobile не понимает -modfile, поэтому на время
# сборки подменяем go.mod/go.sum и восстанавливаем их при любом выходе.
bash "${ROOT}/scripts/mk-patched-deps.sh" >/dev/null
cp "${ROOT}/go.mod" "${OUT}/go.mod.orig"
cp "${ROOT}/go.sum" "${OUT}/go.sum.orig"
restore_gomod() {
  cp "${OUT}/go.mod.orig" "${ROOT}/go.mod"
  cp "${OUT}/go.sum.orig" "${ROOT}/go.sum"
}
trap restore_gomod EXIT
cp "${OUT}/patched.mod" "${ROOT}/go.mod"
cp "${OUT}/patched.sum" "${ROOT}/go.sum"

require_gomobile() {
  if ! command -v gomobile >/dev/null 2>&1; then
    echo "ошибка: gomobile не найден. Установите:" >&2
    echo "  go install golang.org/x/mobile/cmd/gomobile@latest" >&2
    echo "  go install golang.org/x/mobile/cmd/gobind@latest" >&2
    echo "  gomobile init" >&2
    exit 1
  fi
}

build_android() {
  require_gomobile
  echo ">> gomobile bind android (tags=${TAGS}) → ${OUT}/exarobot.aar"
  # androidapi 21 = Android 5.0, минимум для VpnService-сценариев exarobot.
  ( cd "${ROOT}" && gomobile bind \
      -target=android \
      -androidapi 21 \
      -tags "${TAGS}" \
      -o "${OUT}/exarobot.aar" \
      -javapkg io.caramba.core \
      "${PKG}" )
  echo ">> готово: ${OUT}/exarobot.aar (подключить в apps/caramba-client/android)"
}

build_ios() {
  require_gomobile
  echo ">> gomobile bind ios (tags=${TAGS}) → ${OUT}/exarobot.xcframework"
  # iossdk автоопределяется Xcode; gomobile собирает device+simulator в xcframework.
  ( cd "${ROOT}" && gomobile bind \
      -target=ios \
      -tags "${TAGS}" \
      -o "${OUT}/exarobot.xcframework" \
      -prefix Caramba \
      "${PKG}" )
  echo ">> готово: ${OUT}/exarobot.xcframework (подключить в apps/caramba-client/ios)"
}

case "${1:-all}" in
  android) build_android ;;
  ios)     build_ios ;;
  all)     build_android; build_ios ;;
  *)
    echo "использование: $0 [android|ios|all]" >&2
    exit 2
    ;;
esac
