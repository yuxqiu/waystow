//! An edge catcher: a layer surface on one monitor edge. Idle it is a thin
//! strip (plus a small handle while the shelf has items); when a drag enters
//! it grows into the drop zones, and it opens into the shelf on a drop, a
//! handle click or `waystow toggle`. The state machine is
//! [`waystow_core::catcher::Catcher`]; this file only renders it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use gtk::gdk;
use gtk::glib;
use gtk::graphene;
use gtk::gsk;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge as LsEdge, KeyboardMode, Layer, LayerShell};
use waystow_core::catcher::{self as machine, Event, OpenedBy, Phase};
use waystow_core::config::Edge;
use waystow_core::spring::Spring;

use super::dnd::{self, Offer};
use super::panel::{self, Zone, ZoneTarget};
use super::{Inner, Tab};
use crate::daemon::{Cmd, Payload};

/// A drag's payload, or why it couldn't be read.
type DropResult = Result<Payload, String>;

/// Gap between the panel and the screen edge.
const GAP: i32 = 10;
/// Transparent room for the client-drawn shadow.
const SHADOW: i32 = 40;

pub struct Catcher {
    inner: Weak<Inner>,
    pub window: gtk::Window,
    edge: Edge,
    roaming: bool,
    monitor: Option<gdk::Monitor>,
    machine: RefCell<machine::Catcher>,
    fixed: super::stage::Stage,
    handle: gtk::Box,
    panel: gtk::Overlay,
    content: gtk::Box,
    toast: gtk::Revealer,
    toast_label: gtk::Label,
    toast_timer: Rc<RefCell<Option<glib::SourceId>>>,
    reveal: RefCell<Spring>,
    animating: Cell<bool>,
    timer: RefCell<Option<glib::SourceId>>,
    handle_allowed: Cell<bool>,
    handle_on: Cell<bool>,
    zones: RefCell<Vec<Zone>>,
    hovered: RefCell<Option<usize>>,
    offer: RefCell<Option<Offer>>,
    /// The payload of the current drag, read once while it hovers (GDK can't
    /// read a drop twice): `None` while the read is in flight.
    payload: Rc<RefCell<Option<(u64, DropResult)>>>,
    session: Cell<u64>,
    shown: Cell<Option<Phase>>,
    dirty: Cell<bool>,
    render_queued: Cell<bool>,
    pub parts: RefCell<panel::ShelfParts>,
    progress: RefCell<HashMap<i64, gtk::ProgressBar>>,
    epoch: Instant,
}

impl Catcher {
    pub fn new(inner: &Rc<Inner>, monitor: Option<gdk::Monitor>, edge: Edge, roaming: bool) -> Rc<Self> {
        let window = gtk::Window::new();
        window.add_css_class("waystow");
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("waystow"));
        window.set_exclusive_zone(0);
        window.set_keyboard_mode(KeyboardMode::None);
        if let Some(m) = &monitor {
            window.set_monitor(Some(m));
        }
        window.set_anchor(ls_edge(edge), true);
        window.set_decorated(false);

        let fixed = super::stage::Stage::default();
        // Never fully transparent: GTK skips the buffer for an empty frame, and
        // an unmapped surface gets no drag events.
        fixed.add_css_class("ws-root");
        let handle = gtk::Box::new(gtk::Orientation::Vertical, 0);
        handle.add_css_class("ws-handle");
        if !edge.is_vertical() {
            handle.set_size_request(52, 4);
        }
        handle.set_visible(false);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let panel = gtk::Overlay::new();
        panel.add_css_class("ws-panel");
        panel.set_overflow(gtk::Overflow::Hidden);
        panel.set_child(Some(&content));
        let toast_label = gtk::Label::new(None);
        toast_label.add_css_class("ws-toast");
        toast_label.set_wrap(true);
        toast_label.set_max_width_chars(40);
        let toast = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(180)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(10)
            .child(&toast_label)
            .build();
        toast.set_can_target(false);
        panel.add_overlay(&toast);
        panel.set_visible(false);
        fixed.add(&handle);
        fixed.add(&panel);
        window.set_child(Some(&fixed));

        let this = Rc::new(Self {
            inner: Rc::downgrade(inner),
            window,
            edge,
            roaming,
            monitor,
            machine: RefCell::new(machine::Catcher::new()),
            fixed,
            handle,
            panel,
            content,
            toast,
            toast_label,
            toast_timer: Rc::default(),
            reveal: RefCell::new(Spring::with(0.0, 260.0, 0.78)),
            animating: Cell::new(false),
            timer: RefCell::new(None),
            handle_allowed: Cell::new(false),
            handle_on: Cell::new(false),
            zones: RefCell::new(Vec::new()),
            hovered: RefCell::new(None),
            offer: RefCell::new(None),
            payload: Rc::default(),
            session: Cell::new(0),
            shown: Cell::new(None),
            dirty: Cell::new(false),
            render_queued: Cell::new(false),
            parts: RefCell::default(),
            progress: RefCell::new(HashMap::new()),
            epoch: Instant::now(),
        });
        this.connect();
        this.layout_idle();
        if !roaming && !inner.config.borrow().catcher.edges.is_empty() {
            this.window.present();
        }
        this
    }

    fn inner(&self) -> Option<Rc<Inner>> {
        self.inner.upgrade()
    }

    fn now(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    pub fn phase(&self) -> Phase {
        self.machine.borrow().phase()
    }

    pub fn destroy(&self) {
        if let Some(t) = self.timer.borrow_mut().take() {
            t.remove();
        }
        self.window.destroy();
    }

    pub fn set_handle_allowed(&self, allowed: bool) {
        self.handle_allowed.set(allowed);
    }

    pub fn set_handle(&self, has_items: bool) {
        let enabled = self.inner().is_some_and(|i| i.config.borrow().catcher.handle);
        let on = has_items && enabled && self.handle_allowed.get() && !self.roaming;
        if on != self.handle_on.get() {
            self.handle_on.set(on);
            if self.phase() == Phase::Idle {
                self.layout_idle();
            }
        }
    }

    // ── Geometry ─────────────────────────────────────────────────────────

    fn monitor_size(&self) -> (i32, i32) {
        let m = self.monitor.clone().or_else(|| {
            let display = gdk::Display::default()?;
            self.window
                .surface()
                .and_then(|s| display.monitor_at_surface(&s))
                .or_else(|| display.monitors().item(0).and_downcast::<gdk::Monitor>())
        });
        m.map_or((1920, 1080), |m| {
            let g = m.geometry();
            (g.width(), g.height())
        })
    }

    fn strip_len(&self) -> i32 {
        let (w, h) = self.monitor_size();
        let len = self.inner().map_or(0.3, |i| i.config.borrow().catcher.length);
        (f64::from(if self.edge.is_vertical() { h } else { w }) * len) as i32
    }

    fn thickness(&self) -> i32 {
        let t = self.inner().map_or(2, |i| i.config.borrow().catcher.thickness) as i32;
        if self.handle_on.get() { t.max(10) } else { t }
    }

    fn margin(&self) -> i32 {
        if self.inner().is_some_and(|i| i.config.borrow().ui.client_shadow) { SHADOW } else { 0 }
    }

    fn panel_size(&self) -> (i32, i32) {
        let (_, mh) = self.monitor_size();
        let w = self.inner().map_or(360, |i| i.config.borrow().ui.width) as i32;
        let h = if self.edge.is_vertical() { (f64::from(mh) * 0.78) as i32 } else { (f64::from(mh) * 0.62) as i32 };
        (w, h.clamp(340, 660))
    }

    /// Window size when open and where the panel sits in it.
    fn open_geometry(&self) -> ((i32, i32), (i32, i32)) {
        let (pw, ph) = self.panel_size();
        let m = self.margin();
        let len = self.strip_len();
        if self.edge.is_vertical() {
            let wh = (ph + 2 * m).max(len);
            ((pw + 2 * m, wh), (m, (wh - ph) / 2))
        } else {
            let ww = (pw + 2 * m).max(len);
            ((ww, ph + 2 * m), ((ww - pw) / 2, m))
        }
    }

    fn layout_idle(&self) {
        let (t, len) = (self.thickness(), self.strip_len());
        let (w, h) = if self.edge.is_vertical() { (t, len) } else { (len, t) };
        self.window.set_margin(ls_edge(self.edge), 0);
        self.panel.set_visible(false);
        // GTK sizes a window with no natural content size at 200×200; pin it.
        self.fixed.set_size(w, h);
        self.window.set_default_size(w, h);
        self.handle.set_visible(self.handle_on.get());
        if self.handle_on.get() {
            let (hw, hh) = if self.edge.is_vertical() { (4, 52) } else { (52, 4) };
            let (x, y) = match self.edge {
                Edge::Right => (w - hw - 3, (h - hh) / 2),
                Edge::Left => (3, (h - hh) / 2),
                Edge::Top => ((w - hw) / 2, 3),
                Edge::Bottom => ((w - hw) / 2, h - hh - 3),
            };
            self.fixed.place(&self.handle, x, y, hw, hh);
        }
        self.set_input(None);
        self.window.set_keyboard_mode(KeyboardMode::None);
    }

    fn layout_open(&self) {
        let ((w, h), (x, y)) = self.open_geometry();
        let (pw, ph) = self.panel_size();
        self.window.set_margin(ls_edge(self.edge), if self.margin() == 0 { GAP } else { GAP - SHADOW / 3 });
        self.handle.set_visible(false);
        self.fixed.place(&self.panel, x, y, pw, ph);
        self.panel.set_visible(true);
        self.fixed.set_size(w, h);
        self.window.set_default_size(w, h);
    }

    /// Input region: the panel rect when open, the whole window otherwise.
    fn set_input(&self, panel_only: Option<()>) {
        let Some(surface) = self.window.surface() else { return };
        if panel_only.is_none() {
            surface.set_input_region(None);
            return;
        }
        let (_, (x, y)) = self.open_geometry();
        let (pw, ph) = self.panel_size();
        let region = gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(x, y, pw, ph));
        surface.set_input_region(Some(&region));
    }

    // ── State ────────────────────────────────────────────────────────────

    pub fn event(self: &Rc<Self>, ev: Event) {
        let before = self.phase();
        self.machine.borrow_mut().on(ev, self.now());
        if debug() {
            eprintln!(
                "waystow: catcher {:?}{}: {ev:?}: {before:?} -> {:?}",
                self.edge,
                if self.roaming { " (roaming)" } else { "" },
                self.phase()
            );
        }
        self.apply(before);
    }

    fn apply(self: &Rc<Self>, before: Phase) {
        let view = self.machine.borrow().view();
        let after = view.phase;
        self.schedule_tick();
        if (before == Phase::Idle) != (after == Phase::Idle)
            && let Some(i) = self.inner()
        {
            i.sync_handles();
        }
        if after == before && self.shown.get() == Some(after) {
            return;
        }
        match after {
            Phase::Idle => self.close(),
            Phase::Dragging | Phase::Open(_) => {
                if before == Phase::Idle || self.reveal.borrow().target < 0.5 {
                    self.open();
                }
                if after == Phase::Open(OpenedBy::Toggle)
                    && let Some(i) = self.inner()
                {
                    i.model.borrow_mut().tab = Tab::Shelf;
                }
                self.render_now();
            }
        }
        let mode = match after {
            Phase::Open(OpenedBy::Toggle) => KeyboardMode::Exclusive,
            Phase::Open(OpenedBy::Pointer) => KeyboardMode::OnDemand,
            _ => KeyboardMode::None,
        };
        self.window.set_keyboard_mode(mode);
        if matches!(after, Phase::Open(_)) {
            self.set_input(Some(()));
        } else {
            self.set_input(None);
        }
    }

    fn schedule_tick(self: &Rc<Self>) {
        if let Some(t) = self.timer.borrow_mut().take() {
            t.remove();
        }
        let Some(at) = self.machine.borrow().deadline() else { return };
        let wait = at.saturating_sub(self.now());
        let weak = Rc::downgrade(self);
        *self.timer.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_millis(wait), move || {
            if let Some(c) = weak.upgrade() {
                c.timer.borrow_mut().take();
                c.event(Event::Tick);
            }
        }));
    }

    fn reduced_motion(&self) -> bool {
        self.inner().is_some_and(|i| i.config.borrow().ui.reduced_motion)
    }

    fn open(self: &Rc<Self>) {
        self.layout_open();
        if self.roaming || !self.window.is_visible() {
            self.window.present();
        }
        {
            let mut s = self.reveal.borrow_mut();
            if self.reduced_motion() {
                s.snap(1.0);
            } else {
                s.target = 1.0;
            }
        }
        self.animate();
    }

    fn close(self: &Rc<Self>) {
        self.shown.set(Some(Phase::Idle));
        self.offer.borrow_mut().take();
        self.zones.borrow_mut().clear();
        if let Some(i) = self.inner() {
            i.close_quick_look();
        }
        {
            let mut s = self.reveal.borrow_mut();
            if self.reduced_motion() {
                s.snap(0.0);
            } else {
                s.target = 0.0;
            }
        }
        self.animate();
    }

    /// Drive the reveal spring on the frame clock.
    fn animate(self: &Rc<Self>) {
        self.apply_reveal();
        if self.animating.get() {
            return;
        }
        if self.reveal.borrow().is_settled() {
            self.settled();
            return;
        }
        self.animating.set(true);
        let weak = Rc::downgrade(self);
        let last: Cell<Option<i64>> = Cell::new(None);
        self.fixed.add_tick_callback(move |_, clock| {
            let Some(c) = weak.upgrade() else { return glib::ControlFlow::Break };
            let t = clock.frame_time();
            let dt = last.get().map_or(1.0 / 60.0, |l| (t - l) as f64 / 1e6);
            last.set(Some(t));
            c.reveal.borrow_mut().step(dt);
            c.apply_reveal();
            if c.reveal.borrow().is_settled() {
                c.animating.set(false);
                c.settled();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    fn apply_reveal(&self) {
        let p = self.reveal.borrow().value;
        let (pw, ph) = self.panel_size();
        let travel = f64::from(if self.edge.is_vertical() { pw } else { ph } + self.margin() + GAP) * 0.55;
        let off = ((1.0 - p) * travel) as f32;
        let (dx, dy) = match self.edge {
            Edge::Right => (off, 0.0),
            Edge::Left => (-off, 0.0),
            Edge::Top => (0.0, -off),
            Edge::Bottom => (0.0, off),
        };
        let scale = (p.clamp(0.0, 1.2) as f32).mul_add(0.06, 0.94);
        let (cx, cy) = (pw as f32 / 2.0, ph as f32 / 2.0);
        let t = gsk::Transform::new()
            .translate(&graphene::Point::new(dx + cx, dy + cy))
            .scale(scale, scale)
            .translate(&graphene::Point::new(-cx, -cy));
        self.fixed.set_transform(&self.panel, Some(t));
        self.panel.set_opacity(p.clamp(0.0, 1.0));
    }

    fn settled(&self) {
        if self.reveal.borrow().target < 0.5 {
            if self.roaming {
                self.window.set_visible(false);
            }
            self.layout_idle();
            self.content_clear();
        }
    }

    fn content_clear(&self) {
        while let Some(c) = self.content.first_child() {
            self.content.remove(&c);
        }
        self.progress.borrow_mut().clear();
    }

    // ── Rendering ────────────────────────────────────────────────────────

    /// Re-render if open (deferred while the user drags something out).
    pub fn render(self: &Rc<Self>) {
        if self.phase() == Phase::Idle {
            return;
        }
        if self.inner().is_some_and(|i| !i.dragging_ids.borrow().is_empty() || i.popups.get() > 0) {
            self.dirty.set(true);
            return;
        }
        self.render_now();
    }

    /// Render on the next idle: never inside an event handler, where removing
    /// the widget under the pointer trips GTK's crossing invariants.
    pub fn render_now(self: &Rc<Self>) {
        if self.render_queued.replace(true) {
            return;
        }
        let w = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(c) = w.upgrade() {
                c.render_queued.set(false);
                if c.phase() != Phase::Idle {
                    c.do_render();
                }
            }
        });
    }

    fn do_render(self: &Rc<Self>) {
        let Some(inner) = self.inner() else { return };
        self.dirty.set(false);
        let phase = self.phase();
        let scroll = find_scroll(&self.content).map(|s| s.vadjustment().value());
        self.content_clear();
        let (widget, zones) = match phase {
            Phase::Dragging => {
                let offer = self.offer.borrow().clone().unwrap_or_default();
                panel::drop_view(&inner, &offer)
            }
            _ => panel::shelf_view(&inner, self),
        };
        if phase == Phase::Dragging && self.shown.get() != Some(Phase::Dragging) {
            widget.add_css_class("ws-reveal");
        }
        self.content.append(&widget);
        *self.zones.borrow_mut() = zones;
        *self.hovered.borrow_mut() = None;
        self.shown.set(Some(phase));
        if let (Some(v), Some(s)) = (scroll, find_scroll(&self.content)) {
            let adj = s.vadjustment();
            glib::idle_add_local_once(move || adj.set_value(v));
        }
    }

    pub fn register_progress(&self, job: i64, bar: gtk::ProgressBar) {
        self.progress.borrow_mut().insert(job, bar);
    }

    pub fn progress(&self, job: i64, f: f32) {
        if let Some(bar) = self.progress.borrow().get(&job) {
            bar.set_fraction(f64::from(f));
        }
    }

    pub fn toast(&self, msg: &str) {
        self.toast_label.set_text(msg);
        self.toast.set_reveal_child(true);
        if let Some(t) = self.toast_timer.borrow_mut().take() {
            t.remove();
        }
        let toast = self.toast.clone();
        let slot = self.toast_timer.clone();
        *self.toast_timer.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_millis(2600), move || {
            toast.set_reveal_child(false);
            slot.borrow_mut().take();
        }));
    }

    // ── Events ───────────────────────────────────────────────────────────

    fn connect(self: &Rc<Self>) {
        // Pointer in and out (without a drag).
        let motion = gtk::EventControllerMotion::new();
        let w = Rc::downgrade(self);
        motion.connect_enter(move |_, _, _| {
            if let Some(c) = w.upgrade() {
                c.event(Event::PointerEnter);
            }
        });
        let w = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            if let Some(c) = w.upgrade() {
                c.event(Event::PointerLeave);
            }
        });
        self.window.add_controller(motion);

        // A click on the handle opens the shelf.
        let click = gtk::GestureClick::new();
        let w = Rc::downgrade(self);
        click.connect_released(move |_, _, _, _| {
            if let Some(c) = w.upgrade()
                && c.phase() == Phase::Idle
            {
                c.event(Event::HandleClick);
            }
        });
        self.handle.add_controller(click);

        // Keyboard.
        let keys = gtk::EventControllerKey::new();
        let w = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(c) = w.upgrade() else { return glib::Propagation::Proceed };
            c.key(key, state)
        });
        self.window.add_controller(keys);

        // Drops.
        let target = gtk::DropTargetAsync::new(None, gdk::DragAction::COPY);
        let w = Rc::downgrade(self);
        target.connect_drag_enter(move |_, drop, x, y| {
            let Some(c) = w.upgrade() else { return gdk::DragAction::empty() };
            c.drag_enter(drop);
            c.drag_hover(x, y);
            gdk::DragAction::COPY
        });
        let w = Rc::downgrade(self);
        target.connect_drag_motion(move |_, _, x, y| {
            let Some(c) = w.upgrade() else { return gdk::DragAction::empty() };
            c.drag_hover(x, y);
            gdk::DragAction::COPY
        });
        let w = Rc::downgrade(self);
        target.connect_drag_leave(move |_, _| {
            if let Some(c) = w.upgrade() {
                c.set_hover(None);
                c.event(Event::DragLeave);
            }
        });
        let w = Rc::downgrade(self);
        target.connect_drop(move |_, drop, x, y| {
            let Some(c) = w.upgrade() else { return false };
            c.drop(drop, x, y)
        });
        self.fixed.add_controller(target);
    }

    fn drag_enter(self: &Rc<Self>, drop: &gdk::Drop) {
        let local = self.inner().is_some_and(|i| !i.dragging_ids.borrow().is_empty());
        let fresh = self.phase() == Phase::Idle || (self.phase() != Phase::Dragging && !local);
        let session = self.session.get() + 1;
        self.session.set(session);
        if fresh && !local {
            *self.offer.borrow_mut() = Some(Offer::from_formats(&drop.formats()));
            self.payload.borrow_mut().take();
            // Read what is being dragged now: the drop zones can then offer matching
            // actions, and the drop itself reuses this read.
            let w = Rc::downgrade(self);
            let drop = drop.clone();
            let cap = self.inner().map_or(64 << 20, |i| i.config.borrow().shelf.max_payload());
            glib::spawn_future_local(async move {
                let result = dnd::read(&drop, cap).await;
                if debug() {
                    eprintln!("waystow: read drag: {:?}", result.as_ref().map(dnd::Offer::of));
                }
                let Some(c) = w.upgrade() else { return };
                // A late answer for an older drag is dropped (L4).
                if c.session.get() != session {
                    return;
                }
                let offer = result.as_ref().ok().map(Offer::of);
                *c.payload.borrow_mut() = Some((session, result));
                if let Some(offer) = offer
                    && c.phase() == Phase::Dragging
                {
                    *c.offer.borrow_mut() = Some(offer);
                    c.render_now();
                }
            });
        }
        self.event(Event::DragEnter);
    }

    fn zone_at(&self, x: f64, y: f64) -> Option<usize> {
        let p = graphene::Point::new(x as f32, y as f32);
        self.zones.borrow().iter().position(|z| {
            z.widget.is_visible() && z.widget.compute_bounds(&self.fixed).is_some_and(|b| b.contains_point(&p))
        })
    }

    fn drag_hover(&self, x: f64, y: f64) {
        self.set_hover(self.zone_at(x, y));
    }

    fn set_hover(&self, idx: Option<usize>) {
        let mut h = self.hovered.borrow_mut();
        if *h == idx {
            return;
        }
        let zones = self.zones.borrow();
        if let Some(old) = h.and_then(|i| zones.get(i)) {
            old.widget.remove_css_class("drop-hover");
        }
        if let Some(new) = idx.and_then(|i| zones.get(i)) {
            new.widget.add_css_class("drop-hover");
        }
        *h = idx;
    }

    fn drop(self: &Rc<Self>, drop: &gdk::Drop, x: f64, y: f64) -> bool {
        let Some(inner) = self.inner() else { return false };
        let zone = self.zone_at(x, y).and_then(|i| self.zones.borrow().get(i).map(|z| z.target.clone()));
        self.set_hover(None);
        // Our own items dropped back: only a collection chip means something (move).
        let local: Vec<i64> = inner.dragging_ids.borrow().clone();
        if !local.is_empty() {
            if let Some(ZoneTarget::Keep(collection)) = zone {
                inner.send(Cmd::Move(local, collection));
                // Finish with an action the source offered (COPY): finishing with
                // one it didn't is a protocol error, and the compositor disconnects us.
                drop.finish(gdk::DragAction::COPY);
                return true;
            }
            return false;
        }
        let collection = inner.model.borrow().collection;
        let (collection, action) = match zone {
            Some(ZoneTarget::Keep(c)) => (c, None),
            Some(ZoneTarget::Action(a)) => (collection, Some(a)),
            None => (collection, None),
        };
        if action.is_none() {
            let mut m = inner.model.borrow_mut();
            m.collection = collection;
            m.tab = Tab::Shelf;
        }
        let drop = drop.clone();
        let w = Rc::downgrade(self);
        let session = self.session.get();
        let slot = self.payload.clone();
        if debug() {
            eprintln!("waystow: drop at {x:.0},{y:.0} action {action:?} session {session}");
        }
        glib::spawn_future_local(async move {
            if debug() {
                eprintln!("waystow: drop future started");
            }
            // Wait for the read started on enter (up to 10 s for big payloads).
            let mut result = None;
            for _ in 0..200 {
                if let Some((s, r)) = slot.borrow_mut().take() {
                    if s == session {
                        result = Some(r);
                    }
                    break;
                }
                glib::timeout_future(Duration::from_millis(50)).await;
            }
            let result = result.unwrap_or_else(|| Err("reading the drop took too long".into()));
            if debug() {
                eprintln!("waystow: drop: {:?}", result.as_ref().map(dnd::Offer::of));
            }
            drop.finish(if result.is_ok() { gdk::DragAction::COPY } else { gdk::DragAction::empty() });
            let Some(c) = w.upgrade() else { return };
            match result {
                Ok(payload) => {
                    if let Some(i) = c.inner() {
                        i.send(Cmd::Drop { payload, collection, action });
                    }
                }
                Err(e) => c.toast(&e),
            }
        });
        self.event(Event::Drop);
        true
    }

    fn key(self: &Rc<Self>, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        let Some(inner) = self.inner() else { return glib::Propagation::Proceed };
        // Text fields keep their keys.
        if GtkWindowExt::focus(&self.window)
            .is_some_and(|f| f.is::<gtk::Text>() || f.ancestor(gtk::Entry::static_type()).is_some())
        {
            if key == gdk::Key::Escape {
                GtkWindowExt::set_focus(&self.window, None::<&gtk::Widget>);
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }
        let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
        match key {
            gdk::Key::Escape => {
                if inner.close_quick_look() {
                    return glib::Propagation::Stop;
                }
                let had = !inner.model.borrow().selection.is_empty();
                if had {
                    inner.model.borrow_mut().selection.clear();
                    panel::refresh_selection(&inner, self);
                } else {
                    self.event(Event::Escape);
                }
                glib::Propagation::Stop
            }
            gdk::Key::Up | gdk::Key::Down | gdk::Key::k | gdk::Key::j => {
                panel::move_selection(&inner, matches!(key, gdk::Key::Down | gdk::Key::j));
                panel::refresh_selection(&inner, self);
                glib::Propagation::Stop
            }
            gdk::Key::space => {
                panel::quick_look_selection(&inner);
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                panel::open_selection(&inner);
                glib::Propagation::Stop
            }
            gdk::Key::Delete | gdk::Key::BackSpace => {
                let ids: Vec<i64> = inner.model.borrow().selection.iter().copied().collect();
                if !ids.is_empty() {
                    inner.send(Cmd::Remove(ids));
                }
                glib::Propagation::Stop
            }
            gdk::Key::a if ctrl => {
                let mut m = inner.model.borrow_mut();
                m.selection = m.snap.items.iter().filter(|i| i.collection == m.collection).map(|i| i.id).collect();
                drop(m);
                panel::refresh_selection(&inner, self);
                glib::Propagation::Stop
            }
            gdk::Key::Tab => {
                {
                    let mut m = inner.model.borrow_mut();
                    m.tab = match m.tab {
                        Tab::Shelf => Tab::Recent,
                        Tab::Recent => Tab::Activity,
                        Tab::Activity => Tab::Shelf,
                    };
                }
                self.render_now();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }

    /// Our own drag out started or ended.
    pub fn drag_out(self: &Rc<Self>, active: bool) {
        self.event(if active { Event::DragOutBegin } else { Event::DragOutEnd });
        if !active && self.dirty.get() {
            self.render();
        }
    }
}

/// `WAYSTOW_DEBUG=1` logs catcher events.
fn debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("WAYSTOW_DEBUG").is_some_and(|v| v != "0"))
}

const fn ls_edge(e: Edge) -> LsEdge {
    match e {
        Edge::Left => LsEdge::Left,
        Edge::Right => LsEdge::Right,
        Edge::Top => LsEdge::Top,
        Edge::Bottom => LsEdge::Bottom,
    }
}

fn find_scroll(root: &impl IsA<gtk::Widget>) -> Option<gtk::ScrolledWindow> {
    let mut stack = vec![root.as_ref().clone()];
    while let Some(w) = stack.pop() {
        if let Ok(s) = w.clone().downcast::<gtk::ScrolledWindow>()
            && s.has_css_class("ws-main-scroll")
        {
            return Some(s);
        }
        let mut c = w.first_child();
        while let Some(ch) = c {
            c = ch.next_sibling();
            stack.push(ch);
        }
    }
    None
}
