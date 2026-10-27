# Indie overlay / edge / notch / hotkey utilities (2024–2026): novel ideas for a Wayland daily driver

Researched 2026-10-26 with web search (Product Hunt pages, HN, MacStories/MacRumors roundups, Setapp catalog, GitHub).
Search snippets were the main evidence. Where a number comes only from a vendor's own site or a third-party aggregator, the entry says so. Dates labelled "PH page" are approximate because PH "launched X ago" labels and metadata disagree.

Excluded per brief: launcher, clipboard, notifications, media/now-playing, volume/brightness HUD, calendar/weather/sysmon, dock, screenshots/annotation, file shelf, AI-agent status islands, window snapping, notes, color picker, keep-awake, generic AI chat, text-rewrite popups.

Wayland feasibility key: LS = a layer-shell surface works (anchored, click-through via an empty input region); HK = needs a compositor keybinding (Hyprland `bind`/`bindr`, sway `bindsym --release`) calling the app over IPC/DBus; CAM/MIC = fine; CAP = screencast portal / wlr-screencopy; INJ = text/input injection through the virtual-keyboard protocol (wlroots), `wtype`/`ydotool` (uinput), or the RemoteDesktop portal. On GNOME, injection is the weak spot.

---

## Candidate ideas (24)

### A. Body / health nudges driven by the camera
1. **Posture → progressive screen blur** — Posturr, now Dorso (https://github.com/tldev/posturr, https://tomjohnell.com/posturr-a-macos-app-that-blurs-your-screen-when-you-slouch/). ~Jan 2026.
   - Interaction: the webcam tracks the nose-to-shoulder offset, and the whole screen blurs more the further you slouch. Sitting up clears the blur.
   - Novel because the penalty is graded, ambient and impossible to ignore, where other apps send a notification you can dismiss.
   - Traction: HN 611 points / 194 comments; ~2.3k GitHub stars (AlternativeTo figure); GIGAZINE coverage; MIT.
   - Wayland: CAM fine (MediaPipe/OpenCV pose). A real blur of other apps' content would need CAP frames re-rendered on an LS overlay, which is expensive. Cheap substitutes that work: a progressive dim/tint/vignette on an LS overlay, or a desaturating shader where the compositor supports one (Hyprland `screen_shader`). Feasible.
2. **Body-focused-habit detector (nail biting, face touching, hair pulling)** — NailBiteMenu (HN Show, 2025), Nailed ($4.99), Hands Off, Awaira, Habitron, Habit Guard (https://github.com/killian31/NailBiteMenu, https://habitron.app/, https://awaira.app/).
   - Interaction: the webcam spots hand-to-mouth or hand-to-face motion, and the app flashes the screen or beeps.
   - Novel: a cluster of new paid apps in 2025–26 suggests real demand.
   - Traction: several paid competitors; Show HN. Upvote counts were not verified.
   - Wayland: CAM plus an LS flash. Fully feasible.
3. **Smart break / blink / posture reminders** — LookAway (https://lookaway.com/, PH 2024-03-21, LookAway 2 on PH 2026-04-08; Setapp).
   - Interaction: a timed full-screen "rest" overlay that pauses itself during meetings, recording and fullscreen video, plus a small countdown that follows the cursor.
   - Novel parts: context-aware pausing and the pre-break countdown. The break timer itself is mature (Stretchly, Time Out, Safe Eyes).
   - Traction: two PH launches; reviews mention daily use; on Setapp and the App Store.
   - Wayland: LS full-screen overlay. Meeting detection via PipeWire (is the camera or mic in use?) or the idle-inhibit protocol. A cursor-following countdown is impossible, so anchor it to a corner instead. HealthyNotch (PH) does the same from the notch.
4. **Shoulder-surfer auto-blur** — BlurAway (one search snippet only, **unverified**).
   - Interaction: if the camera sees a second face, the screen blurs.
   - Wayland: CAM plus an LS opaque or dim overlay. Feasible.

### B. Meetings, camera and voice
5. **One-click camera mirror** — Hand Mirror (older, still active), plus mirrors inside NotchNook, Notchy and NotchNest (https://notchy.dev/mac-camera-mirror/).
   - Interaction: hover or tap the notch (or a hotkey) to get a live webcam preview for a "do I look ok" check before a call.
   - Traction: every notch app adds it, and Hand Mirror has a paid "Plus" tier.
   - Wayland: CAM plus an LS popup at the top edge on a hotkey or hot edge. Trivial.
6. **Screen-edge ring light** — LightBuddy (Guilherme Rambo), RingLight (https://github.com/itsOmSarraf/ringlight). Apple shipped "Edge Light" in macOS 26.2 (Dec 2025).
   - Interaction: a bright, warm-white glowing frame around the screen edges lights your face on video calls, with a hole left where the pointer is.
   - Novel: Apple shipping it as a headline feature validates the idea.
   - Traction: 9to5Mac, Engadget and Forbes coverage of Edge Light.
   - Wayland: four LS edge strips (or one full-screen LS with an empty input region) at max brightness. Auto-on when the camera is active is possible by watching PipeWire camera nodes. The pointer hole is not possible, but the overlay is click-through anyway.
7. **Always-visible mic-mute state plus global mute / push-to-talk** — Mutify, "Am I on Mute?", teenymute, Notchy (https://notchy.dev/mac-mic-mute/, https://teenymute.com/).
   - Interaction: a persistent pill or border shows whether the mic is live, and one hotkey mutes the source system-wide.
   - Novel: system-level rather than per-app mute, with an edge flash on toggle.
   - Traction: many small paid apps (vendor comparison pages).
   - Wayland: `wpctl set-mute @DEFAULT_SOURCE@` plus an LS pill. HK via the compositor; push-to-talk needs release binds (`bindr`/`--release`). Feasible.
8. **Notch / top-edge teleprompter with voice-synced scroll, hidden from screen share** — Notchie (https://www.notchie.app/, PH Dec 2025, #4 of the day, ~295 upvotes per its site), Moody, CueNotch, NotchPrompter, Textream.
   - Interaction: the script sits right under the camera so your eyes look at the lens, and it scrolls at your speaking pace.
   - Novel: the placement solves eye contact, and local ASR follows your speech.
   - Traction: five competitors within about a year; on Setapp. One PH commenter said the voice sync was flaky.
   - Wayland: LS top-center plus MIC with a local ASR (whisper.cpp/Vosk). Exclusion from screen share is **not generally possible**: portal casts of a whole output include layer surfaces. Some compositors have per-window no-screenshare rules (Hyprland `noscreenshare`, **unverified** version), and casting a single window avoids the problem.
9. **Live captions + translation overlay for any system audio** — VoxTori (PH 2026), Seagull (PH, Mac/Win/Linux), LiveCaptions.app, EchoCap, SubtitleOverlay (https://www.producthunt.com/products/seagull-subtitles-for-everything, https://livecaptions.app/).
   - Interaction: a click-through, always-on-top subtitle band translates whatever is playing (calls, videos, streams).
   - Novel: per-app or all-system audio, on-device ASR plus translation, usable during meetings.
   - Traction: lots of 2025–26 entrants; Apple added Live Captions without translation, which leaves a gap.
   - Wayland: PipeWire monitor or per-app capture, whisper/Parakeet ASR, an LS bottom band with an empty input region. Very feasible. Linux already has abb128/LiveCaptions (a GTK window, English only, no translation, not layer-shell), so the gap is translation plus a proper overlay.
10. **Push-to-talk dictation pill** — VoiceInk (GPL, https://github.com/Beingpax/VoiceInk, ~6.5k stars per a review), Superwhisper, Wispr Flow, Spokenly. PowerToys 0.102 added dictation to ZoomIt.
    - Interaction: hold a key, a small waveform pill appears at the screen edge, and the text is typed into the focused app when you release.
    - Novel: borderline mature on macOS, but it's daily-use for many and still rough on Linux.
    - Traction: several paid competitors publish comparison pages against VoiceInk.
    - Wayland: MIC fine; LS pill; HK hold/release via `bindr`. INJ is the hard part: `wtype` works on wlroots and Hyprland, GNOME needs the RemoteDesktop portal or `ydotool`, and a clipboard-paste fallback goes through data-control. Feasible with caveats.
11. **Live speaking coach overlay** — Poised (shut down 2026-10-08 per its own site), Yoodli live.
    - Interaction: during calls, an overlay warns about filler words, pace and your share of talk time.
    - Traction: Poised shutting down is a negative signal.
    - Wayland: MIC plus ASR plus LS. Feasible, but the market evidence is weak.

### C. Time and attention at the edges
12. **Thin edge timebar** — Timebar by VelvetShark (https://velvetshark.com/timebar, https://github.com/velvet-shark/timebar, ~Sep 2026), TimeBarX, ProgressTimer, Progress Bar OSX (PH).
    - Interaction: a timer drawn as a 0.5–8 pt line under the menu bar or along the top/bottom edge that shrinks as time runs out. It is click-through and never takes focus.
    - Novel: peripheral-vision time awareness with zero UI chrome.
    - Traction: modest (a blog post plus OSS); the "day/year progress" variant has a PH history.
    - Wayland: an LS anchored to an edge with an empty input region. One of the easiest ideas here, and it pairs naturally with a bar or OSD.
13. **Periodic "what are you doing?" sampling** — Daily – Hours & Time Tracker (Niels Mouthaan; App Store 4.6).
    - Interaction: every N minutes a small prompt asks which project you're on, and you get timesheets without starting or stopping timers.
    - Not new (pre-2024), but rare on Linux.
    - Wayland: LS popup with keyboard focus (on-demand interactivity). Idle detection via ext-idle-notify. Feasible.
14. **Intention gate before distracting apps** — one sec for Mac (https://one-sec.app/mac/), Opal, Jomo.
    - Interaction: launching a flagged app triggers a breathing animation plus "why are you opening this?" and shows how many attempts you've made today.
    - Wayland: there's no generic hook to intercept app launch. Options are compositor IPC (watch for new windows of a class, then cover them with an LS overlay) or wrapping `.desktop` Exec lines. Partly feasible; Hyprland/sway IPC makes it practical.
15. **"One Thing" pinned focus line** (MacRumors "apps for 2026", Dec 2025; launch date **unverified**).
    - Interaction: one sentence for today's main task, always visible in the bar.
    - Borderline "notes"; included only as a bar module idea.
    - Wayland: trivial (a bar module).
16. **Ambient spaced-repetition flashcards** — PopCards, Wunderbar (Show HN), Pikard, Ficha (https://popcards.app/, https://github.com/lsst25/Pikard).
    - Interaction: every N minutes a tiny card slides in from a corner. You flip it, grade it, and it disappears.
    - Novel: learning in the cracks of the day, without opening Anki.
    - Traction: small (OSS plus a Show HN with criticism).
    - Wayland: LS corner card with on-demand keyboard. Can read the Anki collection (sqlite) or AnkiConnect. Feasible.

### D. Keyboard / input awareness
17. **Shortcut cheat-sheet on double-tap or hold of a modifier** — Keymap (https://keymap.app/, PH ~Aug–Sep 2026), KeyClu, CheatSheet (old). PowerToys redesigned Shortcut Guide in v0.100 (Jun 2026, third-party changelog, **unverified**). HotkeyClash (PH 2026-06-23, 83 upvotes, #25) adds conflict detection.
    - Interaction: double-press a modifier and a searchable overlay of the current app's (and the system's) shortcuts appears.
    - Novel: built automatically from live data rather than a hand-written database, with conflict detection.
    - Wayland: app menus aren't introspectable, but compositor binds are (`hyprctl binds -j`, sway config, niri config). A WM-binding overlay plus per-app user files, with conflict detection between WM binds and app binds, is very valuable for tiling users. Double-tapping a modifier can't be detected as a compositor bind (sometimes possible with `bindr` on SUPER release); a normal chord is fine.
18. **Keyboard-layout indicator on switch** — Input Source Pro, Keyla, InputMethodHinter.
    - Interaction: a large badge shows the new layout when it changes or when you focus a text field.
    - Wayland: no cursor position, so show it centered or at a corner. Layout comes from compositor IPC (Hyprland `activelayout` event, sway input events) or fcitx5 DBus. Feasible, but it's close to the excluded OSD/HUD category.
19. **Typing sound synthesis** — Klack, Klakk (PH Indie list 2025-10-30), FunKey 3.0.
    - Not an overlay. Global key events aren't available to normal Wayland clients, so it needs evdev/input-group access. Low priority; listed for completeness.
20. **Webcam hand-gesture commands** — Pawvis (PH 2026-08-22, 100 upvotes, #7, FOSS), Airpoint, AirTouch, Gstrl.
    - Interaction: hand gestures move the cursor or trigger mapped actions.
    - Wayland: CAM fine. Gesture-to-command (running `hyprctl dispatch …`) is easy; cursor control needs uinput or the RemoteDesktop portal. Partly feasible.

### E. Privacy and presence
21. **Screen-share privacy masks** — ScreenVeil (PH ~Sep 2026), Scrim, Cloaky, Veil, Hush, All Blur (PH 2025).
    - Interaction: pin opaque or blurred floating panels over sensitive regions or windows, or blur everything except the active app, with auto-enable when sharing starts.
    - Wayland: LS opaque panels over regions do appear in output casts, so they work. Window-attached masks need compositor IPC for geometry. Detect sharing via PipeWire video-source nodes or the portal session. Feasible.
22. **Desktop pet / companion** — CozyPet (PH Jul 2025), Docko (PH Mar 2025, #8), Mochi (a pet that tidies loose Desktop files), Paw-Paw (claims PH top 10; reacts to typing).
    - Interaction: a small sprite walks along the bottom edge, naps when you're idle, and reacts to events.
    - Novel angle: pets doubling as ambient status (Mochi tidying files; NotiSprite delivering reminders).
    - Wayland: LS bottom strip with an empty input region except over the sprite. Idle via ext-idle-notify. Reacting to typing isn't possible without evdev. Feasible. Shimeji-style Linux projects exist but are mostly X11.

### F. Panels and misc
23. **Web app in a menu-bar popover** — MenubarX (Setapp).
    - Interaction: pin any URL (e.g. a translation site or a dashboard) as a persistent mini-browser panel that drops down from the bar on a hotkey.
    - Wayland: LS panel with WebKitGTK. Feasible, but borderline "generic".
24. **Pin any window on top with opacity and click-through** — Floaty (Show HN, ~Dec 2025, $7).
    - Interaction: picture-in-picture for any window.
    - Wayland: there's no client-side way to do this; it's a compositor feature (Hyprland `pin`, `opacity`, `nofocus`). Skip as a separate app, but a picture-in-picture of any window via CAP onto an LS thumbnail is possible.

Also seen but excluded or not novel: Lounge (a Bartender alternative with an AI-agent island), ExtraBar (floating command bar, launcher-like), Supercharge, Folder Preview, Substage (natural-language terminal in Finder), Screenpipe (screen recall, YC S26, Linux/Wayland support claimed in its newsletter; launcher/AI-adjacent).

---

## Top 5 for a Linux/Wayland daily driver

1. **Live captions + translation overlay (#9).** Many 2025–26 entrants, all Mac/Win-first. The Linux pieces (PipeWire monitor capture, whisper.cpp/Parakeet, layer-shell click-through band) are all first-class. The Linux incumbent has no translation and no overlay. Daily use: calls, foreign-language video, streams.
2. **Posture / habit camera nudges with progressive dim (#1 + #2).** Posturr's HN response (611 points) is the strongest grassroots signal found, and camera access is one of the things Wayland doesn't restrict. A graded vignette or dim (rather than a true blur) on layer-shell is cheap. One engine (pose plus hand landmarks) covers both slouching and face-touching.
3. **Edge timebar / ambient progress line (#12).** Probably the smallest build here, and fully native to layer-shell (anchored, zero input region). Useful all day for pomodoro sessions, meeting end times and "time left in the workday". It complements a waybar-style bar instead of competing with it.
4. **Compositor-aware shortcut overlay with conflict detection (#17).** Keymap, HotkeyClash and PowerToys Shortcut Guide all shipped or were redesigned in 2026. On Linux the forgotten shortcuts are mostly in the WM config, which `hyprctl binds -j` and sway/niri configs expose cleanly. That makes it more reliable than the Mac versions, which scrape app menus.
5. **Push-to-talk dictation pill (#10)**, or, if text injection on GNOME is a dealbreaker, the **mic-mute state pill + push-to-talk (#7)**. Dictation has the highest everyday value, and demand is visible in the competition around VoiceInk. On wlroots/Hyprland, `bindr` plus `wtype`/virtual-keyboard makes it work today. The mute pill is the zero-risk fallback: `wpctl` plus layer-shell.

Honourable mentions: the screen-edge ring light (#6, trivial and Apple-validated) and the hotkey camera mirror (#5).

## Unverified / caveats
- BlurAway (shoulder-surfer blur): seen in a single snippet.
- Notchie's 295 upvotes come from its own site, not the PH page.
- Paw-Paw "PH top 10": its own claim.
- The PowerToys v0.98–0.101 feature list comes from a third-party changelog (powertoy.net).
- Hyprland `noscreenshare` rule: whether it exists and in which version is unverified.
- VoiceInk star count comes from review sites, not GitHub directly.
- "One Thing" launch date: unknown.
- Screenpipe's Wayland support comes from its newsletter title only.
