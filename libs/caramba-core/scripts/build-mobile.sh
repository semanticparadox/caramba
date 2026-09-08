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
#   scripts/build-mobile.sh ios            # → build/ios/exarobot.xcframework
#   scripts/build-mobile.sh macos          # → build/macos/exarobot.xcframework
#   scripts/build-mobile.sh all            # все три цели
#
# ios и macos кладут результат в РАЗНЫЕ подкаталоги build/: имя файла задаёт имя
# Swift-модуля (exarobot.xcframework → `import Exarobot`), поэтому переименовать
# один из них нельзя, а в одном каталоге они затирали бы друг друга.
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
# accessToken,refreshToken,accessExpiryUnix), SetTunFd, Up→JSON, Down,
# StatusJSON→{stage,detail?,connectedSinceMs},
# TrafficJSON→{downBps,upBps,downTotal,upTotal}, плюс
# Login*/SetProtocol/SetRelay/ApplyPreset/SetSplitTunnel/ListPresets/AutoTune.
#
# Менять сигнатуру любого из этих методов — значит менять ABI вендоренного AAR.
# Сам по себе APK об этом не узнает: Gradle соберётся против ТОГО .aar, что
# лежит в packages/caramba_vpn/android/libs/, и молча увезёт старое ядро. После
# правки mobile/ этот скрипт обязателен, а результат — перекопировать поверх
# packages/caramba_vpn/android/libs/caramba.aar. Проверить, что доехало:
#   unzip -p .../caramba.aar classes.jar > /tmp/c.jar && \
#   javap -classpath /tmp/c.jar io.caramba.core.mobile.Client | grep configure
#
# Куда вендорится артефакт (пути ведут в плагин packages/caramba_vpn, а не в
# приложение: именно их читают build.gradle и caramba_vpn.podspec). Копирование
# делает сам скрипт: раньше это был ручной шаг из README, и «собрал, но забыл
# скопировать» давал сборку против прошлого ядра:
#   android → apps/caramba-client/packages/caramba_vpn/android/libs/caramba.aar
#   ios     → apps/caramba-client/packages/caramba_vpn/darwin/Frameworks/ios/exarobot.xcframework
#   macos   → apps/caramba-client/packages/caramba_vpn/darwin/Frameworks/macos/exarobot.xcframework
#
# ИМЕНА В SWIFT. gobind даёт префикс классов = -prefix + имя Go-пакета, а имя
# Swift-модуля = базовое имя выходного файла. Пакет называется mobile, файл —
# exarobot.xcframework, значит на стороне Swift это `import Exarobot` и классы
# `CarambaMobileClient` / `CarambaMobileNewClient`, а НЕ `Caramba`/`CarambaClient`.
# Менять -prefix или имя файла — значит менять эти имена в darwin/Classes.
#
set -euo pipefail

# Каталог модуля caramba-core (родитель scripts/).
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/build"
PKG="github.com/semanticparadox/caramba/libs/caramba-core/mobile"

# Каталог плагина, куда вендорятся артефакты (../../apps/caramba-client/... от
# libs/caramba-core). Считаем от ROOT, чтобы скрипт работал из любого cwd.
PLUGIN="$(cd "${ROOT}/../.." && pwd)/apps/caramba-client/packages/caramba_vpn"

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
  # go install кладёт инструменты в GOPATH/bin, которого обычно нет в PATH у
  # неинтерактивных оболочек: добавляем сами, иначе сборка падает с «не найден»
  # при установленном gomobile.
  GOBIN_DIR="$(go env GOBIN)"
  [[ -z "${GOBIN_DIR}" ]] && GOBIN_DIR="$(go env GOPATH)/bin"
  case ":${PATH}:" in
    *":${GOBIN_DIR}:"*) ;;
    *) export PATH="${GOBIN_DIR}:${PATH}" ;;
  esac
  if ! command -v gomobile >/dev/null 2>&1; then
    echo "ошибка: gomobile не найден. Установите:" >&2
    echo "  go install golang.org/x/mobile/cmd/gomobile@latest" >&2
    echo "  go install golang.org/x/mobile/cmd/gobind@latest" >&2
    echo "  gomobile init" >&2
    exit 1
  fi
}

# Кладёт свежесобранный артефакт в плагин. rm -rf перед cp: xcframework — это
# каталог, и cp -R поверх существующего вложил бы его внутрь старого.
vendor() {
  local src="$1" dst="$2"
  mkdir -p "$(dirname "${dst}")"
  rm -rf "${dst}"
  cp -R "${src}" "${dst}"
  echo ">> вендорено: ${dst}"
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
  vendor "${OUT}/exarobot.aar" "${PLUGIN}/android/libs/caramba.aar"
  echo ">> готово: ${OUT}/exarobot.aar"
}

build_ios() {
  require_gomobile
  echo ">> gomobile bind ios (tags=${TAGS}) → ${OUT}/ios/exarobot.xcframework"
  # iossdk автоопределяется Xcode; gomobile собирает device+simulator в xcframework.
  ( cd "${ROOT}" && gomobile bind \
      -target=ios \
      -tags "${TAGS}" \
      -o "${OUT}/ios/exarobot.xcframework" \
      -prefix Caramba \
      "${PKG}" )
  vendor "${OUT}/ios/exarobot.xcframework" "${PLUGIN}/darwin/Frameworks/ios/exarobot.xcframework"
  echo ">> готово: ${OUT}/ios/exarobot.xcframework"
}

build_macos() {
  require_gomobile
  echo ">> gomobile bind macos (tags=${TAGS}) → ${OUT}/macos/exarobot.xcframework"
  # Путь Network/System Extension на macOS: тот же биндинг, что и на iOS, но
  # собранный под macOS SDK. Путь dart:ffi (proxy-режим) им НЕ пользуется — он
  # грузит libcaramba_core.dylib из build-desktop-lib.sh, это другой артефакт.
  ( cd "${ROOT}" && gomobile bind \
      -target=macos \
      -tags "${TAGS}" \
      -o "${OUT}/macos/exarobot.xcframework" \
      -prefix Caramba \
      "${PKG}" )
  vendor "${OUT}/macos/exarobot.xcframework" "${PLUGIN}/darwin/Frameworks/macos/exarobot.xcframework"
  echo ">> готово: ${OUT}/macos/exarobot.xcframework"
}

case "${1:-all}" in
  android) build_android ;;
  ios)     build_ios ;;
  macos)   build_macos ;;
  all)     build_android; build_ios; build_macos ;;
  *)
    echo "использование: $0 [android|ios|macos|all]" >&2
    exit 2
    ;;
esac
