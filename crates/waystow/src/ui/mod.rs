//! The GTK side: edge catchers (one per monitor and edge), the shelf panel,
//! the island and Quick Look. The UI never touches the store; it renders
//! [`Snapshot`]s and sends [`Cmd`]s.

mod catcher;
mod dnd;
mod icons;
mod island;
mod panel;
mod quicklook;
mod stage;
mod theme;
mod thumbs;
mod widgets;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;
use waystow_core::action::Action;
use waystow_core::catcher::{Event, Phase};
use waystow_core::config::{Config, Edge};
use waystow_core::model::{Collection, Item, Job, Recent, RecentKind, TEMPORARY};

use crate::daemon::Cmd;
use crate::recent::Download;
use crate::system;

use self::catcher::Catcher;
use self::island::Island;
use self::theme::{Look, Styles};

/// Everything the UI shows.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub collections: Vec<Collection>,
    pub items: Vec<Item>,
    pub jobs: Vec<Job>,
    pub recent: Vec<Recent>,
    pub actions: Rc<Vec<Action>>,
}

/// Which page of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Shelf,
    Recent,
    Activity,
}

/// UI-side state.
pub struct Model {
    pub snap: Snapshot,
    pub collection: i64,
    pub selection: BTreeSet<i64>,
    pub tab: Tab,
    pub recent_filter: Option<RecentKind>,
    pub progress: HashMap<i64, f32>,
    pub downloads: Vec<Download>,
    /// Items already shown once (new ones get an entrance animation).
    pub seen: HashSet<i64>,
    /// What the open panel last showed; unchanged data doesn't re-render it.
    pub shown_key: String,
}

impl Model {
    /// Items of the current collection.
    pub fn shelf(&self) -> Vec<&Item> {
        self.snap.items.iter().filter(|i| i.collection == self.collection).collect()
    }

    /// The selection, or the whole collection when nothing is selected.
    pub fn targets(&self) -> Vec<&Item> {
        let shelf = self.shelf();
        let picked: Vec<&Item> = shelf.iter().copied().filter(|i| self.selection.contains(&i.id)).collect();
        if picked.is_empty() { shelf } else { picked }
    }

    /// A digest of what the current tab shows.
    fn view_key(&self) -> String {
        let running: Vec<(i64, &str)> =
            self.snap.jobs.iter().filter(|j| !j.status.is_final()).map(|j| (j.id, j.status.as_str())).collect();
        let body = match self.tab {
            Tab::Shelf => serde_json::to_string(&(&self.snap.collections, &self.snap.items, running)),
            Tab::Recent => serde_json::to_string(&self.snap.recent),
            Tab::Activity => serde_json::to_string(&(&self.snap.jobs, self.downloads.len())),
        };
        format!("{:?}{}{}", self.tab, self.collection, body.unwrap_or_default())
    }

    /// Actions applying to these MIME types, primary first.
    pub fn actions_for(&self, mimes: &[&str]) -> Vec<Action> {
        let mut v: Vec<Action> = self.snap.actions.iter().filter(|a| a.applies_to(mimes)).cloned().collect();
        v.sort_by_key(|a| !a.primary);
        v
    }
}

pub struct Inner {
    pub cmds: async_channel::Sender<Cmd>,
    pub config: RefCell<Config>,
    pub model: RefCell<Model>,
    catchers: RefCell<Vec<Rc<Catcher>>>,
    roaming: RefCell<Option<Rc<Catcher>>>,
    pub island: Island,
    styles: Styles,
    desktop: Cell<system::Appearance>,
    quicklook: RefCell<Option<quicklook::QuickLook>>,
    /// Items being dragged out of a panel right now.
    pub dragging_ids: RefCell<Vec<i64>>,
    /// Popovers open in a panel; renders wait until they close.
    pub popups: Cell<u32>,
    _appearance: Option<gio::SignalSubscription>,
}

impl Inner {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.cmds.try_send(cmd);
    }

    fn all_catchers(&self) -> Vec<Rc<Catcher>> {
        let mut v = self.catchers.borrow().clone();
        if let Some(r) = self.roaming.borrow().clone() {
            v.push(r);
        }
        v
    }

    /// Show edge handles only while no panel is open.
    pub fn sync_handles(&self) {
        let open = self.all_catchers().iter().any(|c| c.phase() != Phase::Idle);
        let has_items = !self.model.borrow().snap.items.is_empty();
        for c in self.catchers.borrow().iter() {
            c.set_handle(has_items && !open);
        }
    }

    /// Count a popover as open while it shows, and render what it held back.
    pub fn track_popover(self: &Rc<Self>, pop: &gtk::Popover) {
        let w = Rc::downgrade(self);
        pop.connect_show(move |_| {
            if let Some(i) = w.upgrade() {
                i.popups.set(i.popups.get() + 1);
            }
        });
        let w = Rc::downgrade(self);
        pop.connect_closed(move |_| {
            if let Some(i) = w.upgrade() {
                i.popups.set(i.popups.get().saturating_sub(1));
                if i.popups.get() == 0 {
                    i.render();
                }
            }
        });
    }

    /// Re-render every open panel.
    pub fn render(&self) {
        for c in self.all_catchers() {
            c.render();
        }
    }

    /// Show `path` (or an item) in Quick Look.
    pub fn quick_look(self: &Rc<Self>, path: &Path, mime: &str) {
        if let Some(q) = self.quicklook.borrow_mut().take() {
            q.close();
        }
        *self.quicklook.borrow_mut() = Some(quicklook::QuickLook::open(self, path, mime));
    }

    pub fn close_quick_look(&self) -> bool {
        self.quicklook.borrow_mut().take().map(quicklook::QuickLook::close).is_some()
    }

    fn apply_look(&self) {
        let cfg = self.config.borrow();
        let look = Look::resolve(&cfg.ui, self.desktop.get());
        self.styles.apply(&look, &cfg.ui.font);
    }

    /// The edge the shelf opens from when toggled.
    pub fn home_edge(&self) -> Edge {
        self.config.borrow().catcher.edges.first().copied().unwrap_or(Edge::Right)
    }
}

/// Handle owned by the daemon.
pub struct Ui {
    inner: Rc<Inner>,
}

impl Ui {
    pub fn new(config: &Config, cmds: async_channel::Sender<Cmd>) -> Self {
        let styles = Styles::install();
        let inner = Rc::new_cyclic(|weak: &std::rc::Weak<Inner>| {
            let weak = weak.clone();
            let (desktop, subscription) = system::watch_appearance(move |a| {
                if let Some(inner) = weak.upgrade() {
                    inner.desktop.set(a);
                    inner.apply_look();
                }
            });
            Inner {
                island: Island::new(cmds.clone(), config),
                cmds,
                config: RefCell::new(config.clone()),
                model: RefCell::new(Model {
                    snap: Snapshot::default(),
                    collection: TEMPORARY,
                    selection: BTreeSet::new(),
                    tab: Tab::Shelf,
                    recent_filter: None,
                    progress: HashMap::new(),
                    downloads: Vec::new(),
                    seen: HashSet::new(),
                    shown_key: String::new(),
                }),
                catchers: RefCell::new(Vec::new()),
                roaming: RefCell::new(None),
                styles,
                desktop: Cell::new(desktop),
                quicklook: RefCell::new(None),
                dragging_ids: RefCell::new(Vec::new()),
                popups: Cell::new(0),
                _appearance: subscription,
            }
        });
        inner.island.attach(&inner);
        inner.apply_look();
        let ui = Self { inner };
        ui.rebuild_catchers();
        if let Some(display) = gdk::Display::default() {
            let weak = Rc::downgrade(&ui.inner);
            display.monitors().connect_items_changed(move |_, _, _, _| {
                if let Some(inner) = weak.upgrade() {
                    rebuild_catchers(&inner);
                }
            });
        }
        ui
    }

    fn rebuild_catchers(&self) {
        rebuild_catchers(&self.inner);
    }

    pub fn reconfigure(&self, config: &Config) {
        *self.inner.config.borrow_mut() = config.clone();
        self.inner.apply_look();
        self.inner.island.reconfigure(config);
        rebuild_catchers(&self.inner);
    }

    pub fn update(&self, snap: Snapshot) {
        {
            let mut m = self.inner.model.borrow_mut();
            if !snap.collections.iter().any(|c| c.id == m.collection) {
                m.collection = TEMPORARY;
            }
            let live: HashSet<i64> = snap.items.iter().map(|i| i.id).collect();
            m.selection.retain(|id| live.contains(id));
            m.progress.retain(|id, _| snap.jobs.iter().any(|j| j.id == *id && !j.status.is_final()));
            m.snap = snap;
        }
        self.inner.sync_handles();
        let changed = {
            let mut m = self.inner.model.borrow_mut();
            let key = m.view_key();
            let changed = key != m.shown_key;
            m.shown_key = key;
            changed
        };
        if changed {
            self.inner.render();
        }
        self.inner.island.refresh(&self.inner);
    }

    pub fn catcher(&self, ev: Event) {
        let open = self.inner.all_catchers().into_iter().find(|c| c.phase() != Phase::Idle);
        match (open, ev) {
            (Some(c), _) => c.event(ev),
            (None, Event::Toggle | Event::Show) => {
                // Not on a particular monitor: the compositor puts it on the focused one.
                let existing = self.inner.roaming.borrow().clone();
                let c = existing.unwrap_or_else(|| {
                    let c = Catcher::new(&self.inner, None, self.inner.home_edge(), true);
                    *self.inner.roaming.borrow_mut() = Some(c.clone());
                    c
                });
                c.event(ev);
            }
            (None, _) => {}
        }
    }

    pub fn phase_name(&self) -> &'static str {
        match self.inner.all_catchers().iter().map(|c| c.phase()).find(|p| *p != Phase::Idle) {
            None | Some(Phase::Idle) => "idle",
            Some(Phase::Dragging) => "dragging",
            Some(Phase::Open(_)) => "open",
        }
    }

    pub fn monitors() -> usize {
        gdk::Display::default().map_or(0, |d| d.monitors().n_items() as usize)
    }

    pub fn select_collection(&self, id: i64) {
        let mut m = self.inner.model.borrow_mut();
        m.collection = id;
        m.selection.clear();
        m.tab = Tab::Shelf;
    }

    pub fn toast(&self, msg: &str) {
        let open: Vec<_> = self.inner.all_catchers().into_iter().filter(|c| c.phase() != Phase::Idle).collect();
        if open.is_empty() {
            self.inner.island.message(&self.inner, msg);
        } else {
            for c in open {
                c.toast(msg);
            }
        }
    }

    pub fn progress(&self, job: i64, f: f32) {
        self.inner.model.borrow_mut().progress.insert(job, f);
        for c in self.inner.all_catchers() {
            c.progress(job, f);
        }
        self.inner.island.refresh(&self.inner);
    }

    pub fn finished(&self, job: &Job) {
        self.inner.island.finished(&self.inner, job);
    }

    pub fn screenshot(&self, path: &Path) {
        self.inner.island.screenshot(&self.inner, path);
    }

    pub fn downloads(&self, list: Vec<Download>) {
        self.inner.model.borrow_mut().downloads = list;
        self.inner.island.refresh(&self.inner);
    }
}

fn rebuild_catchers(inner: &Rc<Inner>) {
    for c in inner.catchers.borrow_mut().drain(..) {
        c.destroy();
    }
    let Some(display) = gdk::Display::default() else { return };
    let monitors = display.monitors();
    let edges = inner.config.borrow().catcher.edges.clone();
    let has_items = !inner.model.borrow().snap.items.is_empty();
    let mut v = Vec::new();
    for i in 0..monitors.n_items() {
        let Some(m) = monitors.item(i).and_downcast::<gdk::Monitor>() else { continue };
        for (n, edge) in edges.iter().enumerate() {
            let c = Catcher::new(inner, Some(m.clone()), *edge, false);
            c.set_handle_allowed(n == 0);
            c.set_handle(has_items);
            v.push(c);
        }
    }
    *inner.catchers.borrow_mut() = v;
}
