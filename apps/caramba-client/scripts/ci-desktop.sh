#!/usr/bin/env bash
#
# ci-desktop.sh — единственный путь сборки десктопных артефактов Caramba Connect
# (macOS DMG, Windows Setup.exe + ZIP, Linux tar.gz) плюс проверка компиляции iOS.
#
# Тот же принцип, что у ci-android.sh: один скрипт крутится и локально, и в
# GitHub Actions (.github/workflows/client-desktop.yml). Шаги в workflow —
# только установка тулчейна и кэши; вся логика здесь. Расхождение «на машине» и
# «в CI» — самый дешёвый способ увезти в релиз бандл без нативного ядра и
# заметить это уже у пользователя: на десктопе ядро вендорится ОТДЕЛЬНЫМ файлом
# (dylib/so/dll), и его отсутствие сборку не роняет — приложение просто молча
# уходит в mock. Поэтому здесь после каждой сборки стоит явная проверка, что
# ядро доехало внутрь артефакта.
#
# Использование:
#   bash apps/caramba-client/scripts/ci-desktop.sh macos     # → DMG
#   bash apps/caramba-client/scripts/ci-desktop.sh windows   # → Setup.exe + ZIP (только на Windows)
#   bash apps/caramba-client/scripts/ci-desktop.sh linux     # → tar.gz (только на Linux)
#   bash apps/caramba-client/scripts/ci-desktop.sh ios       # проверка компиляции, без ассета
#
# Переменные окружения:
#   CARAMBA_SKIP_CORE=1      — не пересобирать ядро, если артефакт уже на диске
#                              (в CI выставляется при попадании в actions/cache)
#   CARAMBA_MACOS_ARCHS      — срезы universal-dylib (по умолчанию «arm64 amd64»),
#                              пробрасывается в build-desktop-lib.sh
#   CARAMBA_GOMOBILE_VERSION — версия gomobile для цели ios (по умолчанию из go.mod)
#   USE_NATIVE_VPN=false     — собрать на моке (build.sh экспортирует флаг, его
#                              видит и podspec на Darwin во время pod install)
#   CARAMBA_NO_TIMEOUT=1     — не ограничивать шаги по времени (по умолчанию
#                              каждый длинный шаг обёрнут в timeout, см. T_* ниже)
#   CARAMBA_T_CORE, CARAMBA_T_BUILD, CARAMBA_T_PUB, CARAMBA_T_FETCH,
#   CARAMBA_T_ZIP, CARAMBA_T_TOOLCHECK — лимиты соответствующих шагов, секунды
#   CARAMBA_ISCC             — путь к ISCC.exe (Inno Setup 6), если он не в
#                              стандартном месте; только для цели windows
#   CARAMBA_ALLOW_NO_VCRT=1  — не падать, если VC++-рантайм (msvcp140.dll и
#                              компания) не нашёлся в Visual Studio раннера
#
# Артефакты (все gitignored, см. .gitignore:51 «build/»):
#   macos   → apps/caramba-client/build/dist/Caramba-Connect-macOS-arm64.dmg
#   windows → apps/caramba-client/build/dist/Caramba-Connect-Setup-x64.exe
#             + Caramba-Connect-Windows-x64-portable.zip
#   linux   → apps/caramba-client/build/dist/Caramba-Connect-Linux-x64.tar.gz
#   рядом с каждым артефактом — манифест версии Caramba-Connect-<platform>.json
#   (ci-manifest.sh): по нему панель и бот узнают о новой сборке
#   Единая схема имён Caramba-Connect-<OS>-<arch>.<ext> — контракт с
#   apps/caramba-installer (список ассетов) и /api/client/app/downloads в
#   панели: переименовав файл здесь, нужно переименовать его и там.
#   ios     → ассета нет (подписи и таргета Network Extension у проекта нет)
#
set -euo pipefail

# --- пути ---------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_ROOT="$(cd "${CLIENT_DIR}/../.." && pwd)"
CORE_DIR="${REPO_ROOT}/libs/caramba-core"
PLUGIN_DIR="${CLIENT_DIR}/packages/caramba_vpn"
DIST_DIR="${CLIENT_DIR}/build/dist"

# --- вывод ---------------------------------------------------------------------
# Метка времени в каждой строке — не украшение. Windows-прогон в GitHub Actions
# однажды простоял ДВА ЧАСА без единой строки и был отменён руками: последняя
# строка была «go version», следующая (flutter) так и не напечаталась, и по
# логу нельзя было сказать ни что висит, ни сколько. Теперь у каждого шага есть
# начало, конец и длительность.
ts()   { date '+%H:%M:%S'; }
log()  { echo "==> [$(ts)] $*"; }
warn() { echo "предупреждение: [$(ts)] $*" >&2; }
die()  { echo "ошибка: [$(ts)] $*" >&2; exit 1; }

# Ни один инструмент в этой цепочке не имеет права ждать ввода. В GitHub Actions
# stdin шага не закрыт, поэтому вопрос вида «Username for https://github.com:»
# или «Skip this patch? [y]» выглядит не как ошибка, а как тишина до отмены
# джоба. Отдельно душим Git Credential Manager: на windows-раннере он всплывает
# именно из git-вызовов внутри лаунчера Flutter.
export GIT_TERMINAL_PROMPT=0
export GIT_ASKPASS="${GIT_ASKPASS:-echo}"
export GCM_INTERACTIVE=never
export FLUTTER_SUPPRESS_ANALYTICS=true

# Лимиты шагов (секунды). Смысл не в точности, а в том, чтобы зависший шаг
# уронил прогон с внятным текстом, а не молчал до ручной отмены.
T_TOOLCHECK="${CARAMBA_T_TOOLCHECK:-600}"
T_CORE="${CARAMBA_T_CORE:-3600}"
T_FETCH="${CARAMBA_T_FETCH:-600}"
T_PUB="${CARAMBA_T_PUB:-900}"
T_BUILD="${CARAMBA_T_BUILD:-2700}"
T_ZIP="${CARAMBA_T_ZIP:-900}"

# GNU timeout есть в Git Bash и в coreutils на Linux; на macOS его нет вовсе
# (только gtimeout из brew, если поставлен). Отдельная засада — Windows: в PATH
# раньше msys-овского /usr/bin/timeout может оказаться C:\Windows\System32\
# timeout.exe, совершенно другая программа (пауза на N секунд), которая к тому же
# падает при перенаправлённом stdin. Поэтому берём только тот бинарь, который сам
# представляется coreutils, а если такого нет — работаем без лимитов, но честно
# об этом говорим.
TIMEOUT_BIN=""
pick_timeout() {
  if [[ "${CARAMBA_NO_TIMEOUT:-0}" == "1" ]]; then
    log "лимиты шагов отключены (CARAMBA_NO_TIMEOUT=1)"
    return 0
  fi
  local cand
  for cand in /usr/bin/timeout gtimeout timeout; do
    command -v "${cand}" >/dev/null 2>&1 || continue
    if "${cand}" --version </dev/null 2>/dev/null | head -1 | grep -qi coreutils; then
      TIMEOUT_BIN="${cand}"
      log "лимиты шагов через ${cand}"
      return 0
    fi
  done
  warn "GNU timeout не найден — шаги пойдут без ограничения по времени"
}

# Запуск внешнего шага: отметка времени до и после, stdin закрыт, вывод идёт
# ПРЯМО в лог прогона. Не в файл и не в $( ... ): именно подстановка с
# «2>/dev/null» превратила зависший flutter в два часа тишины.
_run() {
  local soft="$1" secs="$2" name="$3"; shift 3
  local rc=0 start end
  start="$(date +%s)"
  log "начало: ${name} (лимит ${secs}s)"
  if [[ -n "${TIMEOUT_BIN}" ]]; then
    "${TIMEOUT_BIN}" -k 30 "${secs}" "$@" </dev/null || rc=$?
  else
    "$@" </dev/null || rc=$?
  fi
  end="$(date +%s)"
  local took=$(( end - start ))
  if [[ "${rc}" -eq 124 || "${rc}" -eq 137 ]]; then
    if [[ "${soft}" == "soft" ]]; then
      warn "${name}: не уложился в ${secs}s и был убит — шаг завис"
      return 0
    fi
    die "${name}: не уложился в ${secs}s и был убит — шаг завис (вывод шага выше)"
  fi
  if [[ "${rc}" -ne 0 ]]; then
    if [[ "${soft}" == "soft" ]]; then
      warn "${name}: код возврата ${rc} за ${took}s — продолжаю (шаг диагностический)"
      return 0
    fi
    die "${name}: код возврата ${rc} (через ${took}s)"
  fi
  log "готово: ${name} (${took}s)"
}
run_step() { _run hard "$@"; }
# То же, но провал не роняет прогон: для диагностики, а не для сборки.
run_soft() { _run soft "$@"; }

# -s обязателен: без него du по каталогу (.app, xcframework) печатает строку на
# каждый вложенный файл, и лог прогона превращается в простыню.
size_of() { du -sh "$1" 2>/dev/null | cut -f1; }

need_cmd() { command -v "$1" >/dev/null 2>&1 || die "$1 не найден в PATH"; }

# Манифест версии платформы: Caramba-Connect-<platform>.json рядом с
# артефактом. Это единственный машиночитаемый ответ на вопрос «какая это
# версия»: панель отдаёт его приложению (GET /api/v2/app/version), бот
# рассылает по нему «вышла новая версия» один раз на сборку. Формат и
# проверки — в ci-manifest.sh, здесь только вызов.
write_manifest() {
  local platform="$1"; shift
  run_step "${T_ZIP}" "манифест версии (${platform})" \
    bash "${SCRIPT_DIR}/ci-manifest.sh" "${platform}" "${DIST_DIR}/Caramba-Connect-${platform}.json" "$@"
}

# --- стартовый лок Flutter -----------------------------------------------------
# Лаунчер flutter на POSIX-оболочке (bin/internal/shared.sh) берёт «стартовый
# лок» так:
#     _lock() { if hash flock;  then flock --nonblock --exclusive 7
#               elif hash shlock; then shlock -f "$1" -p $$
#               else mkdir "$1" 2>/dev/null; fi }
#     _wait_for_lock() { while ! _lock "$LOCK"; do sleep .1; done }
# В Git Bash нет ни flock, ни shlock — остаётся mkdir, а это НЕ лок, а просто
# каталог: если предыдущий процесс flutter убили (отменили прогон, сработал
# timeout), каталог bin/cache/.upgrade_lock остаётся на диске, и КАЖДЫЙ
# следующий flutter крутит `sleep .1` вечно. Единственный признак жизни —
# printf с \r в stderr. На раннере SDK ещё и кэшируется целиком
# (subosito/flutter-action, cache: true), поэтому один отменённый прогон
# отравляет кэш и вешает все последующие.
#
# В PATH на Windows лежат и `flutter` (sh-скрипт), и `flutter.bat`; Git Bash
# выбирает первый, то есть ровно этот код, — поэтому чистим лок сами.
flutter_root() {
  if [[ -n "${FLUTTER_ROOT:-}" && -d "${FLUTTER_ROOT}/bin/internal" ]]; then
    echo "${FLUTTER_ROOT}"
    return 0
  fi
  local bin resolved target guard=0 dir
  bin="$(command -v flutter 2>/dev/null || true)"
  [[ -n "${bin}" ]] || return 1
  # Не readlink -f: в macOS он появился только в 12.3, а flutter из brew лежит
  # за симлинком.
  resolved="${bin}"
  while [[ -L "${resolved}" && ${guard} -lt 10 ]]; do
    target="$(readlink "${resolved}")"
    case "${target}" in
      /*) resolved="${target}" ;;
      *)  resolved="$(cd "$(dirname "${resolved}")" && pwd -P)/${target}" ;;
    esac
    guard=$(( guard + 1 ))
  done
  dir="$(cd "$(dirname "${resolved}")" && pwd -P)"
  [[ -d "${dir}/internal" ]] || return 1
  (cd "${dir}/.." && pwd -P)
}

mtime_of() {
  stat -f %m "$1" 2>/dev/null || stat -c %Y "$1" 2>/dev/null || echo 0
}

clear_stale_flutter_lock() {
  local root lock age
  root="$(flutter_root || true)"
  if [[ -z "${root}" ]]; then
    warn "не удалось определить FLUTTER_ROOT — проверку .upgrade_lock пропускаю"
    return 0
  fi
  log "FLUTTER_ROOT=${root}"
  lock="${root}/bin/cache/.upgrade_lock"
  [[ -e "${lock}" ]] || return 0
  if [[ "${CI:-}" == "true" ]]; then
    warn "нашёл ${lock}: в CI параллельного flutter быть не может, это мусор от убитого процесса — удаляю"
    rm -rf -- "${lock}"
    return 0
  fi
  age=$(( $(date +%s) - $(mtime_of "${lock}") ))
  if [[ "${age}" -gt 600 ]]; then
    warn "нашёл протухший ${lock} (возраст ${age}s) — удаляю"
    rm -rf -- "${lock}"
  else
    warn "есть ${lock} (возраст ${age}s): похоже, рядом работает другой flutter — не трогаю, но следующий шаг может ждать"
  fi
}

# Кросс-сборки Flutter-раннера не существует: `flutter build windows` идёт
# только на Windows, `linux` — только на Linux. Падать честно и сразу лучше,
# чем на середине через десять минут работы Go-тулчейна.
need_host() {
  local want="$1" have
  have="$(go env GOOS 2>/dev/null || echo unknown)"
  [[ "${have}" == "${want}" ]] || die "цель ${TARGET} собирается только на ${want} (здесь ${have})"
}

TARGET="${1:-}"
case "${TARGET}" in
  macos|windows|linux|ios) ;;
  *)
    echo "использование: $0 <macos|windows|linux|ios>" >&2
    exit 2
    ;;
esac

need_cmd go
need_cmd flutter
# git нужен не только чекауту: mk-patched-deps.sh накладывает патчи mihomo
# через `git apply` (раньше — утилитой patch, см. комментарий там).
need_cmd git
pick_timeout
log "цель: ${TARGET}"
log "go: $(command -v go)"
log "$(go version)"
log "flutter: $(command -v flutter)"
clear_stale_flutter_lock
# Версия печатается НАПРЯМУЮ. Здесь раньше стояло
#     log "flutter $(flutter --version 2>/dev/null | head -1)"
# и именно эта строка два часа висела в Windows-прогоне: подстановка забирала
# stdout, 2>/dev/null съедал stderr (а там единственное сообщение зависшего
# лаунчера — «Waiting for another flutter command to release the startup
# lock...»), `| head -1` рвал канал нативному процессу, лимита времени не было.
# Диагностика не имеет права быть тише того, что она диагностирует.
run_soft "${T_TOOLCHECK}" "flutter --version" \
  flutter --suppress-analytics --no-version-check --version
mkdir -p "${DIST_DIR}"

# Ядро собирается ОДИН раз на прогон и переиспользуется из кэша, если workflow
# так решил. Проверка на -s, а не -e: пустой файл из оборванного кэша обязан
# считаться отсутствующим, иначе бандл уедет с нулевым ядром.
core_cached() {
  [[ "${CARAMBA_SKIP_CORE:-0}" == "1" && -s "$1" ]]
}

# --- macOS --------------------------------------------------------------------
build_macos() {
  need_host darwin
  local dylib="${CORE_DIR}/build/libcaramba_core.dylib"
  local vendored="${PLUGIN_DIR}/darwin/Libraries/libcaramba_core.dylib"

  if core_cached "${dylib}"; then
    log "ядро из кэша: ${dylib} ($(size_of "${dylib}"))"
  else
    run_step "${T_CORE}" "сборка universal dylib ядра (build-desktop-lib.sh macos)" \
      bash "${CORE_DIR}/scripts/build-desktop-lib.sh" macos
  fi
  [[ -s "${dylib}" ]] || die "ядро не собралось: ${dylib}"
  log "срезы ядра: $(lipo -archs "${dylib}")"

  # Вендоринг обязателен ДО pod install: podspec объявляет vendored_libraries
  # по пути Libraries/libcaramba_core.dylib, и без файла ядро просто не попадёт
  # в .app — сборка при этом останется зелёной.
  mkdir -p "$(dirname "${vendored}")"
  cp "${dylib}" "${vendored}"
  log "ядро → ${vendored} ($(size_of "${vendored}"))"

  cd "${CLIENT_DIR}"
  run_step "${T_PUB}" "flutter pub get" flutter pub get

  # macos-dmg = flutter build macos --release + hdiutil. Отдельной команды
  # hdiutil здесь нет намеренно: локальная проверка и CI обязаны выполнять
  # байт-в-байт одну последовательность (см. build.sh).
  run_step "${T_BUILD}" "flutter build macos --release + DMG (build.sh macos-dmg)" \
    bash "${SCRIPT_DIR}/build.sh" macos-dmg

  local app
  app="$(/usr/bin/find "${CLIENT_DIR}/build/macos/Build/Products/Release" -maxdepth 1 -name '*.app' -print -quit)"
  [[ -n "${app}" ]] || die "не найден .app после сборки"
  log "собрано: ${app} ($(size_of "${app}"))"

  # Имя бинаря берём из Info.plist, а не из имени папки .app: бандл теперь
  # «Caramba Connect.app» (PRODUCT_NAME с пробелом), и любая догадка по
  # basename рано или поздно разъедется с CFBundleExecutable, а lipo уронит
  # прогон на несуществующем файле.
  local exe
  exe="$(plutil -extract CFBundleExecutable raw -o - "${app}/Contents/Info.plist" 2>/dev/null || true)"
  [[ -n "${exe}" ]] || die "в ${app}/Contents/Info.plist нет CFBundleExecutable"
  [[ -f "${app}/Contents/MacOS/${exe}" ]] || die "нет бинаря Contents/MacOS/${exe}"
  log "бинарь бандла: Contents/MacOS/${exe}"

  # Ядро внутри бандла — единственная проверка, отличающая релиз от mock-сборки.
  if [[ -f "${app}/Contents/Frameworks/libcaramba_core.dylib" ]]; then
    log "ядро в бандле: Contents/Frameworks/libcaramba_core.dylib"
    lipo -archs "${app}/Contents/MacOS/${exe}" | sed 's/^/    срезы .app: /'
  elif [[ "${USE_NATIVE_VPN:-true}" == "false" ]]; then
    warn "ядра в бандле нет — но это осознанная mock-сборка (USE_NATIVE_VPN=false)"
  else
    die "в бандле нет Contents/Frameworks/libcaramba_core.dylib — уехал бы mock"
  fi

  local dmg="${CLIENT_DIR}/build/Caramba-Connect-macOS-arm64.dmg"
  [[ -s "${dmg}" ]] || die "DMG не собрался: ${dmg}"
  cp "${dmg}" "${DIST_DIR}/Caramba-Connect-macOS-arm64.dmg"
  log "артефакт: ${DIST_DIR}/Caramba-Connect-macOS-arm64.dmg ($(size_of "${dmg}"))"
  write_manifest macos "${DIST_DIR}/Caramba-Connect-macOS-arm64.dmg"
  # Подписи нет (сертификата Apple Developer у проекта нет) — Gatekeeper на
  # чужой машине потребует «Открыть всё равно». Пишем это в лог прогона, чтобы
  # факт не терялся между релизами.
  log "ВНИМАНИЕ: DMG НЕ подписан и не нотаризован"
}

# --- Linux --------------------------------------------------------------------
build_linux() {
  need_host linux
  local so="${CORE_DIR}/build/libcaramba_core.so"
  local vendored="${PLUGIN_DIR}/linux/lib/libcaramba_core.so"

  if core_cached "${so}"; then
    log "ядро из кэша: ${so} ($(size_of "${so}"))"
  else
    run_step "${T_CORE}" "сборка ядра (build-desktop-lib.sh linux)" \
      bash "${CORE_DIR}/scripts/build-desktop-lib.sh" linux
  fi
  [[ -s "${so}" ]] || die "ядро не собралось: ${so}"

  mkdir -p "$(dirname "${vendored}")"
  cp "${so}" "${vendored}"
  # Явная проверка, а не «надеемся»: CMake плагина обёрнут в if(EXISTS) и без
  # ядра собирает mock БЕЗ ошибки (см. packages/caramba_vpn/linux/CMakeLists.txt).
  [[ -s "${vendored}" ]] || die "ядро не довендорилось: ${vendored}"
  log "ядро → ${vendored} ($(size_of "${vendored}"))"

  cd "${CLIENT_DIR}"
  run_step "${T_PUB}" "flutter pub get" flutter pub get
  run_step "${T_BUILD}" "flutter build linux --release (build.sh)" \
    bash "${SCRIPT_DIR}/build.sh" linux --release

  local bundle="${CLIENT_DIR}/build/linux/x64/release/bundle"
  [[ -d "${bundle}" ]] || die "не найден бандл: ${bundle}"
  if [[ -f "${bundle}/lib/libcaramba_core.so" ]]; then
    log "ядро в бандле: lib/libcaramba_core.so"
    # `|| true`: под set -euo pipefail grep без совпадений роняет весь скрипт,
    # а ноль экспортов должен диагностироваться сообщением, а не молчанием.
    local exports
    exports="$(nm -D --defined-only "${bundle}/lib/libcaramba_core.so" 2>/dev/null | grep -c ' T Caramba' || true)"
    log "экспортов Caramba* в ядре: ${exports}"
    [[ "${exports}" -ge 1 ]] || die "ядро без экспортов Caramba* — собралась пустышка"
  elif [[ "${USE_NATIVE_VPN:-true}" == "false" ]]; then
    warn "ядра в бандле нет — но это осознанная mock-сборка (USE_NATIVE_VPN=false)"
  else
    die "в бандле нет lib/libcaramba_core.so — уехал бы mock"
  fi

  # Архив с корневой папкой caramba-connect/ (без пробела: путь идёт в Exec
  # .desktop-файла и в /opt), а не голый bundle/: владелец просил нормальное
  # имя папки установки на всех системах. Внутрь кладём install.sh, шаблон
  # .desktop и иконку — install.sh сам ставит всё в /opt/caramba-connect,
  # регистрирует обработчик caramba:// и выдаёт бинарю CAP_NET_ADMIN.
  local stage="${CLIENT_DIR}/build/linux-stage" root="caramba-connect"
  rm -rf "${stage}"
  mkdir -p "${stage}"
  cp -R "${bundle}" "${stage}/${root}"
  for extra in linux/install.sh linux/caramba-connect.desktop linux/icons/caramba-connect.png; do
    [[ -f "${CLIENT_DIR}/${extra}" ]] || die "нет ${extra} — архив без установщика уехал бы в релиз"
    cp "${CLIENT_DIR}/${extra}" "${stage}/${root}/"
  done
  chmod 755 "${stage}/${root}/install.sh" "${stage}/${root}/caramba_client"

  local out="${DIST_DIR}/Caramba-Connect-Linux-x64.tar.gz"
  rm -f "${out}"
  tar -czf "${out}" -C "${stage}" "${root}"
  log "артефакт: ${out} ($(size_of "${out}"))"
  write_manifest linux "${out}"
  log "ВНИМАНИЕ: пакета (deb/AppImage) нет; установка — sudo ./caramba-connect/install.sh (setcap cap_net_admin+ep)"
}

# --- Windows ------------------------------------------------------------------
# zip-архиватора, доступного везде, не существует: в Git Bash на образе
# windows-latest нет ни zip, ни (гарантированно) unzip, зато есть 7z и
# PowerShell. Перебираем то, что реально нашлось.
make_zip() {
  local src_dir="$1" out="$2"
  # 7z.exe и powershell — НАТИВНЫЕ Windows-программы: путь вида /d/a/repo/... из
  # Git Bash они не понимают. zip, если он есть, наоборот — msys-сборка и хочет
  # msys-путь. Поэтому держим обе формы.
  local out_native="${out}" src_native="${src_dir}"
  if command -v cygpath >/dev/null 2>&1; then
    out_native="$(cygpath -w "${out}")"
    src_native="$(cygpath -w "${src_dir}")"
  fi
  rm -f "${out}"
  # Флаги везде подобраны так, чтобы архиватор физически не мог задать вопрос:
  # 7z -y (на всё «да») + -bso0/-bsp0 (без простыни файлов и без прогресс-бара,
  # но stderr остаётся), zip -q, powershell -NonInteractive + -Force.
  # Вывод при этом не уходит в файл: зависший шаг должен быть виден по времени
  # начала и концу, а не по отсутствию строк.
  if command -v 7z >/dev/null 2>&1; then
    ( cd "${src_dir}" && run_step "${T_ZIP}" "7z a -tzip → ${out}" \
        7z a -tzip -mx=7 -y -bso0 -bsp0 "${out_native}" ./* )
  elif command -v zip >/dev/null 2>&1; then
    ( cd "${src_dir}" && run_step "${T_ZIP}" "zip -qr → ${out}" zip -qr "${out}" . )
  elif command -v powershell >/dev/null 2>&1; then
    run_step "${T_ZIP}" "Compress-Archive → ${out}" \
      powershell -NoLogo -NoProfile -NonInteractive -Command \
      "\$ProgressPreference='SilentlyContinue'; Compress-Archive -Path '${src_native}\\*' -DestinationPath '${out_native}' -Force"
  else
    die "нечем упаковать zip (нет 7z, zip и powershell)"
  fi
  [[ -s "${out}" ]] || die "zip не создался: ${out}"
}

# fetch-wintun.sh распаковывает архив через unzip, а без него — через python3.
# На windows-раннере Python зовётся python.exe, и без этого шима запасной путь
# скрипта не срабатывает, хотя интерпретатор на машине есть.
ensure_python3_shim() {
  command -v unzip >/dev/null 2>&1 && return 0
  command -v python3 >/dev/null 2>&1 && return 0
  command -v python >/dev/null 2>&1 || return 0
  python -c 'import sys; sys.exit(0 if sys.version_info[0] == 3 else 1)' </dev/null >/dev/null 2>&1 || return 0
  local shim_dir="${CLIENT_DIR}/build/ci-bin"
  mkdir -p "${shim_dir}"
  printf '#!/usr/bin/env bash\nexec python "$@"\n' > "${shim_dir}/python3"
  chmod +x "${shim_dir}/python3"
  export PATH="${shim_dir}:${PATH}"
  log "python3 → шим на python (нужен fetch-wintun.sh для распаковки)"
}

# Единая схема имён ассетов (та же у бота, мини-аппа и инсталлятора панели):
# Caramba-Connect-<Платформа>-<арх>.<ext>. Портативный ZIP несёт внутри
# корневую папку «Caramba Connect/» — распаковка даёт человеческое имя, а не
# россыпь DLL рядом с архивом.
WIN_SETUP_NAME="Caramba-Connect-Setup-x64.exe"
WIN_ZIP_NAME="Caramba-Connect-Windows-x64-portable.zip"
WIN_STAGING_ROOT="Caramba Connect"

# ISCC.exe на образе windows-latest лежит в фиксированном месте и в PATH не
# добавлен. Локально путь можно задать через CARAMBA_ISCC.
find_iscc() {
  if [[ -n "${CARAMBA_ISCC:-}" ]]; then
    [[ -f "${CARAMBA_ISCC}" ]] || die "CARAMBA_ISCC указывает на несуществующий файл: ${CARAMBA_ISCC}"
    echo "${CARAMBA_ISCC}"
    return 0
  fi
  local cand
  for cand in "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" "/c/Program Files/Inno Setup 6/ISCC.exe"; do
    if [[ -f "${cand}" ]]; then
      echo "${cand}"
      return 0
    fi
  done
  if command -v ISCC.exe >/dev/null 2>&1; then
    command -v ISCC.exe
    return 0
  fi
  if command -v iscc >/dev/null 2>&1; then
    command -v iscc
    return 0
  fi
  return 1
}

# flutter build windows НЕ кладёт в бандл рантайм Visual C++ (msvcp140.dll,
# vcruntime140.dll, vcruntime140_1.dll), а без него на чистой Windows exe
# падает с «msvcp140.dll не найден» ещё до первого кадра. Документация Flutter
# велит класть эти три файла рядом с exe (вариант «application-local»); берём
# их из Visual Studio на раннере — она там есть всегда, иначе не собрался бы
# и сам Flutter.
copy_vc_runtime() {
  local dst="$1" name
  local missing=()
  for name in msvcp140.dll vcruntime140.dll vcruntime140_1.dll; do
    [[ -f "${dst}/${name}" ]] || missing+=("${name}")
  done
  if [[ ${#missing[@]} -eq 0 ]]; then
    log "VC++-рантайм уже в бандле"
    return 0
  fi
  local base="" crt_dir=""
  if [[ -n "${VCToolsRedistDir:-}" ]]; then
    base="$(cygpath -u "${VCToolsRedistDir}")"
  else
    local vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
    if [[ -x "${vswhere}" ]]; then
      local vs_root
      vs_root="$("${vswhere}" -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath </dev/null 2>/dev/null | tr -d '\r' | head -1 || true)"
      # Идентификатор компонента мог смениться между версиями VS — тогда
      # берём просто последнюю установленную студию.
      [[ -n "${vs_root}" ]] || vs_root="$("${vswhere}" -latest -products '*' -property installationPath </dev/null 2>/dev/null | tr -d '\r' | head -1 || true)"
      [[ -n "${vs_root}" ]] && base="$(cygpath -u "${vs_root}")/VC/Redist/MSVC"
    fi
  fi
  if [[ -n "${base}" && -d "${base}" ]]; then
    # Несколько версий тулсета могут лежать рядом (14.3x, 14.4x): берём самую
    # новую; DLL обратно совместимы в пределах VC14x.
    # onecore/ и debug_nonredist/ лежат рядом — это не тот рантайм.
    crt_dir="$(find "${base}" -type f -name msvcp140.dll -path '*x64*' -path '*.CRT*' -not -path '*onecore*' -not -path '*debug*' 2>/dev/null | sort -V | tail -1 || true)"
    [[ -n "${crt_dir}" ]] && crt_dir="$(dirname "${crt_dir}")"
  fi
  if [[ -n "${crt_dir}" ]]; then
    for name in "${missing[@]}"; do
      [[ -f "${crt_dir}/${name}" ]] && cp "${crt_dir}/${name}" "${dst}/${name}"
    done
    log "VC++-рантайм из ${crt_dir} → ${dst}"
  fi
  local still=()
  for name in "${missing[@]}"; do
    [[ -f "${dst}/${name}" ]] || still+=("${name}")
  done
  if [[ ${#still[@]} -gt 0 ]]; then
    if [[ "${CARAMBA_ALLOW_NO_VCRT:-0}" == "1" ]]; then
      warn "в бандле нет ${still[*]} — на чистой Windows понадобится vc_redist.x64.exe (CARAMBA_ALLOW_NO_VCRT=1)"
    else
      die "не нашёл VC++-рантайм (${still[*]}) в Visual Studio раннера; задайте VCToolsRedistDir или CARAMBA_ALLOW_NO_VCRT=1"
    fi
  fi
}

# Проверка, что ZIP несёт корневую папку: make_zip пакует «всё из каталога», и
# если staging собрали не тем уровнем, архив уедет россыпью файлов, а
# заметит это только пользователь после распаковки.
check_zip_root() {
  local zip="$1" py
  for py in python3 python; do
    command -v "${py}" >/dev/null 2>&1 || continue
    if "${py}" -c 'import sys, zipfile; names = {n.replace("\\", "/") for n in zipfile.ZipFile(sys.argv[1]).namelist()}; sys.exit(0 if sys.argv[2] in names else 1)' \
        "${zip}" "${WIN_STAGING_ROOT}/caramba_client.exe" </dev/null; then
      log "в ZIP есть ${WIN_STAGING_ROOT}/caramba_client.exe"
      return 0
    fi
    die "в ZIP нет ${WIN_STAGING_ROOT}/caramba_client.exe — архив без корневой папки"
  done
  warn "нет python — содержимое ZIP не проверено"
}

# Setup.exe через Inno Setup 6 (скрипт windows/installer/caramba-connect.iss).
# Версию берём из pubspec.yaml, чтобы запись в «Приложения и возможности»
# совпадала с тегом релиза client-v<pubspec>.
build_windows_installer() {
  local staging="$1" out_dir="$2"
  local iscc
  iscc="$(find_iscc)" || die "ISCC.exe (Inno Setup 6) не найден: на windows-latest это C:\\Program Files (x86)\\Inno Setup 6\\ISCC.exe, локально задайте CARAMBA_ISCC"
  log "Inno Setup: ${iscc}"
  local version build
  version="$(sed -n 's/^version:[[:space:]]*\([0-9][0-9.]*\)+\([0-9][0-9]*\).*/\1/p' "${CLIENT_DIR}/pubspec.yaml")"
  build="$(sed -n 's/^version:[[:space:]]*\([0-9][0-9.]*\)+\([0-9][0-9]*\).*/\2/p' "${CLIENT_DIR}/pubspec.yaml")"
  [[ -n "${version}" && -n "${build}" ]] || die "не разобрал version: в pubspec.yaml (ожидаю X.Y.Z+N)"
  local iss="${CLIENT_DIR}/windows/installer/caramba-connect.iss"
  [[ -f "${iss}" ]] || die "нет скрипта инсталлятора: ${iss}"
  # ISCC — нативная программа: пути отдаём в форме C:\..., см. make_zip.
  local staging_native out_native iss_native
  staging_native="$(cygpath -w "${staging}")"
  out_native="$(cygpath -w "${out_dir}")"
  iss_native="$(cygpath -w "${iss}")"
  rm -f "${out_dir}/${WIN_SETUP_NAME}"
  # MSYS_NO_PATHCONV / MSYS2_ARG_CONV_EXCL: Git Bash при запуске нативной
  # программы переписывает аргументы вида /DFoo=bar как пути (D:\Foo=bar), и
  # ISCC получает мусор вместо определений. /Qp — без простыни, но с прогрессом.
  MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' run_step "${T_ZIP}" "ISCC → ${WIN_SETUP_NAME}" \
    "${iscc}" /Qp "/DAppVersion=${version}" "/DAppBuild=${build}" \
    "/DSourceDir=${staging_native}" "/O${out_native}" "${iss_native}"
  [[ -s "${out_dir}/${WIN_SETUP_NAME}" ]] || die "инсталлятор не создался: ${out_dir}/${WIN_SETUP_NAME}"
}

build_windows() {
  need_host windows
  # Утилита patch больше не нужна: mk-patched-deps.sh перешёл на `git apply`
  # (её в Git Bash могло не быть вовсе, а при неудачном применении она уходит в
  # диалог и читает stdin — в CI это тишина до отмены прогона). Проверка на git
  # стоит выше, на общем пути.
  local dll="${CORE_DIR}/build/libcaramba_core.dll"
  local lib_dir="${PLUGIN_DIR}/windows/lib"

  if core_cached "${dll}"; then
    log "ядро из кэша: ${dll} ($(size_of "${dll}"))"
    mkdir -p "${lib_dir}"
    cp "${dll}" "${lib_dir}/libcaramba_core.dll"
  else
    run_step "${T_CORE}" "сборка ядра (build-windows-lib.sh; он же вендорит DLL в плагин)" \
      bash "${CORE_DIR}/scripts/build-windows-lib.sh"
  fi

  ensure_python3_shim
  run_step "${T_FETCH}" "wintun.dll (fetch-wintun.sh, SHA-256 зашита в скрипте)" \
    bash "${CORE_DIR}/scripts/fetch-wintun.sh"

  # CMake плагина объявляет ОБА файла в caramba_vpn_bundled_libraries без
  # if(EXISTS): отсутствие любого валит конфигурацию, но с невнятным текстом.
  [[ -s "${lib_dir}/libcaramba_core.dll" ]] || die "нет ${lib_dir}/libcaramba_core.dll"
  [[ -s "${lib_dir}/wintun.dll" ]]          || die "нет ${lib_dir}/wintun.dll"
  log "ядро → ${lib_dir}/libcaramba_core.dll ($(size_of "${lib_dir}/libcaramba_core.dll"))"
  log "wintun → ${lib_dir}/wintun.dll ($(size_of "${lib_dir}/wintun.dll"))"

  cd "${CLIENT_DIR}"
  run_step "${T_PUB}" "flutter pub get" flutter pub get
  run_step "${T_BUILD}" "flutter build windows --release (build.sh)" \
    bash "${SCRIPT_DIR}/build.sh" windows --release

  local rel="${CLIENT_DIR}/build/windows/x64/runner/Release"
  [[ -d "${rel}" ]] || die "не найден каталог сборки: ${rel}"
  [[ -f "${rel}/caramba_client.exe" ]] || die "нет caramba_client.exe в ${rel}"
  for need in libcaramba_core.dll wintun.dll; do
    if [[ ! -f "${rel}/${need}" ]]; then
      if [[ "${USE_NATIVE_VPN:-true}" == "false" ]]; then
        warn "${need} не в Release — mock-сборка (USE_NATIVE_VPN=false)"
      else
        die "в Release нет ${need} — уехал бы бандл без туннеля"
      fi
    fi
  done
  log "содержимое Release проверено (exe + ядро + wintun)"

  # Staging: копия Release под именем «Caramba Connect/». Из него и ZIP (с
  # корневой папкой), и инсталлятор — оба ассета гарантированно из одних и тех
  # же файлов. Release не трогаем: пусть остаётся тем, что выдал Flutter.
  local staging_parent="${CLIENT_DIR}/build/dist-staging/windows"
  local staging="${staging_parent}/${WIN_STAGING_ROOT}"
  rm -rf "${staging_parent}"
  mkdir -p "${staging}" "${DIST_DIR}"
  cp -R "${rel}/." "${staging}/"
  copy_vc_runtime "${staging}"
  log "staging: ${staging} ($(size_of "${staging}"))"

  local zip_out="${DIST_DIR}/${WIN_ZIP_NAME}"
  log "упаковываю ${staging_parent} → ${zip_out}"
  make_zip "${staging_parent}" "${zip_out}"
  check_zip_root "${zip_out}"
  log "артефакт: ${zip_out} ($(size_of "${zip_out}"))"

  build_windows_installer "${staging}" "${DIST_DIR}"
  log "артефакт: ${DIST_DIR}/${WIN_SETUP_NAME} ($(size_of "${DIST_DIR}/${WIN_SETUP_NAME}"))"
  # Главный файл — инсталлятор (по нему панель строит download_url), ZIP идёт
  # вторым в списке files.
  write_manifest windows "${DIST_DIR}/${WIN_SETUP_NAME}" "${zip_out}"
  # Подписи кода нет: SmartScreen предупредит и про Setup.exe, и про exe из
  # ZIP. Права администратора exe запрашивает сам (runner.exe.manifest,
  # requireAdministrator) — wintun без них адаптер не создаст.
  log "ВНИМАНИЕ: Setup.exe и exe НЕ подписаны (SmartScreen); exe запрашивает права администратора через манифест"
}

# --- iOS (только проверка компиляции) -----------------------------------------
# Ассета нет и не может быть: сертификата Apple Developer у проекта нет, таргет
# Network Extension не создан. Смысл шага — не дать Swift-мосту разъехаться с
# настоящим биндингом gomobile: без ядра сборка с USE_NATIVE_VPN=true обязана
# падать (#error через podspec), а с ядром — линковаться с mihomo внутри.
check_ios() {
  need_host darwin
  local xcf_src="${CORE_DIR}/build/ios/exarobot.xcframework"
  local xcf_dst="${PLUGIN_DIR}/darwin/Frameworks/ios/exarobot.xcframework"

  # gomobile ставится в GOBIN, которого нет в PATH неинтерактивной оболочки.
  # Версия берётся из go.mod ядра: gobind генерирует код по внутреннему API
  # golang.org/x/mobile, и более новый gomobile падает на несовпадении.
  local gobin
  gobin="$(go env GOBIN)"
  [[ -n "${gobin}" ]] || gobin="$(go env GOPATH)/bin"
  export PATH="${gobin}:${PATH}"

  if [[ "${CARAMBA_SKIP_CORE:-0}" == "1" && -s "${xcf_src}/Info.plist" ]]; then
    log "xcframework из кэша: ${xcf_src}"
    mkdir -p "$(dirname "${xcf_dst}")"
    rm -rf "${xcf_dst}"
    cp -R "${xcf_src}" "${xcf_dst}"
  else
    if ! command -v gomobile >/dev/null 2>&1; then
      local xmobile="${CARAMBA_GOMOBILE_VERSION:-}"
      if [[ -z "${xmobile}" ]]; then
        xmobile="$(cd "${CORE_DIR}" && go list -m -f '{{.Version}}' golang.org/x/mobile 2>/dev/null || true)"
      fi
      [[ -n "${xmobile}" ]] || xmobile="latest"
      run_step "${T_FETCH}" "go install gomobile@${xmobile}" \
        go install "golang.org/x/mobile/cmd/gomobile@${xmobile}"
      run_step "${T_FETCH}" "go install gobind@${xmobile}" \
        go install "golang.org/x/mobile/cmd/gobind@${xmobile}"
      run_step "${T_FETCH}" "gomobile init" gomobile init
    fi
    run_step "${T_CORE}" "gomobile bind ios (build-mobile.sh ios; он же вендорит xcframework)" \
      bash "${CORE_DIR}/scripts/build-mobile.sh" ios
  fi
  [[ -d "${xcf_dst}" ]] || die "xcframework не довендорился: ${xcf_dst}"

  cd "${CLIENT_DIR}"
  run_step "${T_PUB}" "flutter pub get" flutter pub get
  # Порядок критичен: podspec решает mock/native во время pod install по наличию
  # этого xcframework и по USE_NATIVE_VPN из окружения (build.sh его экспортирует).
  # ios/Pods gitignored, на чистом чекауте flutter сам сделает pod install.
  run_step "${T_BUILD}" "flutter build ios (симулятор, без подписи)" \
    bash "${SCRIPT_DIR}/build.sh" ios --simulator --debug --no-codesign

  # Exarobot линкуется СТАТИЧЕСКИ: отдельного Exarobot.framework в Runner.app не
  # будет, символы ядра лежат внутри бинаря плагина.
  local bin="${CLIENT_DIR}/build/ios/iphonesimulator/Runner.app/Frameworks/caramba_vpn.framework/caramba_vpn"
  [[ -f "${bin}" ]] || die "не найден бинарь плагина: ${bin}"
  local mihomo_syms entry
  mihomo_syms="$(nm -a "${bin}" 2>/dev/null | grep -c 'metacubex/mihomo' || true)"
  entry="$(nm -gU "${bin}" 2>/dev/null | grep -c '_CarambaMobileNewClient' || true)"
  log "символов mihomo в бинаре плагина: ${mihomo_syms}; точек входа CarambaMobileNewClient: ${entry}"
  if [[ "${USE_NATIVE_VPN:-true}" != "false" ]]; then
    [[ "${mihomo_syms}" -gt 1000 ]] || die "ядра mihomo нет в бинаре — собралась пустышка"
    [[ "${entry}" -ge 1 ]] || die "нет символа CarambaMobileNewClient — биндинг не залинкован"
  fi
  log "iOS: компиляция и линковка с настоящим ядром подтверждены (симулятор)"
  log "ВНИМАНИЕ: ассета нет — ни сертификата Apple, ни таргета Network Extension"
}

case "${TARGET}" in
  macos)   build_macos ;;
  linux)   build_linux ;;
  windows) build_windows ;;
  ios)     check_ios ;;
esac

log "готово: ${TARGET}"
