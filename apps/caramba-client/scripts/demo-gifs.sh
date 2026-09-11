#!/usr/bin/env bash
# Собирает демо-анимации из кадров, которые рисуют headless-тесты
# test/demo/*_demo_test.dart (запуск: flutter test test/demo
# --dart-define=CARAMBA_DEMO=1). На входе build/demo/<scene>/NNN.png +
# frames.json ([{file, delay_ms}]), на выходе build/demo/<scene>.gif
# (ширина $WIDTH, по умолчанию 540) и, если есть ffmpeg, build/demo/<scene>.mp4
# (H.264, без звука).
#
# Только magick (ImageMagick 7): ffmpeg на Маке не стоит, а GIF Telegram
# показывает как анимацию сам.
set -euo pipefail

cd "$(dirname "$0")/.."
WIDTH="${WIDTH:-540}"
LIMIT_BYTES=$((6 * 1024 * 1024))

command -v magick >/dev/null || { echo "нужен magick (brew install imagemagick)" >&2; exit 1; }

shopt -s nullglob
scenes=()
if [[ $# -gt 0 ]]; then
  scenes=("$@")
else
  for d in build/demo/*/; do
    s="$(basename "$d")"
    [[ "$s" == _* ]] && continue
    [[ -f "$d/frames.json" ]] && scenes+=("$s")
  done
fi
[[ ${#scenes[@]} -gt 0 ]] || { echo "кадров нет: сначала flutter test test/demo --dart-define=CARAMBA_DEMO=1" >&2; exit 1; }

for scene in "${scenes[@]}"; do
  dir="build/demo/$scene"
  frames="$dir/frames.json"
  [[ -f "$frames" ]] || { echo "[$scene] нет frames.json, пропуск" >&2; continue; }

  args=()
  count=0
  while IFS=$'\t' read -r file delay; do
    cs=$(( (delay + 5) / 10 ))
    (( cs < 2 )) && cs=2
    args+=(-delay "$cs" "$dir/$file")
    count=$((count + 1))
  done < <(jq -r '.[] | [.file, .delay_ms] | @tsv' "$frames")

  out="build/demo/$scene.gif"
  magick "${args[@]}" -resize "${WIDTH}x" -coalesce -layers optimize-frame -loop 0 "$out"
  size=$(stat -f%z "$out" 2>/dev/null || stat -c%s "$out")
  # Слишком большой — пережимаем с меньшей палитрой, потом уже ширину.
  if (( size > LIMIT_BYTES )); then
    magick "${args[@]}" -resize "${WIDTH}x" -coalesce -colors 128 -layers optimize-frame -loop 0 "$out"
    size=$(stat -f%z "$out" 2>/dev/null || stat -c%s "$out")
  fi
  if (( size > LIMIT_BYTES )); then
    magick "${args[@]}" -resize "$(( WIDTH * 3 / 4 ))x" -coalesce -colors 128 -layers optimize-frame -loop 0 "$out"
    size=$(stat -f%z "$out" 2>/dev/null || stat -c%s "$out")
  fi
  printf '%-10s %3d кадров  %6.2f МБ  %s\n' "$scene" "$count" "$(echo "$size / 1048576" | bc -l)" "$out"

  if command -v ffmpeg >/dev/null; then
    concat="$dir/concat.txt"
    : > "$concat"
    while IFS=$'\t' read -r file delay; do
      printf "file '%s'\nduration %s\n" "$file" "$(python3 -c "print(f'{$delay/1000:.3f}')")" >> "$concat"
    done < <(jq -r '.[] | [.file, .delay_ms] | @tsv' "$frames")
    last="$(jq -r '.[-1].file' "$frames")"
    printf "file '%s'\n" "$last" >> "$concat"
    ffmpeg -y -loglevel error -f concat -safe 0 -i "$concat" \
      -vf "scale=${WIDTH}:-2:flags=lanczos,format=yuv420p" -c:v libx264 -pix_fmt yuv420p -an \
      "build/demo/$scene.mp4"
    echo "           mp4: build/demo/$scene.mp4"
  fi
done
