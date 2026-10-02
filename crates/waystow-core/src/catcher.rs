//! The edge catcher's state machine, free of GTK so every path is tested.
//!
//! The UI feeds it events with a clock in milliseconds and renders
//! [`Catcher::view`]; timers call [`Event::Tick`] at [`Catcher::deadline`].

/// How long a drag may leave the panel before it closes.
pub const DRAG_GRACE_MS: u64 = 350;
/// How long the pointer may leave a hover-opened panel before it closes.
pub const POINTER_GRACE_MS: u64 = 800;

/// Why the panel is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenedBy {
    /// `waystow toggle` or a keybind: stays until toggled or Esc.
    Toggle,
    /// A click on the handle, or a finished drop: closes when the pointer leaves.
    Pointer,
}

/// Where the catcher is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Only the hot strip (and maybe the handle).
    Idle,
    /// A drag is over the panel: show drop zones.
    Dragging,
    /// The shelf is shown.
    Open(OpenedBy),
}

/// Inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A drag entered the strip or panel.
    DragEnter,
    /// The drag left the panel.
    DragLeave,
    /// Something was dropped on the panel.
    Drop,
    /// The pointer (no drag) entered the panel.
    PointerEnter,
    /// The pointer left the panel.
    PointerLeave,
    /// The handle was clicked.
    HandleClick,
    /// `waystow toggle`.
    Toggle,
    /// `waystow show`.
    Show,
    /// `waystow hide`.
    Hide,
    /// Escape pressed.
    Escape,
    /// The user started dragging items out of the panel.
    DragOutBegin,
    /// That drag ended (dropped somewhere or cancelled).
    DragOutEnd,
    /// Time passed.
    Tick,
}

/// What the input region should cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// The thin strip (plus the handle, if shown).
    Strip,
    /// The whole panel.
    Panel,
}

/// What to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    /// Phase to render.
    pub phase: Phase,
    /// Input region.
    pub input: Input,
    /// Grab the keyboard (only when opened by toggle).
    pub keyboard: bool,
}

/// The state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catcher {
    phase: Phase,
    inside: bool,
    dragging_out: bool,
    close_at: Option<u64>,
}

impl Default for Catcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Catcher {
    /// Idle.
    #[must_use]
    pub const fn new() -> Self {
        Self { phase: Phase::Idle, inside: false, dragging_out: false, close_at: None }
    }

    /// Current phase.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// When the UI should send [`Event::Tick`] next.
    #[must_use]
    pub const fn deadline(&self) -> Option<u64> {
        self.close_at
    }

    /// What to render.
    #[must_use]
    pub const fn view(&self) -> View {
        View {
            phase: self.phase,
            input: if matches!(self.phase, Phase::Idle) { Input::Strip } else { Input::Panel },
            keyboard: matches!(self.phase, Phase::Open(OpenedBy::Toggle)),
        }
    }

    const fn close(&mut self) {
        self.phase = Phase::Idle;
        self.close_at = None;
    }

    const fn open(&mut self, by: OpenedBy) {
        self.phase = Phase::Open(by);
        self.close_at = None;
    }

    /// Feed an event at time `now` (ms).
    pub fn on(&mut self, event: Event, now: u64) {
        match event {
            Event::DragEnter => {
                // Our own items dragged back over the panel don't turn it into drop zones.
                if !self.dragging_out {
                    self.phase = Phase::Dragging;
                }
                self.inside = true;
                self.close_at = None;
            }
            Event::DragLeave => {
                self.inside = false;
                if self.phase == Phase::Dragging {
                    self.close_at = Some(now + DRAG_GRACE_MS);
                }
            }
            Event::Drop => {
                self.inside = true;
                self.open(OpenedBy::Pointer);
            }
            Event::PointerEnter => {
                self.inside = true;
                if self.phase == Phase::Open(OpenedBy::Pointer) {
                    self.close_at = None;
                }
            }
            Event::PointerLeave => {
                self.inside = false;
                if self.phase == Phase::Open(OpenedBy::Pointer) && !self.dragging_out {
                    self.close_at = Some(now + POINTER_GRACE_MS);
                }
            }
            Event::HandleClick => self.open(OpenedBy::Pointer),
            Event::Toggle => {
                if matches!(self.phase, Phase::Open(_)) {
                    self.close();
                } else {
                    self.open(OpenedBy::Toggle);
                }
            }
            Event::Show => self.open(OpenedBy::Toggle),
            Event::Hide | Event::Escape => {
                if !self.dragging_out {
                    self.close();
                }
            }
            Event::DragOutBegin => {
                self.dragging_out = true;
                self.close_at = None;
            }
            Event::DragOutEnd => {
                self.dragging_out = false;
                if !self.inside && self.phase == Phase::Open(OpenedBy::Pointer) {
                    self.close_at = Some(now + POINTER_GRACE_MS);
                }
                if self.phase == Phase::Dragging {
                    // Our own drag hovered the strip; nothing was caught.
                    self.open(OpenedBy::Pointer);
                }
            }
            Event::Tick => {
                if self.close_at.is_some_and(|t| now >= t) {
                    self.close();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Event::*;

    fn run(events: &[(Event, u64)]) -> Catcher {
        let mut c = Catcher::new();
        for &(e, t) in events {
            c.on(e, t);
        }
        c
    }

    #[test]
    fn drag_opens_and_leaving_closes_after_grace() {
        let mut c = run(&[(DragEnter, 0), (DragLeave, 100)]);
        assert_eq!(c.phase(), Phase::Dragging);
        assert_eq!(c.view().input, Input::Panel);
        c.on(Tick, 100 + DRAG_GRACE_MS - 1);
        assert_eq!(c.phase(), Phase::Dragging);
        c.on(Tick, 100 + DRAG_GRACE_MS);
        assert_eq!(c.phase(), Phase::Idle);
        assert_eq!(c.view().input, Input::Strip);
    }

    #[test]
    fn reentering_cancels_the_close() {
        let mut c = run(&[(DragEnter, 0), (DragLeave, 100), (DragEnter, 200)]);
        c.on(Tick, 10_000);
        assert_eq!(c.phase(), Phase::Dragging);
    }

    #[test]
    fn drop_shows_the_shelf_until_the_pointer_leaves() {
        let mut c = run(&[(DragEnter, 0), (Drop, 50)]);
        assert_eq!(c.phase(), Phase::Open(OpenedBy::Pointer));
        assert!(!c.view().keyboard);
        c.on(Tick, 100_000);
        assert_eq!(c.phase(), Phase::Open(OpenedBy::Pointer), "no timer while the pointer is inside");
        c.on(PointerLeave, 1000);
        c.on(Tick, 1000 + POINTER_GRACE_MS);
        assert_eq!(c.phase(), Phase::Idle);
    }

    #[test]
    fn toggle_holds_until_toggled_or_escape() {
        let mut c = run(&[(Toggle, 0), (PointerLeave, 10), (Tick, 100_000)]);
        assert_eq!(c.phase(), Phase::Open(OpenedBy::Toggle));
        assert!(c.view().keyboard);
        c.on(Toggle, 1);
        assert_eq!(c.phase(), Phase::Idle);
        c.on(Show, 2);
        c.on(Escape, 3);
        assert_eq!(c.phase(), Phase::Idle);
    }

    #[test]
    fn dragging_out_keeps_the_panel_open() {
        // Closing the source surface mid-drag would cancel the drag.
        let mut c = run(&[(HandleClick, 0), (DragOutBegin, 10), (PointerLeave, 20), (Escape, 30)]);
        c.on(Tick, 100_000);
        assert_eq!(c.phase(), Phase::Open(OpenedBy::Pointer));
        c.on(DragOutEnd, 200);
        c.on(Tick, 200 + POINTER_GRACE_MS);
        assert_eq!(c.phase(), Phase::Idle);
    }

    #[test]
    fn our_own_drag_does_not_become_drop_zones() {
        let mut c = run(&[(HandleClick, 0), (DragOutBegin, 10), (DragLeave, 20), (DragEnter, 30)]);
        assert_eq!(c.phase(), Phase::Open(OpenedBy::Pointer));
        c.on(DragOutEnd, 40);
        assert_eq!(c.phase(), Phase::Open(OpenedBy::Pointer));
    }

    #[test]
    fn stale_ticks_do_nothing() {
        let mut c = Catcher::new();
        c.on(Tick, 5);
        assert_eq!(c, Catcher::new());
        c.on(DragLeave, 6);
        assert_eq!(c.phase(), Phase::Idle);
        assert_eq!(c.deadline(), None);
    }
}
