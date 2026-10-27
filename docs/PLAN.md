# WayStow — Project Plan

> A transient file shelf and action surface for Wayland.
> **Recent → Shelf → Actions → Activity**

Status: v0.1.0 implemented (2026-10-02); see §11.
Revision 2: Rust, standalone app, GTK4 UI, with lessons from `../quillway` (§2).

---

## 1. Product definition

### 1.1 What WayStow is

A layer around the files you are working with *right now*. Your shell (DMS, Waybar, whatever) keeps everything else.

| Module | One-line job |
|---|---|
| **Shelf** | Hold references to files from anywhere, then drag them out together. |
| **Actions** | Run MIME-aware operations (convert, compress, OCR, merge…) on shelf items. Every action is a call to an existing CLI tool. |
| **Recent** | Show files you just downloaded, saved, screenshotted, copied or opened. |
| **Activity** | Show progress for running jobs and downloads, and offer actions on new screenshots. |

The **shelf is the primitive**. Everything else feeds into it or acts on what it holds.

### 1.2 Non-goals

- Anything a desktop shell already does: launcher, file search, clipboard history UI, notifications, media, network/BT/audio, calendar, weather, sysmon, dock, lock screen.
- **Snap zones.** Moving windows is the compositor's job, and no agnostic protocol lets a client do it.
- A file manager. Shelf items are references. WayStow never moves or deletes an original unless the user asks it to (Trash, Rename).
- Its own screenshot tool or image editor. It reacts to new screenshot files and hands annotation to `satty` or `swappy`.
- A DMS plugin. WayStow is a **standalone app** (author's decision, 2026-10-01). It must look at home next to DMS, but it depends on nothing from it.
- GNOME/Mutter, which has no `wlr-layer-shell`.

### 1.3 Design principles

1. **Never destructive by default.** Outputs are new files that never overwrite anything. Every action can be undone through Trash.
2. **Contextual, not exhaustive.** At most 4–6 primary actions per MIME type, with the rest under "More".
3. **Transient by default.** Items expire (1 h by default) unless pinned.
4. **Compositor agnostic.** Use `wlr-layer-shell`, `wl_data_device`, `ext-data-control-v1` (falling back to `wlr-data-control`), `ext-background-effect-v1`, and freedesktop specs. Compositor-specific extras are optional and documented.
5. **Orchestrate mature tools.** Use ImageMagick, ffmpeg, qpdf, Ghostscript, OCRmyPDF, Tesseract, exiftool, 7z/bsdtar and jq. Write no codecs.

---

## 2. Lessons from Quillway

Quillway (`../quillway`) is the same kind of app: a Rust layer-shell popup with a resident daemon, a CLI and Nix packaging. It went from plan to v0.1.1 in about a day. Most of that time went into review findings about races, truncation, child processes and test isolation. WayStow adopts the following up front.

| # | Quillway experience | WayStow rule |
|---|---|---|
| L1 | **One binary.** `quillway daemon` hosts the UI in-process, and every other subcommand is a thin NDJSON client over `$XDG_RUNTIME_DIR/quillway.sock`. | Same: `waystow daemon` plus `waystow toggle|add|list|run|status|doctor|reload|quit`. No separate UI process, so there is no second IPC layer to keep in sync. |
| L2 | **The toolkit choice was driven by visuals.** iced + iced_layershell works, but it needed `winit-core` pins and has an unfixed upstream bug (key repeat reported as presses). | **DnD drives WayStow's choice.** Neither iced layer-shell binding implements `wl_data_device` at all (`docs/research/ui-toolkit-dnd.md`). We use **GTK4 + gtk4-layer-shell** (§3.1). Pin versions and record every upstream workaround in DECISIONS with a "drop when…" condition. |
| L3 | **Spikes found real constraints early.** niri blurs the *whole* layer surface, so a client-drawn shadow margin gets blurred too. | M0 spikes first (§6). Shadows follow Quillway's `client_shadow` switch: client-drawn by default, off when a compositor rule draws them. The README ships per-compositor layer rules for namespace `waystow`. |
| L4 | **Stale async results** hit a newer popup: clipboard captures and focus results arriving late. | Every async result carries the id of what it belongs to: drag session, item, job, panel instance. The receiver drops results whose id is no longer current. |
| L5 | **Truncated output was reported as success** (an SSE stream ending without `[DONE]`, or `finish_reason: length`). | A job succeeds only if it exits 0, its output exists, is non-empty, and was renamed from a temp name. Anything else fails, and partial files are removed or kept clearly labelled. |
| L6 | **Silent truncation of input:** the clipboard was capped by cutting text. | Size caps (clipboard images, text snippets) *reject* with a visible message. They never truncate. |
| L7 | **Child processes outlived the daemon.** A merged SIGTERM stalled llama-server; fixed with `PR_SET_PDEATHSIG = SIGKILL`. | Every job runs in **its own process group** (ocrmypdf and ffmpeg spawn children) with PDEATHSIG SIGKILL and `kill_on_drop`. Cancel sends SIGTERM to the group, then SIGKILL after 3 s. A regression test uses a child that ignores SIGTERM. |
| L8 | **Config vs state.** The Nix-managed config is read-only, so the `models use` choice lives in `$XDG_STATE_HOME`. Unknown keys are rejected and values validated at load. `reload` replies only after the new config is applied. | Same split: `config.toml` and `actions/*.toml` in config; SQLite plus the stash in `$XDG_STATE_HOME/waystow/`. Same strict validation and reload semantics. |
| L9 | **Security basics:** socket mode 0600 even with the `/tmp` fallback, and a second daemon is detected. | Same. Plus: actions run as argv only (never a shell), and inputs beginning with `-` get a `./` prefix. |
| L10 | **UI tests touched the real desktop.** `wtype` typed into the author's window once. | All UI automation runs in an **isolated headless sway** with its own `XDG_RUNTIME_DIR`. All action tests run in temp directories with `XDG_*` overridden. Nothing under `$HOME` is touched. |
| L11 | **Input automation is hard.** A headless sway has no pointer, so a custom wlr-virtual-pointer client was needed. `wtype` drops the first key and fakes keycodes. | Build a test tool, `dnd-probe` (SCTK), early in M1. It acts as a drag source or drop target driven by a virtual pointer, so DnD gets automated tests (§7). |
| L12 | **Four holistic reviews** found mostly state-machine edge cases: phase × key × focus. | Write explicit state machines with tests *before* the UI: EdgeCatcher, drag session, job lifecycle (§3.2). |
| L13 | **`doctor`** caught environment problems: protocols, binaries, model. | `waystow doctor` checks layer-shell, data-control, background-effect, the portal, every action's `requires`, and GTK version features. |
| L14 | **Packaging:** crane + rust-overlay, a home-manager module with a systemd user service, `nix flake check` (clippy pedantic+nursery with warnings denied, tests, fmt, build) in CI on x86_64 and aarch64. The dev shell pins Mesa. | Reuse it nearly verbatim. **Difference:** GTK4 is linked, not dlopened, and Ubuntu 22.04 has only GTK 4.6. A portable "glibc only" tarball no longer works, so release via Nix, an AUR package and a distro-agnostic build recipe (§8). |
| L15 | **Docs discipline** (PLAN, DECISIONS, HANDOFF, SESSION_LOG, RUNBOOK) kept the long sessions coherent. | Keep it. Research goes in `docs/research/`. |
| L16 | Deferred in Quillway: following the system theme, a close animation, client-side blur. | First-class in WayStow: portal `color-scheme` and accent colour, open/close springs, and blur from CSS `backdrop-filter` (GTK ≥ 4.24). |

---

## 3. Architecture

```
┌──────────────────────────── waystow daemon (one process) ─────────────────────────────┐
│                                                                                        │
│  GTK main thread (glib MainContext)               tokio runtime (worker threads)        │
│  ┌───────────────────────────────┐   channels    ┌──────────────────────────────────┐   │
│  │ UI: EdgeCatcher · ShelfPanel   │◄────────────►│ IPC server (NDJSON, 0600 socket) │   │
│  │ ActionSheet · QuickLook        │  (ids on     │ JobRunner (process groups)       │   │
│  │ RecentDrawer · Island · Toast  │   every msg) │ RecentIndexer (inotify, xbel)    │   │
│  │ DnD: DropTarget / DragSource   │              │ Thumbnailer (spec cache)         │   │
│  └──────────────┬────────────────┘              │ Store (SQLite) · Stash · Expiry   │   │
│                 │ owns                           │ ClipboardWatch (data-control)    │   │
│           AppState (single source of truth on the main thread; workers send events)   │
└────────────────────────────────────────────────────────────────────────────────────────┘
        ▲ NDJSON                                          ▲ argv only
   waystow CLI (toggle, add, list, run, doctor…)     magick · ffmpeg · qpdf · gs · ocrmypdf …
```

### 3.1 UI toolkit: GTK4 + gtk4-layer-shell (gtk4-rs)

Full research: `docs/research/ui-toolkit-dnd.md`.

**Why GTK4:**
- It is the only Rust-usable option that does both DnD directions on layer surfaces.
- Drag-out to Flatpak apps works automatically: since GTK 4.8.1 a `GdkFileList` drag also offers `application/vnd.portal.filetransfer`.
- Since GTK 4.24, `ext-background-effect-v1` blur comes from CSS `backdrop-filter`.

**How we use it, so it doesn't look like libadwaita:**
- Plain `gtk4-rs`, no relm4. A small Elm-like loop of `AppState` + `Msg` on the main thread, the way Quillway's iced code was structured. All state transitions are pure functions in `waystow-core` and unit-tested (L12).
- Custom widgets draw in `snapshot()`: rounded clips, transforms, shadows, `push_blur`. A tick-callback **spring** drives every motion. Use `adw::SpringAnimation` if `adw::init()` alone suffices (to be verified in S4), otherwise a 60-line spring integrator of our own.
- Our own CSS theme only. The libadwaita stylesheet is not loaded.
- **Version floor:** at least GTK 4.8.1 (portal drag-out). The exact floor, together with gtk4-layer-shell's own requirement, is set in S1. On GTK < 4.24 blur is simply off and the tinted fallback is used. `doctor` reports which applies.

**Rejected:**
- iced: no DnD on layer surfaces.
- libcosmic: has DnD, but is a git-only dependency carrying an iced fork, and its drag source lacks the portal.
- Raw SCTK: we'd be writing a toolkit.

### 3.2 State machines (written and tested first)

**EdgeCatcher, per output:**

```
Idle ──drag enter strip──► Peek(offer mimes) ──80 ms dwell──► Open(tiles)
  ▲                          │ leave                            │ drop on tile → Dropping(session id) → Idle
  └─────────── 300 ms grace ◄┴──────────────── leave ◄──────────┘ leave
```

**Job:** `Queued → Running(progress) → {Done(outputs) | Failed(stderr tail) | Cancelled}`. Undo is only possible from `Done` and only once.

**Drag-out session:** each session has an id. If an item expires or is removed during the drag, the session keeps its snapshot of URIs, and the expiry is deferred until the drag ends.

### 3.3 Crates

```
waystow/
├── crates/
│   ├── waystow-core/   # pure: config, action spec + templating, MIME match, naming, expiry,
│   │                   #   state machines, IPC types (wire-format tests)
│   ├── waystow-jobs/   # process-group runner, progress parsers, cancel, undo via Trash
│   ├── waystow-index/  # SQLite store, stash, recent indexer (inotify, xbel), thumbnails
│   ├── waystow-wl/     # data-control clipboard watch/read/write (from quillway-wl)
│   └── waystow/        # bin: CLI, IPC server, GTK UI, doctor
├── actions/            # built-in action pack (*.toml, embedded with include_str!)
├── dev/dnd-probe/      # SCTK test client for DnD automation (excluded from releases)
├── nix/                # package.nix, hm-module.nix
└── docs/
```

Suggested dependencies: `gtk4` + `gtk4-layer-shell` (+ `libadwaita` for springs only, if S4 confirms), `tokio`, `async-channel`, `rusqlite` (bundled), `notify`, `wl-clipboard-rs` + `wayland-client`/`wayland-protocols` (as in quillway-wl), `tree_magic_mini` or `xdg-mime`, `serde`/`toml`, `trash`, `zbus` (FileManager1, thumbnailer, portal settings), `clap`, `libc`. Workspace lints are the same as Quillway's and zippel's.

---

## 4. Key technical designs

### 4.1 The universal drag target (EdgeCatcher)

**Constraint:** Wayland never tells a client that a drag started elsewhere. A client only gets `wl_data_device.enter` when the drag enters *its* surface.

**Design:** one layer surface per output, namespace `waystow`:
- Layer `overlay`, anchored to one edge (right by default), `exclusive_zone = 0`, keyboard interactivity `none` (switched to `on_demand` while the panel is open).
- The surface is sized for the open panel. The **input region** (`gdk::Surface::set_input_region`) is only the hot strip: 2 px × 30% of the edge, centred.
- A `DropTargetAsync` reads `Drop::formats()` on enter, which drives the action tiles before anything is dropped. On open, the input region grows to the panel.
- Drop on **Keep** adds to the shelf. Drop on an action tile runs the action at once and puts the output on the shelf.

**Trade-off:** the strip eats normal clicks at that spot. Mitigations: keep it short and configurable (edge, length, off), plus `waystow toggle` for a compositor keybind.

**Open question for S1:** does resizing the input region mid-drag behave on every compositor? The fallback keeps a full-size input region during `Open` only.

### 4.2 Drag-in

| Offered MIME | Becomes |
|---|---|
| `text/uri-list` (`file://`) / `GdkFileList` | File references (multi) |
| `application/vnd.portal.filetransfer` | Resolved by GTK into a `GdkFileList` |
| `text/uri-list` (`http(s)://`) | URL item (open, download to stash, copy) |
| `image/*` raw bytes (browser image drags) | PNG in the stash, then a file item |
| `text/plain` | Text snippet (save as file, copy, QR, format JSON) |

The stash is `$XDG_STATE_HOME/waystow/stash/`. WayStow owns these files, so they are deleted when the item expires unpinned. Raw payloads over `limits.drop_bytes` (default 64 MiB) are rejected with a message (L6).

### 4.3 Drag-out

- A `DragSource` provides `ContentProvider::for_value(&gdk::FileList)`. GTK offers `text/uri-list` and the portal format automatically.
- Drag one tile, a selection, or a whole collection. The drag icon is a rendered `ThumbStack` with a count badge.
- **Known limitation:** Chromium/Electron don't consume the FileTransfer portal, so a *Flatpak* Chromium only receives files at paths it can already read. Document this in the README.

### 4.4 Data model (SQLite, `$XDG_STATE_HOME/waystow/waystow.db`)

```
collection(id, name, emoji, color, expiry_secs NULL, created_at, sort)
item(id, collection_id, kind{file,stash,text,url}, path|text|url, mime, size,
     source{drag,clipboard,screenshot,cli,action,recent}, added_at, expires_at NULL,
     pinned, missing)
job(id, action_id, params_json, status, progress, started_at, finished_at,
    stderr_tail, undo_json)
job_io(job_id, role{in,out}, path)
recent(path, kind{downloaded,created,modified,opened,screenshot,copied}, ts, mime)
```

- A "Temporary" collection always exists.
- Missing originals are greyed out and marked `missing`, never deleted.
- Schema migrations run through `PRAGMA user_version`.

### 4.5 Action system

Actions are declarative TOML. The built-in pack is embedded in the binary. User actions live in `~/.config/waystow/actions/*.toml`, and a user action with the same `id` overrides the built-in one.

```toml
[[action]]
id       = "image.to-webp"
title    = "Convert to WebP"
icon     = "image-x-generic-symbolic"
group    = "Convert"
mime     = ["image/*"]
exclude  = ["image/webp", "image/svg+xml"]
arity    = "each"                     # each | all | 2
requires = ["magick"]                 # hidden, with a doctor hint, when missing
params   = { quality = { type = "int", default = 82, min = 1, max = 100 } }
command  = ["magick", "{in}", "-quality", "{quality}", "{out}"]
output   = "{dir}/{stem}.webp"        # collision-safe: foo.webp → foo (1).webp
progress = "none"                     # none | ffmpeg | percent-regex:"(\\d+)%"
```

Execution rules:
- argv only. Inputs starting with `-` get a `./` prefix (L9).
- Write to `{out}.waystow-tmp` and rename on success. Success means exit 0 and a non-empty output (L5).
- Output goes next to the input, or to `~/Downloads/WayStow/` if that directory isn't writable.
- Undo: trash the outputs. In-place actions (strip EXIF, rename) copy the original to Trash first and restore from there.
- Concurrency defaults to `ncpu/2`. Each job gets its own process group, with PDEATHSIG SIGKILL (L7).
- Outputs go back onto the shelf, so actions chain.

**Built-in pack v1** (about 25 actions):

| Type | Primary | More |
|---|---|---|
| Any | Copy path · Reveal (FileManager1) · Open with… | Rename · Checksum · Archive · Trash |
| Image | Convert · Resize · Optimize · OCR → clipboard | Strip metadata · Copy image · Images → PDF (`all`) |
| Video | Compress · Make GIF · Extract audio | Convert container · Copy first frame |
| Audio | Convert (mp3/opus/flac) | Normalize |
| PDF | Merge (`all`) · Compress · OCR | Split · Extract pages |
| Archive | Extract here · List contents | Extract to… |
| Text/code | Copy contents · Format (jq) | Diff (`2`) · QR · Checksum |
| Directory | Terminal here (`xdg-terminal-exec`) · Archive | Copy path |

Interactive actions (Trim, Crop, a page picker) come after v1.

### 4.6 Recent indexer

| Kind | Source |
|---|---|
| opened | `~/.local/share/recently-used.xbel` (watched) |
| downloaded | inotify on `XDG_DOWNLOAD_DIR`: rename from `.part`, `.crdownload` or `.tmp`, or `CLOSE_WRITE` |
| created / modified | inotify on configured directories (shallow), with ignore rules |
| screenshot | inotify on configured screenshot directories (`~/Pictures/Screenshots` by default) |
| copied | ClipboardWatch (§4.7) |

It is a time-ordered feed capped at about 7 days, not a search. Tabs: **Today · Downloads · Screenshots · Copied · Opened**.

### 4.7 Clipboard intake

`waystow-wl` grows Quillway's watcher (ext-data-control, falling back to wlr) into one that also reads offers:
- Copied `text/uri-list` becomes a `copied` recent entry.
- Copied images are stashed only when the user asks ("Shelve clipboard"). Any read is capped and rejected beyond the cap (L6). Each read carries a request id (L4).
- Quillway's lesson applies: a new device reports the current selection at once, and that is not a copy.

### 4.8 Activity / Island

- A top-centre layer surface (position configurable, so it doesn't collide with a top bar). It is a pill that springs between compact and expanded.
- It shows jobs (determinate progress where the tool reports it), downloads (bytes and rate with an indeterminate bar, because browsers don't write totals), and the screenshot toast (Annotate · OCR · Copy · Shelf).
- Fallback: freedesktop notifications with the `value` hint.

### 4.9 Thumbnails & Quick Look

- Read the freedesktop thumbnail cache first. On a miss, use tumbler over D-Bus, then `gdk-pixbuf` / `ffmpegthumbnailer` / `pdftoppm`, and write the result back to the cache.
- Results carry the item id (L4).
- Space opens Quick Look: a large preview of an image, PDF page 1, video poster or text head.

---

## 5. Visual & interaction design

Design comes before code. Mockups for every surface are part of M0 (D1).

**Material**
- Floating panels with a 22–26 px radius, a 1 px inner highlight and a two-layer soft shadow.
- Shadows are client-drawn into a transparent margin by default. Set `ui.client_shadow = false` when a compositor rule draws them (niri `layer-rule`, Hyprland `layerrule`), because niri blurs the whole surface including the margin (L3).
- Blur comes from CSS `backdrop-filter` on GTK ≥ 4.24 with `ext-background-effect-v1` (KWin 6.7, niri 26.04, COSMIC), or from a Hyprland `layerrule`. Elsewhere (e.g. Sway), use a high-opacity tinted surface with subtle grain.

**Motion** (springs, not durations)
- The catcher grows from the edge with damping ≈ 0.8 and a slight overshoot.
- Dropped files fly from the drop point, fan out, then settle into a **ThumbStack**.
- Hover lifts a tile 2 px. Dragging out lifts the stack with a count badge.
- The island morphs its shape continuously between states.
- Open and close are both animated (L16).
- `ui.reduced_motion` turns the motion off.

**Content**
- Real thumbnails throughout. Filenames are middle-truncated. Numbers use tabular figures.
- One accent colour, from config, else the portal accent, else a default. Light/dark follows the portal `color-scheme`, overridable as `ui.theme = system|dark|light`.

**Keyboard** (after `waystow toggle`)
- Arrow keys move, Space opens Quick Look, ↵ runs the default action, typing filters actions, Delete removes, Esc closes.
- Shortcuts match by character first and fall back to the physical key, as in Quillway.

---

## 6. Milestones

### M0 — Spikes & design (go/no-go)

| Spike | Question | Pass criteria |
|---|---|---|
| **S1** DnD in | Does a gtk4-layer-shell surface with a 2 px input region get enter (with formats) and drop on niri, Hyprland, Sway and KWin? Can the input region grow mid-drag? | Enter within 50 ms. Drops from Nautilus, Dolphin, Thunar, Firefox and Chromium arrive. |
| **S2** DnD out | Does a multi-file `FileList` drag land in native and Flatpak apps? | Works in Firefox (native and Flatpak), Chromium, Thunderbird, Telegram, Nautilus, Dolphin and foot. Portal path confirmed. |
| **S3** MIME census | What do common sources actually offer? | `docs/research/dnd-mime-census.md` |
| **S4** Material & motion | Blur matrix per compositor. Does `adw::SpringAnimation` work with only `adw::init()`? Is a 60 fps snapshot animation achievable on an iGPU? | Matrix, a spring choice, and a fallback look that's signed off |
| **S5** dnd-probe | Can a virtual pointer plus an SCTK probe drive a drag into WayStow in headless sway? | One automated drop test passes in CI |
| **D1** Design | Mockups of every surface | Approved before M1 UI |

### M1 — Shelf MVP
- Workspace and Nix skeleton, copied from Quillway's flake, crane, lints, hm-module and CI.
- `waystow daemon` with IPC, store, stash and expiry. CLI commands `add|list|clear|toggle|status|doctor`.
- EdgeCatcher, ShelfPanel, ThumbStack, drag in and out, remove, pin, Quick Look, multi-monitor.

✅ *Accept:* gather 3 files from 3 directories plus a browser image, then drag all four into an email in one gesture. The items expire on schedule. The DnD drop test passes in headless sway.

### M2 — Actions
Registry, TOML validation, job runner (process groups, progress, cancel, undo), the built-in pack, ActionSheet, drop-on-tile, outputs flowing back to the shelf, action history.

✅ *Accept:* PNG → WebP from the catcher, then undo moves it to Trash. Merging 3 PDFs works. Cancelling ocrmypdf leaves no processes. A user TOML action appears after `waystow reload`.

### M3 — Collections & clipboard
Named collections (create, rename, colour, per-collection expiry), dragging between collections, "Shelve clipboard", text snippets → files.

### M4 — Recent
Indexer plus the RecentDrawer, and dragging from Recent to the shelf or straight out.

✅ *Accept:* a finished download shows up under Downloads within 1 s.

### M5 — Activity
Island, job progress, download watcher, screenshot toast, notification fallback.

### M6 — Release
README with light/dark screenshots (a script, as in Quillway), man page, AUR package, release workflow, v0.1.0.

---

## 7. Testing strategy

- **waystow-core:** unit tests for templating, collision-safe naming, MIME matching, expiry, state machines (catcher, job, drag session), IPC wire format and config validation.
- **waystow-jobs:** run each built-in action on fixtures in temp directories (skipped when the tool is missing). Assert output MIME type, dimensions and page count. Process-group cancellation and SIGTERM-ignoring children (L7). Temp-name → rename success rule (L5).
- **waystow-index:** inotify scenarios (`.part` → final rename, burst writes, ignore rules), xbel parsing, migrations.
- **UI / DnD:** `dev/dnd-probe` plus a wlr-virtual-pointer in an isolated headless sway (own runtime dir, L10/L11). Drag a file from the probe to the strip and assert it lands on the shelf via `waystow list`. Drag a shelf item to the probe and assert the URIs it receives.
  - Screenshots via grim for visual regression.
  - GTK's GL renderer is preferred. `GSK_RENDERER=cairo` keeps transparency, unlike iced's tiny-skia, as a fallback for CI.
- **Manual matrix per compositor:** real apps for S1/S2, recorded in `docs/research/`.

---

## 8. Packaging

- **Nix:** crane + rust-overlay, `nix flake check` (clippy `-D warnings`, tests, fmt, build) on x86_64 and aarch64, a home-manager module (`services.waystow`), and a systemd user service bound to `graphical-session.target`. Adapted from Quillway.
- **Runtime tools** are optional: missing ones hide their actions. The Nix package offers a `waystow-full` variant with the tools on PATH.
- **Non-Nix:** the binary links GTK and gtk4-layer-shell, so there's no single portable tarball (Ubuntu 22.04 ships GTK 4.6). Provide an AUR package and document `cargo build` dependencies. Flatpak is a poor fit (layer-shell plus host tools), so skip it.
- **Licence:** MIT OR Apache-2.0, matching Quillway (proposed).

---

## 9. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Input region changes mid-drag misbehave on some compositor | Catcher feels broken | S1. Fallback to a full-size region while Open. |
| Edge strip steals clicks | Annoyance | Short, configurable strip. Keybind alternative. |
| Flatpak Chromium/Electron ignore the portal | Drag-out fails there | Document it. Native builds work. |
| GTK < 4.24 on user distros | No blur | Tinted fallback. `doctor` explains. |
| Hard to reach macOS-grade motion in GTK | "Beautiful" goal at risk | S4 measures this early. Custom snapshot widgets and springs. |
| No layer-shell on GNOME | Unsupported | Stated clearly. |
| Download progress has no totals | Weaker island | Show rate plus an indeterminate bar. |

---

## 10. Decisions

Resolved (2026-10-01):
- **Rust**, a **standalone app** (author).
- **GTK4 + gtk4-layer-shell**: required by DnD (research).
- **A single binary with an in-process UI** (Quillway L1).

Still open:
1. Licence: MIT OR Apache-2.0 (proposed).
2. Default edge for the catcher: right (proposed).
3. Default expiry: 1 h (proposed). Does "Temporary" expire while other collections persist?

---

## 11. Implementation status (2026-10-02)

**Done and verified** (headless sway end to end; protocols checked on niri 26.04):
- **M0:**
  - DnD in and out of a layer surface (S1/S2) with the `dnd-probe` virtual-pointer client (S5).
  - Material and motion (S4): a spring-driven reveal on our own `Stage` container.
  - MIME peek during the hover (S3, partly; see deferred).
- **M1 Shelf:**
  - Daemon, NDJSON IPC and the CLI.
  - SQLite store, stash and expiry (Temporary only; pinned items stay).
  - EdgeCatcher on any configured edges and monitors, with a handle.
  - The shelf panel: stack, rows, selection, Quick Look and keyboard.
  - Drag-out of the stack, a row or the selection; moving to collections by drag.
- **M2 Actions:**
  - 38 built-in actions (27 command actions tested with real tools).
  - TOML user actions and reload.
  - Process-group runner with progress, cancel and undo; drop-on-tile; the params popover; history (Activity tab).
- **M3 Collections and clipboard:**
  - Collections: create, delete, move, chips as drop targets.
  - Clipboard watch (ext/wlr data-control).
  - `add --clipboard` for files, images (to the stash) and text or URLs.
- **M4 Recent:** downloads (`.part`/`.crdownload` renames), screenshots, watched folders, xbel, copied files; tab with filters; drag out or add.
- **M5 Activity:** the island (jobs with progress ring, downloads with rate, finished result, screenshot toast with actions, messages); notification fallback.
- **M6:**
  - README with screenshots (`scripts/screenshots.sh`).
  - Nix packages, home-manager module, systemd unit, CI (flake check plus e2e).

**Changes from the plan:**
- GTK idiosyncrasies (DECISIONS #13).
- The catcher resizes instead of using an input region (#12).
- No in-place actions (#10).
- Icons drawn with GSK (#13).
- No AUR package or man page yet.

**Deferred / not done:**
- S3 MIME census against real apps (Firefox, Chromium, Nautilus, Dolphin).
- Flatpak drag-out verified in a real session.
- Hyprland, KDE and COSMIC runs.
- Client-side blur (needs GTK 4.24).
- Interactive actions (Trim, Crop, page picker).
- Download totals (would need a browser extension).
- AUR package; man page.
