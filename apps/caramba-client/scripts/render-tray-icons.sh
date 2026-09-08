#!/usr/bin/env bash
#
# render-tray-icons.sh — рендерит пять состояний иконки трея (idle, busy,
# connected, blocked, error) из ОДНОГО SVG-источника в mac/win/linux форматы.
#
# Зачем один источник и --stylesheet, а не пять отдельных SVG: у источника
# ровно один визуальный контракт (силуэт станции), и пять состояний — это
# просто разная видимость четырёх слоёв (outline/fill/dot/cross) поверх него.
# Так замена плейсхолдера на финальный tray-template.svg победителя J1 не
# требует трогать этот скрипт вообще — только сам SVG, если он держит тот же
# viewBox и те же четыре класса.
#
# Зачем recolor через альфа-канал, а не -opaque/-fill на самом PNG: источник
# рисуется чёрным (mac этого и требует для isTemplate), а край антиалиасинга
# даёт полупрозрачный НЕ чисто чёрный пиксель — -opaque black его пропустит и
# оставит серую кайму. Заливка сплошным цветом с копированием альфы исходного
# рендера красит фигуру целиком, включая сглаженные края, без этого артефакта.
#
# Использование:
#   bash scripts/render-tray-icons.sh [--src <path/to/tray-template.svg>]
#
# Требует: rsvg-convert, magick (ImageMagick 7).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
SRC="${CLIENT_DIR}/assets/tray/src/tray-template.svg"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --src)
      SRC="$2"
      shift 2
      ;;
    *)
      echo "ошибка: неизвестный аргумент: $1" >&2
      exit 1
      ;;
  esac
done

[[ -f "$SRC" ]] || { echo "ошибка: SVG-источник не найден: $SRC" >&2; exit 1; }
command -v rsvg-convert >/dev/null || { echo "ошибка: rsvg-convert не найден в PATH" >&2; exit 1; }
command -v magick >/dev/null || { echo "ошибка: magick (ImageMagick 7) не найден в PATH" >&2; exit 1; }

MAC_DIR="${CLIENT_DIR}/assets/tray/mac"
WIN_DIR="${CLIENT_DIR}/assets/tray/win"
LINUX_DIR="${CLIENT_DIR}/assets/tray/linux"
mkdir -p "$MAC_DIR" "$WIN_DIR" "$LINUX_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# Состояние → цвет заливки (Windows/Linux; mac всегда чёрный — isTemplate) и
# какие из четырёх классов слоя скрыть (outline/fill/dot/cross). Обычные
# функции вместо `declare -A`, потому что системный /bin/bash на macOS — 3.2
# (лицензия Apple держит его без ассоциативных массивов бэшем 4+; смена на
# другой bash здесь не входит в задачу), а `env bash` в шебанге резолвится
# именно в него.
state_color() {
  case "$1" in
    idle) echo "#A0A0A0" ;;
    busy) echo "#FF9F0A" ;;
    connected) echo "#30D158" ;;
    blocked) echo "#FF9F0A" ;;
    error) echo "#FF453A" ;;
    *) echo "ошибка: неизвестное состояние: $1" >&2; exit 1 ;;
  esac
}

# outline — контур станции; fill — заливка станции (со штрихами-вырезом);
# dot — точка 4px «у носа»; cross — крест 6px поверх контура.
state_hide() {
  case "$1" in
    idle) echo "fill,dot,cross" ;;
    busy) echo "fill,cross" ;;
    connected) echo "outline,dot,cross" ;;
    blocked) echo "outline,cross" ;;
    error) echo "fill,dot" ;;
    *) echo "ошибка: неизвестное состояние: $1" >&2; exit 1 ;;
  esac
}

# render_black <state> <size> <out_png> — рендерит состояние чёрным на
# прозрачном фоне в квадрат size×size px.
render_black() {
  local state="$1" size="$2" out="$3"
  local css="${TMP_DIR}/${state}.css"
  local hide
  hide="$(state_hide "$state")"
  : >"$css"
  IFS=',' read -ra classes <<<"$hide"
  for cls in "${classes[@]}"; do
    [[ -z "$cls" ]] && continue
    printf '.%s{display:none}\n' "$cls" >>"$css"
  done
  rsvg-convert -w "$size" -h "$size" --stylesheet="$css" -o "$out" "$SRC"
}

# recolor <in_png> <hex_color> <out_png> — красит непрозрачные пиксели
# in_png в hex_color, сохраняя исходную альфу (включая антиалиасинг краёв).
recolor() {
  local in="$1" color="$2" out="$3"
  local w h alpha solid
  w="$(magick identify -format '%w' "$in")"
  h="$(magick identify -format '%h' "$in")"
  alpha="${TMP_DIR}/$(basename "$in").alpha.png"
  solid="${TMP_DIR}/$(basename "$in").solid.png"
  magick "$in" -alpha extract "$alpha"
  magick -size "${w}x${h}" "xc:${color}" "$solid"
  magick "$solid" "$alpha" -alpha off -compose CopyOpacity -composite "$out"
}

for state in idle busy connected blocked error; do
  # macOS: 44×44, чёрный на прозрачном, для setIcon(..., isTemplate: true)
  render_black "$state" 44 "${MAC_DIR}/${state}.png"

  color="$(state_color "$state")"

  # Linux: 22×22, окрашенный
  black22="${TMP_DIR}/${state}-22-black.png"
  render_black "$state" 22 "$black22"
  recolor "$black22" "$color" "${LINUX_DIR}/${state}.png"

  # Windows: .ico из 16/24/32, окрашенный
  ico_inputs=()
  for size in 16 24 32; do
    black="${TMP_DIR}/${state}-${size}-black.png"
    colored="${TMP_DIR}/${state}-${size}-color.png"
    render_black "$state" "$size" "$black"
    recolor "$black" "$color" "$colored"
    ico_inputs+=("$colored")
  done
  magick "${ico_inputs[@]}" "${WIN_DIR}/${state}.ico"

  echo "==> ${state}: mac/${state}.png, win/${state}.ico, linux/${state}.png"
done

echo "готово: assets/tray/{mac,win,linux}/*"
