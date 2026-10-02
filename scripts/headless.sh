#!/usr/bin/env bash
# Run a command inside an isolated headless sway: its own runtime dir and its
# own HOME and XDG dirs, so nothing touches your desktop or your files.
#   scripts/headless.sh [--keep] COMMAND...
# Inside, WAYLAND_DISPLAY points at the headless sway (1280x800 logical; set
# WAYSTOW_TEST_SCALE=2 for HiDPI), and
# $WAYSTOW_TEST_ROOT is the scratch root. Run from inside `nix develop`.
set -euo pipefail

keep=0
if [[ ${1:-} == --keep ]]; then keep=1; shift; fi
[[ $# -gt 0 ]] || { echo "usage: $0 [--keep] COMMAND..." >&2; exit 2; }
command -v sway >/dev/null || { echo "sway not found; run inside nix develop" >&2; exit 2; }

root=$(mktemp -d "${TMPDIR:-/tmp}/waystow-headless.XXXXXX")
sway_pid=
cleanup() {
  for m in "$root/run/doc" "$root/run/gvfs"; do
    if mountpoint -q "$m" 2>/dev/null; then fusermount3 -uz "$m" 2>/dev/null || fusermount -uz "$m" 2>/dev/null || true; fi
  done
  # sway runs in its own process group (the nixpkgs `sway` is a wrapper around
  # sway-unwrapped), so kill the whole group, not just the wrapper.
  if [[ -n $sway_pid ]]; then kill -- "-$sway_pid" 2>/dev/null || true; wait "$sway_pid" 2>/dev/null || true; fi
  if [[ $keep == 0 ]]; then rm -rf "$root"; else echo "kept $root" >&2; fi
}
trap cleanup EXIT

export XDG_RUNTIME_DIR=$root/run HOME=$root/home
export XDG_CONFIG_HOME=$HOME/.config XDG_STATE_HOME=$HOME/.local/state XDG_CACHE_HOME=$HOME/.cache XDG_DATA_HOME=$HOME/.local/share
mkdir -p -m700 "$XDG_RUNTIME_DIR"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$HOME/Downloads" "$HOME/Pictures/Screenshots"
unset WAYLAND_DISPLAY DISPLAY SWAYSOCK DBUS_SESSION_BUS_ADDRESS

cat > "$root/sway.conf" <<CONF
output HEADLESS-1 resolution $((1280 * ${WAYSTOW_TEST_SCALE:-1}))x$((800 * ${WAYSTOW_TEST_SCALE:-1})) scale ${WAYSTOW_TEST_SCALE:-1} bg ${WAYSTOW_TEST_BG:-#3b4252} solid_color
default_border none
CONF

WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=${WLR_RENDERER:-pixman} \
  setsid sway -c "$root/sway.conf" >"$root/sway.log" 2>&1 &
sway_pid=$!
for _ in $(seq 100); do
  [[ -S $XDG_RUNTIME_DIR/wayland-1 ]] && break
  sleep 0.05
done
[[ -S $XDG_RUNTIME_DIR/wayland-1 ]] || { echo "headless sway didn't start:" >&2; cat "$root/sway.log" >&2; exit 1; }

export WAYLAND_DISPLAY=wayland-1 WAYSTOW_TEST_ROOT=$root GSK_RENDERER=${GSK_RENDERER:-cairo} GDK_BACKEND=wayland GVFS_DISABLE_FUSE=1
# A private session bus, so portals and file managers of the real session aren't reached.
if command -v dbus-run-session >/dev/null; then
  dbus-run-session -- "$@"
else
  "$@"
fi
