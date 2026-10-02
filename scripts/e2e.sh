#!/usr/bin/env bash
# End-to-end tests in an isolated headless sway: drag-and-drop in and out,
# collections, clipboard, actions and undo, expiry, Recent, edges, reload.
#   nix develop --command scripts/e2e.sh
# Never touches your desktop: everything runs inside scripts/headless.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -z ${WAYSTOW_TEST_ROOT:-} ]]; then
  cargo build -q -p waystow -p dnd-probe
  exec scripts/headless.sh "$0" "$@"
fi

W=$PWD/target/debug/waystow
P=$PWD/target/debug/dnd-probe
F=$HOME/files
pass=0
fail=0
ok() { echo "  ✓ $1"; pass=$((pass + 1)); }
bad() { echo "  ✗ $1"; fail=$((fail + 1)); }
check() { local what=$1; shift; if "$@" >/dev/null 2>&1; then ok "$what"; else bad "$what"; fi; }
items() { "$W" list --json; }
count() { items | jq "$1" ; }

start() {
  "$W" daemon >>"$WAYSTOW_TEST_ROOT/daemon.log" 2>&1 &
  D=$!
  for _ in $(seq 100); do "$W" status >/dev/null 2>&1 && return; sleep 0.05; done
  echo "daemon didn't start"; cat "$WAYSTOW_TEST_ROOT/daemon.log"; exit 1
}
stop() { "$W" quit >/dev/null 2>&1 || true; wait "$D" 2>/dev/null || true; }

mkdir -p "$F" "$XDG_CONFIG_HOME/waystow"
magick -size 640x400 gradient:'#ff7a59-#7c6cf2' "$F/sunset.png"
magick -size 300x300 plasma:fractal "$F/plasma.jpg"
printf 'hello\n' >"$F/notes.txt"

echo "Drag and drop"
start
"$P" drag-in 1279 400 1100 330 -- "$F/sunset.png" "$F/plasma.jpg" >/dev/null
sleep 0.5
check "two files dropped onto the right edge" test "$(count 'length')" = 2
check "the panel opened after the drop" sh -c "\"$W\" status | grep -q 'panel: open'"
out=$("$P" drag-out 1050 230)
check "dragging the stack out offers both files" sh -c "echo '$out' | grep -c '^file://' | grep -qx 2"
check "drag-out URIs are percent-encoded file URIs" sh -c "echo '$out' | grep -q 'file://.*/files/sunset.png'"
"$W" collection new Upload >/dev/null
"$W" hide; sleep 0.4; "$W" toggle; sleep 0.8
"$P" drag 1000 314 1022 157 >/dev/null; sleep 0.6
check "a row dragged onto a collection chip moves there" test "$(count '[.[] | select(.collection != 1)] | length')" = 1
check "the daemon survived the internal drag" "$W" status

echo "Clipboard"
magick -size 120x80 xc:'#e06c75' png:- | wl-copy --type image/png; sleep 0.2
check "an image on the clipboard is shelved as a PNG" "$W" add --clipboard
check "it lives in the stash" sh -c "\"$W\" list --json | jq -e '.[0].content.kind == \"stash\"'"
echo "https://example.com/a" | wl-copy; sleep 0.2
"$W" add --clipboard >/dev/null
check "a copied URL becomes a link item" sh -c "\"$W\" list --json | jq -e '.[0].mime == \"text/x-uri\"'"
id=$(items | jq '[.[] | select(.content.value | tostring | endswith("notes.txt") | not) | select(.mime == "image/png")][0].id')
"$W" run any.copy-path --item "$id" >/dev/null; sleep 0.2
check "Copy path puts the path on the clipboard" sh -c "wl-paste | grep -q '/'"

echo "Actions"
"$W" add "$F/notes.txt" >/dev/null
sid=$(items | jq '[.[] | select(.mime == "image/png" and .content.kind == "file")][0].id')
check "Convert to WebP runs and waits" "$W" run image.convert --item "$sid" --set format=webp --wait
check "the output sits next to the input" test -s "$F/sunset.webp"
check "the output is on the shelf" sh -c "\"$W\" list --json | jq -e 'any(.[]; .content.value | tostring | endswith(\"sunset.webp\"))'"
"$W" run image.convert --item "$sid" --set format=webp --wait >/dev/null
check "a second run never overwrites (sunset (1).webp)" test -s "$F/sunset (1).webp"
job=$("$W" jobs --json | jq '.[0].id')
check "undo moves the output to the Trash" "$W" undo "$job"
check "…and it's gone from disk" test ! -e "$F/sunset (1).webp"
check "a bad param is refused" sh -c "! \"$W\" run image.convert --item $sid --set format=bmp"
check "an action that doesn't fit is refused" sh -c "! \"$W\" run pdf.merge --item $sid"
check "doctor runs" sh -c "\"$W\" doctor || true"
stop

echo "Expiry"
printf '[shelf]\nexpiry = "2s"\n' >"$XDG_CONFIG_HOME/waystow/config.toml"
start
"$W" clear >/dev/null; "$W" clear -c Upload >/dev/null
magick -size 50x50 xc:blue png:- | wl-copy --type image/png; sleep 0.2
"$W" add --clipboard >/dev/null
stash=$(items | jq -r '.[0].content.value')
"$W" add "$F/notes.txt" >/dev/null
"$W" pin "$(items | jq '[.[] | select(.content.value | tostring | endswith("notes.txt"))][0].id')"
sleep 3.2
check "unpinned items in Temporary expire" test "$(count '[.[] | select(.pinned | not)] | length')" = 0
check "pinned items stay" test "$(count 'length')" = 1
check "an expired stash file is deleted" test ! -e "$stash"
check "the original file is never deleted" test -e "$F/notes.txt"

echo "Recent"
head -c 200000 /dev/urandom >"$HOME/Downloads/big.iso.part"; sleep 0.5
mv "$HOME/Downloads/big.iso.part" "$HOME/Downloads/big.iso"; sleep 0.6
check "a finished download shows up" sh -c "\"$W\" recent --kind downloaded | grep -q big.iso"
magick -size 200x100 xc:gray "$HOME/Pictures/Screenshots/shot.png"; sleep 0.6
check "a new screenshot shows up" sh -c "\"$W\" recent --kind screenshot | grep -q shot.png"
rm "$HOME/Downloads/big.iso"; sleep 0.5
check "a deleted file leaves Recent" sh -c "! \"$W\" recent | grep -q big.iso"
stop

echo "Edges and reload"
printf '[catcher]\nedges = ["left", "bottom"]\n' >"$XDG_CONFIG_HOME/waystow/config.toml"
start
"$W" clear >/dev/null
"$P" drag-in 0 400 200 400 -- "$F/plasma.jpg" >/dev/null 2>&1 || true
sleep 0.4
check "the left edge catches drops" test "$(count '[.[] | select(.content.value | tostring | endswith("plasma.jpg"))] | length')" = 1
printf '[ui]\nthme = "dark"\n' >"$XDG_CONFIG_HOME/waystow/config.toml"
check "reload rejects a typo" sh -c "! \"$W\" reload"
printf '[ui]\ntheme = "light"\n' >"$XDG_CONFIG_HOME/waystow/config.toml"
check "reload accepts a valid config" "$W" reload
stop

if grep -qE 'panicked|CRITICAL|ERROR' "$WAYSTOW_TEST_ROOT/daemon.log"; then
  bad "the daemon log is clean"
  grep -E 'panicked|CRITICAL|ERROR' "$WAYSTOW_TEST_ROOT/daemon.log" | head -5
else
  ok "the daemon log is clean"
fi
echo "$pass passed, $fail failed"
[[ $fail == 0 ]]
