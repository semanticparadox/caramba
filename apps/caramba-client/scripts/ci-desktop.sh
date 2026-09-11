#!/usr/bin/env bash
#
# ci-desktop.sh — единственный путь сборки десктопных артефактов Caramba Connect
# (macOS DMG, Windows ZIP, Linux tar.gz) плюс проверка компиляции iOS.
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
#   bash apps/caramba-client/scripts/ci-desktop.sh windows   # → ZIP (только на Windows)
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
#
# Артефакты (все gitignored, см. .gitignore:51 «build/»):
#   macos   → apps/caramba-client/build/dist/caramba-connect-macos-arm64.dmg
#   windows → apps/caramba-client/build/dist/caramba-connect-windows-x64.zip
#   linux   → apps/caramba-client/build/dist/caramba-connect-linux-x64.tar.gz
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

  # Имя бинаря берём из Info.plist, а не из имени папки .app: EXECUTABLE_NAME
  # развязан с PRODUCT_NAME (внутри caramba_client.app лежит «Caramba Connect»),
  # и прежний basename нашёл бы несуществующий файл, а lipo уронил бы прогон.
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

  local dmg="${CLIENT_DIR}/build/caramba-connect-macos-arm64.dmg"
  [[ -s "${dmg}" ]] || die "DMG не собрался: ${dmg}"
  cp "${dmg}" "${DIST_DIR}/caramba-connect-macos-arm64.dmg"
  log "артефакт: ${DIST_DIR}/caramba-connect-macos-arm64.dmg ($(size_of "${dmg}"))"
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

  local out="${DIST_DIR}/caramba-connect-linux-x64.tar.gz"
  rm -f "${out}"
  tar -czf "${out}" -C "${CLIENT_DIR}/build/linux/x64/release" bundle
  log "артефакт: ${out} ($(size_of "${out}"))"
  # .desktop с обработчиком схемы caramba:// в архив не кладётся: путь Exec
  # знает только упаковщик (deb/AppImage). Шаблон — linux/caramba-connect.desktop.
  log "ВНИМАНИЕ: пакета (deb/AppImage) нет, запуск туннеля требует CAP_NET_ADMIN"
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

  local out="${DIST_DIR}/caramba-connect-windows-x64.zip"
  log "упаковываю ${rel} → ${out}"
  make_zip "${rel}" "${out}"
  log "артефакт: ${out} ($(size_of "${out}"))"
  # Ни подписи кода, ни манифеста с requireAdministrator: SmartScreen будет
  # ругаться, а wintun без прав администратора адаптер не создаст.
  log "ВНИМАНИЕ: exe НЕ подписан; wintun требует запуска от администратора"
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
