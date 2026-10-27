# Research: Rust layer-shell toolkits and drag-and-drop

Researched 2026-10-01 against source code at HEAD. Nothing here was tested at runtime yet; spikes S1/S2 do that.

WayStow needs drag-and-drop in both directions on a `wlr-layer-shell` surface:
- **(a) Drop target:** the MIME list is available on enter, plus enter/motion/leave events.
- **(b) Drag source:** multiple `text/uri-list` items with a custom icon, also offering `application/vnd.portal.filetransfer` for Flatpak targets.

| Candidate | (a) drop target | (b) drag source | Portal FileTransfer | Notes |
|---|---|---|---|---|
| iced 0.14 + waycrate `iced_layershell` 0.19.1 / `iced_exwlshell` 0.20 | No | No | No | No `wl_data_device` code at all (Quillway's stack) |
| MalpenZibo/iced_layershell 0.6.1 | No (empty stubs) | No | No | "Drag and drop is stubbed (not supported by design)" |
| libcosmic + pop-os iced fork | Yes (`dnd_destination`: enter with MIME list, motion, leave, drop) | Yes (`dnd_source` + `drag_icon`) | Receiving only. The source side must be written by hand (ashpd) | Git-only dependency, carries an iced fork. Works on layer surfaces (cosmic-workspaces) |
| **gtk4-rs + gtk4-layer-shell** | **Yes** (`DropTarget` / `DropTargetAsync`, `Drop::formats()` on enter) | **Yes** (`DragSource` + `set_icon`) | **Automatic** for `GdkFileList` / `GFile` since GTK 4.8.1, whenever the portal is present, sandboxed or not | Nothing reported against DnD in the gtk4-layer-shell tracker |
| Slint 1.19 | Only on the Qt backend | Only on the Qt backend | No | No first-party layer shell |
| Raw SCTK 0.21 + a renderer | Yes (write it yourself) | Yes (write it yourself) | Write it yourself | The DnD part is about 500–800 lines. The real cost is building a UI toolkit |

## Findings
- **GTK portal handling:** `gdk/filetransferportal.c` registers `application/vnd.portal.filetransfer` (and the legacy `application/vnd.portal.files`) serializers for `GdkFileList`. A `DragSource` with `ContentProvider::for_value(&FileList)` therefore offers both uri-list and filetransfer with no extra code.
- **Chromium/Electron don't consume the FileTransfer portal** (crbug 40898398). A Flatpak Chromium can only open dropped files at host paths it can already read, whatever toolkit we use.
- **Blur (`ext-background-effect-v1`):** GTK binds it itself since 4.23.3 (stable 4.24, 2026-09-11), on every `wl_surface` including layer surfaces. The region comes from CSS `backdrop-filter: blur()`. **Don't bind it manually on GTK ≥ 4.23.3**: a second `get_background_effect` is a protocol error (`background_effect_exists`).
- **Animation:**
  - `GskGLShader` has been deprecated since 4.16, so don't build on it.
  - Use custom `snapshot()` drawing (transforms, opacity, `push_blur`, rounded clips) driven by `add_tick_callback`.
  - `adw::SpringAnimation` with `CallbackAnimationTarget` needs only `adw::init()`, not `AdwApplication`. This comes from memory of the docs and is unverified.

## Sources
- https://github.com/waycrate/exwlshelleventloop
- https://github.com/MalpenZibo/iced_layershell/blob/main/src/handlers/data_device.rs
- https://github.com/pop-os/libcosmic/blob/master/src/widget/dnd_destination.rs · https://github.com/pop-os/cosmic-files/issues/1422
- https://gitlab.gnome.org/GNOME/gtk/-/blob/main/gdk/filetransferportal.c · GTK NEWS 4.8.1, 4.21.3, 4.23.3
- https://issues.chromium.org/issues/40898398
- https://github.com/Smithay/client-toolkit (`examples/data_device.rs`)
