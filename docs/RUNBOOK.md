# Runbook

## Daily use
```sh
systemctl --user status waystow          # daemon (home-manager service)
journalctl --user -u waystow -f          # logs; WAYSTOW_DEBUG=1 logs catcher and drop events
waystow status | waystow doctor
waystow reload                           # after editing config.toml or actions/*.toml
```
The service needs `WAYLAND_DISPLAY` in the systemd user environment; niri, Sway and Hyprland session scripts export it.

## Development
```sh
nix develop                              # toolchain, GTK, sway, grim, wtype, dbus and every action tool
cargo test --workspace                   # unit tests, plus all 27 built-in command actions on fixtures
cargo clippy --workspace --all-targets -- --deny warnings
scripts/e2e.sh                           # 30 end-to-end checks in an isolated headless sway
scripts/screenshots.sh                   # re-render assets/ (scale 2, dark and light)
nix build .#waystow-full && result/bin/waystow doctor
nix flake check                          # clippy, tests, rustfmt, package build
nix flake check --all-systems --no-build # also evaluates aarch64
```

## Driving the UI in tests
- Everything that moves a pointer or types runs inside `scripts/headless.sh COMMAND…`: its own sway (1280×800 logical, `WAYSTOW_TEST_SCALE=2` for HiDPI), runtime dir, HOME and XDG dirs, and session bus. Never run `dnd-probe` or `wtype` on a real desktop.
- `target/debug/dnd-probe` (a virtual pointer plus a 160×160 surface at the top left):
  - `drag-in X Y [X2 Y2] -- FILE…`: drag files from the probe to (X,Y), then (X2,Y2), and drop. Prints `dropped`.
  - `drag-out X Y`: press at (X,Y) and drag onto the probe. Prints the received URIs.
  - `drag X1 Y1 X2 Y2`: a drag between two points of other clients (e.g. a shelf row onto a chip).
  - `move` and `click`.
- With the right edge at 1280×800, the panel spans about x 884–1242; tabs at y≈115, chips at y≈157, rows from y≈314 (52 px apart), the stack card at y≈230.
- `wtype`: send whole sequences in one call starting with a throwaway key (`wtype -k Shift_L -s 300 -k Down …`), and only while `waystow status` says `panel: open`.
- Kill leftovers by exact pattern only (the user's compositor is niri): `pgrep -f '[s]way-unwrapped -c /tmp/nix-shell.*/waystow-headless' | xargs -r kill`. Never `pkill -f` with a pattern that appears in your own command line.

## Packaging
- `packages.waystow` (tools from the user's PATH) and `packages.waystow-full` (`nix/tools.nix` on PATH). `dist/waystow.service` ships in `share/systemd/user`.
- home-manager module `services.waystow.{enable,package,settings,actions}`; evaluated against home-manager master on 2026-10-01.
