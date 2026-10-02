//! The daemon: owns all state on the GTK main thread. IPC requests, job
//! events, UI commands, file-system and clipboard events all arrive on
//! channels and are handled one at a time, so nothing re-enters.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, bail, ensure};
use gtk::glib;
use waystow_core::action::{self, Action, Internal, Stdout};
use waystow_core::config::Config;
use waystow_core::ipc::{ActionInfo, Request, Response, Status};
use waystow_core::model::{self, Content, Item, Job, JobStatus, RecentKind, Source, TEMPORARY, Undo};
use waystow_core::{naming, paths, uri};
use waystow_jobs::{Event as JobEvent, Outcome, Runner};
use waystow_store::{NewItem, Store};

use crate::files::{self, SNIPPET, URL};
use crate::ipc::{self, Incoming};
use crate::recent::{self, RecentEvent};
use crate::system;
use crate::ui::{Snapshot, Ui};

/// Something dropped or pasted.
#[derive(Debug, Clone)]
pub enum Payload {
    /// Local files.
    Files(Vec<PathBuf>),
    /// Raw image bytes (e.g. dragged from a browser).
    Image {
        /// MIME type.
        mime: String,
        /// The bytes.
        bytes: Vec<u8>,
    },
    /// Text: a snippet, or web addresses.
    Text(String),
}

/// Commands from the UI and the watchers.
#[derive(Debug)]
pub enum Cmd {
    /// Something was dropped: keep it in `collection`, or run `action` on it.
    Drop { payload: Payload, collection: i64, action: Option<String> },
    /// Add files (from Recent).
    AddPaths(Vec<PathBuf>, i64),
    /// Take items off the shelf.
    Remove(Vec<i64>),
    /// Pin or unpin.
    Pin(i64, bool),
    /// Empty a collection.
    Clear(i64),
    /// Move items into a collection.
    Move(Vec<i64>, i64),
    /// Create a collection.
    NewCollection(String),
    /// Delete a collection.
    DeleteCollection(i64),
    /// Run an action on items.
    Run { action: String, ids: Vec<i64>, params: BTreeMap<String, String> },
    /// Run an action on files not on the shelf (Recent, screenshot toast).
    RunPaths { action: String, paths: Vec<PathBuf>, params: BTreeMap<String, String> },
    /// Cancel a job.
    Cancel(i64),
    /// Undo a job.
    Undo(i64),
    /// Open an item with its default app.
    Open(i64),
    /// Open a path.
    OpenPath(PathBuf),
    /// Show a path in the file manager.
    Reveal(Vec<PathBuf>),
    /// A drag out of the panel started or ended (expiry waits meanwhile).
    DraggingOut(bool),
    /// The UI needs fresh data.
    Refresh,
    /// The Recent watchers saw something.
    Recent(RecentEvent),
    /// The clipboard changed; these MIME types are on offer.
    Copied(Vec<String>),
    /// Stop the daemon (signal).
    Quit,
}

/// A job the daemon is tracking.
struct Running {
    title: String,
    stdout: Stdout,
    collection: i64,
    status: JobStatus,
    progress: Option<f32>,
}

pub struct Daemon {
    config: Config,
    actions: Vec<Action>,
    total_actions: usize,
    store: Store,
    runner: Runner,
    running: HashMap<i64, Running>,
    ui: Ui,
    cmds: async_channel::Sender<Cmd>,
    watchers: recent::Watchers,
    expiry: Rc<RefCell<Option<glib::SourceId>>>,
    dragging_out: bool,
    quit: glib::MainLoop,
}

/// Run the daemon until `waystow quit` or a signal.
pub fn run() -> anyhow::Result<()> {
    let listener = ipc::bind()?;
    gtk::init().context("starting GTK (is WAYLAND_DISPLAY set?)")?;
    let config = Config::load()?;
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    let (req_tx, req_rx) = async_channel::unbounded::<Incoming>();
    ipc::serve(rt.handle(), listener, req_tx);
    let (job_tx, job_rx) = async_channel::unbounded();
    let (cmd_tx, cmd_rx) = async_channel::unbounded::<Cmd>();
    let clip_tx = cmd_tx.clone();
    let main_loop = glib::MainLoop::new(None, false);

    files::clear_materialized();
    let daemon = Rc::new(RefCell::new(Daemon::new(config, rt.handle().clone(), job_tx, cmd_tx, main_loop.clone())?));
    daemon.borrow_mut().refresh();
    daemon.borrow_mut().schedule_expiry();

    let d = daemon.clone();
    glib::spawn_future_local(async move {
        while let Ok((req, reply)) = req_rx.recv().await {
            let resp = Daemon::request(&d, req).await;
            let _ = reply.send(resp);
        }
    });
    let d = daemon.clone();
    glib::spawn_future_local(async move {
        while let Ok(ev) = job_rx.recv().await {
            d.borrow_mut().job_event(ev);
        }
    });
    let d = daemon.clone();
    glib::spawn_future_local(async move {
        while let Ok(cmd) = cmd_rx.recv().await {
            Daemon::command(&d, cmd);
        }
    });
    let quit_tx = daemon.borrow().cmds.clone();
    rt.spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut term), Ok(mut int)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt())) else {
            return;
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
        let _ = quit_tx.send(Cmd::Quit).await;
    });
    if let Err(e) = waystow_wl::watch(move |mimes| {
        let _ = clip_tx.send_blocking(Cmd::Copied(mimes));
    }) {
        eprintln!("waystow: clipboard watching is off: {e:#}");
    }
    main_loop.run();
    // Jobs die with us (PDEATHSIG); record them as interrupted.
    let _ = daemon.borrow().store.fail_orphaned_jobs(model::now());
    let _ = std::fs::remove_file(paths::socket());
    drop(rt);
    Ok(())
}

impl Daemon {
    fn new(
        config: Config,
        rt: tokio::runtime::Handle,
        job_tx: async_channel::Sender<JobEvent>,
        cmds: async_channel::Sender<Cmd>,
        quit: glib::MainLoop,
    ) -> anyhow::Result<Self> {
        let store = Store::open(&paths::state_dir().join("waystow.db"), config.shelf.expiry_secs()?)?;
        let failed = store.fail_orphaned_jobs(model::now())?;
        if failed > 0 {
            eprintln!("waystow: {failed} job(s) were interrupted by the last shutdown");
        }
        let (actions, total_actions) = load_actions()?;
        let runner = Runner::new(config.jobs.limit(), job_tx, rt);
        let ui = Ui::new(&config, cmds.clone());
        let watchers = recent::Watchers::start(&config, &cmds);
        Ok(Self {
            config,
            actions,
            total_actions,
            store,
            runner,
            running: HashMap::new(),
            ui,
            cmds,
            watchers,
            expiry: Rc::default(),
            dragging_out: false,
            quit,
        })
    }

    fn reload(&mut self) -> anyhow::Result<()> {
        let config = Config::load()?;
        let (actions, total) = load_actions()?;
        self.store.temporary_expiry = config.shelf.expiry_secs()?;
        self.actions = actions;
        self.total_actions = total;
        self.watchers = recent::Watchers::start(&config, &self.cmds);
        self.ui.reconfigure(&config);
        self.config = config;
        self.refresh();
        self.schedule_expiry();
        Ok(())
    }

    // ── IPC ──────────────────────────────────────────────────────────────

    async fn request(this: &Rc<RefCell<Self>>, req: Request) -> Response {
        // Clipboard reads may block on another client; do them off the main thread.
        if let Request::AddClipboard { collection } = req {
            let cid = match this.borrow().store.collection_id(collection.as_deref()) {
                Ok(c) => c,
                Err(e) => return err(&e),
            };
            let cap = this.borrow().config.shelf.max_payload();
            return match gtk::gio::spawn_blocking(move || read_clipboard(cap)).await {
                Ok(Ok(Some(payload))) => {
                    let r = this.borrow_mut().add_payload(payload, cid, Source::Clipboard);
                    r.map_or_else(|e| err(&e), |ids| Response::Added { ids })
                }
                Ok(Ok(None)) => Response::Error { message: "the clipboard is empty".into() },
                Ok(Err(e)) => err(&e),
                Err(_) => Response::Error { message: "reading the clipboard failed".into() },
            };
        }
        let mut d = this.borrow_mut();
        d.handle(req).unwrap_or_else(|e| err(&e))
    }

    fn handle(&mut self, req: Request) -> anyhow::Result<Response> {
        let now = model::now();
        let read_only = matches!(
            req,
            Request::List { .. }
                | Request::Actions { .. }
                | Request::Jobs { .. }
                | Request::Recent { .. }
                | Request::Status
        );
        let resp = match req {
            Request::Toggle => {
                self.ui.catcher(waystow_core::catcher::Event::Toggle);
                Response::Ok
            }
            Request::Show => {
                self.ui.catcher(waystow_core::catcher::Event::Show);
                Response::Ok
            }
            Request::Hide => {
                self.ui.catcher(waystow_core::catcher::Event::Hide);
                Response::Ok
            }
            Request::Add { paths, collection } => {
                let cid = self.store.collection_id(collection.as_deref())?;
                Response::Added { ids: self.add_paths(&paths, cid, Source::Cli)? }
            }
            Request::AddText { text, collection } => {
                let cid = self.store.collection_id(collection.as_deref())?;
                Response::Added { ids: self.add_payload(Payload::Text(text), cid, Source::Cli)? }
            }
            Request::AddClipboard { .. } => unreachable!("handled in request()"),
            Request::List { collection } => {
                let cid = collection.as_deref().map(|c| self.store.collection_id(Some(c))).transpose()?;
                Response::Items { collections: self.store.collections()?, items: self.store.items(cid)? }
            }
            Request::Remove { ids } => {
                self.remove(&ids)?;
                Response::Ok
            }
            Request::Clear { collection } => {
                let cid = self.store.collection_id(collection.as_deref())?;
                let gone = self.store.clear(cid)?;
                self.forget(&gone);
                Response::Ok
            }
            Request::Pin { id, pinned } => {
                self.store.set_pinned(id, pinned, now)?;
                Response::Ok
            }
            Request::Move { ids, collection } => {
                let cid = self.store.collection_id(Some(&collection))?;
                self.store.move_items(&ids, cid, now)?;
                Response::Ok
            }
            Request::NewCollection { name, emoji } => {
                self.store.new_collection(&name, emoji.as_deref().unwrap_or(""), now)?;
                Response::Ok
            }
            Request::DeleteCollection { name } => {
                let cid = self.store.collection_id(Some(&name))?;
                let gone = self.store.delete_collection(cid)?;
                self.forget(&gone);
                Response::Ok
            }
            Request::Actions { ids } => {
                let items = self.items(&ids)?;
                let mimes: Vec<&str> = items.iter().map(|i| i.mime.as_str()).collect();
                let mut actions: Vec<ActionInfo> = self
                    .actions
                    .iter()
                    .filter(|a| a.applies_to(&mimes))
                    .map(|a| ActionInfo {
                        id: a.id.clone(),
                        title: a.title.clone(),
                        icon: a.icon.clone(),
                        primary: a.primary,
                        has_params: !a.params.is_empty(),
                    })
                    .collect();
                actions.sort_by_key(|a| !a.primary);
                Response::Actions { actions }
            }
            Request::Run { action, ids, paths, params } => {
                let jobs = self.run_action(&action, &ids, &paths, &params)?;
                Response::Started { jobs }
            }
            Request::Jobs { limit } => Response::Jobs { jobs: self.jobs(limit)? },
            Request::Cancel { job } => {
                ensure!(self.runner.cancel(job), "job {job} isn't running");
                Response::Ok
            }
            Request::Undo { job } => {
                self.undo(job)?;
                Response::Ok
            }
            Request::Recent { kind, limit } => Response::Recent { recent: self.store.recent(kind, limit)? },
            Request::Status => Response::Status(Status {
                version: env!("CARGO_PKG_VERSION").into(),
                panel: self.ui.phase_name().into(),
                items: self.store.items(None)?.len(),
                jobs: self.running.len(),
                monitors: Ui::monitors(),
                actions: (self.actions.len(), self.total_actions),
            }),
            Request::Reload => {
                self.reload()?;
                Response::Ok
            }
            Request::Quit => {
                self.quit.quit();
                Response::Ok
            }
        };
        if !read_only {
            self.refresh();
            self.schedule_expiry();
        }
        Ok(resp)
    }

    // ── UI commands ──────────────────────────────────────────────────────

    fn command(this: &Rc<RefCell<Self>>, cmd: Cmd) {
        let result = {
            let mut d = this.borrow_mut();
            d.apply(cmd)
        };
        let d = this.borrow();
        if let Err(e) = result {
            d.ui.toast(&format!("{e:#}"));
        }
        d.refresh();
        d.schedule_expiry();
    }

    fn apply(&mut self, cmd: Cmd) -> anyhow::Result<()> {
        let now = model::now();
        match cmd {
            Cmd::Drop { payload, collection, action: None } => {
                self.add_payload(payload, collection, Source::Drag)?;
            }
            Cmd::Drop { payload, collection: _, action: Some(action) } => {
                // Run on what was dropped without keeping the inputs; outputs land in Temporary.
                let paths = self.payload_paths(payload)?;
                self.run_action(&action, &[], &paths, &BTreeMap::new())?;
            }
            Cmd::AddPaths(paths, c) => {
                self.add_paths(&paths, c, Source::Recent)?;
            }
            Cmd::Remove(ids) => self.remove(&ids)?,
            Cmd::Pin(id, p) => {
                self.store.set_pinned(id, p, now)?;
            }
            Cmd::Clear(c) => {
                let gone = self.store.clear(c)?;
                self.forget(&gone);
            }
            Cmd::Move(ids, c) => self.store.move_items(&ids, c, now)?,
            Cmd::NewCollection(name) => {
                let c = self.store.new_collection(&name, "", now)?;
                self.ui.select_collection(c.id);
            }
            Cmd::DeleteCollection(c) => {
                let gone = self.store.delete_collection(c)?;
                self.forget(&gone);
                self.ui.select_collection(TEMPORARY);
            }
            Cmd::Run { action, ids, params } => {
                self.run_action(&action, &ids, &[], &params)?;
            }
            Cmd::RunPaths { action, paths, params } => {
                self.run_action(&action, &[], &paths, &params)?;
            }
            Cmd::Cancel(j) => {
                self.runner.cancel(j);
            }
            Cmd::Undo(j) => self.undo(j)?,
            Cmd::Open(id) => {
                let item = self.store.item(id)?.context("that item is gone")?;
                match &item.content {
                    Content::File(p) | Content::Stash(p) => system::open(&uri::from_path(p))?,
                    Content::Url(u) => system::open(u)?,
                    Content::Text(t) => {
                        waystow_wl::copy_text(t)?;
                        self.ui.toast("Copied");
                    }
                }
            }
            Cmd::OpenPath(p) => system::open(&uri::from_path(&p))?,
            Cmd::Reveal(p) => system::reveal(&p),
            Cmd::DraggingOut(on) => self.dragging_out = on,
            Cmd::Refresh => {}
            Cmd::Recent(ev) => self.recent_event(ev)?,
            Cmd::Copied(mimes) => self.copied(&mimes),
            Cmd::Quit => self.quit.quit(),
        }
        Ok(())
    }

    // ── Shelf ────────────────────────────────────────────────────────────

    fn items(&self, ids: &[i64]) -> anyhow::Result<Vec<Item>> {
        ids.iter().map(|&id| self.store.item(id)?.with_context(|| format!("no item {id}"))).collect()
    }

    fn add_paths(&self, paths: &[PathBuf], collection: i64, source: Source) -> anyhow::Result<Vec<i64>> {
        let now = model::now();
        let mut ids = Vec::new();
        for p in paths {
            ensure!(p.is_absolute(), "{} isn't an absolute path", p.display());
            let (mime, size) = files::probe(p)?;
            let content = if files::in_stash(p) { Content::Stash(p.clone()) } else { Content::File(p.clone()) };
            let item = self.store.add(&NewItem { collection, content, mime, size, source }, now)?;
            ids.push(item.id);
        }
        Ok(ids)
    }

    fn add_payload(&self, payload: Payload, collection: i64, source: Source) -> anyhow::Result<Vec<i64>> {
        let now = model::now();
        let cap = self.config.shelf.max_payload();
        match payload {
            Payload::Files(paths) => self.add_paths(&paths, collection, source),
            Payload::Image { mime, bytes } => {
                ensure!(bytes.len() as u64 <= cap, "the image is larger than the {} MiB limit", cap >> 20);
                let name = format!("Image {}.{}", files::clock_stamp(), files::image_ext(&mime));
                let path = files::stash_bytes(&name, &bytes)?;
                let size = Some(bytes.len() as u64);
                let item =
                    self.store.add(&NewItem { collection, content: Content::Stash(path), mime, size, source }, now)?;
                Ok(vec![item.id])
            }
            Payload::Text(text) => {
                ensure!(!text.trim().is_empty(), "the text is empty");
                ensure!(text.len() as u64 <= cap, "the text is larger than the {} MiB limit", cap >> 20);
                let lines = uri::parse_list(&text);
                let urls = !lines.is_empty() && lines.iter().all(|l| uri::is_web(l));
                let mut ids = Vec::new();
                if urls {
                    for u in lines {
                        let n = NewItem { collection, content: Content::Url(u), mime: URL.into(), size: None, source };
                        ids.push(self.store.add(&n, now)?.id);
                    }
                } else {
                    let size = Some(text.len() as u64);
                    let n = NewItem { collection, content: Content::Text(text), mime: SNIPPET.into(), size, source };
                    ids.push(self.store.add(&n, now)?.id);
                }
                Ok(ids)
            }
        }
    }

    /// Files for running an action on a dropped payload.
    fn payload_paths(&self, payload: Payload) -> anyhow::Result<Vec<PathBuf>> {
        match payload {
            Payload::Files(p) => Ok(p),
            other => {
                let ids = self.add_payload(other, TEMPORARY, Source::Drag)?;
                self.items(&ids)?.iter().map(files::input_path).collect()
            }
        }
    }

    fn remove(&self, ids: &[i64]) -> anyhow::Result<()> {
        let gone = self.store.remove(ids)?;
        self.forget(&gone);
        Ok(())
    }

    /// Delete stash files of removed items that nothing else references.
    fn forget(&self, gone: &[Item]) {
        for item in gone {
            if let Content::Stash(p) = &item.content
                && !self.store.stash_in_use(p).unwrap_or(true)
            {
                files::delete_stash_file(p);
            }
            let _ =
                std::fs::remove_file(paths::stash_dir().join("materialized").join(format!("snippet-{}.txt", item.id)));
            let _ = std::fs::remove_file(paths::stash_dir().join("materialized").join(format!("link-{}.txt", item.id)));
        }
    }

    fn schedule_expiry(&self) {
        // The callback clears the slot when it fires, so a stale id is never removed twice.
        if let Some(id) = self.expiry.borrow_mut().take() {
            id.remove();
        }
        let Ok(Some(next)) = self.store.next_expiry() else { return };
        let wait = (next - model::now()).clamp(1, 3600);
        let cmds = self.cmds.clone();
        let slot = self.expiry.clone();
        // An expiry tick is just a refresh: `refresh` expires due items first.
        *self.expiry.borrow_mut() = Some(glib::timeout_add_local_once(Duration::from_secs(wait as u64), move || {
            slot.borrow_mut().take();
            let _ = cmds.try_send(Cmd::Refresh);
        }));
    }

    fn refresh(&self) {
        if !self.dragging_out
            && let Ok(gone) = self.store.expire(model::now())
            && !gone.is_empty()
        {
            self.forget(&gone);
        }
        let snapshot = Snapshot {
            collections: self.store.collections().unwrap_or_default(),
            items: self.store.items(None).unwrap_or_default(),
            jobs: self.jobs(50).unwrap_or_default(),
            recent: self.store.recent(None, 300).unwrap_or_default(),
            actions: Rc::new(self.actions.clone()),
        };
        self.ui.update(snapshot);
    }

    // ── Actions and jobs ─────────────────────────────────────────────────

    fn action(&self, id: &str) -> anyhow::Result<&Action> {
        self.actions
            .iter()
            .find(|a| a.id == id)
            .with_context(|| format!("no action {id:?} (or its tools aren't installed; see `waystow doctor`)"))
    }

    fn run_action(
        &mut self,
        id: &str,
        ids: &[i64],
        extra: &[PathBuf],
        given: &BTreeMap<String, String>,
    ) -> anyhow::Result<Vec<i64>> {
        let action = self.action(id)?.clone();
        let items = self.items(ids)?;
        let mut mimes: Vec<String> = items.iter().map(|i| i.mime.clone()).collect();
        for p in extra {
            mimes.push(files::probe(p)?.0);
        }
        ensure!(!mimes.is_empty(), "nothing selected");
        let refs: Vec<&str> = mimes.iter().map(String::as_str).collect();
        ensure!(action.applies_to(&refs), "{} doesn't apply to this selection", action.title);
        let mut params = action.resolve_params(given)?;
        // Text params may use placeholders such as {name}; expand them per input below.
        if let Some(internal) = action.internal {
            self.run_internal(internal, &action, &items, extra, &params)?;
            return Ok(Vec::new());
        }
        let mut inputs: Vec<PathBuf> = items.iter().map(files::input_path).collect::<Result<_, _>>()?;
        inputs.extend(extra.iter().cloned());
        let collection = items.first().map_or(TEMPORARY, |i| i.collection);
        let fallback = paths::user_dir("XDG_DOWNLOAD_DIR", "Downloads").join("WayStow");
        let mut jobs = Vec::new();
        for group in action.group(&inputs) {
            let now = model::now();
            let job = self.store.new_job(&action.id, &action.title, &group, now)?;
            expand_text_params(&action, &group[0], &mut params);
            let plan = match action.plan(job, &group, &params, writable, Path::exists, &fallback) {
                Ok(p) => p,
                Err(e) => {
                    self.finish_job(job, &Outcome::Failed(format!("{e:#}")), action.stdout, collection);
                    continue;
                }
            };
            if plan.output.as_ref().is_some_and(|o| o.final_path.starts_with(&fallback)) {
                std::fs::create_dir_all(&fallback)?;
            }
            self.running.insert(
                job,
                Running {
                    title: action.title.clone(),
                    stdout: action.stdout,
                    collection,
                    status: JobStatus::Queued,
                    progress: None,
                },
            );
            self.runner.spawn(job, plan);
            jobs.push(job);
        }
        Ok(jobs)
    }

    fn run_internal(
        &self,
        internal: Internal,
        action: &Action,
        items: &[Item],
        extra: &[PathBuf],
        params: &BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        let mut paths: Vec<PathBuf> = items.iter().filter_map(|i| i.content.path().map(Path::to_path_buf)).collect();
        paths.extend(extra.iter().cloned());
        let now = model::now();
        match internal {
            Internal::CopyPath => {
                let mut lines: Vec<String> = items
                    .iter()
                    .map(|i| match &i.content {
                        Content::File(p) | Content::Stash(p) => p.display().to_string(),
                        Content::Url(u) | Content::Text(u) => u.clone(),
                    })
                    .collect();
                lines.extend(extra.iter().map(|p| p.display().to_string()));
                waystow_wl::copy_text(&lines.join("\n"))?;
                self.ui.toast(if lines.len() == 1 { "Path copied" } else { "Paths copied" });
            }
            Internal::CopyFile => {
                ensure!(!paths.is_empty(), "nothing to copy");
                let uris = uri::list_of(&paths);
                let gnome = format!("copy\n{}", paths.iter().map(|p| uri::from_path(p)).collect::<Vec<_>>().join("\n"));
                let plain = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n");
                waystow_wl::copy(vec![
                    ("text/uri-list".into(), uris.into_bytes()),
                    ("x-special/gnome-copied-files".into(), gnome.into_bytes()),
                    ("text/plain;charset=utf-8".into(), plain.into_bytes()),
                ])?;
                self.ui.toast("Copied — paste into a folder or chat");
            }
            Internal::CopyImage => {
                let p = paths.first().context("no image")?;
                let mime = files::probe(p)?.0;
                let bytes = read_capped(p, self.config.shelf.max_payload())?;
                waystow_wl::copy(vec![(mime, bytes)])?;
                self.ui.toast("Image copied");
            }
            Internal::CopyContents => {
                let text = if let Some(Content::Text(t)) = items.first().map(|i| &i.content) {
                    t.clone()
                } else {
                    let p = paths.first().context("no file")?;
                    String::from_utf8_lossy(&read_capped(p, self.config.shelf.max_payload())?).into_owned()
                };
                waystow_wl::copy_text(&text)?;
                self.ui.toast("Contents copied");
            }
            Internal::Reveal => system::reveal(&paths),
            Internal::Open => {
                for i in items {
                    match &i.content {
                        Content::Url(u) => system::open(u)?,
                        Content::File(p) | Content::Stash(p) => system::open(&uri::from_path(p))?,
                        Content::Text(_) => {}
                    }
                }
                for p in extra {
                    system::open(&uri::from_path(p))?;
                }
            }
            Internal::Rename => {
                let from = paths.first().context("nothing to rename")?.clone();
                let mut params = params.clone();
                expand_text_params(action, &from, &mut params);
                let name = params.get("new_name").map(|s| s.trim().to_owned()).unwrap_or_default();
                ensure!(naming::is_valid_name(&name), "{name:?} isn't a valid file name");
                let to = from.with_file_name(&name);
                if to == from {
                    return Ok(());
                }
                waystow_jobs::rename_noreplace(&from, &to).with_context(|| {
                    if to.exists() { format!("{name} already exists") } else { format!("renaming to {name}") }
                })?;
                for i in items {
                    if i.content.path() == Some(from.as_path()) {
                        self.store.remove(&[i.id])?;
                        let (mime, size) = files::probe(&to)?;
                        let content = Content::File(to.clone());
                        self.store
                            .add(&NewItem { collection: i.collection, content, mime, size, source: i.source }, now)?;
                    }
                }
                self.record(
                    action,
                    std::slice::from_ref(&from),
                    vec![to.clone()],
                    Undo::Rename { from: from.clone(), to },
                )?;
            }
            Internal::Trash => {
                ensure!(!paths.is_empty(), "nothing to move to the Trash");
                let mut done = Vec::new();
                for p in &paths {
                    system::trash(p)?;
                    done.push(p.clone());
                }
                let ids: Vec<i64> = items
                    .iter()
                    .filter(|i| i.content.path().is_some_and(|p| done.iter().any(|d| d == p)))
                    .map(|i| i.id)
                    .collect();
                self.remove(&ids)?;
                self.record(action, &done, Vec::new(), Undo::RestoreTrashed { paths: done.clone() })?;
                self.ui.toast("Moved to the Trash");
            }
            Internal::SaveText => {
                let item = items.first().context("nothing to save")?;
                let Content::Text(text) = &item.content else { bail!("only snippets can be saved as files") };
                let dir = paths::user_dir("XDG_DOCUMENTS_DIR", "Documents");
                let dir = if writable(&dir) { dir } else { paths::user_dir("XDG_DOWNLOAD_DIR", "Downloads") };
                std::fs::create_dir_all(&dir)?;
                let target = naming::unique(&dir.join(files::snippet_file_name(text)), Path::exists);
                std::fs::write(&target, text).with_context(|| format!("writing {}", target.display()))?;
                let (mime, size) = files::probe(&target)?;
                let n = NewItem {
                    collection: item.collection,
                    content: Content::File(target.clone()),
                    mime,
                    size,
                    source: Source::Action,
                };
                self.store.add(&n, now)?;
                self.record(action, &[], vec![target.clone()], Undo::TrashOutputs)?;
                self.ui
                    .toast(&format!("Saved {}", target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
            }
        }
        Ok(())
    }

    /// Record an internal action in the job history (so it can be undone).
    fn record(&self, action: &Action, inputs: &[PathBuf], outputs: Vec<PathBuf>, undo: Undo) -> anyhow::Result<()> {
        let now = model::now();
        let id = self.store.new_job(&action.id, &action.title, inputs, now)?;
        let mut job = self.store.job(id)?.context("job vanished")?;
        job.status = JobStatus::Done;
        job.outputs = outputs;
        job.undo = undo;
        job.finished_at = Some(now);
        self.store.save_job(&job)
    }

    fn job_event(&mut self, ev: JobEvent) {
        match ev {
            JobEvent::Started(id) => {
                if let Some(r) = self.running.get_mut(&id) {
                    r.status = JobStatus::Running;
                }
            }
            JobEvent::Progress(id, f) => {
                if let Some(r) = self.running.get_mut(&id) {
                    r.progress = Some(f);
                }
                // Progress only touches the island and the activity rows.
                self.ui.progress(id, f);
                return;
            }
            JobEvent::Finished(id, outcome) => {
                let Some(r) = self.running.remove(&id) else { return };
                self.finish_job(id, &outcome, r.stdout, r.collection);
                if !self.config.island.enabled {
                    let body = match &outcome {
                        Outcome::Done { outputs, .. } => {
                            outputs.first().map(|o| o.display().to_string()).unwrap_or_default()
                        }
                        Outcome::Failed(e) => e.clone(),
                        Outcome::Cancelled => "Cancelled".into(),
                    };
                    system::notify(&r.title, &body, None);
                }
            }
        }
        self.refresh();
        self.schedule_expiry();
    }

    fn finish_job(&self, id: i64, outcome: &Outcome, stdout: Stdout, collection: i64) {
        let Ok(Some(mut job)) = self.store.job(id) else { return };
        job.finished_at = Some(model::now());
        match outcome {
            Outcome::Done { outputs, stdout: text } => {
                job.status = JobStatus::Done;
                job.outputs.clone_from(outputs);
                job.undo = if outputs.is_empty() { Undo::Nothing } else { Undo::TrashOutputs };
                if let Err(e) = self.add_paths(outputs, collection, Source::Action) {
                    self.ui.toast(&format!("{e:#}"));
                }
                match (stdout, text) {
                    (Stdout::Clipboard, Some(t)) if !t.is_empty() => match waystow_wl::copy_text(t) {
                        Ok(()) => self.ui.toast("Copied to the clipboard"),
                        Err(e) => self.ui.toast(&format!("{e:#}")),
                    },
                    (Stdout::Clipboard | Stdout::Shelf, Some(t)) if t.trim().is_empty() => {
                        self.ui.toast(&format!("{}: nothing found", job.title));
                    }
                    (Stdout::Shelf, Some(t)) => {
                        if let Err(e) = self.add_payload(Payload::Text(t.clone()), collection, Source::Action) {
                            self.ui.toast(&format!("{e:#}"));
                        }
                    }
                    _ => {}
                }
                self.ui.finished(&job);
            }
            Outcome::Failed(e) => {
                job.status = JobStatus::Failed;
                job.error = Some(e.clone());
                self.ui.toast(&format!("{} failed: {e}", job.title));
            }
            Outcome::Cancelled => job.status = JobStatus::Cancelled,
        }
        if let Err(e) = self.store.save_job(&job) {
            eprintln!("waystow: saving job {id}: {e:#}");
        }
        let _ = self.store.prune_jobs(500);
    }

    fn jobs(&self, limit: u32) -> anyhow::Result<Vec<Job>> {
        let mut jobs = self.store.jobs(limit)?;
        for j in &mut jobs {
            if let Some(r) = self.running.get(&j.id) {
                j.status = r.status;
                j.progress = r.progress;
            }
        }
        Ok(jobs)
    }

    fn undo(&self, id: i64) -> anyhow::Result<()> {
        let mut job = self.store.job(id)?.with_context(|| format!("no job {id}"))?;
        ensure!(job.status == JobStatus::Done, "only finished jobs can be undone");
        match &job.undo {
            Undo::Nothing => bail!("{} can't be undone", job.title),
            Undo::TrashOutputs => {
                for o in &job.outputs {
                    if o.exists() {
                        system::trash(o)?;
                    }
                }
                let ids: Vec<i64> = self
                    .store
                    .items(None)?
                    .into_iter()
                    .filter(|i| i.content.path().is_some_and(|p| job.outputs.iter().any(|o| o == p)))
                    .map(|i| i.id)
                    .collect();
                self.remove(&ids)?;
            }
            Undo::Rename { from, to } => {
                waystow_jobs::rename_noreplace(to, from)
                    .with_context(|| format!("renaming {} back to {}", to.display(), from.display()))?;
                let now = model::now();
                for i in self.store.items(None)? {
                    if i.content.path() == Some(to.as_path()) {
                        self.store.remove(&[i.id])?;
                        let (mime, size) = files::probe(from)?;
                        let n = NewItem {
                            collection: i.collection,
                            content: Content::File(from.clone()),
                            mime,
                            size,
                            source: i.source,
                        };
                        self.store.add(&n, now)?;
                    }
                }
            }
            Undo::RestoreTrashed { paths } => {
                for p in paths {
                    system::restore(p)?;
                }
                self.add_paths(paths, TEMPORARY, Source::Action)?;
            }
        }
        job.status = JobStatus::Undone;
        self.store.save_job(&job)?;
        self.ui.toast(&format!("Undid {}", job.title));
        Ok(())
    }

    // ── Recent and clipboard ─────────────────────────────────────────────

    fn recent_event(&self, ev: RecentEvent) -> anyhow::Result<()> {
        let now = model::now();
        match ev {
            RecentEvent::Seen { path, kind, at } => {
                self.store.add_recent(&path, kind, at)?;
                if kind == RecentKind::Screenshot && self.config.island.enabled && self.config.island.screenshots {
                    self.ui.screenshot(&path);
                }
            }
            RecentEvent::Gone(path) => self.store.forget_recent(&path)?,
            RecentEvent::Downloads(list) => self.ui.downloads(list),
        }
        self.store.prune_recent(now - i64::from(self.config.recent.days) * 86_400)?;
        Ok(())
    }

    fn copied(&self, mimes: &[String]) {
        if !self.config.recent.copied || !mimes.iter().any(|m| m == "text/uri-list") {
            return;
        }
        let cmds = self.cmds.clone();
        gtk::gio::spawn_blocking(move || {
            let Ok(Some(bytes)) = waystow_wl::read("text/uri-list", 1 << 20) else { return };
            let now = model::now();
            for line in uri::parse_list(&String::from_utf8_lossy(&bytes)) {
                if let Some(path) = uri::to_path(&line)
                    && path.exists()
                {
                    let _ =
                        cmds.send_blocking(Cmd::Recent(RecentEvent::Seen { path, kind: RecentKind::Copied, at: now }));
                }
            }
        });
    }
}

fn err(e: &anyhow::Error) -> Response {
    Response::Error { message: format!("{e:#}") }
}

/// Actions whose tools are installed, plus the total count.
fn load_actions() -> anyhow::Result<(Vec<Action>, usize)> {
    let all = action::load(&paths::config_dir().join("actions"))?;
    let total = all.len();
    Ok((all.into_iter().filter(|a| a.requires.iter().all(|r| action::on_path(r))).collect(), total))
}

/// Can we create files in `dir`?
fn writable(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return false };
    // SAFETY: valid NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::W_OK | libc::X_OK) == 0 }
}

/// Expand `{name}`-style placeholders in text params against an input.
#[allow(clippy::literal_string_with_formatting_args)] // our placeholder syntax
fn expand_text_params(action: &Action, input: &Path, params: &mut BTreeMap<String, String>) {
    let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = naming::split_name(&name);
    for p in &action.params {
        if matches!(p.kind, waystow_core::action::ParamKind::Text { .. })
            && let Some(v) = params.get_mut(&p.name)
        {
            *v = v.replace("{name}", &name).replace("{stem}", stem).replace("{ext}", ext);
        }
    }
}

fn read_capped(path: &Path, cap: u64) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .take(cap + 1)
        .read_to_end(&mut buf)?;
    ensure!(buf.len() as u64 <= cap, "{} is larger than the {} MiB limit", path.display(), cap >> 20);
    Ok(buf)
}

/// The clipboard as a payload: files, then an image, then text.
fn read_clipboard(cap: u64) -> anyhow::Result<Option<Payload>> {
    let mimes = waystow_wl::mime_types()?;
    if mimes.iter().any(|m| m == "text/uri-list")
        && let Some(bytes) = waystow_wl::read("text/uri-list", 1 << 20)?
    {
        let paths: Vec<PathBuf> =
            uri::parse_list(&String::from_utf8_lossy(&bytes)).iter().filter_map(|u| uri::to_path(u)).collect();
        if !paths.is_empty() {
            return Ok(Some(Payload::Files(paths)));
        }
    }
    for m in ["image/png", "image/jpeg", "image/webp", "image/gif"] {
        if mimes.iter().any(|x| x == m)
            && let Some(bytes) = waystow_wl::read(m, cap)?
        {
            return Ok(Some(Payload::Image { mime: m.into(), bytes }));
        }
    }
    for m in ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"] {
        if mimes.iter().any(|x| x == m)
            && let Some(bytes) = waystow_wl::read(m, cap)?
        {
            return Ok(Some(Payload::Text(String::from_utf8_lossy(&bytes).into_owned())));
        }
    }
    Ok(None)
}
