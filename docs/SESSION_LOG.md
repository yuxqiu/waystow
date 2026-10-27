# Session Log

## 2026-10-01 22:30 UTC — Initial planning
- Turned the idea into `docs/PLAN.md`: product definition, daemon + Quickshell architecture, EdgeCatcher DnD design, action TOML spec, recent indexer, island, visual design language, milestones M0–M6, testing, risks, open decisions.
- Created README, HANDOFF, DECISIONS, RUNBOOK.
- Next: resolve the open decisions, then run spikes S1/S2.

## 2026-10-01 23:10 UTC
- Author chose Rust and a standalone app, and pointed at `../quillway` for lessons.
- Read Quillway's docs (PLAN §11, DECISIONS 1–29, SESSION_LOG, RUNBOOK) and key code (IPC, clipboard watch, Nix module). Distilled lessons L1–L16 into PLAN §2.
- Researched Rust layer-shell toolkits for DnD (`docs/research/ui-toolkit-dnd.md`). iced/iced_layershell has no DnD, so the UI is GTK4 + gtk4-layer-shell. GTK handles the portal drag-out and (≥ 4.24) CSS blur.
- Rewrote PLAN.md (revision 2) and DECISIONS.md. Updated HANDOFF.

## 2026-10-02 05:00 UTC
- Author: licence MIT OR Apache-2.0, configurable edges, only Temporary expires; implement everything.
- Built v0.1.0:
  - core (config, model, actions and templating, naming, catcher state machine, springs, progress, Recent rules, xbel, IPC)
  - store (SQLite)
  - jobs (process groups, cancel, safe renames)
  - wl (clipboard)
  - waystow (daemon, CLI, GTK UI: catcher, panel, island, Quick Look, GSK-drawn icons)
  - dev/dnd-probe
  - Nix packaging, home-manager module, CI, README with screenshots
- Found and fixed in headless sway (DECISIONS #12–17):
  - GTK's 200×200 fallback size.
  - Unmapped transparent surfaces.
  - `gtk::Fixed` resizing the surface while animating, replaced by a `Stage` container.
  - A crossing-invariant abort on rebuilds inside event handlers.
  - The double drop read hanging.
  - A MOVE finish protocol error that disconnected us.
  - Filled symbolic icons.
  - A list rebuild that killed drags.
  - Popovers destroyed by refreshes.
  - The headless harness leaking 23 sway instances; all killed by exact pattern, and the harness now kills the process group.
- Verified: `nix flake check` passes (75 tests incl. 27 real-tool actions), e2e 30/30, the home-manager module evaluates, and doctor on niri finds every protocol. Not committed (repo has no commits).

## 2026-10-26 UTC — Review and archive
- Author raised three problems: too complex for a first version (about 12.2k lines, 5 crates), no reliable visual testing (menus looked wrong), and doubtful value (a file shelf is used rarely).
- Ran three research passes on macOS floating, notch and edge tools and on 2024–26 indie launches (Product Hunt, Show HN, GitHub). Saved them in `docs/research/`.
- The author reviewed and rejected every candidate (see HANDOFF) and decided to archive the project.
- Replaced the README's WIP banner with an archive notice (staged, not committed). Updated HANDOFF.
