#!/usr/bin/env bash
#
# install.sh — установка Caramba Connect на Linux из архива
# Caramba-Connect-Linux-x64.tar.gz.
#
# Зачем скрипт, а не deb/AppImage: пакета у проекта пока нет, а без установки
# «как у людей» приложение из распакованной папки не регистрируется
# обработчиком ссылок caramba:// и не появляется в меню, то есть путь
# «ссылка из бота -> экран подключения» на Linux не работает вовсе.
#
# Что делает (нужен root, скрипт сам перезапустится через sudo):
#   1. копирует бандл (этот каталог) в /opt/caramba-connect;
#   2. кладёт caramba-connect.desktop с реальным Exec в /usr/share/applications
#      (регистрация схем caramba:// и carambaconnect:// через MimeType);
#   3. кладёт иконку в /usr/share/icons/hicolor/512x512/apps;
#   4. обновляет базы desktop-файлов и иконок;
#   5. выдаёт бинарю CAP_NET_ADMIN (setcap) и прописывает lib/ в ld.so.conf.d.
#
# Использование:
#   tar -xzf Caramba-Connect-Linux-x64.tar.gz
#   sudo ./caramba-connect/install.sh            # установка
#   sudo ./caramba-connect/install.sh --uninstall
#
# Проверка после установки:
#   xdg-mime query default x-scheme-handler/caramba   # -> caramba-connect.desktop
#   getcap /opt/caramba-connect/caramba_client         # -> cap_net_admin=ep
set -euo pipefail

PREFIX="${CARAMBA_PREFIX:-/opt/caramba-connect}"
BIN_NAME="caramba_client"
DESKTOP_ID="caramba-connect.desktop"
ICON_NAME="caramba-connect"
APPS_DIR="/usr/share/applications"
ICON_DIR="/usr/share/icons/hicolor/512x512/apps"
LDCONF="/etc/ld.so.conf.d/caramba-connect.conf"

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

log() { echo "==> $*"; }
die() { echo "ошибка: $*" >&2; exit 1; }

if [[ "$(id -u)" -ne 0 ]]; then
  command -v sudo >/dev/null 2>&1 || die "нужен root: запустите через sudo"
  exec sudo -- "${BASH_SOURCE[0]}" "$@"
fi

# Не падаем на системах без этих утилит: регистрация обработчика ссылок и
# кэш иконок подхватятся при следующем логине, а установка обязана дойти до
# конца (setcap ниже важнее косметики).
refresh_desktop_db() {
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${APPS_DIR}" || true
  fi
  if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
  fi
}

uninstall() {
  log "удаляю ${PREFIX}, ${APPS_DIR}/${DESKTOP_ID}, иконку и ${LDCONF}"
  rm -rf "${PREFIX}"
  rm -f "${APPS_DIR}/${DESKTOP_ID}" "${ICON_DIR}/${ICON_NAME}.png" "${LDCONF}"
  if command -v ldconfig >/dev/null 2>&1; then ldconfig || true; fi
  refresh_desktop_db
  log "Caramba Connect удалён (настройки в ~/.local и ~/.config не тронуты)"
}

if [[ "${1:-}" == "--uninstall" ]]; then
  uninstall
  exit 0
fi

[[ -f "${SRC}/${BIN_NAME}" ]] || die "рядом со скриптом нет ${BIN_NAME}: запускайте из распакованной папки caramba-connect"
[[ -f "${SRC}/${DESKTOP_ID}" ]] || die "рядом со скриптом нет ${DESKTOP_ID}"
[[ -f "${SRC}/${ICON_NAME}.png" ]] || die "рядом со скриптом нет ${ICON_NAME}.png"
command -v setcap >/dev/null 2>&1 || die "нет setcap (пакет libcap2-bin в Debian/Ubuntu, libcap в Fedora/Arch)"

# 1. бандл. Сначала во временный каталог рядом, потом атомарная замена: если
# копирование оборвётся, старая установка останется рабочей.
log "копирую бандл в ${PREFIX}"
STAGE="${PREFIX}.new.$$"
rm -rf "${STAGE}"
mkdir -p "$(dirname "${PREFIX}")"
cp -a "${SRC}" "${STAGE}"
# Служебные файлы установщика приложению не нужны и в /opt не кладутся.
rm -f "${STAGE}/install.sh" "${STAGE}/${DESKTOP_ID}" "${STAGE}/${ICON_NAME}.png"
rm -rf "${PREFIX}"
mv "${STAGE}" "${PREFIX}"
chmod 755 "${PREFIX}/${BIN_NAME}"

# 2. desktop-файл с реальным путём. Шаблон в архиве содержит путь по умолчанию;
# sed нужен на случай CARAMBA_PREFIX. %u сохраняем: без него URI из xdg-open не
# попадёт в argv и ссылка из бота не откроет экран подключения.
log "регистрирую ${DESKTOP_ID} (обработчик caramba:// и carambaconnect://)"
mkdir -p "${APPS_DIR}"
sed "s|^Exec=.*|Exec=${PREFIX}/${BIN_NAME} %u|" "${SRC}/${DESKTOP_ID}" > "${APPS_DIR}/${DESKTOP_ID}"
chmod 644 "${APPS_DIR}/${DESKTOP_ID}"

# 3. иконка: hicolor/512x512 покрывает и меню, и док, и окно «О программе».
mkdir -p "${ICON_DIR}"
install -m 644 "${SRC}/${ICON_NAME}.png" "${ICON_DIR}/${ICON_NAME}.png"

# 4. базы desktop-файлов и иконок.
refresh_desktop_db

# 5. права на tun. Ядро (mihomo внутри libcaramba_core.so) само создаёт
# tun-устройство, для этого процессу нужен CAP_NET_ADMIN. Выдаём его файлу,
# чтобы приложение запускалось обычным пользователем без sudo/pkexec.
#
# Побочный эффект setcap, о котором молчат инструкции: бинарь с capability
# запускается в secure-execution mode, и динамический загрузчик ИГНОРИРУЕТ
# $ORIGIN в rpath (Flutter собирает раннер с rpath=$ORIGIN/lib), поэтому без
# следующего шага приложение не найдёт libflutter_linux_gtk.so и libapp.so и
# умрёт на старте. Путь к lib/ прописывается системно через ld.so.conf.d —
# кэш ldconfig в secure-режиме честно используется.
log "прописываю ${PREFIX}/lib в ${LDCONF} и ставлю cap_net_admin+ep на ${BIN_NAME}"
echo "${PREFIX}/lib" > "${LDCONF}"
if command -v ldconfig >/dev/null 2>&1; then ldconfig; fi
setcap cap_net_admin+ep "${PREFIX}/${BIN_NAME}"

log "готово: Caramba Connect установлен в ${PREFIX}"
log "запуск: из меню приложений или ${PREFIX}/${BIN_NAME}; удаление: sudo ${PREFIX}/install.sh --uninstall"
# install.sh в /opt не копируется (см. выше), поэтому подсказываем путь из архива.
log "(для удаления можно снова запустить install.sh --uninstall из распакованного архива)"
