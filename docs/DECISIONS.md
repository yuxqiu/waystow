# Decisions

Rationale is in `docs/PLAN.md` and `docs/research/`.

1. **Scope** (2026-10-01): four connected modules, Recent → Shelf → Actions → Activity, with the shelf first. Out of scope: snap zones and anything a desktop shell already provides. GNOME/Mutter is unsupported (no layer-shell).
2. **Rust, standalone app** (author, 2026-10-01). No DMS plugin. It must look at home beside DMS but depend on nothing from it.
3. **A single binary with the UI in-process** (2026-10-01, from Quillway). `waystow daemon` hosts the GTK UI. Other subcommands talk NDJSON over `$XDG_RUNTIME_DIR/waystow.sock` (mode 0600). This replaces the earlier proposal of a separate daemon plus a Quickshell/QML shell.
4. **UI: GTK4 + gtk4-layer-shell via gtk4-rs, without relm4 or the libadwaita stylesheet** (2026-10-01, `docs/research/ui-toolkit-dnd.md`).
   - It is the only Rust option with drag-and-drop in both directions on layer surfaces, and with automatic portal FileTransfer (GTK ≥ 4.8.1).
   - Neither iced layer-shell binding implements `wl_data_device`. libcosmic is a git-only fork without a portal drag source.
   - Blur comes from CSS `backdrop-filter` on GTK ≥ 4.24. Don't bind `ext-background-effect-v1` manually there (that is a protocol error).
5. **Actions are declarative TOML.** argv only, never overwrite, write to a temp name and rename on success, undo through Trash, and each job in its own process group with PDEATHSIG SIGKILL (2026-10-01, Quillway lessons L5/L7).
6. **Async results carry an id** (drag session, item, job, panel instance). Stale results are dropped (Quillway L4).
7. **Size caps reject; they never truncate** (Quillway L6).
8. **All UI/DnD automation runs in an isolated headless sway** using a `dnd-probe` test client and a virtual pointer. Tests never touch `$HOME` (Quillway L10/L11).
9. **Licence MIT OR Apache-2.0; catcher edges configurable (`catcher.edges`, default right); only Temporary expires** (author, 2026-10-01). Named collections and pinned items never expire.
10. **No action modifies a file in place** (2026-10-01). "Remove metadata" writes `foo-clean.jpg`. Undo is therefore always "trash the outputs", except Rename (renamed back) and Trash (restored from `trash:///` by original path).
11. **GIO instead of extra crates** (2026-10-01): file monitors (Recent), MIME sniffing, D-Bus (`FileManager1`, portal settings, notifications), Trash. No zbus, notify or tree_magic.
12. **The catcher surface resizes between strip and panel** (2026-10-01) instead of a panel-sized surface with a 2 px input region. Compositor blur rules blur the whole layer surface (Quillway L3), so an idle transparent panel-sized surface would show as a blurred box. Verified: drag-enter on a 2 px strip, then growing mid-drag, keeps the drag (headless sway).
13. **GTK quirks we rely on** (2026-10-01, all found in headless sway):
    - A window whose content has no natural size gets 200×200 (`NO_CONTENT_CHILD_NAT`), so the catcher pins its size.
    - A fully transparent frame is committed without a buffer, so the surface never maps and gets no input. The root has an effectively invisible background (alpha 0.004).
    - `gtk::Fixed` measures child transforms, so animating resized the layer surface every frame. We use our own `Stage` container (fixed size, children allocated with transforms).
    - Removing the widget under the pointer inside an event handler trips `check_crossing_invariants` (abort). All panel renders are deferred to idle, and selection changes update rows in place.
    - A `GdkDrop` can't be read twice (the second read hangs without `receive`). The payload is read once on drag-enter, which also gives the drop zones the dragged files' MIME types, and the drop reuses it.
    - Finishing a drop with an action the source didn't offer (MOVE vs COPY) is a protocol error and the compositor disconnects us. Local moves finish with COPY.
    - Symbolic SVG icons can't be stroked (GTK forces `fill` when recolouring). Bundled icons are drawn with GSK paths (`ui/icons.rs`).
    - GTK's theme paints buttons with `background-image`; our CSS clears it.
14. **No blur from the client on GTK < 4.24** (2026-10-01). nixpkgs ships GTK 4.22.4. On GTK ≥ 4.24 the panel CSS adds `backdrop-filter: blur(24px)` (version-gated, untested until nixpkgs has 4.24); until then the README documents compositor rules with `ui.client_shadow = false`.
15. **The panel only re-renders when what the current tab shows changes, never while a popover is open, and read-only IPC requests don't refresh** (2026-10-02). Otherwise a download finishing or a `waystow status` poll destroyed an open popover mid-typing.
16. **No portable release tarball** (2026-10-01). GTK is linked, not dlopened (unlike Quillway's iced build), and Ubuntu 22.04 has GTK 4.6. Release through Nix (`waystow`, `waystow-full`), with a `cargo build` recipe in the README.
17. **The headless harness starts sway with `setsid` and kills the process group** (2026-10-02). nixpkgs' `sway` is a wrapper, so `kill $!` left `sway-unwrapped` running (23 leaked instances, all cleaned up).
