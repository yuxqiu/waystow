//! The panel's views: the shelf (with Recent and Activity tabs) and the
//! drop zones shown while something is dragged over the catcher.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use waystow_core::action::{Action, ParamKind};
use waystow_core::model::{self, Content, Item, JobStatus, RecentKind, TEMPORARY, Undo};

use super::catcher::Catcher;
use super::dnd::{self, Offer};
use super::widgets::{self, icon_button, label, two_lines};
use super::{Inner, Tab};
use crate::daemon::Cmd;
use crate::files;

/// Where a drop lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneTarget {
    /// Keep in this collection.
    Keep(i64),
    /// Run this action on the dropped things.
    Action(String),
}

/// Live widgets of the shelf view, so selection changes update in place
/// (rebuilding would destroy a row mid-press and cancel its drag).
#[derive(Default)]
pub struct ShelfParts {
    rows: Vec<(i64, gtk::Box)>,
    hint: Option<gtk::Label>,
    bar: Option<gtk::Box>,
}

/// Apply the model's selection to the live shelf view.
pub fn refresh_selection(inner: &Rc<Inner>, c: &Catcher) {
    let parts = c.parts.borrow();
    let selection = inner.model.borrow().selection.clone();
    for (id, row) in &parts.rows {
        if selection.contains(id) {
            row.add_css_class("selected");
        } else {
            row.remove_css_class("selected");
        }
    }
    if let Some(h) = &parts.hint {
        h.set_text(&stack_hint(selection.len()));
    }
    if let Some(bar) = &parts.bar {
        while let Some(ch) = bar.first_child() {
            bar.remove(&ch);
        }
        fill_action_bar(inner, bar);
    }
}

fn stack_hint(selected: usize) -> String {
    if selected == 0 {
        "Drag the stack to take them all".to_owned()
    } else {
        format!("{selected} selected · drag a row to take the selection")
    }
}

fn reselect(c: &std::rc::Weak<Catcher>, inner: &Rc<Inner>) {
    if let Some(c) = c.upgrade() {
        refresh_selection(inner, &c);
    }
}

/// A widget that accepts drops.
pub struct Zone {
    pub widget: gtk::Widget,
    pub target: ZoneTarget,
}

fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

fn rerender(c: &std::rc::Weak<Catcher>) {
    if let Some(c) = c.upgrade() {
        c.render_now();
    }
}

// ── Shelf view ───────────────────────────────────────────────────────────

pub fn shelf_view(inner: &Rc<Inner>, c: &Rc<Catcher>) -> (gtk::Widget, Vec<Zone>) {
    *c.parts.borrow_mut() = ShelfParts::default();
    let root = vbox(10);
    root.set_vexpand(true);
    let mut zones = Vec::new();
    root.append(&header(inner, c));
    let tab = inner.model.borrow().tab;
    match tab {
        Tab::Shelf => shelf_tab(inner, c, &root, &mut zones),
        Tab::Recent => recent_tab(inner, c, &root),
        Tab::Activity => activity_tab(inner, c, &root),
    }
    (root.upcast(), zones)
}

fn header(inner: &Rc<Inner>, c: &Rc<Catcher>) -> gtk::Box {
    let row = hbox(6);
    let tabs = hbox(2);
    tabs.add_css_class("ws-tabs");
    let current = inner.model.borrow().tab;
    let running = inner.model.borrow().snap.jobs.iter().filter(|j| !j.status.is_final()).count();
    for (t, name) in [
        (Tab::Shelf, "Shelf".to_owned()),
        (Tab::Recent, "Recent".to_owned()),
        (Tab::Activity, if running > 0 { format!("Activity · {running}") } else { "Activity".to_owned() }),
    ] {
        let b = gtk::Button::with_label(&name);
        if t == current {
            b.add_css_class("active");
        }
        let (wi, wc) = (Rc::downgrade(inner), Rc::downgrade(c));
        b.connect_clicked(move |_| {
            if let Some(i) = wi.upgrade() {
                i.model.borrow_mut().tab = t;
                rerender(&wc);
            }
        });
        tabs.append(&b);
    }
    row.append(&tabs);
    let spacer = hbox(0);
    spacer.set_hexpand(true);
    row.append(&spacer);
    if current == Tab::Shelf {
        row.append(&shelf_menu(inner));
    }
    let close = icon_button("ws-close-symbolic", "Close (Esc)");
    let wc = Rc::downgrade(c);
    close.connect_clicked(move |_| {
        if let Some(c) = wc.upgrade() {
            c.event(waystow_core::catcher::Event::Hide);
        }
    });
    row.append(&close);
    row
}

fn shelf_menu(inner: &Rc<Inner>) -> gtk::MenuButton {
    let pop = gtk::Popover::new();
    pop.add_css_class("ws-pop");
    inner.track_popover(&pop);
    let col = vbox(6);
    let entry = gtk::Entry::builder().placeholder_text("New collection…").build();
    entry.add_css_class("ws-entry");
    let wi = Rc::downgrade(inner);
    let p = pop.clone();
    entry.connect_activate(move |e| {
        let name = e.text().trim().to_owned();
        if let (false, Some(i)) = (name.is_empty(), wi.upgrade()) {
            i.send(Cmd::NewCollection(name));
            p.popdown();
        }
    });
    col.append(&entry);
    let (collection, has_unpinned) = {
        let m = inner.model.borrow();
        (m.collection, m.shelf().iter().any(|i| !i.pinned))
    };
    let clear = gtk::Button::with_label("Clear unpinned items");
    clear.add_css_class("ws-action");
    clear.set_sensitive(has_unpinned);
    let (wi, p) = (Rc::downgrade(inner), pop.clone());
    clear.connect_clicked(move |_| {
        if let Some(i) = wi.upgrade() {
            i.send(Cmd::Clear(collection));
        }
        p.popdown();
    });
    col.append(&clear);
    if collection != TEMPORARY {
        let del = gtk::Button::with_label("Delete this collection");
        del.add_css_class("ws-action");
        let (wi, p) = (Rc::downgrade(inner), pop.clone());
        del.connect_clicked(move |_| {
            if let Some(i) = wi.upgrade() {
                i.send(Cmd::DeleteCollection(collection));
            }
            p.popdown();
        });
        col.append(&del);
    }
    pop.set_child(Some(&col));
    let mb = gtk::MenuButton::new();
    mb.set_child(Some(&super::icons::glyph("more", 16)));
    mb.add_css_class("ws-icon");
    mb.set_popover(Some(&pop));
    mb.set_tooltip_text(Some("Collections"));
    mb
}

fn chips(inner: &Rc<Inner>, c: &Rc<Catcher>, zones: &mut Vec<Zone>) -> gtk::ScrolledWindow {
    let row = hbox(6);
    let m = inner.model.borrow();
    for col in &m.snap.collections {
        let n = m.snap.items.iter().filter(|i| i.collection == col.id).count();
        let text = match (col.emoji.is_empty(), n) {
            (true, 0) => col.name.clone(),
            (true, n) => format!("{}  {n}", col.name),
            (false, 0) => format!("{} {}", col.emoji, col.name),
            (false, n) => format!("{} {}  {n}", col.emoji, col.name),
        };
        let b = gtk::Button::with_label(&text);
        b.add_css_class("ws-chip");
        if col.id == m.collection {
            b.add_css_class("active");
        }
        let id = col.id;
        let (wi, wc) = (Rc::downgrade(inner), Rc::downgrade(c));
        b.connect_clicked(move |_| {
            if let Some(i) = wi.upgrade() {
                let mut m = i.model.borrow_mut();
                m.collection = id;
                m.selection.clear();
                drop(m);
                rerender(&wc);
            }
        });
        zones.push(Zone { widget: b.clone().upcast(), target: ZoneTarget::Keep(id) });
        row.append(&b);
    }
    let sw = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&row)
        .build();
    sw.add_css_class("ws-scroll");
    sw
}

fn shelf_tab(inner: &Rc<Inner>, c: &Rc<Catcher>, root: &gtk::Box, zones: &mut Vec<Zone>) {
    root.append(&chips(inner, c, zones));
    let (items, selection, collection): (Vec<Item>, Vec<i64>, i64) = {
        let m = inner.model.borrow();
        (m.shelf().into_iter().cloned().collect(), m.selection.iter().copied().collect(), m.collection)
    };
    if items.is_empty() {
        let empty = vbox(6);
        empty.add_css_class("ws-empty");
        empty.set_vexpand(true);
        empty.set_valign(gtk::Align::Fill);
        let inner_box = vbox(8);
        inner_box.set_vexpand(true);
        inner_box.set_valign(gtk::Align::Center);
        inner_box.append(&super::icons::glyph("tray", 40));
        let t = label("Drop files here", "ws-heading");
        t.set_xalign(0.5);
        inner_box.append(&t);
        let s = label(
            "Drag anything to the screen edge to keep it here,\nthen drag it out wherever you need it.",
            "ws-sub",
        );
        s.set_xalign(0.5);
        s.set_justify(gtk::Justification::Center);
        inner_box.append(&s);
        empty.append(&inner_box);
        zones.push(Zone { widget: empty.clone().upcast(), target: ZoneTarget::Keep(collection) });
        root.append(&empty);
        jobs_strip(inner, c, root);
        return;
    }

    // The stack: drag it to take every item at once.
    let card = hbox(14);
    card.add_css_class("ws-stack");
    card.append(&widgets::thumb_stack(&items, 54, false));
    let words = vbox(2);
    words.set_valign(gtk::Align::Center);
    words.append(&label(
        &if items.len() == 1 { "1 item".into() } else { format!("{} items", items.len()) },
        "ws-heading",
    ));
    let hint = label(&stack_hint(selection.len()), "ws-sub");
    words.append(&hint);
    card.append(&words);
    let all: Vec<i64> = items.iter().map(|i| i.id).collect();
    card.add_controller(dnd::source(inner, c, move || all.clone()));
    let click = gtk::GestureClick::new();
    let (wi, wc) = (Rc::downgrade(inner), Rc::downgrade(c));
    click.connect_released(move |_, _, _, _| {
        if let Some(i) = wi.upgrade() {
            i.model.borrow_mut().selection.clear();
            reselect(&wc, &i);
        }
    });
    card.add_controller(click);
    zones.push(Zone { widget: card.clone().upcast(), target: ZoneTarget::Keep(collection) });
    root.append(&card);

    let list = vbox(2);
    let now = model::now();
    let new_ids: Vec<i64> = {
        let mut m = inner.model.borrow_mut();
        let first_render = m.seen.is_empty();
        let fresh: Vec<i64> = items.iter().map(|i| i.id).filter(|id| !m.seen.contains(id)).collect();
        m.seen.extend(fresh.iter().copied());
        if first_render { Vec::new() } else { fresh }
    };
    let mut rows = Vec::new();
    for item in &items {
        let row = item_row(inner, c, item, selection.contains(&item.id), new_ids.contains(&item.id), now);
        list.append(&row);
        rows.push((item.id, row));
    }
    let sw =
        gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&list).build();
    sw.add_css_class("ws-scroll");
    sw.add_css_class("ws-main-scroll");
    root.append(&sw);
    jobs_strip(inner, c, root);
    let bar = hbox(6);
    bar.add_css_class("ws-actions");
    fill_action_bar(inner, &bar);
    root.append(&bar);
    *c.parts.borrow_mut() = ShelfParts { rows, hint: Some(hint), bar: Some(bar) };
}

fn item_row(inner: &Rc<Inner>, c: &Rc<Catcher>, item: &Item, selected: bool, new: bool, now: i64) -> gtk::Box {
    let row = hbox(10);
    row.add_css_class("ws-row");
    if selected {
        row.add_css_class("selected");
    }
    if new {
        row.add_css_class("new");
    }
    if widgets::missing(item) {
        row.add_css_class("missing");
    }
    row.append(&widgets::item_thumb(item, 40));
    let mode =
        if item.content.path().is_some() { gtk::pango::EllipsizeMode::Middle } else { gtk::pango::EllipsizeMode::End };
    row.append(&widgets::two_lines_with(&item.display_name(), &widgets::item_detail(item, now), mode));
    let tools = hbox(0);
    tools.add_css_class("ws-row-tools");
    if item.pinned {
        tools.add_css_class("pinned");
    }
    let pin = icon_button("ws-pin-symbolic", if item.pinned { "Unpin" } else { "Pin (never expires)" });
    if item.pinned {
        pin.add_css_class("on");
    }
    let (id, pinned) = (item.id, item.pinned);
    let wi = Rc::downgrade(inner);
    pin.connect_clicked(move |_| {
        if let Some(i) = wi.upgrade() {
            i.send(Cmd::Pin(id, !pinned));
        }
    });
    tools.append(&pin);
    let rm = icon_button("ws-close-symbolic", "Remove from shelf (the file is kept)");
    let wi = Rc::downgrade(inner);
    rm.connect_clicked(move |_| {
        if let Some(i) = wi.upgrade() {
            i.send(Cmd::Remove(vec![id]));
        }
    });
    tools.append(&rm);
    row.append(&tools);

    let click = gtk::GestureClick::new();
    click.set_button(0);
    let (wi, wc) = (Rc::downgrade(inner), Rc::downgrade(c));
    click.connect_pressed(move |g, n, _, _| {
        let Some(i) = wi.upgrade() else { return };
        let state = g.current_event_state();
        if n == 2 {
            i.send(Cmd::Open(id));
            return;
        }
        let mut m = i.model.borrow_mut();
        if state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
            if !m.selection.remove(&id) {
                m.selection.insert(id);
            }
        } else if state.contains(gtk::gdk::ModifierType::SHIFT_MASK) && !m.selection.is_empty() {
            let order: Vec<i64> = m.shelf().iter().map(|i| i.id).collect();
            let anchor = m.selection.iter().filter_map(|s| order.iter().position(|o| o == s)).min().unwrap_or(0);
            let at = order.iter().position(|o| *o == id).unwrap_or(0);
            let (a, b) = (anchor.min(at), anchor.max(at));
            m.selection = order[a..=b].iter().copied().collect();
        } else if m.selection.len() == 1 && m.selection.contains(&id) && g.current_button() == 1 {
            m.selection.clear();
        } else {
            m.selection.clear();
            m.selection.insert(id);
        }
        drop(m);
        reselect(&wc, &i);
    });
    row.add_controller(click);
    let wi = Rc::downgrade(inner);
    row.add_controller(dnd::source(inner, c, move || {
        let Some(i) = wi.upgrade() else { return vec![id] };
        let sel: Vec<i64> = i.model.borrow().selection.iter().copied().collect();
        if sel.contains(&id) { sel } else { vec![id] }
    }));
    row
}

fn fill_action_bar(inner: &Rc<Inner>, bar: &gtk::Box) {
    let (targets, actions): (Vec<i64>, Vec<Action>) = {
        let m = inner.model.borrow();
        let t = m.targets();
        let mimes: Vec<&str> = t.iter().map(|i| i.mime.as_str()).collect();
        (t.iter().map(|i| i.id).collect(), m.actions_for(&mimes))
    };
    if actions.is_empty() {
        return;
    }
    let (primary, rest): (Vec<&Action>, Vec<&Action>) = {
        let p: Vec<&Action> = actions.iter().filter(|a| a.primary).take(3).collect();
        let r: Vec<&Action> = actions.iter().filter(|a| !p.iter().any(|x| x.id == a.id)).collect();
        (p, r)
    };
    // One labelled primary button, the next ones as icons, the rest under More.
    for (n, a) in primary.iter().enumerate() {
        let b = action_button(inner, a, &targets, n == 0);
        if n == 0 {
            b.add_css_class("primary");
            b.set_hexpand(true);
        }
        bar.append(&b);
    }
    if !rest.is_empty() {
        let pop = gtk::Popover::new();
        pop.add_css_class("ws-pop");
        inner.track_popover(&pop);
        let col = vbox(4);
        for a in rest {
            let b = action_button(inner, a, &targets, true);
            b.add_css_class("flat");
            b.set_halign(gtk::Align::Fill);
            if let Some(c) = b.child() {
                c.set_halign(gtk::Align::Start);
            }
            let p = pop.clone();
            b.connect_clicked(move |_| p.popdown());
            col.append(&b);
        }
        let sw = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(360)
            .child(&col)
            .build();
        pop.set_child(Some(&sw));
        let mb = gtk::MenuButton::new();
        mb.set_label("More");
        mb.add_css_class("ws-action");
        mb.set_popover(Some(&pop));
        bar.append(&mb);
    }
}

fn action_button(inner: &Rc<Inner>, a: &Action, targets: &[i64], labelled: bool) -> gtk::Button {
    let content = hbox(6);
    content.append(&super::icons::widget(&a.icon, 15));
    if labelled {
        let l = gtk::Label::new(Some(&a.title));
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        content.append(&l);
    }
    content.set_halign(gtk::Align::Center);
    let b = gtk::Button::new();
    b.set_child(Some(&content));
    b.add_css_class("ws-action");
    b.set_tooltip_text(Some(&a.title));
    let (wi, action, ids) = (Rc::downgrade(inner), a.clone(), targets.to_vec());
    b.connect_clicked(move |b| {
        let Some(i) = wi.upgrade() else { return };
        if action.params.is_empty() {
            i.send(Cmd::Run { action: action.id.clone(), ids: ids.clone(), params: BTreeMap::new() });
        } else {
            params_popover(&i, b, &action, &ids);
        }
    });
    b
}

/// Reads one param editor's value.
type Getter = Box<dyn Fn() -> String>;

/// Param editors with defaults, then Run.
fn params_popover(inner: &Rc<Inner>, anchor: &gtk::Button, action: &Action, ids: &[i64]) {
    let pop = gtk::Popover::new();
    pop.add_css_class("ws-pop");
    inner.track_popover(&pop);
    pop.set_parent(anchor);
    let col = vbox(8);
    col.append(&label(&action.title, "ws-heading"));
    let first_name = {
        let m = inner.model.borrow();
        m.snap.items.iter().find(|i| ids.contains(&i.id)).map(Item::display_name).unwrap_or_default()
    };
    let mut getters: Vec<(String, Getter)> = Vec::new();
    for p in &action.params {
        col.append(&label(if p.label.is_empty() { &p.name } else { &p.label }, "ws-caption"));
        match &p.kind {
            ParamKind::Int { default, min, max } => {
                let spin = gtk::SpinButton::with_range(*min as f64, *max as f64, 1.0);
                spin.set_value(*default as f64);
                col.append(&spin);
                getters.push((p.name.clone(), Box::new(move || (spin.value() as i64).to_string())));
            }
            ParamKind::Choice { choices, default } => {
                let refs: Vec<&str> = choices.iter().map(String::as_str).collect();
                let dd = gtk::DropDown::from_strings(&refs);
                dd.set_selected(choices.iter().position(|c| c == default).unwrap_or(0) as u32);
                col.append(&dd);
                let choices = choices.clone();
                getters.push((p.name.clone(), Box::new(move || choices[dd.selected() as usize].clone())));
            }
            ParamKind::Text { default } => {
                let e = gtk::Entry::new();
                e.add_css_class("ws-entry");
                e.set_text(&default.replace("{name}", &first_name));
                col.append(&e);
                getters.push((p.name.clone(), Box::new(move || e.text().to_string())));
            }
        }
    }
    let run = gtk::Button::with_label("Run");
    run.add_css_class("ws-action");
    run.add_css_class("primary");
    let (wi, id, ids, p) = (Rc::downgrade(inner), action.id.clone(), ids.to_vec(), pop.clone());
    run.connect_clicked(move |_| {
        if let Some(i) = wi.upgrade() {
            let params = getters.iter().map(|(k, g)| (k.clone(), g())).collect();
            i.send(Cmd::Run { action: id.clone(), ids: ids.clone(), params });
        }
        p.popdown();
    });
    col.append(&run);
    pop.set_child(Some(&col));
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    pop.popup();
}

/// Running and queued jobs with progress and a cancel button.
fn jobs_strip(inner: &Rc<Inner>, c: &Rc<Catcher>, root: &gtk::Box) {
    let (jobs, progress) = {
        let m = inner.model.borrow();
        let jobs: Vec<_> = m.snap.jobs.iter().filter(|j| !j.status.is_final()).cloned().collect();
        (jobs, m.progress.clone())
    };
    for j in jobs {
        let row = vbox(4);
        row.add_css_class("ws-job");
        let top = hbox(8);
        let name =
            j.inputs.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let status = if j.status == JobStatus::Queued { "waiting" } else { "working…" };
        top.append(&two_lines(&j.title, &format!("{name} · {status}")));
        let cancel = icon_button("ws-stop-symbolic", "Cancel");
        let (wi, id) = (Rc::downgrade(inner), j.id);
        cancel.connect_clicked(move |_| {
            if let Some(i) = wi.upgrade() {
                i.send(Cmd::Cancel(id));
            }
        });
        top.append(&cancel);
        row.append(&top);
        let bar = gtk::ProgressBar::new();
        bar.add_css_class("ws-progress");
        match progress.get(&j.id) {
            Some(f) => bar.set_fraction(f64::from(*f)),
            None => pulse(&bar),
        }
        c.register_progress(j.id, bar.clone());
        row.append(&bar);
        root.append(&row);
    }
}

fn pulse(bar: &gtk::ProgressBar) {
    bar.set_pulse_step(0.12);
    let weak = bar.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
        let Some(b) = weak.upgrade() else { return glib::ControlFlow::Break };
        if b.fraction() > 0.0 && b.fraction() < 1.0 && b.pulse_step() == 0.0 {
            return glib::ControlFlow::Break;
        }
        b.pulse();
        glib::ControlFlow::Continue
    });
}

// ── Recent ───────────────────────────────────────────────────────────────

fn recent_tab(inner: &Rc<Inner>, c: &Rc<Catcher>, root: &gtk::Box) {
    let filters = gtk::FlowBox::new();
    filters.set_selection_mode(gtk::SelectionMode::None);
    filters.set_column_spacing(6);
    filters.set_row_spacing(6);
    filters.set_max_children_per_line(6);
    filters.set_homogeneous(false);
    let current = inner.model.borrow().recent_filter;
    for (k, name) in [
        (None, "All"),
        (Some(RecentKind::Downloaded), "Downloads"),
        (Some(RecentKind::Screenshot), "Screenshots"),
        (Some(RecentKind::Copied), "Copied"),
        (Some(RecentKind::Opened), "Opened"),
        (Some(RecentKind::Created), "Files"),
    ] {
        let b = gtk::Button::with_label(name);
        b.add_css_class("ws-chip");
        if k == current {
            b.add_css_class("active");
        }
        let (wi, wc) = (Rc::downgrade(inner), Rc::downgrade(c));
        b.connect_clicked(move |_| {
            if let Some(i) = wi.upgrade() {
                i.model.borrow_mut().recent_filter = k;
                rerender(&wc);
            }
        });
        filters.insert(&b, -1);
    }
    root.append(&filters);

    let entries: Vec<_> = {
        let m = inner.model.borrow();
        m.snap
            .recent
            .iter()
            .filter(|r| match current {
                None => true,
                Some(RecentKind::Created) => matches!(r.kind, RecentKind::Created | RecentKind::Modified),
                Some(k) => r.kind == k,
            })
            .filter(|r| r.path.exists())
            .take(150)
            .cloned()
            .collect()
    };
    let list = vbox(2);
    let now = model::now();
    let mut last_day = String::new();
    for r in &entries {
        let day = day_label(r.at, now);
        if day != last_day {
            let h = label(&day.to_uppercase(), "ws-caption");
            h.set_margin_top(if last_day.is_empty() { 0 } else { 8 });
            h.set_margin_start(6);
            list.append(&h);
            last_day = day;
        }
        list.append(&recent_row(inner, c, &r.path, r.kind, now - r.at));
    }
    if entries.is_empty() {
        let e = label("Nothing recent yet.\nDownloads, screenshots and files you copy show up here.", "ws-sub");
        e.set_justify(gtk::Justification::Center);
        e.set_xalign(0.5);
        e.set_vexpand(true);
        list.append(&e);
    }
    let sw =
        gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&list).build();
    sw.add_css_class("ws-scroll");
    sw.add_css_class("ws-main-scroll");
    root.append(&sw);
}

fn day_label(at: i64, now: i64) -> String {
    let day =
        |t: i64| glib::DateTime::from_unix_local(t).ok().and_then(|d| d.format("%Y-%m-%d").ok()).map(|s| s.to_string());
    if day(at) == day(now) {
        "Today".into()
    } else if day(at) == day(now - 86_400) {
        "Yesterday".into()
    } else {
        glib::DateTime::from_unix_local(at)
            .ok()
            .and_then(|d| d.format("%A, %e %B").ok())
            .map_or_else(|| "Earlier".into(), |s| s.to_string())
    }
}

fn recent_row(inner: &Rc<Inner>, c: &Rc<Catcher>, path: &std::path::Path, kind: RecentKind, age: i64) -> gtk::Box {
    let row = hbox(10);
    row.add_css_class("ws-row");
    let mime = files::probe(path).map_or_else(|_| "application/octet-stream".into(), |(m, _)| m);
    row.append(&widgets::thumb(Some(path), &mime, 36));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let what = match kind {
        RecentKind::Downloaded => "Downloaded",
        RecentKind::Screenshot => "Screenshot",
        RecentKind::Copied => "Copied",
        RecentKind::Opened => "Opened",
        RecentKind::Created => "New",
        RecentKind::Modified => "Changed",
    };
    row.append(&two_lines(&name, &format!("{what} · {}", files::ago(age))));
    let tools = hbox(0);
    tools.add_css_class("ws-row-tools");
    let add = icon_button("ws-add-symbolic", "Put on the shelf");
    let (wi, p) = (Rc::downgrade(inner), path.to_path_buf());
    add.connect_clicked(move |_| {
        if let Some(i) = wi.upgrade() {
            let c = i.model.borrow().collection;
            i.send(Cmd::AddPaths(vec![p.clone()], c));
        }
    });
    tools.append(&add);
    let reveal = icon_button("ws-folder-symbolic", "Show in folder");
    let (wi, p) = (Rc::downgrade(inner), path.to_path_buf());
    reveal.connect_clicked(move |_| {
        if let Some(i) = wi.upgrade() {
            i.send(Cmd::Reveal(vec![p.clone()]));
        }
    });
    tools.append(&reveal);
    row.append(&tools);
    let click = gtk::GestureClick::new();
    let (wi, p) = (Rc::downgrade(inner), path.to_path_buf());
    click.connect_pressed(move |_, n, _, _| {
        if let (2, Some(i)) = (n, wi.upgrade()) {
            i.send(Cmd::OpenPath(p.clone()));
        }
    });
    row.add_controller(click);
    let p = path.to_path_buf();
    row.add_controller(dnd::path_source(c, move || vec![p.clone()]));
    row
}

// ── Activity ─────────────────────────────────────────────────────────────

fn activity_tab(inner: &Rc<Inner>, c: &Rc<Catcher>, root: &gtk::Box) {
    let list = vbox(6);
    let (downloads, jobs, progress) = {
        let m = inner.model.borrow();
        (m.downloads.clone(), m.snap.jobs.clone(), m.progress.clone())
    };
    for d in &downloads {
        let row = vbox(4);
        row.add_css_class("ws-job");
        row.append(&two_lines(
            &d.name,
            &format!("Downloading · {} · {}/s", files::human_size(d.bytes), files::human_size(d.rate as u64)),
        ));
        let bar = gtk::ProgressBar::new();
        bar.add_css_class("ws-progress");
        pulse(&bar);
        row.append(&bar);
        list.append(&row);
    }
    let now = model::now();
    for j in &jobs {
        let row = vbox(4);
        row.add_css_class("ws-job");
        if j.status == JobStatus::Failed {
            row.add_css_class("failed");
        }
        let top = hbox(8);
        let icon = match j.status {
            JobStatus::Done => "ws-check-symbolic",
            JobStatus::Failed => "ws-close-symbolic",
            JobStatus::Cancelled | JobStatus::Undone => "ws-undo-symbolic",
            JobStatus::Queued | JobStatus::Running => "ws-convert-symbolic",
        };
        top.append(&super::icons::widget(icon, 16));
        let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ins = match j.inputs.len() {
            0 => String::new(),
            1 => name(&j.inputs[0]),
            n => format!("{n} files"),
        };
        let detail = match (j.status, &j.error) {
            (JobStatus::Failed, Some(e)) => e.clone(),
            (JobStatus::Done, _) if !j.outputs.is_empty() => {
                format!("{} · → {}", files::ago(now - j.finished_at.unwrap_or(now)), name(&j.outputs[0]))
            }
            (s, _) => format!(
                "{ins} · {}",
                match s {
                    JobStatus::Queued => "waiting",
                    JobStatus::Running => "working…",
                    JobStatus::Cancelled => "cancelled",
                    JobStatus::Undone => "undone",
                    _ => "done",
                }
            ),
        };
        let words = two_lines(&j.title, &detail);
        top.append(&words);
        let id = j.id;
        if !j.status.is_final() {
            let b = icon_button("ws-stop-symbolic", "Cancel");
            let wi = Rc::downgrade(inner);
            b.connect_clicked(move |_| {
                if let Some(i) = wi.upgrade() {
                    i.send(Cmd::Cancel(id));
                }
            });
            top.append(&b);
        }
        if j.status == JobStatus::Done && j.outputs.iter().any(|o| o.exists()) {
            let b = icon_button("ws-folder-symbolic", "Show result");
            let (wi, outs) = (Rc::downgrade(inner), j.outputs.clone());
            b.connect_clicked(move |_| {
                if let Some(i) = wi.upgrade() {
                    i.send(Cmd::Reveal(outs.clone()));
                }
            });
            top.append(&b);
            let o = j.outputs.clone();
            words.add_controller(dnd::path_source(c, move || o.clone()));
        }
        if j.status == JobStatus::Done && j.undo != Undo::Nothing {
            let b = icon_button("ws-undo-symbolic", "Undo");
            let wi = Rc::downgrade(inner);
            b.connect_clicked(move |_| {
                if let Some(i) = wi.upgrade() {
                    i.send(Cmd::Undo(id));
                }
            });
            top.append(&b);
        }
        row.append(&top);
        if !j.status.is_final() {
            let bar = gtk::ProgressBar::new();
            bar.add_css_class("ws-progress");
            match progress.get(&j.id) {
                Some(f) => bar.set_fraction(f64::from(*f)),
                None => pulse(&bar),
            }
            c.register_progress(j.id, bar.clone());
            row.append(&bar);
        }
        list.append(&row);
    }
    if downloads.is_empty() && jobs.is_empty() {
        let e = label("No activity yet.\nActions you run show up here, and can be undone.", "ws-sub");
        e.set_justify(gtk::Justification::Center);
        e.set_xalign(0.5);
        e.set_vexpand(true);
        list.append(&e);
    }
    let sw =
        gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&list).build();
    sw.add_css_class("ws-scroll");
    sw.add_css_class("ws-main-scroll");
    root.append(&sw);
}

// ── Drop zones ───────────────────────────────────────────────────────────

pub fn drop_view(inner: &Rc<Inner>, offer: &Offer) -> (gtk::Widget, Vec<Zone>) {
    let root = vbox(12);
    root.set_vexpand(true);
    let mut zones = Vec::new();
    let (collections, current, actions) = {
        let m = inner.model.borrow();
        let mimes: Vec<&str> = offer.mimes.iter().map(String::as_str).collect();
        let actions: Vec<Action> = if mimes.is_empty() {
            Vec::new()
        } else {
            m.actions_for(&mimes).into_iter().filter(|a| a.primary && a.internal.is_none()).take(6).collect()
        };
        (m.snap.collections.clone(), m.collection, actions)
    };
    let caption = match (offer.names.len(), offer.names.first()) {
        (1, Some(n)) => n.clone(),
        (0, _) => "Drop to keep".into(),
        (n, _) => format!("{n} files"),
    };
    let cap = label(&caption, "ws-caption");
    cap.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    root.append(&cap);

    let keep = vbox(6);
    keep.add_css_class("ws-zone");
    keep.set_vexpand(true);
    let k_inner = vbox(6);
    k_inner.set_vexpand(true);
    k_inner.set_valign(gtk::Align::Center);
    k_inner.append(&super::icons::glyph("tray", 40));
    let name = collections.iter().find(|c| c.id == current).map_or_else(|| "Temporary".into(), |c| c.name.clone());
    let t = label("Keep on shelf", "ws-heading");
    t.set_xalign(0.5);
    k_inner.append(&t);
    let s = label(&name, "ws-sub");
    s.set_xalign(0.5);
    k_inner.append(&s);
    keep.append(&k_inner);
    zones.push(Zone { widget: keep.clone().upcast(), target: ZoneTarget::Keep(current) });
    root.append(&keep);

    let others: Vec<_> = collections.iter().filter(|c| c.id != current).collect();
    if !others.is_empty() {
        let row = gtk::FlowBox::new();
        row.set_selection_mode(gtk::SelectionMode::None);
        row.set_max_children_per_line(4);
        row.set_column_spacing(6);
        row.set_row_spacing(6);
        for col in others {
            let chip = gtk::Label::new(Some(&if col.emoji.is_empty() {
                col.name.clone()
            } else {
                format!("{} {}", col.emoji, col.name)
            }));
            chip.add_css_class("ws-chip");
            chip.set_ellipsize(gtk::pango::EllipsizeMode::End);
            zones.push(Zone { widget: chip.clone().upcast(), target: ZoneTarget::Keep(col.id) });
            row.insert(&chip, -1);
        }
        root.append(&row);
    }

    if !actions.is_empty() {
        root.append(&label("OR RUN NOW", "ws-caption"));
        let grid = gtk::Grid::new();
        grid.set_column_spacing(8);
        grid.set_row_spacing(8);
        grid.set_column_homogeneous(true);
        for (n, a) in actions.iter().enumerate() {
            let tile = vbox(6);
            tile.add_css_class("ws-tile");
            tile.append(&super::icons::widget(&a.icon, 22));
            let l = label(&a.title, "ws-sub");
            l.set_xalign(0.5);
            l.set_wrap(true);
            l.set_justify(gtk::Justification::Center);
            l.set_lines(2);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            tile.append(&l);
            zones.push(Zone { widget: tile.clone().upcast(), target: ZoneTarget::Action(a.id.clone()) });
            grid.attach(&tile, (n % 3) as i32, (n / 3) as i32, 1, 1);
        }
        root.append(&grid);
    }
    (root.upcast(), zones)
}

// ── Keyboard helpers ─────────────────────────────────────────────────────

pub fn move_selection(inner: &Rc<Inner>, down: bool) {
    let mut m = inner.model.borrow_mut();
    let order: Vec<i64> = m.shelf().iter().map(|i| i.id).collect();
    if order.is_empty() {
        return;
    }
    let at = m.selection.iter().filter_map(|s| order.iter().position(|o| o == s)).max();
    let next = match at {
        None => 0,
        Some(i) if down => (i + 1).min(order.len() - 1),
        Some(i) => i.saturating_sub(1),
    };
    m.selection.clear();
    m.selection.insert(order[next]);
}

pub fn quick_look_selection(inner: &Rc<Inner>) {
    let target = {
        let m = inner.model.borrow();
        m.targets().first().map(|i| (*i).clone())
    };
    let Some(item) = target else { return };
    if inner.close_quick_look() {
        return;
    }
    let path = match &item.content {
        Content::File(p) | Content::Stash(p) => p.clone(),
        _ => match files::input_path(&item) {
            Ok(p) => p,
            Err(_) => return,
        },
    };
    let mime =
        if matches!(item.content, Content::Text(_) | Content::Url(_)) { "text/plain" } else { item.mime.as_str() };
    inner.quick_look(&path, mime);
}

pub fn open_selection(inner: &Rc<Inner>) {
    let ids: Vec<i64> = inner.model.borrow().selection.iter().copied().take(8).collect();
    for id in ids {
        inner.send(Cmd::Open(id));
    }
}
