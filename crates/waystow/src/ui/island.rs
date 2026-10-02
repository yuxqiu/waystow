//! The island: a pill at the top (or bottom) centre for things in flight:
//! running jobs, downloads, a finished result, a new screenshot, a message.
//! It is unmapped when there's nothing to show.

use std::cell::RefCell;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge as LsEdge, KeyboardMode, Layer, LayerShell};
use waystow_core::config::{Config, IslandPosition};
use waystow_core::model::{Job, JobStatus};

use super::Inner;
use super::widgets::{self, icon_button, label};
use crate::daemon::Cmd;
use crate::files;

/// Builds the island's content.
type Builder<'a> = Box<dyn Fn(&Island) -> gtk::Widget + 'a>;

const FLASH: Duration = Duration::from_millis(3200);
const MESSAGE: Duration = Duration::from_millis(2600);

#[derive(Default)]
struct State {
    message: Option<(String, Instant)>,
    screenshot: Option<(PathBuf, Instant)>,
    finished: Option<(Job, Instant)>,
    hovered: bool,
    timer: Option<glib::SourceId>,
    shown: Option<String>,
}

pub struct Island {
    window: gtk::Window,
    body: gtk::Box,
    cmds: async_channel::Sender<Cmd>,
    state: Rc<RefCell<State>>,
    config: RefCell<Config>,
    inner: RefCell<Weak<Inner>>,
}

impl Island {
    pub fn new(cmds: async_channel::Sender<Cmd>, config: &Config) -> Self {
        let window = gtk::Window::new();
        window.add_css_class("waystow");
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("waystow-island"));
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_exclusive_zone(0);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("ws-island");
        body.set_halign(gtk::Align::Center);
        window.set_child(Some(&body));
        let this = Self {
            window,
            body,
            cmds,
            state: Rc::default(),
            config: RefCell::new(config.clone()),
            inner: RefCell::new(Weak::new()),
        };
        this.place();
        let motion = gtk::EventControllerMotion::new();
        let s = this.state.clone();
        motion.connect_enter(move |_, _, _| s.borrow_mut().hovered = true);
        let s = this.state.clone();
        motion.connect_leave(move |_| s.borrow_mut().hovered = false);
        this.body.add_controller(motion);
        this
    }

    pub fn attach(&self, inner: &Rc<Inner>) {
        *self.inner.borrow_mut() = Rc::downgrade(inner);
    }

    fn place(&self) {
        let c = self.config.borrow();
        let (on, off) = match c.island.position {
            IslandPosition::Top => (LsEdge::Top, LsEdge::Bottom),
            IslandPosition::Bottom => (LsEdge::Bottom, LsEdge::Top),
        };
        self.window.set_anchor(on, true);
        self.window.set_anchor(off, false);
        // Room for the shadow inside the surface (it would be clipped otherwise).
        let (near, far, side) = if c.ui.client_shadow { (12, 26, 22) } else { (0, 0, 0) };
        let (top, bottom) = if c.island.position == IslandPosition::Top { (near, far) } else { (far, near) };
        self.body.set_margin_top(top);
        self.body.set_margin_bottom(bottom);
        self.body.set_margin_start(side);
        self.body.set_margin_end(side);
        self.window.set_margin(on, (c.island.margin as i32 - near).max(0));
    }

    pub fn reconfigure(&self, config: &Config) {
        *self.config.borrow_mut() = config.clone();
        self.place();
        if let Some(i) = self.inner.borrow().upgrade() {
            self.refresh(&i);
        }
    }

    pub fn message(&self, inner: &Rc<Inner>, msg: &str) {
        self.state.borrow_mut().message = Some((msg.to_owned(), Instant::now()));
        self.refresh(inner);
    }

    pub fn finished(&self, inner: &Rc<Inner>, job: &Job) {
        if job.status == JobStatus::Done && !job.outputs.is_empty() {
            self.state.borrow_mut().finished = Some((job.clone(), Instant::now()));
            self.refresh(inner);
        }
    }

    pub fn screenshot(&self, inner: &Rc<Inner>, path: &Path) {
        self.state.borrow_mut().screenshot = Some((path.to_path_buf(), Instant::now()));
        self.refresh(inner);
    }

    /// Re-evaluate what to show.
    pub fn refresh(&self, inner: &Rc<Inner>) {
        if !self.config.borrow().island.enabled {
            self.window.set_visible(false);
            return;
        }
        let toast = Duration::from_secs(u64::from(self.config.borrow().island.toast_secs));
        let (hovered, shot, finished, message) = {
            let mut s = self.state.borrow_mut();
            let hovered = s.hovered;
            if !hovered {
                s.screenshot = s.screenshot.take().filter(|(_, t)| t.elapsed() < toast);
                s.finished = s.finished.take().filter(|(_, t)| t.elapsed() < FLASH);
                s.message = s.message.take().filter(|(_, t)| t.elapsed() < MESSAGE);
            }
            (hovered, s.screenshot.clone(), s.finished.clone(), s.message.clone())
        };
        let (jobs, downloads, progress) = {
            let m = inner.model.borrow();
            let jobs: Vec<Job> = m.snap.jobs.iter().filter(|j| !j.status.is_final()).cloned().collect();
            (jobs, m.downloads.clone(), m.progress.clone())
        };

        // What to show, by priority; the key avoids rebuilding identical content.
        let (key, build): (String, Builder<'_>) = if let Some((path, _)) = shot {
            (format!("shot:{}", path.display()), Box::new(move |s: &Self| s.screenshot_card(inner, &path)))
        } else if !jobs.is_empty() || !downloads.is_empty() {
            let total = jobs.len() + downloads.len();
            let fraction = if jobs.is_empty() {
                None
            } else {
                let known: Vec<f32> = jobs.iter().filter_map(|j| progress.get(&j.id).copied()).collect();
                (known.len() == jobs.len()).then(|| known.iter().sum::<f32>() / known.len() as f32)
            };
            let title = jobs.first().map_or_else(
                || {
                    if downloads.len() == 1 {
                        format!("Downloading {}", downloads[0].name)
                    } else {
                        format!("{} downloads", downloads.len())
                    }
                },
                |j| if jobs.len() == 1 { j.title.clone() } else { format!("{} · {} jobs", j.title, jobs.len()) },
            );
            let sub = match (fraction, downloads.first()) {
                (Some(f), _) => format!("{:.0}%", f * 100.0),
                (None, Some(d)) if jobs.is_empty() => {
                    format!("{} · {}/s", files::human_size(d.bytes), files::human_size(d.rate as u64))
                }
                _ => "working…".into(),
            };
            let ids: Vec<i64> = jobs.iter().map(|j| j.id).collect();
            let expanded = hovered && total > 0;
            (
                format!("busy:{title}:{sub}:{expanded}:{}", fraction.map_or(-1, |f| (f * 200.0) as i32)),
                Box::new(move |s: &Self| s.busy_pill(&title, &sub, fraction, if expanded { &ids } else { &[] })),
            )
        } else if let Some((job, _)) = finished {
            (format!("done:{}", job.id), Box::new(move |s: &Self| s.done_pill(&job)))
        } else if let Some((msg, _)) = message {
            (format!("msg:{msg}"), Box::new(move |_: &Self| msg_pill(&msg)))
        } else {
            self.window.set_visible(false);
            self.state.borrow_mut().shown = None;
            self.reschedule(inner, false);
            return;
        };
        if self.state.borrow().shown.as_deref() != Some(key.as_str()) {
            while let Some(c) = self.body.first_child() {
                self.body.remove(&c);
            }
            self.body.append(&build(self));
            self.state.borrow_mut().shown = Some(key);
            // Shrink-wrap the surface to the new content's natural size.
            let (_, w, _, _) = self.body.measure(gtk::Orientation::Horizontal, -1);
            let (_, h, _, _) = self.body.measure(gtk::Orientation::Vertical, w);
            self.window.set_default_size(w, h);
        }
        if !self.window.is_visible() {
            self.window.present();
        }
        self.reschedule(inner, true);
    }

    /// Re-check in a moment, so toasts expire and hover changes apply.
    fn reschedule(&self, inner: &Rc<Inner>, visible: bool) {
        if let Some(t) = self.state.borrow_mut().timer.take() {
            t.remove();
        }
        if !visible {
            return;
        }
        let weak = Rc::downgrade(inner);
        let state = self.state.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(400), move || {
            state.borrow_mut().timer.take();
            if let Some(i) = weak.upgrade() {
                i.island.refresh(&i);
            }
        });
        self.state.borrow_mut().timer = Some(id);
    }

    fn busy_pill(&self, title: &str, sub: &str, fraction: Option<f32>, jobs: &[i64]) -> gtk::Widget {
        let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.append(&ring(fraction));
        let t = label(title, "ws-title");
        t.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        t.set_max_width_chars(36);
        row.append(&t);
        row.append(&label(sub, "ws-sub"));
        col.append(&row);
        for &id in jobs {
            let b = gtk::Button::with_label("Cancel");
            b.add_css_class("ws-action");
            let cmds = self.cmds.clone();
            b.connect_clicked(move |_| {
                let _ = cmds.try_send(Cmd::Cancel(id));
            });
            col.append(&b);
        }
        col.upcast()
    }

    fn done_pill(&self, job: &Job) -> gtk::Widget {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let ok = super::icons::glyph("check", 16);
        ok.add_css_class("ws-ring");
        row.append(&ok);
        let out = &job.outputs[0];
        let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let t = label(&name, "ws-title");
        t.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        t.set_max_width_chars(32);
        row.append(&t);
        let show = icon_button("ws-folder-symbolic", "Show in folder");
        let outs = job.outputs.clone();
        let cmds = self.cmds.clone();
        show.connect_clicked(move |_| {
            let _ = cmds.try_send(Cmd::Reveal(outs.clone()));
        });
        row.append(&show);
        let o = job.outputs.clone();
        t.add_controller(simple_drag(move || o.clone()));
        row.upcast()
    }

    fn screenshot_card(&self, inner: &Rc<Inner>, path: &Path) -> gtk::Widget {
        let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
        col.set_margin_top(4);
        col.set_margin_bottom(4);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let thumb = widgets::thumb(Some(path), "image/png", 72);
        thumb.add_css_class("ws-shot");
        let p = path.to_path_buf();
        thumb.add_controller(simple_drag(move || vec![p.clone()]));
        top.append(&thumb);
        let words = gtk::Box::new(gtk::Orientation::Vertical, 2);
        words.set_valign(gtk::Align::Center);
        words.append(&label("Screenshot", "ws-heading"));
        words.append(&label("Drag it anywhere, or:", "ws-sub"));
        top.append(&words);
        let close = icon_button("ws-close-symbolic", "Dismiss");
        close.set_valign(gtk::Align::Start);
        close.set_halign(gtk::Align::End);
        close.set_hexpand(true);
        let state = self.state.clone();
        let w = Rc::downgrade(inner);
        close.connect_clicked(move |_| {
            state.borrow_mut().screenshot = None;
            state.borrow_mut().hovered = false;
            if let Some(i) = w.upgrade() {
                i.island.refresh(&i);
            }
        });
        top.append(&close);
        col.append(&top);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let available = |id: &str| inner.model.borrow().snap.actions.iter().any(|a| a.id == id);
        let mut buttons: Vec<(&str, &str, Option<&str>)> = Vec::new();
        if available("image.annotate") {
            buttons.push(("Annotate", "ws-edit-symbolic", Some("image.annotate")));
        }
        if available("image.ocr") {
            buttons.push(("Copy text", "ws-text-symbolic", Some("image.ocr")));
        }
        buttons.push(("Copy", "ws-copy-symbolic", Some("image.copy")));
        buttons.push(("Keep", "ws-tray-symbolic", None));
        for (title, icon, action) in buttons {
            let b = gtk::Button::new();
            let c = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            c.append(&super::icons::widget(icon, 15));
            c.append(&gtk::Label::new(Some(title)));
            b.set_child(Some(&c));
            b.add_css_class("ws-action");
            if action.is_none() {
                b.add_css_class("primary");
            }
            let (p, a, state) = (path.to_path_buf(), action.map(str::to_owned), self.state.clone());
            let cmds = self.cmds.clone();
            let w = Rc::downgrade(inner);
            b.connect_clicked(move |_| {
                let cmd = a.as_ref().map_or_else(
                    || Cmd::AddPaths(vec![p.clone()], waystow_core::model::TEMPORARY),
                    |a| Cmd::RunPaths {
                        action: a.clone(),
                        paths: vec![p.clone()],
                        params: std::collections::BTreeMap::default(),
                    },
                );
                let _ = cmds.try_send(cmd);
                state.borrow_mut().screenshot = None;
                state.borrow_mut().hovered = false;
                if let Some(i) = w.upgrade() {
                    i.island.refresh(&i);
                }
            });
            row.append(&b);
        }
        col.append(&row);
        col.upcast()
    }
}

fn msg_pill(msg: &str) -> gtk::Widget {
    let l = label(msg, "ws-title");
    l.set_wrap(true);
    l.set_max_width_chars(48);
    l.set_margin_start(6);
    l.upcast()
}

fn simple_drag(paths: impl Fn() -> Vec<PathBuf> + 'static) -> gtk::DragSource {
    let src = gtk::DragSource::new();
    src.set_actions(gtk::gdk::DragAction::COPY);
    src.connect_prepare(move |_, _, _| {
        let files: Vec<gtk::gio::File> = paths().iter().filter(|p| p.exists()).map(gtk::gio::File::for_path).collect();
        (!files.is_empty())
            .then(|| gtk::gdk::ContentProvider::for_value(&gtk::gdk::FileList::from_array(&files).to_value()))
    });
    src
}

/// A progress ring (spinning arc when the fraction is unknown).
fn ring(fraction: Option<f32>) -> gtk::Widget {
    let area = gtk::DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    area.add_css_class("ws-ring");
    let start = Instant::now();
    area.set_draw_func(move |a, cr, w, h| {
        let c = a.color();
        let (cx, cy, r) = (f64::from(w) / 2.0, f64::from(h) / 2.0, f64::from(w.min(h)) / 2.0 - 2.0);
        cr.set_line_width(2.5);
        cr.set_source_rgba(f64::from(c.red()), f64::from(c.green()), f64::from(c.blue()), 0.22);
        cr.arc(cx, cy, r, 0.0, 2.0 * PI);
        let _ = cr.stroke();
        cr.set_source_rgba(f64::from(c.red()), f64::from(c.green()), f64::from(c.blue()), 1.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        if let Some(f) = fraction {
            cr.arc(cx, cy, r, -PI / 2.0, (2.0 * PI).mul_add(f64::from(f.clamp(0.02, 1.0)), -PI / 2.0));
        } else {
            let t = start.elapsed().as_secs_f64() * 2.0 * PI;
            cr.arc(cx, cy, r, t, PI.mul_add(0.6, t));
        }
        let _ = cr.stroke();
    });
    if fraction.is_none() {
        area.add_tick_callback(|a, _| {
            a.queue_draw();
            glib::ControlFlow::Continue
        });
    }
    area.upcast()
}
