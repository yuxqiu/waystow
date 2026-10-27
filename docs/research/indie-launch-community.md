# Indie overlay / notch / menu-bar ideas (2024–2026) for a niri/Wayland power user

Research date: 2026-10-26. Sources: HN Algolia API (Show HN, title-restricted, points >15, since 2024-01-01), GitHub search API (notch, layer-shell, quickshell, niri, menubar topics), web search for r/macapps / niri / Wayland discussions.

**Method caveats**
- Reddit could not be read directly: reddit.com returned a block page and old.reddit returned "Rate limit exceeded ... does not provide free scraping resources for agents". Reddit traction below therefore comes second-hand, through web-search summaries and blogs. Treat it as **unverified**.
- Many comparison blogs for notch apps are written by competing vendors (Notchy, Seam, Brow, CueNotch, MacNotch), so their rankings are biased.
- HN points and GitHub stars were read on 2026-10-26.

Exclusions applied: launcher, clipboard history, notifications, media/now playing, volume/brightness HUD, calendar/weather/sysmon, dock, screenshots/annotation, file shelf, AI agent islands, snapping, notes, color picker, keep-awake, AI chat launchers, text-rewrite popups.

Observation: the notch space in 2025–26 is dominated by agent-status islands (CodeIsland 2.5k★, vibe-notch 2.5k★, coucou 6.5k★, notchi, MioIsland...). Those are excluded. The non-excluded novelty clusters around **video-call ergonomics** (teleprompter, camera mirror, hide-from-screen-share), **health nudges that are context-aware**, **hardware-event inspectors**, and **live mirrors/PiP**.

---

## Candidate list

Wayland key: LS = wlr-layer-shell overlay (fine on niri); BIND = trigger via niri `binds` + `spawn`/IPC; no global pointer/selection for normal clients.

### A. Video-call ergonomics

**1. Notch teleprompter, hidden from screen share**
- Examples:
  - NotchPrompter: https://notchprompter.com, GitHub jpomykala/NotchPrompter, 666★, created 2025-12.
  - notchprompt: GitHub saif0200/notchprompt, **1,390★**, 2026-02.
  - openTeleprompt: ArunNGun, 77★, Tauri.
  - Paid: CueNotch ($29.99, free tier of 3 activations/day), Notchie ($29.99), Moody ($39). Notchy bundles one too ("Cuely").
- Interaction: script text scrolls in a strip directly under the webcam. Voice-paced scrolling advances as you speak and pauses when you stop. The strip is invisible to Zoom/Meet/OBS capture.
- Why it's novel: it places text at the camera so your eyes stay on the lens, and voice pacing replaces a fixed speed. Many indie and paid entrants appeared within 12 months, which signals real demand.
- Traction: 1.4k★ for a single-purpose open-source app, at least 5 paid competitors, and multiple "best notch teleprompter 2026" roundups.
- Wayland feasibility: **good**.
  - Use an LS surface anchored top-center.
  - niri has a `block-out-from "screencast"` layer rule, matched by namespace. If you share a single window, the overlay is not captured at all. If you share the whole monitor, it shows as a **black rectangle**, not invisible.
  - Voice pacing needs only the mic and a small local ASR (Vosk or whisper keyword alignment).
- Linux gap: QPrompt is the only cross-platform option, and it is a normal window. I found no webcam-anchored, hidden-from-capture Linux prompter, but I did not search exhaustively.

**2. Instant camera mirror (hover or hotkey)**
- Examples: Hand Mirror (menu bar, paid "Notch Trigger"), Rearview Mirror (Mac App Store, $2.99, notch hover + ⇧⌘R), and as features in Notchy, NotchNest, Dynamic Notch, Boring Notch (11k★) and Notchify.
- Interaction: hover the top-center or press a key, and a live webcam preview drops down. Moving away closes it.
- Why it's novel: it's a zero-app "how do I look" check right before a call. Nearly every 2025–26 notch app copied it.
- Traction: it's a standard feature in almost every notch app, which suggests broad daily use. Per-app numbers are not available.
- Wayland feasibility: **trivial**.
  - Camera through PipeWire camera portal or v4l2, shown in an LS surface, with a BIND to open it.
  - For hover, a 1-px-tall LS strip at the top edge receives pointer-enter (that's allowed).
  - Add `block-out-from` so the preview never leaks into shares.

**3. Mid-call "key moment" flag pill**
- Example: Trace, https://traceapp.info, Show HN 2026-06-13, **205 pts / 84 comments**.
- Interaction: ⇧⌘K during a call opens a tiny input pill. You type a note, press Enter, and it is pinned at that second of the local recording/transcript. ⇧⌘/ expands the pill into a recap of the last few minutes.
- Why it's novel: it's "bookmark this moment" for live audio. There is no bot and nothing leaves the machine.
- Wayland feasibility: **good**.
  - BIND opens an LS surface with on-demand keyboard focus.
  - Audio comes from PipeWire (mic plus a monitor of the call app's stream).
  - Transcription is optional, and the flag-only version is trivial.
- Overlap: it sits next to "notes" (excluded). It is distinct only if it is tied to a recording timeline.

**4. Region share / "share just this rectangle"**
- Example: clipscreen (X11), https://github.com/splitbrain/clipscreen, Show HN 2024-10-14, **346 pts / 154 comments**, 567★.
- Interaction: draw a rectangle. It becomes a virtual monitor that you share in any call app, and a frame shows what's being shared.
- Why it's novel: the comments are full of Linux users. One wrote, "This exact use case, on Linux, is why our company selected Zoom instead of Meet."
- Wayland feasibility: **partial**.
  - The portal *can* grant a region, but most apps and backends only offer window or monitor.
  - niri's Dynamic Cast Target (25.05) switches between windows and monitors, **not regions**.
  - Workarounds exist: waymirror (region → window), wf-recorder → v4l2loopback (may not work on niri, unverified), or OBS crop.
- Unmet wish: yes. Nick Janetakis's niri screen-sharing post works around it with OBS plus struts.

### B. Live mirrors / PiP

**5. Crop-and-pin live thumbnail of any window region (sticky across workspaces)**
- Inspiration: PowerToys **Crop And Lock** (Windows); ZenCrop is a Windows reimplementation.
- Interaction: press a hotkey, select part of a window (a build log tail, a video, a dashboard tile), and get a small always-on-top **live** thumbnail that follows you across workspaces.
- Linux status: **unmet**. A KDE Discuss thread asks for exactly this and only suggests "a KWin effect could do it". niri issue **#932** ("Pinned/sticky/show on all workspaces floating windows", for PiP and webcam) is still **open**, and #1916 is related. Users have built daemons to fake it: niri-float-sticky (113★), nsticky, a niri-pip-follow gist, and a niri fork (klnwlks/niri-sticky).
- Wayland feasibility: **good, and an elegant fit**.
  - LS surfaces are per-output, not per-workspace, so they are sticky by construction.
  - Get frames from the ScreenCast portal (window source, once per session with a restore token) or from niri's dynamic cast target, then crop client-side.
  - The thumbnail is view-only, like Crop and Lock's thumbnail mode. You can't click through into the source app.

### C. Context-aware health nudges

**6. Break reminder that knows when *not* to interrupt**
- Example: LookAway, https://lookaway.com.
  - Show HN 2025-02-20: 25 pts.
  - Show HN 2026-06-24: **77 pts / 26 comments**.
  - Also on Setapp.
- Interaction: shows a heads-up countdown pill before a break. It auto-pauses during meetings, video, screen recording, fullscreen and typing/dictation, then dims the screen for 20 seconds. It also has blink and posture nudges.
- Why it's novel: the "smart pause" is the product. The author's reasoning is that people uninstall timers that fire mid-meeting. HN comments show both sides:
  - "Consciously taking breaks every 20 minutes has completely [removed] the symptoms."
  - "I use this through Setapp, mostly for posture reminders."
  - Complaint: "long break starting in 1 minute" interrupts mid-writing.
- Linux status: Stretchly, Safe Eyes and Workrave exist but are schedule-dumb. Smart pause looks like the gap (**not exhaustively verified**).
- Wayland feasibility: **good**.
  - Fullscreen and focused app come from niri IPC (`niri msg -j event-stream`).
  - Meeting detection: camera or mic in use, from the PipeWire graph.
  - Screencast active: niri exposes cast state.
  - Idle comes from ext-idle-notify.
  - "Typing right now" can't be read globally. Use a short idle-notify timeout (for example 3 s) as a proxy.
  - The break overlay is LS on the overlay layer.

**7. Posture coach that blurs the screen when you slouch**
- Example: Posturr, now Dorso. https://github.com/tldev/dorso, **2,607★**, 2026-01-24.
  - HN title search found the Show HN at only 17 pts.
  - The author claims it was "solidly #1" on HN for a day, probably under another submission. **Unverified.**
  - It reached the Mac App Store on 2026-02-02.
- Interaction: the webcam tracks face/nose versus shoulders. The screen blurs progressively as you slouch and clears instantly when you sit up.
- Why it's novel: the punishment is graded and ambient, not a notification. Built-in dead-zone and sensitivity settings.
- Related: AirPosture (AirPods head tracking, 26 pts), BlinkMore (eye strain, 43 pts).
- Wayland feasibility: **good for camera** (MediaPipe pose locally).
  - True backdrop blur from a client LS surface needs compositor blur. niri has been landing blur (YaLTeR mentioned an "xray blur"), but **verify** layer-surface support in your niri version.
  - Fallback: a progressive dim or vignette, which needs only an LS surface with alpha.

### D. Hardware-event inspectors

**8. USB-C cable / power-delivery inspector**
- Example: WhatCable, https://github.com/darrylmorley/whatcable.
  - Show HN 2026-05-01: **566 pts / 168 comments**, **9,029★**.
  - A blind user commented: "This will stop me from needing to buy a sbc ... to test cables."
- Interaction: plug in a cable and get a plain-English answer to what it can do (wattage, data speed, DisplayPort, Thunderbolt).
- Linux ports appeared the same day: Zetaphor/whatcable-linux (KDE, 306★, packaged in the AUR) and vzaliva/whatcable-linux-cli. Both are apps or CLIs, **not event-triggered overlays**.
- Wayland feasibility: **excellent**.
  - Data comes from `/sys/class/typec/port*/{cable,partner}`, usb_power_delivery and lsucpd.
  - A udev event on plug pops an LS card near a screen corner for a few seconds.
  - No privileged Wayland access needed.
- Novel angle for Linux: an "event card" overlay rather than an app you have to open.

**9. Device-connect card (Bluetooth or peripheral)**
- Concept: an AirPods-style card on connect, showing battery from BlueZ `Battery1` and the codec or profile in use.
- macOS ships this natively. Linux has no equivalent overlay; desktops only send a notification. **Demand is unverified**: this is my extrapolation.
- Related demand: the CrystalClear Sound HN post (223 pts / 309 comments) was about AirPods dropping to a low-quality headset profile. A card that warns "mic active → dropped to HFP" would address the Linux version of that pain (PipeWire exposes the profile).
- Wayland feasibility: **excellent** (D-Bus plus LS).

**10. USB device tray**
- Example: MenuBarUSB, 613★, 2025-08.
- Interaction: lists attached USB devices, which helps debug "is it enumerated?"
- Wayland feasibility: trivial. Low novelty.

### E. Developer glanceables

**11. Listening-ports / dev-server panel with kill**
- Examples:
  - Port Kill: https://github.com/treadiehq/port-kill, Show HN 2025-08-24, **128 pts / 48 comments**, **2,040★**.
  - Blink, "a little robot in your menu bar watching dev servers & simulators", 187★, 2026-03.
  - kftray (port-forward GUI).
  - RelayBar (SSH forwards, 45 pts).
- Interaction: a bar-anchored dropdown lists every listening port with its process and project dir. Each row has kill, open-in-browser and copy URL.
- Why it's novel: it replaces `lsof -i | grep LISTEN; kill`.
- Wayland feasibility: **excellent** (`/proc/net/tcp*`, `ss`, LS dropdown).

**12. Shortcut cheat-sheet on hold**
- Examples: KeyClu (free, double-tap and hold ⌘), Kommand. On Wayland: deskmat (13★), a GTK4-LS tldr cheat sheet.
- niri already ships a hotkey overlay for its *own* binds. The novel part would be *per-app* shortcuts keyed by the focused app_id from niri IPC.
- Wayland feasibility: **BIND only**. You cannot detect "hold ⌘" globally, so use a dedicated bind instead.
- Low-medium novelty.

**13. niri minimap overlay**
- Example: nirimap, https://github.com/alexandergknoll/nirimap, 29★, 2025-12.
- Interaction: a tiny strip at a screen edge shows the columns of the scrolling workspace and where your viewport sits.
- Why it's novel: it's a scrolling-WM-native affordance that macOS and Windows don't need. niri's built-in Overview is modal; a minimap is ambient.
- Wayland feasibility: **excellent**. niri IPC exposes windows and layout, and the minimap is an LS surface.
- Traction is low, but it's niche and niri-specific.

### F. Focus helpers

**14. Dim everything except the focused window**
- Examples: Zen Mode (Show HN 2026-07-12, 30 pts, 68★) and a crowd of clones (Dimmer, VeilWin, FocusFrame, Ombrelle, Window Focus).
- One commenter wished it could focus *multiple* windows from different apps together.
- On niri, window rules (`match is-focused=false` → `opacity`) already get most of the way there natively. **Low value for niri. Skip.**

**15. Edge / hot-corner custom actions**
- niri hot corners only toggle the Overview; corner selection arrived in 25.11.
- Requests:
  - Discussion #1494 asks for arbitrary commands on all four corners.
  - #1473 asks for pressure/barrier triggering.
  - Workaround: waycorner (152★).
- Feasible client-side with thin LS edge strips plus dwell or pressure heuristics. It's useful mainly as the **trigger layer** for other overlays (camera mirror, teleprompter, PiP), not as a product on its own.

**16. Floating countdown pill**
- Examples: Super Easy Timer, ClyTimer (click the notch to create a timer), TinyTimekeeper, wlapse (Wayland stopwatch overlay, 11★).
- Unmet even on macOS: none of the results auto-showed **"time left in the current meeting"** from the calendar. That idea is calendar-adjacent, which you excluded, so this is only a mention.

### G. Saturated or already solved on Linux (listed so you can skip them)

**17. Hold-to-talk dictation pill**
- macOS examples: Ghost Pepper (467 pts), Whispering (591 pts), BetterWispr (220 pts), Yap, FnScribe.
- **Linux is already saturated:** whisrs (tested on niri), mavor, whisper-overlay, voxtype, OpenHyprWhisper, utter, shuvoice.

**18. Live captions overlay**
- Linux already has LiveCaptions (abb128), Local-Live-Captions, subtidal (LS, docked or floating) and whisper-live. Saturated.

**19. Radial / pie menu**
- Examples: Pie Menu (309 pts / 130 comments), Kando (6.5k★, *partial* niri support since 2.0 via `kando --menu` binds), waypie (LS, tested on niri). Already covered.

**20. Keystroke / gesture visualizer**
- Examples: showmethekey, touchview (LS), screenkey-wayland clones. Covered.

**21. Screen OCR / translate overlay**
- Example: kizurium-translator (tested on niri). Covered. TextSniper-style tools are mature.

**22. Wrong-keyboard-layout fixer**
- Example: keyboop (296★, 2026-06). Retypes the last word in the correct layout.
- **Infeasible-ish on Wayland:** it needs global key capture (evdev/root) plus virtual-keyboard injection. Skip.

**23. Keyboard-grid clicking**
- Examples: Homerow / Mouseless. Linux already has wl-kbptr and warpd.

### H. Long tail (low fit or excluded-adjacent)

**24. "Where to click" guidance overlay**
- Example: Ourguide (52 pts). A VLM finds the target and the overlay highlights it.
- Feasible with portal screenshots plus LS, but heavy (VLM) and close to "AI assistant".

**25. Any-app unread badge in the bar**
- Example: Badgeify (118 pts / 53 comments).
- Linux only has the Unity `LauncherEntry` D-Bus signal, which some apps emit. Partial.

**26. Keyboard-driven menu-bar command center**
- Example: ExtraBar (60 pts). Overlaps with a launcher.

**27. Smart-home panel**
- Example: Itsyhome (HomeKit, 57 pts). The Linux equivalent would be a Home Assistant LS panel. Niche.

**28. Continuous screen history with a hotkey timeline**
- Example: screenpipe (22k★). It is now a YC company, which makes it mature, and it is privacy-heavy.

**29. Personal-metric glance**
- Example: Notch Buddy (revenue, traffic). Generic dashboard. Skip.

**30. Fun / novelty**
- Examples: SpinWin (rotate windows), window-sweaters (822★), MacBook lid-angle sensor (60 pts), and desktop mates.
- They show that delight sells, but they aren't daily tools.

---

## Unmet wishes in Linux / Wayland / niri communities (evidence)

- **Sticky PiP / pin on all workspaces**: niri #932 (open), #1916. At least 4 community workaround daemons or forks exist. → Candidate 5.
- **Share a screen region**: the clipscreen HN thread; niri's dynamic cast is window/monitor only; the OBS workaround post. → Candidate 4 (and 5).
- **Crop and Lock equivalent**: a KDE Discuss brainstorm asks for it, and no finished tool exists. → Candidate 5.
- **Hot corners running custom commands, with pressure**: niri #1494 and #1473. → Candidate 15.
- **Screen magnifier**: niri #1024 / PR #3246. This belongs in the compositor, not a client: wooz exists, and an LS magnifier would need screencopy of what's under the pointer, which it can't know globally. **Not a good client-side project.**
- **Toggle `block-out-from` per window with a keybind**: mentioned as an open feature request in the Janetakis post (**unverified** on GitHub). Relevant to the teleprompter and camera mirror.
- **Smart-pausing break reminder, webcam-anchored teleprompter, cable inspector overlay**: I found no Linux equivalents (absence not exhaustively verified).

---

## Top 5 picks

1. **Sticky live crop-and-pin mirror (PiP for anything)** (Candidate 5, with 4 folded in)
   - It fixes a *documented, still-open* niri pain (#932) in a way that is natural for niri: layer-shell surfaces don't belong to workspaces.
   - It brings PowerToys Crop and Lock, a feature Linux users explicitly ask for, to Linux.
   - The same capture-and-crop pipeline could later expose the cropped region as a shareable window, which addresses the clipscreen/region-share demand (346 pts).
   - Daily uses: watching a build or log tail, a video, or a dashboard tile while scrolling columns.
   - Risk: the portal picker appears once per source. Use restore tokens.

2. **Webcam-anchored teleprompter + camera mirror ("virtual notch")** (Candidates 1 + 2)
   - It's the strongest non-excluded indie cluster of 2025–26: 1.4k★ notchprompt, at least 5 paid apps, and the camera mirror is copied everywhere.
   - niri's `block-out-from "screencast"` layer rule gives you share-safety natively.
   - Hover strip or bind; voice pacing needs only the mic.
   - It's daily for anyone on calls or recording.

3. **Context-aware break + posture coach** (Candidates 6 + 7)
   - LookAway proves "smart pause" is what makes people keep a break app (two Show HNs, Setapp users who say they use it daily).
   - Posturr/Dorso (2.6k★) proves the ambient-blur punishment idea.
   - On niri all the signals you need are available without hacks: IPC for fullscreen, the PipeWire graph for camera/mic in use, and idle-notify.
   - Use dim if blur isn't available.

4. **Hardware event cards: USB-C cable/PD on plug, Bluetooth device/profile on connect** (Candidates 8 + 9)
   - WhatCable is the biggest Show HN of the set: 566 pts, 9k★.
   - Linux has the data in sysfs and BlueZ, and same-day ports proved the demand, but no one did it as a transient overlay.
   - Cheap to build, no Wayland privilege issues, and a genuinely new interaction on Linux.
   - Keep it distinct from generic notifications: it's an inspector card with structured capability rows.

5. **Dev ports panel** (Candidate 11), with **niri minimap** (Candidate 13) as runner-up
   - Port Kill (128 pts, 2k★) and Blink show developers want "what's listening and kill it" one click away.
   - Fully feasible from `/proc` and useful daily for a power user.
   - The niri minimap is the more niri-native pick, but its traction is unproven (29★).

**Skip list:** dictation, live captions, pie menu, keystroke visualizer, screen OCR, focus dimming (niri rules already do it), magnifier (belongs in the compositor), and the layout fixer (infeasible).

---

## Key URLs
- WhatCable https://github.com/darrylmorley/whatcable · HN https://news.ycombinator.com/item?id=47972511 · Linux port https://github.com/Zetaphor/whatcable-linux
- clipscreen https://news.ycombinator.com/item?id=41837204
- Pie Menu https://news.ycombinator.com/item?id=41160268 · Kando niri https://kando.menu/installation-on-linux/
- LookAway https://news.ycombinator.com/item?id=48659483
- Posturr/Dorso https://github.com/tldev/dorso · https://tomjohnell.com/posturr-a-macos-app-that-blurs-your-screen-when-you-slouch/
- Trace https://traceapp.info · https://news.ycombinator.com/item?id=48521236
- Port Kill https://github.com/treadiehq/port-kill · https://news.ycombinator.com/item?id=45000982
- Zen Mode https://news.ycombinator.com/item?id=48884663
- Ourguide https://news.ycombinator.com/item?id=46769422
- NotchPrompter https://notchprompter.com · notchprompt https://github.com/saif0200/notchprompt
- Notchy (camera mirror + teleprompter) https://notchy.dev
- niri #932 https://github.com/niri-wm/niri/issues/932 · niri-float-sticky https://github.com/probeldev/niri-float-sticky
- niri hot corners https://github.com/niri-wm/niri/discussions/1494 · https://github.com/niri-wm/niri/issues/1473
- niri zoom https://github.com/niri-wm/niri/issues/1024
- niri screencasting / block-out-from https://niri-wm.github.io/niri/Screencasting.html · Layer rules https://niri-wm.github.io/niri/Configuration:-Layer-Rules.html
- Region share on niri https://nickjanetakis.com/blog/screen-sharing-with-niri-including-specific-regions-with-obs
- Crop and Lock on KDE brainstorm https://discuss.kde.org/t/is-it-possible-to-implement-a-feature-similar-to-the-crop-and-lock-reparent-function-in-windows-powertoys/26973
- nirimap https://github.com/alexandergknoll/nirimap
- Linux dictation saturation: https://github.com/y0sif/whisrs · https://github.com/oddlama/whisper-overlay · https://github.com/mschulkind-oss/mavor
- Linux live captions: https://github.com/abb128/LiveCaptions · https://github.com/jslandau/subtidal
