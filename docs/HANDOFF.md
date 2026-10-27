# Handoff

Last updated: 2026-10-26 UTC

## Current state
- **Archived.** Development stopped by the author on 2026-10-26. The README has an archive notice; the code is at `bb5e7ed` plus the README change (staged, not committed).
- Why:
  - A file shelf is used rarely (Homebrew: Yoink 671, Dropzone 618 installs a year, against 100k+ for launchers and clipboard managers).
  - Three research passes found no daily-use floating-utility gap that DankMaterialShell doesn't already cover or that wouldn't fit better as a DMS plugin.
  - Research: `docs/research/macos-floating-tools.md`, `indie-launch-sites.md`, `indie-launch-community.md`.
- Ideas the author reviewed and rejected:
  - agent-status island
  - screenshot overlay
  - crop-and-pin mirror
  - smart breaks/posture (DMS has breaks)
  - plug-in hardware cards (would be a DMS plugin)
  - teleprompter
  - edge time bar
  - ports panel
- Worth reusing in future layer-shell work:
  - The GTK/Wayland quirks in DECISIONS #12–17.
  - `scripts/headless.sh` (isolated headless sway) and `dev/dnd-probe` (virtual-pointer drag-and-drop driver).
  - The lesson that UI tests need deterministic golden screenshots, not just behaviour checks: menus looked wrong and no test caught it.

## Top 3 next actions
1. Commit the README archive notice, then archive the GitHub repository (`gh repo archive yuxqiu/waystow`) if it is published.
2. For the next project, collect a week of real desktop friction first, then decide between a DMS plugin, a Quillway mode or a new tool.
3. None for WayStow itself.

## Blockers
- None.
