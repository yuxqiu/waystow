# macOS floating, notch, edge and menu-bar utilities, mapped to niri + DMS on Wayland

Researched 2026-10-26. Sources are linked inline. "Unverified" marks claims I could not confirm.

## How usage was measured

No public data says how often people use each app. I used three proxies:

1. **Homebrew cask installs over 365 days** (2025-10-26 to 2026-10-26), from
   https://formulae.brew.sh/api/analytics/cask-install/365d.json. This counts installs, not
   usage, and it favours free and open-source apps and developers. Paid apps sold through the
   App Store, Setapp or their own site are undercounted. That affects NotchNook, Dropover,
   CleanShot, Paste, PopClip, Hand Mirror, Tot and Klack.
2. **Self-reported frequency** in reviews ("daily", "hundreds of times a day", dictation counts).
3. **GitHub stars** for open-source apps.

Homebrew 365-day installs, for scale:

| Cask | Installs | Cask | Installs |
|---|---|---|---|
| stats (menu-bar monitor) | 145,190 | itsycal | 10,891 |
| raycast | 116,337 | wispr-flow | 10,913 |
| rectangle | 116,057 | espanso | 10,659 |
| maccy | 111,037 | easydict | 10,030 |
| betterdisplay | 92,814 | macwhisper | 9,750 |
| alt-tab | 63,493 | claude-usage-tracker (tap) | 9,630 |
| jordanbaird-ice (menu bar) | 58,649 | superwhisper | 8,715 |
| caffeine | 31,615 | voiceink | 8,677 |
| dockdoor (window previews) | 32,219 | opensuperwhisper | 8,527 |
| hiddenbar | 29,136 | cleanshot | 7,880 |
| shottr | 26,584 | loop | 7,309 |
| alfred | 24,277 | snipaste | 6,742 |
| flameshot | 18,669 | meetingbar | 6,584 |
| boring-notch (tap) | 15,686 | claudebar | 5,730 |
| keepingyouawake | 14,559 | pika | 3,905 |
| fluidvoice | 12,103 | alcove | 3,115 |
| | | popclip | 3,111 |
| | | hazel | 2,513 |
| | | codeisland (AI-agent notch) | 2,242 |
| | | notchnook | 1,992 |
| | | paste | 1,245 |
| | | textsniper | 1,077 |
| | | yoink | 671 |
| | | dropzone | 618 |
| | | unclutter | 281 |
| | | rewind | 13 |

Homebrew has no cask for Dropover, Hand Mirror, Tot, Lungo, Supercharge, ColorSlurp, Klack,
Amphetamine or Screenpipe.

## App by app

**Notch hubs**
- **NotchNook** (lo.cafe). Where it lives: the notch, opened by hover or swipe. It shows media,
  calendar with join buttons, notes, to-dos, timers, a camera mirror and a file tray with AirDrop.
  Reviewers say it replaced their media-control and calendar apps. Its strongest daily feature is
  media. Paid; about 2k brew installs.
  https://getnotchnook.com/ · https://macsources.com/notchnook-mac-app-review/ · https://root-nation.com/en/soft-en/apps-en/en-notchnook-review/
- **boring.notch**. Open source, about 11.1k stars, 15.7k brew installs. Where it lives: hover over
  the notch. It shows a music visualizer and controls, calendar, a file shelf (built on NotchDrop)
  and a replacement for the system volume/brightness HUD. Its own testimonials centre on music
  control. https://github.com/TheBoredTeam/boring.notch
- **Alcove** (Henrik Ruscon). Where it lives: the notch. It does HUD replacement, live activities,
  notifications and lock-screen widgets. Reviewers call it the most polished notch app and say it
  "does not deeply change how you work." https://korinotch.com/blog/alcove-mac-review-alternatives.html · https://getseam.app/blog/alcove-vs-boring-notch
- **DynamicLake Pro**. Where it lives: the notch, plus fn+space and fn+tab. It covers the HUD,
  media, calendar and weather, a file clip tray, Bluetooth events, notification replies, a notch app
  switcher and timers. Root-Nation found the widgets distracting in full-screen apps.
  https://thesweetbits.com/tools/dynamiclake-review/ · https://root-nation.com/en/soft-en/apps-en/en-dynamiclake-pro-review/
- **What all notch hubs share.** The core is ambient: now-playing, system HUDs, the next calendar
  event. You glance at it many times a day, but each glance solves little pain. Every one of these
  features is already in DMS. The file trays bundled into these apps are a secondary feature.
- **New since 2025: notches for AI coding agents.** Examples are Vibe Island, CodeIsland,
  Claude Island (now Vibe Notch) and xisland. Each hooks into Claude Code, Codex or Gemini CLI and
  shows whether an agent is working, waiting or needs approval. You approve or deny from the notch
  and jump back to the terminal.
  - Brew installs: codeisland 2.2k, claude-island 0.8k, vibe-island 0.8k. Related menu-bar usage
    meters: claude-usage-tracker 9.6k, claudebar 5.7k.
  - They are used once per agent prompt, which for heavy agent users means dozens of times a day.
  - https://github.com/vibeislandapp/vibe-island · https://github.com/wxtsky/CodeIsland · https://vibeisland.app/best-ai-agent-notch-apps/

**File shelves**
- **Yoink**. Shelf at the screen edge, shown during a drag. 671 brew installs.
- **Dropover**. Shelf appears under the pointer when you shake mid-drag or hold a modifier. Gruber
  has used it for about two years and finds a shelf that appears on every drag distracting.
  https://daringfireball.net/linked/2026/05/15/dropover
- **Dropzone 4/5**. A menu-bar grid with a drop bar plus actions (upload, resize, move to folder).
  Macworld says its drop bar is "not quite as useful as Yoink". 618 brew installs.
  https://www.macworld.com/article/233918/dropzone-4-review.html
- **Unclutter**. Scroll down from the top screen edge to open a drawer with clipboard, files and
  notes tabs. Reviewers say the clipboard tab is "the standout feature". 281 brew installs.
  https://thesweetbits.com/tools/unclutter-app-review/
- **Verdict.** Low install numbers and no strong frequency claims. This agrees with the earlier
  judgment that a file shelf is low-value.

**Text selection, OCR and translation**
- **PopClip**. A popup toolbar appears when you select text. One user says "I use it every day";
  the maker of the similar app Tip says "hundreds of times every day". A 2012 review says the
  popup gets in the way when you select text you don't want to act on. 3.1k brew installs, though
  most sales are outside Homebrew.
  https://www.originalmacguy.com/popclip-the-one-mac-app-i-use-every-day/
- **Easydict**. About 15.1k stars, 10k brew installs. It translates selected text (⌥D), translates
  a screenshot region (⌥S), translates typed input (⌥A) and does silent OCR. It supports many
  engines, including LLMs and local models. https://github.com/tisfeng/Easydict
- **Bob**. About 9.8k stars. Same feature set as Easydict, plus plugins. Its OpenAI translator
  plugin has about 5.7k stars. https://github.com/ripperhe/Bob
- **TextSniper**. Menu bar or hotkey, then drag a region to OCR it to the clipboard. Reviewers say
  they use it "almost daily", but also that its value "depends on your usage frequency".
  https://myappleguide.com/textsniper-review/ · https://thesweetbits.com/tools/textsniper-review/
- **Raycast**. Floating Notes (now Raycast Notes), AI Chat and Translate each open from a hotkey.
  Translate can take the selected text as its source. One user binds grammar fixing and translation
  to hotkeys and says this replaced DeepL and Grammarly.
  https://manual.raycast.com/translate · https://www.raycast.com/blog/raycast-notes · https://medium.com/codex/the-raycast-ai-workflow-that-replaced-5-mac-apps-for-me-347a86defd39
- **Supercharge** (Sindre Sorhus). A bundle of tweaks that includes "Capture text + Translate
  anywhere" (OCR plus instant translation of a screen region), QR scanning, keep-awake, and
  mute-mic-while-typing. https://sindresorhus.com/supercharge

**Screenshots**
- **CleanShot X**. After each capture, a Quick Access Overlay appears in a screen corner with copy,
  save, edit, pin, upload and close. It also pins images on top with click-through, scrolls to
  capture long pages, does OCR and records the screen. Users describe it as the tool they use for
  every screenshot. https://cleanshot.com/features
- **Shottr**. Captures, does OCR and QR, pins to screen, blurs, measures with a ruler and picks
  colors. It ships at about 2 MB. 26.6k brew installs.
  https://setapp.com/app-reviews/cleanshot-x-vs-shottr
- Screenshot tools are among the most-installed free utilities: Flameshot 18.7k, Snipaste 6.7k,
  macshot 6.1k.

**Voice dictation (not on the original list, but now one of the largest categories)**
- **Wispr Flow, Superwhisper, MacWhisper, VoiceInk, FluidVoice, OpenSuperWhisper.** You hold a
  hotkey and a floating pill shows the mic is listening. Text is typed into the focused app and is
  optionally cleaned up by an LLM.
- Together these casks total about 59k installs, which puts the category level with the top
  single utilities.
- Frequency evidence: one user made 5,471 dictations in 90 days, about 60 a day. Another dictates
  about 1,700 words a day. https://mrktcorrect.com/blog/wispr-flow-review · https://www.podfeet.com/blog/2026/03/wispr-flow-scott-willsey/

**Notes**
- **Tot**. A menu-bar app with seven colored scratchpads. Gruber: "the difference between scrap
  paper and a notebook." https://daringfireball.net/2020/02/tot
- **Raycast Notes**. A floating note opened by hotkey (see Raycast above).

**Calendar and meetings**
- **Itsycal**. A menu-bar calendar dropdown. 10.9k brew installs.
- **MeetingBar**. Shows the next meeting in the menu bar, with a one-click or hotkey join and
  full-screen reminders. 6.6k brew installs. https://github.com/leits/MeetingBar
- Both are glanced at several times a day.

**Keep-awake**
- **Amphetamine, Lungo, KeepingYouAwake, Caffeine.** A menu-bar toggle, used occasionally to a few
  times a day. Caffeine 31.6k and KeepingYouAwake 14.6k brew installs.
  https://sindresorhus.com/lungo

**Camera check**
- **Hand Mirror**. Click the menu-bar icon, or the notch, to see your webcam preview. It is used
  before calls, so a few times a week to a few times a day. Free with a Plus upgrade.
  https://setapp.com/apps/hand-mirror

**Color pickers**
- **Pika and ColorSlurp.** A hotkey opens a magnifier to pick a color, with format conversion and
  contrast checking. Pika has 2.6k stars and 3.9k brew installs. Mostly used by designers and
  front-end developers. https://github.com/superhighfives/pika

**Clipboard**
- **Maccy and Paste.** A hotkey opens a searchable history. Used many times a day. Maccy has about
  22k stars and 111k brew installs. https://github.com/p0deje/Maccy

**Automation**
- **Hazel**. Rule-based automation on folders. No visible UI. 2.5k brew installs.

**Window management**
- **Rectangle and Loop.** Window snapping. Rectangle has 116k installs, but niri's scrolling
  tiling makes this category irrelevant.

**Screen memory**
- **Rewind and Screenpipe.** They record your screen around the clock and make it searchable.
  Rewind became Limitless and discontinued the desktop recorder (rewind cask: 13 installs).
  Screenpipe has about 20k stars and joined YC S26.
  https://docs.screenpipe.com/vs-rewind · https://explainx.ai/blog/screenpipe-yc-s26-local-work-memory-agents-july-2026
- Unverified: Meta acquiring Limitless; how well Screenpipe works on Linux/Wayland.

**Novelty**
- **Klack**. Plays mechanical-keyboard sounds as you type. Menu bar. Novelty.
  https://9to5mac.com/2023/04/05/klack-app-mechanical-keyboard-sound-effects-for-mac/

## 1. Categories ranked by how often they are used × how much pain they solve

1. **Clipboard history.** Many uses a day, medium pain. Maccy 111k, Raycast 116k. DMS already
   covers it.
2. **Launcher and command palette.** Many uses a day. DMS covers it.
3. **Voice dictation with a floating indicator.** About 60 uses a day for adopters, high pain.
   The category is about 59k brew installs and growing fast. DMS does not cover it.
4. **Screenshot, then act on it (annotate, OCR, pin, copy).** Several uses a day for developers.
   Shottr 26.6k, Flameshot 18.7k. The win is the "quick-access overlay" that appears after capture.
   DMS takes screenshots. Unverified: whether DMS does OCR or pinning.
5. **AI-agent status and approval panel.** Dozens of uses a day for heavy users of Claude Code,
   Codex and similar agents. Small but fast-growing, with about 20k installs across agent notch
   apps and usage meters. DMS does not cover it.
6. **Translate or look up selected text, and OCR a region.** Many uses a day for people who work
   in a second language, rare for everyone else. Easydict has 15k stars and 10k installs.
   This overlaps heavily with the LLM text-rewrite popup the user already built.
7. **Calendar and next-meeting glance with a join button.** A few uses a day. DMS has a calendar.
   The missing piece is a "join next meeting" action (unverified).
8. **Ambient now-playing and HUD in a notch.** Glanced at constantly, low pain. DMS has media
   controls and OSDs.
9. **Floating scratch note.** Daily for some people. DMS has a notepad.
10. **Keep-awake, camera check, color pick.** Occasional. `dms color pick` exists. Keep-awake is a
    one-line idle-inhibit.
11. **File shelf.** Rare: Yoink 671 installs, Dropzone 618, Unclutter 281.
12. **Screen memory (Rewind-style).** Polarizing and resource-heavy.

## 2. Existing Linux/Wayland answers, and whether DMS covers each

- **Dictation.** Existing tools:
  - Voxtype (Rust): push-to-talk, nine local speech engines, types the text with
    wtype, dotool or ydotool, and falls back to the clipboard. It has no graphical overlay, only a
    state file for status bars. https://github.com/peteonrails/voxtype
  - hyprwhspr (Hyprland-focused).
  - whisper-overlay: a Wayland overlay. Its hotkey is read from evdev and still passes through to
    the focused app. https://github.com/oddlama/whisper-overlay
  - Handy (GUI).
  - None of them is integrated with DMS. A pill/visualizer drawn with layer-shell, plus LLM cleanup
    (reusing the user's local LLM), would be a real gap to fill.
- **Screenshot and annotation.**
  - Existing tools: grim + slurp + satty (or swappy), niri's built-in screenshot UI, and the Tensaku
    fork, which adds scrolling capture. https://nickjanetakis.com/blog/wayland-compatible-annotated-screenshots-with-slurp-grim-and-satty
  - OCR: NormCap, which is active and handles Wayland screenshots through portals.
    https://github.com/dynobo/normcap
  - OCR: Frog. A December 2025 comment calls it "pretty dead". https://github.com/TenderOwl/Frog/discussions/125
  - Gap: I found no tool that offers a CleanShot-style overlay after capture with
    copy, OCR, pin, annotate, translate and ask-LLM in one place. I also found no tool that pins an
    image on top with click-through. Unverified: whether a DMS plugin does this.
- **Screen recording.**
  - gpu-screen-recorder, wl-screenrec (supports niri 25.08) and Kooha (via portal).
  - Existing niri wrappers: a gist script for niri + DMS (https://gist.github.com/AdrianoMoura/4599814443e99c27d53599898fde8a2f)
    and niri-screen-recorder.
- **Translating selected text.**
  - Crow Translate has no global shortcuts on Wayland. You bind a compositor key to its D-Bus call
    `translateSelection`. https://github.com/crow-translate/crow-translate
  - Pot can't find the mouse position on Wayland. Users drive it through its local HTTP API
    (`/selection_translate`, `/ocr_translate`). https://github.com/pot-app/pot-desktop
  - Dialect is a GUI app with no selection hook (from memory, unverified).
  - PopClip clones (LinuxPop, vinoPop, omapop) work natively only on KDE or Hyprland, because only
    the compositor sees the pointer and mouse-button release.
    https://github.com/GaimsDevSoftware/linuxpop · https://github.com/jondkinney/omapop
- **AI-agent notch.** I found no Linux equivalent (unverified). The macOS apps work through agent
  hooks that write to a Unix socket, so the same approach ports directly.
- **Already covered.**
  - Clipboard history, launcher, media, OSD, calendar and notepad: DMS.
  - Color picker: `dms color pick` and the color-picker-dms plugin.
    https://github.com/bernardopg/color-picker-dms
  - Text expansion: espanso.
  - Keep-awake: systemd-inhibit or the idle-inhibit protocol. Unverified: whether DMS has a toggle.
  - Alt-Tab with previews: niri 25.11 recent-windows switcher.
    https://www.phoronix.com/news/Niri-25.11

## 3. What is technically possible on Wayland

- **Layer-shell pills and panels at the top edge or a screen edge:** fine.
- **"Shake while dragging" (Dropover) or "pop up at the selection" (PopClip, Easydict):** not
  possible for a normal client. It cannot see the global pointer position, other windows' contents,
  or where a selection is. The fix is to open at a fixed anchor, such as top-center or the
  "notch" position.
- **Hotkeys:** only through niri `bind` actions that spawn a command or send IPC.
- **Hold-to-talk:** Voxtype's documentation lists key-release binds only for Hyprland, Sway and
  River. Unverified for niri. Plan on toggle mode, or on reading evdev (needs the `input` group).
- **Selected text:** readable from PRIMARY through data-control (wl-paste --primary). Caveat: apps
  re-assert PRIMARY when they regain focus (see vinoPop PR #61), so read it on demand from a
  hotkey, not by watching.
- **Inserting text** (dictation, rewrite): wtype needs virtual-keyboard-v1 (unverified on niri);
  ydotool or dotool through uinput works on any compositor. The fallback is to set the clipboard
  and send a synthetic Ctrl+V.
- **Screen capture:** niri supports wlr-screencopy and portal screencasting. ext-image-copy-capture
  is PR #4554, opened 2026-09; unverified whether it has merged. OCR'ing a region is grim + slurp,
  then tesseract or a local vision LLM.
  https://github.com/niri-wm/niri/wiki/Screencasting · https://github.com/niri-wm/niri/pull/4554
- **Agent status:** easy. Hooks write to a Unix socket, and the layer-shell panel shows status and
  approve/deny buttons. No privileged APIs needed.
- **Camera mirror:** PipeWire camera or v4l2 inside a layer-shell window. Easy.
