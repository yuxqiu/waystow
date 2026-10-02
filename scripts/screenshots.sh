#!/usr/bin/env bash
# Re-render the README screenshots in an isolated headless sway (scale 2).
#   nix develop --command scripts/screenshots.sh [output-dir]   (default: assets/)
set -euo pipefail
cd "$(dirname "$0")/.."
out=$(realpath -m "${1:-assets}")
mkdir -p "$out"

if [[ -z ${WAYSTOW_TEST_ROOT:-} ]]; then
  cargo build -q -p waystow -p dnd-probe
  for theme in dark light; do
    WAYSTOW_SHOT_THEME=$theme WAYSTOW_TEST_SCALE=2 scripts/headless.sh "$0" "$out"
  done
  ls -l "$out"
  exit
fi

theme=$WAYSTOW_SHOT_THEME
W=$PWD/target/debug/waystow
P=$PWD/target/debug/dnd-probe
F=$HOME/Pictures/Holiday
mkdir -p "$F" "$HOME/Documents" "$XDG_CONFIG_HOME/waystow"
printf '[ui]\ntheme = "%s"\naccent = "#7c6cf2"\n' "$theme" >"$XDG_CONFIG_HOME/waystow/config.toml"

# A wallpaper, and some believable files.
if [[ $theme == dark ]]; then wall='#1e2233-#5b4b8a'; else wall='#dfe7f5-#b9c8ec'; fi
magick -size 2560x1600 gradient:"$wall" -swirl 40 "$WAYSTOW_TEST_ROOT/wall.png"
swaymsg -s "$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock)" output '*' bg "$WAYSTOW_TEST_ROOT/wall.png" fill >/dev/null
magick -size 1200x800 gradient:'#ff9a5a-#7c4dff' -swirl 60 "$F/sunset-over-lisbon.jpg"
magick -size 900x900 plasma:'#00c6ff-#ff5f6d' -blur 0x2 "$F/tiles.png"
magick -size 1000x700 gradient:'#43e97b-#38f9d7' -swirl 120 "$F/garden.webp"
ffmpeg -v quiet -f lavfi -i testsrc2=duration=4:size=640x360:rate=25 -pix_fmt yuv420p "$F/clip.mp4"
magick -size 595x842 xc:white -fill '#222' -pointsize 46 -annotate +60+130 'Quarterly report' \
  -fill '#7c6cf2' -draw 'rectangle 60,200 535,230' "$HOME/Documents/report.pdf"

"$W" daemon >"$WAYSTOW_TEST_ROOT/daemon.log" 2>&1 &
D=$!
for _ in $(seq 100); do "$W" status >/dev/null 2>&1 && break; sleep 0.05; done
"$W" add "$HOME/Documents/report.pdf" "$F/clip.mp4" >/dev/null
"$W" add --text "Meeting notes: ship WayStow on Friday" >/dev/null
"$W" collection new "Email to Alice" --emoji "✉" >/dev/null

# Drop zones while dragging three images over the right edge.
"$P" drag-in 1279 400 1100 330 -- "$F/sunset-over-lisbon.jpg" "$F/tiles.png" "$F/garden.webp" >/dev/null &
probe=$!
sleep 1.6
grim "$out/dropzones-$theme.png"
wait "$probe" || { echo "the drop failed"; exit 1; }
sleep 1.5
# The shelf, with a finished action in the island.
id=$("$W" list --json | jq '[.[] | select(.content.value | tostring | endswith("tiles.png"))][0].id')
"$W" run image.convert --item "$id" --set format=webp --wait >/dev/null
sleep 0.8
grim "$out/shelf-$theme.png"
"$W" quit >/dev/null 2>&1 || true
wait "$D" 2>/dev/null || true
# Keep the right 60% (panel and island); the probe's test surface sits top-left.
for f in "$out"/*-"$theme".png; do magick "$f" -gravity East -crop 60%x100%+0+0 +repage "$f"; done
if command -v oxipng >/dev/null; then oxipng -q -o 3 "$out"/*-"$theme".png; fi
