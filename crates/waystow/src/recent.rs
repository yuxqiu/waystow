//! Watches for the Recent feed: finished downloads, new screenshots, new and
//! changed files in watched folders, and files apps opened
//! (`recently-used.xbel`). Also tracks downloads in progress for the island.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use waystow_core::config::Config;
use waystow_core::model::{self, RecentKind, Timestamp};
use waystow_core::{paths, recent};

use crate::daemon::Cmd;

/// What the watchers report.
#[derive(Debug, Clone)]
pub enum RecentEvent {
    /// A file became recent.
    Seen {
        /// The file.
        path: PathBuf,
        /// How.
        kind: RecentKind,
        /// When.
        at: Timestamp,
    },
    /// A file went away.
    Gone(PathBuf),
    /// Downloads in progress right now.
    Downloads(Vec<Download>),
}

/// A download in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    /// Final file name.
    pub name: String,
    /// Bytes so far.
    pub bytes: u64,
    /// Bytes per second (smoothed).
    pub rate: f64,
}

struct Partial {
    size: u64,
    changed: Instant,
    sampled: Instant,
    rate: f64,
}

#[derive(Default)]
struct Shared {
    partials: HashMap<PathBuf, Partial>,
    timer: Option<glib::SourceId>,
    fresh: HashMap<PathBuf, Instant>,
    screenshot_seen: HashMap<PathBuf, Instant>,
    xbel_seen: Timestamp,
}

/// Keeps the monitors alive; dropping it stops watching.
pub struct Watchers {
    _monitors: Vec<gio::FileMonitor>,
    shared: Rc<RefCell<Shared>>,
}

impl Drop for Watchers {
    fn drop(&mut self) {
        if let Some(t) = self.shared.borrow_mut().timer.take() {
            t.remove();
        }
    }
}

const IMAGE_EXTS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "avif", "jxl"];

fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn mtime(p: &Path) -> Option<Timestamp> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(t.as_secs()).ok()
}

impl Watchers {
    /// Start every watcher the config asks for. Missing directories are skipped.
    pub fn start(config: &Config, cmds: &async_channel::Sender<Cmd>) -> Self {
        let shared = Rc::new(RefCell::new(Shared::default()));
        let mut monitors = Vec::new();
        let ignore = Rc::new(config.recent.ignore.clone());
        let since = model::now() - i64::from(config.recent.days) * 86_400;
        let send = {
            let cmds = cmds.clone();
            move |ev: RecentEvent| {
                let _ = cmds.try_send(Cmd::Recent(ev));
            }
        };

        if config.recent.downloads {
            let dir = paths::user_dir("XDG_DOWNLOAD_DIR", "Downloads");
            scan(&dir, since, &ignore, |p| !is_partial(p), RecentKind::Downloaded, &send);
            if let Some(m) = monitor(&dir) {
                let (shared, ignore, send) = (shared.clone(), ignore.clone(), send.clone());
                m.connect_changed(move |_, file, other, event| {
                    downloads_event(&shared, &ignore, &send, file, other, event);
                });
                monitors.push(m);
            }
        }
        for dir in config.recent.screenshot_dirs() {
            scan(&dir, since, &ignore, is_image, RecentKind::Screenshot, &send);
            if let Some(m) = monitor(&dir) {
                let (shared, ignore, send) = (shared.clone(), ignore.clone(), send.clone());
                m.connect_changed(move |_, file, other, event| {
                    let target = match event {
                        gio::FileMonitorEvent::ChangesDoneHint | gio::FileMonitorEvent::MovedIn => file.path(),
                        gio::FileMonitorEvent::Renamed => other.and_then(gio::File::path),
                        gio::FileMonitorEvent::Deleted | gio::FileMonitorEvent::MovedOut => {
                            if let Some(p) = file.path() {
                                send(RecentEvent::Gone(p));
                            }
                            None
                        }
                        _ => None,
                    };
                    let Some(path) = target else { return };
                    if !is_image(&path)
                        || recent::ignored(&name_of(&path), &ignore)
                        || std::fs::metadata(&path).map_or(true, |m| m.len() == 0)
                    {
                        return;
                    }
                    // One screenshot can report several events; toast it once.
                    let mut s = shared.borrow_mut();
                    s.screenshot_seen.retain(|_, t| t.elapsed() < Duration::from_secs(10));
                    if s.screenshot_seen.insert(path.clone(), Instant::now()).is_some() {
                        return;
                    }
                    send(RecentEvent::Seen { path, kind: RecentKind::Screenshot, at: model::now() });
                });
                monitors.push(m);
            }
        }
        for dir in config.recent.watch_dirs() {
            scan(&dir, since, &ignore, Path::is_file, RecentKind::Modified, &send);
            if let Some(m) = monitor(&dir) {
                let (shared, ignore, send) = (shared.clone(), ignore.clone(), send.clone());
                m.connect_changed(move |_, file, other, event| {
                    watched_event(&shared, &ignore, &send, file, other, event);
                });
                monitors.push(m);
            }
        }
        if config.recent.opened {
            let xbel = paths::data_home().join("recently-used.xbel");
            shared.borrow_mut().xbel_seen = since;
            read_xbel(&xbel, &shared, &send);
            if let Ok(m) = gio::File::for_path(&xbel).monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            {
                let (shared, send) = (shared.clone(), send);
                m.connect_changed(move |_, _, _, event| {
                    if matches!(event, gio::FileMonitorEvent::ChangesDoneHint | gio::FileMonitorEvent::Created) {
                        read_xbel(&xbel, &shared, &send);
                    }
                });
                monitors.push(m);
            }
        }
        Self { _monitors: monitors, shared }
    }
}

fn is_partial(p: &Path) -> bool {
    recent::partial_target(&name_of(p)).is_some()
}

fn monitor(dir: &Path) -> Option<gio::FileMonitor> {
    if !dir.is_dir() {
        return None;
    }
    gio::File::for_path(dir).monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE).ok()
}

/// Seed the feed with files changed recently, so Recent isn't empty on first run.
fn scan(
    dir: &Path,
    since: Timestamp,
    ignore: &[String],
    keep: impl Fn(&Path) -> bool,
    kind: RecentKind,
    send: &impl Fn(RecentEvent),
) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten().take(5000) {
        let p = e.path();
        if recent::ignored(&name_of(&p), ignore) || !keep(&p) {
            continue;
        }
        if let Some(t) = mtime(&p)
            && t >= since
        {
            send(RecentEvent::Seen { path: p, kind, at: t });
        }
    }
}

fn downloads_event(
    shared: &Rc<RefCell<Shared>>,
    ignore: &[String],
    send: &(impl Fn(RecentEvent) + Clone + 'static),
    file: &gio::File,
    other: Option<&gio::File>,
    event: gio::FileMonitorEvent,
) {
    let Some(path) = file.path() else { return };
    let finished = |p: PathBuf| {
        if !recent::ignored(&name_of(&p), ignore) && std::fs::metadata(&p).is_ok_and(|m| m.is_file() && m.len() > 0) {
            send(RecentEvent::Seen { path: p, kind: RecentKind::Downloaded, at: model::now() });
        }
    };
    match event {
        gio::FileMonitorEvent::Created | gio::FileMonitorEvent::Changed if is_partial(&path) => {
            track_partial(shared, &path, send.clone());
        }
        gio::FileMonitorEvent::Renamed => {
            let Some(to) = other.and_then(gio::File::path) else { return };
            stop_partial(shared, &path, send);
            if is_partial(&to) {
                track_partial(shared, &to, send.clone());
            } else if is_partial(&path) || recent::ignored(&name_of(&path), ignore) {
                // A download (or a tool's temp file) just got its final name.
                finished(to);
            }
        }
        gio::FileMonitorEvent::MovedIn => finished(path),
        gio::FileMonitorEvent::ChangesDoneHint if !is_partial(&path) => finished(path),
        gio::FileMonitorEvent::Deleted | gio::FileMonitorEvent::MovedOut => {
            stop_partial(shared, &path, send);
            if !is_partial(&path) {
                send(RecentEvent::Gone(path));
            }
        }
        _ => {}
    }
}

fn watched_event(
    shared: &Rc<RefCell<Shared>>,
    ignore: &[String],
    send: &impl Fn(RecentEvent),
    file: &gio::File,
    other: Option<&gio::File>,
    event: gio::FileMonitorEvent,
) {
    let Some(path) = file.path() else { return };
    let now = model::now();
    let mut s = shared.borrow_mut();
    s.fresh.retain(|_, t| t.elapsed() < Duration::from_secs(600));
    match event {
        gio::FileMonitorEvent::Created => {
            s.fresh.insert(path, Instant::now());
        }
        gio::FileMonitorEvent::ChangesDoneHint => {
            if recent::ignored(&name_of(&path), ignore) || !path.is_file() {
                return;
            }
            let kind = if s.fresh.remove(&path).is_some() { RecentKind::Created } else { RecentKind::Modified };
            send(RecentEvent::Seen { path, kind, at: now });
        }
        gio::FileMonitorEvent::MovedIn => {
            if !recent::ignored(&name_of(&path), ignore) && path.is_file() {
                send(RecentEvent::Seen { path, kind: RecentKind::Created, at: now });
            }
        }
        gio::FileMonitorEvent::Renamed => {
            if let Some(to) = other.and_then(gio::File::path)
                && !recent::ignored(&name_of(&to), ignore)
                && to.is_file()
            {
                // Editors save via a temp file and rename it over the original.
                let kind =
                    if recent::ignored(&name_of(&path), ignore) { RecentKind::Modified } else { RecentKind::Created };
                send(RecentEvent::Gone(path));
                send(RecentEvent::Seen { path: to, kind, at: now });
            }
        }
        gio::FileMonitorEvent::Deleted | gio::FileMonitorEvent::MovedOut => send(RecentEvent::Gone(path)),
        _ => {}
    }
}

fn read_xbel(path: &Path, shared: &Rc<RefCell<Shared>>, send: &impl Fn(RecentEvent)) {
    let Ok(xml) = std::fs::read_to_string(path) else { return };
    let mut s = shared.borrow_mut();
    let mut newest = s.xbel_seen;
    for (p, at) in recent::parse_xbel(&xml) {
        if at > s.xbel_seen && p.is_file() {
            newest = newest.max(at);
            send(RecentEvent::Seen { path: p, kind: RecentKind::Opened, at });
        }
    }
    s.xbel_seen = newest;
}

fn track_partial(shared: &Rc<RefCell<Shared>>, path: &Path, send: impl Fn(RecentEvent) + 'static) {
    let mut s = shared.borrow_mut();
    s.partials.entry(path.to_path_buf()).or_insert_with(|| Partial {
        size: std::fs::metadata(path).map_or(0, |m| m.len()),
        changed: Instant::now(),
        sampled: Instant::now(),
        rate: 0.0,
    });
    if s.timer.is_none() {
        let weak = Rc::downgrade(shared);
        s.timer = Some(glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(shared) = weak.upgrade() else { return glib::ControlFlow::Break };
            let mut s = shared.borrow_mut();
            sample(&mut s.partials);
            let list: Vec<Download> = s
                .partials
                .iter()
                .map(|(p, d)| Download {
                    name: recent::partial_target(&name_of(p)).unwrap_or_default().to_owned(),
                    bytes: d.size,
                    rate: d.rate,
                })
                .collect();
            let done = list.is_empty();
            send(RecentEvent::Downloads(list));
            if done {
                s.timer = None;
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        }));
    }
}

fn stop_partial(shared: &Rc<RefCell<Shared>>, path: &Path, send: &impl Fn(RecentEvent)) {
    let mut s = shared.borrow_mut();
    if s.partials.remove(path).is_some() && s.partials.is_empty() {
        send(RecentEvent::Downloads(Vec::new()));
    }
}

/// Update sizes and smoothed rates; drop downloads that vanished or stalled for 2 minutes.
fn sample(partials: &mut HashMap<PathBuf, Partial>) {
    partials.retain(|path, d| {
        let Ok(meta) = std::fs::metadata(path) else { return false };
        let size = meta.len();
        let dt = d.sampled.elapsed().as_secs_f64().max(0.001);
        let inst = size.saturating_sub(d.size) as f64 / dt;
        d.rate = if d.rate == 0.0 { inst } else { 0.7f64.mul_add(d.rate, 0.3 * inst) };
        if size != d.size {
            d.changed = Instant::now();
        }
        d.size = size;
        d.sampled = Instant::now();
        d.changed.elapsed() < Duration::from_secs(120)
    });
}
