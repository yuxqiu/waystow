//! Runs action [`Plan`]s.
//!
//! - Each job runs in its own process group, dies with the daemon
//!   (`PR_SET_PDEATHSIG` = SIGKILL), and is cancelled with SIGTERM to the whole
//!   group, then SIGKILL after [`KILL_AFTER`].
//! - Tools write a hidden temporary sibling of the output. A job succeeds only
//!   if it exits with a success code and its output exists and isn't empty;
//!   then the temp file is renamed into place without ever replacing a file.
//!   Anything else removes the temp output.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Semaphore, oneshot};
use waystow_core::action::{Plan, Progress, Stdout};
use waystow_core::naming;
use waystow_core::progress::Tracker;

/// Grace period between SIGTERM and SIGKILL when cancelling.
pub const KILL_AFTER: Duration = Duration::from_secs(3);
/// Largest captured standard output (clipboard and shelf modes).
pub const MAX_CAPTURE: usize = 16 << 20;
const STDERR_TAIL: usize = 4096;

/// What happened to a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Finished.
    Done {
        /// Final output paths (empty for actions without output, or an
        /// interactive tool closed without saving).
        outputs: Vec<PathBuf>,
        /// Captured standard output, for clipboard and shelf modes.
        stdout: Option<String>,
    },
    /// The tool failed or produced nothing usable.
    Failed(String),
    /// Cancelled by the user.
    Cancelled,
}

/// Events from running jobs.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The job got a slot and its process started.
    Started(i64),
    /// Progress, 0–1.
    Progress(i64, f32),
    /// The job ended.
    Finished(i64, Outcome),
}

/// Runs jobs with bounded concurrency.
#[derive(Clone)]
pub struct Runner {
    slots: Arc<Semaphore>,
    cancels: Arc<Mutex<HashMap<i64, oneshot::Sender<()>>>>,
    events: async_channel::Sender<Event>,
    rt: tokio::runtime::Handle,
}

impl Runner {
    /// A runner allowing `limit` jobs at once, sending [`Event`]s to `events`.
    #[must_use]
    pub fn new(limit: usize, events: async_channel::Sender<Event>, rt: tokio::runtime::Handle) -> Self {
        Self { slots: Arc::new(Semaphore::new(limit.max(1))), cancels: Arc::default(), events, rt }
    }

    /// Queue a job. Its events arrive on the channel.
    #[allow(clippy::significant_drop_tightening)] // the slot permit lives as long as the job
    pub fn spawn(&self, job: i64, plan: Plan) {
        let (tx, mut cancel) = oneshot::channel();
        {
            let mut cancels = self.cancels.lock().unwrap_or_else(PoisonError::into_inner);
            cancels.insert(job, tx);
        }
        let this = self.clone();
        self.rt.spawn(async move {
            // The permit is the job's slot: it must live until the job ends.
            let permit = tokio::select! {
                p = this.slots.clone().acquire_owned() => p.ok(),
                _ = &mut cancel => None,
            };
            let outcome = match permit {
                None => Outcome::Cancelled,
                Some(_permit) => {
                    let _ = this.events.send(Event::Started(job)).await;
                    run(job, &plan, &mut cancel, &this.events).await
                }
            };
            this.cancels.lock().unwrap_or_else(PoisonError::into_inner).remove(&job);
            let _ = this.events.send(Event::Finished(job, outcome)).await;
        });
    }

    /// Cancel a queued or running job. Returns false if it isn't known.
    pub fn cancel(&self, job: i64) -> bool {
        let tx = self.cancels.lock().unwrap_or_else(PoisonError::into_inner).remove(&job);
        tx.is_some_and(|tx| tx.send(()).is_ok())
    }
}

async fn run(
    job: i64,
    plan: &Plan,
    cancel: &mut oneshot::Receiver<()>,
    events: &async_channel::Sender<Event>,
) -> Outcome {
    let program = plan.argv.first().cloned().unwrap_or_default();
    if let Some(out) = &plan.output
        && out.is_dir
        && let Err(e) = std::fs::create_dir(&out.temp)
    {
        return Outcome::Failed(format!("creating {}: {e}", out.temp.display()));
    }
    let mut cmd = Command::new(&program);
    cmd.args(&plan.argv[1..]).stdin(Stdio::null());
    if let Some(cwd) = &plan.cwd {
        cmd.current_dir(cwd);
    }
    if plan.detach {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        // SAFETY: setsid is async-signal-safe; the closure allocates nothing.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        return match cmd.spawn() {
            Ok(mut child) => {
                // Reap it whenever it exits so it never lingers as a zombie.
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
                Outcome::Done { outputs: Vec::new(), stdout: None }
            }
            Err(e) => Outcome::Failed(spawn_error(&program, &e)),
        };
    }
    match (&plan.stdout, &plan.output) {
        (Stdout::File, Some(out)) => match std::fs::File::create(&out.temp) {
            Ok(f) => {
                cmd.stdout(f);
            }
            Err(e) => return Outcome::Failed(format!("creating {}: {e}", out.temp.display())),
        },
        _ => {
            cmd.stdout(Stdio::piped());
        }
    }
    cmd.stderr(Stdio::piped()).kill_on_drop(true);
    // SAFETY: setpgid and prctl are async-signal-safe and allocate nothing.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            cleanup(plan);
            return Outcome::Failed(spawn_error(&program, &e));
        }
    };
    let pgid = child.id().map(|p| p as i32);
    let tracker = Arc::new(Mutex::new(Tracker::default()));
    let capture = matches!(plan.stdout, Stdout::Clipboard | Stdout::Shelf);
    let stdout_task = child.stdout.take().map(|out| {
        let (tracker, events, mode) = (tracker.clone(), events.clone(), plan.progress);
        tokio::spawn(async move {
            if capture {
                read_capped(out).await
            } else {
                follow(out, job, mode, &tracker, &events).await.map(|()| Vec::new())
            }
        })
    });
    let stderr_task = child.stderr.take().map(|err| {
        let (tracker, events, mode) = (tracker.clone(), events.clone(), plan.progress);
        tokio::spawn(async move { tail(err, job, mode, &tracker, &events).await })
    });

    let status = tokio::select! {
        s = child.wait() => s,
        _ = cancel => {
            terminate(&mut child, pgid).await;
            cleanup(plan);
            return Outcome::Cancelled;
        }
    };
    let stdout = match stdout_task {
        Some(t) => t.await.unwrap_or_else(|e| Err(e.to_string())),
        None => Ok(Vec::new()),
    };
    let stderr = match stderr_task {
        Some(t) => t.await.unwrap_or_default(),
        None => String::new(),
    };
    let status = match status {
        Ok(s) => s,
        Err(e) => {
            cleanup(plan);
            return Outcome::Failed(format!("waiting for {program}: {e}"));
        }
    };
    let code = status.code();
    if !code.is_some_and(|c| plan.success_codes.contains(&c)) {
        cleanup(plan);
        let how = code.map_or_else(|| "was killed by a signal".to_owned(), |c| format!("exited with status {c}"));
        let detail = last_lines(&stderr, 3);
        return Outcome::Failed(if detail.is_empty() {
            format!("{program} {how}")
        } else {
            format!("{program} {how}: {detail}")
        });
    }
    let stdout = match stdout {
        Ok(bytes) if capture => Some(String::from_utf8_lossy(&bytes).trim_end().to_owned()),
        Ok(_) => None,
        Err(e) => {
            cleanup(plan);
            return Outcome::Failed(e);
        }
    };
    match finish_output(plan) {
        Ok(outputs) => Outcome::Done { outputs, stdout },
        Err(e) => {
            cleanup(plan);
            Outcome::Failed(e)
        }
    }
}

fn spawn_error(program: &str, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        format!("{program} isn't installed (not found on PATH)")
    } else {
        format!("starting {program}: {e}")
    }
}

/// SIGTERM the group, then SIGKILL it after [`KILL_AFTER`].
async fn terminate(child: &mut Child, pgid: Option<i32>) {
    if let Some(g) = pgid {
        // SAFETY: plain syscall; a negative pid addresses the process group.
        unsafe { libc::kill(-g, libc::SIGTERM) };
    }
    if tokio::time::timeout(KILL_AFTER, child.wait()).await.is_err() {
        if let Some(g) = pgid {
            // SAFETY: as above.
            unsafe { libc::kill(-g, libc::SIGKILL) };
        }
        let _ = child.wait().await;
    }
    // Children that ignored SIGTERM may outlive the leader; the group is still ours.
    if let Some(g) = pgid {
        // SAFETY: as above.
        unsafe { libc::kill(-g, libc::SIGKILL) };
    }
}

/// Validate the temp output and rename it into place.
fn finish_output(plan: &Plan) -> Result<Vec<PathBuf>, String> {
    let Some(out) = &plan.output else { return Ok(Vec::new()) };
    let meta = std::fs::symlink_metadata(&out.temp);
    let present = match &meta {
        Ok(m) if out.is_dir => m.is_dir() && std::fs::read_dir(&out.temp).is_ok_and(|mut d| d.next().is_some()),
        Ok(m) => m.is_file() && m.len() > 0,
        Err(_) => false,
    };
    if !present {
        if plan.interactive {
            cleanup(plan);
            return Ok(Vec::new());
        }
        return Err(if meta.is_ok() {
            "the tool produced an empty result".into()
        } else {
            "the tool produced no output".into()
        });
    }
    // Start from the planned name; if something took it meanwhile, count up from the wanted one.
    let mut n = 0;
    loop {
        let target = if n == 0 { out.final_path.clone() } else { naming::numbered(&out.wanted, n) };
        match rename_noreplace(&out.temp, &target) {
            Ok(()) => return Ok(vec![target]),
            Err(e) if e.raw_os_error() == Some(libc::EEXIST) && n < 1000 => n += 1,
            Err(e) => return Err(format!("moving the result to {}: {e}", target.display())),
        }
    }
}

/// `renameat2(RENAME_NOREPLACE)`: fails with `EEXIST` instead of replacing.
///
/// # Errors
///
/// Any rename error; `EEXIST` when `to` exists.
pub fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(std::io::Error::other);
    let (f, t) = (c(from)?, c(to)?);
    // SAFETY: both are valid NUL-terminated paths, resolved relative to the cwd.
    let r = unsafe { libc::renameat2(libc::AT_FDCWD, f.as_ptr(), libc::AT_FDCWD, t.as_ptr(), libc::RENAME_NOREPLACE) };
    if r == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// Remove a job's temp output (only ever a path carrying our temp marker).
fn cleanup(plan: &Plan) {
    let Some(out) = &plan.output else { return };
    let is_ours = out.temp.file_name().is_some_and(|n| naming::is_temp(&n.to_string_lossy()));
    if !is_ours {
        return;
    }
    let _ = if out.is_dir { std::fs::remove_dir_all(&out.temp) } else { std::fs::remove_file(&out.temp) };
}

async fn read_capped(out: impl AsyncRead + Unpin) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    out.take(MAX_CAPTURE as u64 + 1).read_to_end(&mut buf).await.map_err(|e| e.to_string())?;
    if buf.len() > MAX_CAPTURE {
        return Err(format!("the output is larger than the {} MiB limit", MAX_CAPTURE >> 20));
    }
    Ok(buf)
}

async fn follow(
    out: impl AsyncRead + Unpin,
    job: i64,
    mode: Progress,
    tracker: &Mutex<Tracker>,
    events: &async_channel::Sender<Event>,
) -> Result<(), String> {
    let mut lines = BufReader::new(out).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        feed(&line, job, mode, tracker, events).await;
    }
    Ok(())
}

async fn tail(
    err: impl AsyncRead + Unpin,
    job: i64,
    mode: Progress,
    tracker: &Mutex<Tracker>,
    events: &async_channel::Sender<Event>,
) -> String {
    let mut tail = String::new();
    let mut reader = BufReader::new(err);
    let mut buf = Vec::new();
    // Split on \r too: progress bars redraw with carriage returns.
    loop {
        buf.clear();
        let mut byte = [0u8; 1];
        let mut got_any = false;
        while reader.read_exact(&mut byte).await.is_ok() {
            got_any = true;
            if byte[0] == b'\n' || byte[0] == b'\r' {
                break;
            }
            buf.push(byte[0]);
        }
        if !got_any {
            break;
        }
        let line = String::from_utf8_lossy(&buf).into_owned();
        feed(&line, job, mode, tracker, events).await;
        if !line.trim().is_empty() {
            tail.push_str(&line);
            tail.push('\n');
            if tail.len() > STDERR_TAIL {
                let cut = tail.len() - STDERR_TAIL;
                let cut = (cut..tail.len()).find(|&i| tail.is_char_boundary(i)).unwrap_or(tail.len());
                tail.drain(..cut);
            }
        }
    }
    tail
}

async fn feed(line: &str, job: i64, mode: Progress, tracker: &Mutex<Tracker>, events: &async_channel::Sender<Event>) {
    let update = {
        let mut t = tracker.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = match mode {
            Progress::None => false,
            Progress::Ffmpeg => t.ffmpeg_line(line),
            Progress::Percent => t.percent_line(line),
        };
        let fraction = t.fraction();
        drop(t);
        changed.then_some(fraction).flatten()
    };
    if let Some(f) = update {
        let _ = events.send(Event::Progress(job, f)).await;
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join(" · ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use waystow_core::action;

    use super::*;

    fn spec(toml: &str) -> action::Action {
        action::parse(&format!("[[action]]\nid='t'\ntitle='T'\n{toml}")).unwrap().remove(0)
    }

    fn plan(a: &action::Action, job: i64, input: &Path) -> Plan {
        a.plan(job, &[input.to_path_buf()], &BTreeMap::new(), |_| true, Path::exists, Path::new("/tmp")).unwrap()
    }

    struct Env {
        rt: tokio::runtime::Runtime,
        runner: Runner,
        rx: async_channel::Receiver<Event>,
        dir: tempfile::TempDir,
    }

    impl Env {
        fn new(limit: usize) -> Self {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            let (tx, rx) = async_channel::unbounded();
            let runner = Runner::new(limit, tx, rt.handle().clone());
            let dir = tempfile::tempdir().unwrap();
            Self { rt, runner, rx, dir }
        }

        fn input(&self, name: &str) -> PathBuf {
            let p = self.dir.path().join(name);
            std::fs::write(&p, "data").unwrap();
            p
        }

        fn finish(&self, job: i64) -> (Outcome, Vec<Event>) {
            let mut seen = Vec::new();
            self.rt.block_on(async {
                loop {
                    let e = tokio::time::timeout(Duration::from_secs(20), self.rx.recv()).await.unwrap().unwrap();
                    if let Event::Finished(j, o) = &e
                        && *j == job
                    {
                        return (o.clone(), seen);
                    }
                    seen.push(e);
                }
            })
        }

        fn leftovers(&self) -> Vec<String> {
            let mut v: Vec<_> = std::fs::read_dir(self.dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        }
    }

    #[test]
    fn success_renames_into_place_without_overwriting() {
        let env = Env::new(2);
        let input = env.input("a.txt");
        std::fs::write(env.dir.path().join("a.out"), "existing").unwrap();
        let a = spec("command=['sh','-c','cp \"$1\" \"$2\"','sh','{in}','{out}']\noutput='{dir}/{stem}.out'");
        let p = plan(&a, 1, &input);
        // Someone takes the planned name while the job runs: we still never replace it.
        std::fs::write(&p.output.as_ref().unwrap().final_path, "raced").unwrap();
        env.runner.spawn(1, p);
        let (o, _) = env.finish(1);
        let Outcome::Done { outputs, .. } = o else { panic!("{o:?}") };
        assert_eq!(outputs, [env.dir.path().join("a (2).out")]);
        assert_eq!(std::fs::read_to_string(env.dir.path().join("a.out")).unwrap(), "existing");
        assert_eq!(std::fs::read_to_string(env.dir.path().join("a (1).out")).unwrap(), "raced");
        assert_eq!(env.leftovers(), ["a (1).out", "a (2).out", "a.out", "a.txt"]);
    }

    #[test]
    fn failure_and_empty_output_leave_nothing_behind() {
        let env = Env::new(2);
        let input = env.input("a.txt");
        let fail = spec(
            "command=['sh','-c','echo partial > \"$1\"; echo boom >&2; exit 3','sh','{out}']\noutput='{dir}/x.out'",
        );
        env.runner.spawn(1, plan(&fail, 1, &input));
        let (o, _) = env.finish(1);
        assert_eq!(o, Outcome::Failed("sh exited with status 3: boom".into()));
        let empty = spec("command=['touch','{out}']\noutput='{dir}/x.out'");
        env.runner.spawn(2, plan(&empty, 2, &input));
        assert_eq!(env.finish(2).0, Outcome::Failed("the tool produced an empty result".into()));
        let none = spec("command=['true']\noutput='{dir}/x.out'");
        env.runner.spawn(3, plan(&none, 3, &input));
        assert_eq!(env.finish(3).0, Outcome::Failed("the tool produced no output".into()));
        assert_eq!(env.leftovers(), ["a.txt"]);
    }

    #[test]
    fn interactive_tools_may_produce_nothing() {
        let env = Env::new(1);
        let input = env.input("a.png");
        let a = spec("command=['true']\noutput='{dir}/x.png'\ninteractive=true");
        env.runner.spawn(1, plan(&a, 1, &input));
        assert_eq!(env.finish(1).0, Outcome::Done { outputs: vec![], stdout: None });
    }

    #[test]
    fn missing_program_is_named() {
        let env = Env::new(1);
        let input = env.input("a");
        env.runner.spawn(1, plan(&spec("command=['waystow-no-such-tool']"), 1, &input));
        assert_eq!(env.finish(1).0, Outcome::Failed("waystow-no-such-tool isn't installed (not found on PATH)".into()));
    }

    #[test]
    fn cancel_kills_the_whole_group_even_if_it_ignores_sigterm() {
        let env = Env::new(1);
        let input = env.input("a");
        let pidfile = env.dir.path().join("pids");
        let script = format!("trap '' TERM; sleep 300 & echo $! > {p}; echo $$ >> {p}; wait", p = pidfile.display());
        let a = spec(&format!("command=['sh','-c',{script:?}]\noutput='{{dir}}/x.out'"));
        env.runner.spawn(1, plan(&a, 1, &input));
        let started = Instant::now();
        while std::fs::read_to_string(&pidfile).map_or(0, |s| s.lines().count()) < 2 {
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(env.runner.cancel(1));
        assert_eq!(env.finish(1).0, Outcome::Cancelled);
        let pids: Vec<i32> = std::fs::read_to_string(&pidfile).unwrap().lines().map(|l| l.parse().unwrap()).collect();
        std::thread::sleep(Duration::from_millis(100));
        for pid in pids {
            // SAFETY: signal 0 only checks for existence.
            let alive = unsafe { libc::kill(pid, 0) } == 0
                && !std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.contains(") Z "));
            assert!(!alive, "process {pid} survived the cancel");
        }
        assert!(!env.runner.cancel(1), "already gone");
    }

    #[test]
    fn queued_jobs_wait_for_a_slot_and_can_be_cancelled() {
        let env = Env::new(1);
        let input = env.input("a");
        let slow = spec("command=['sleep','0.5']");
        env.runner.spawn(1, plan(&slow, 1, &input));
        env.runner.spawn(2, plan(&slow, 2, &input));
        std::thread::sleep(Duration::from_millis(100));
        assert!(env.runner.cancel(2));
        let (o2, seen) = env.finish(2);
        assert_eq!(o2, Outcome::Cancelled);
        assert!(!seen.contains(&Event::Started(2)), "never started");
        assert_eq!(env.finish(1).0, Outcome::Done { outputs: vec![], stdout: None });
    }

    #[test]
    fn stdout_capture_and_file_modes() {
        let env = Env::new(2);
        let input = env.input("a.json");
        let cap = spec("command=['printf','hello\\n\\n']\nstdout='clipboard'");
        env.runner.spawn(1, plan(&cap, 1, &input));
        assert_eq!(env.finish(1).0, Outcome::Done { outputs: vec![], stdout: Some("hello".into()) });
        let file = spec("command=['cat','{in}']\nstdout='file'\noutput='{dir}/{stem}-copy.json'");
        env.runner.spawn(2, plan(&file, 2, &input));
        let Outcome::Done { outputs, .. } = env.finish(2).0 else { panic!() };
        assert_eq!(std::fs::read_to_string(&outputs[0]).unwrap(), "data");
    }

    #[test]
    fn success_codes_and_progress() {
        let env = Env::new(1);
        let input = env.input("a");
        let a = spec("command=['sh','-c','echo 10%; echo 60% >&2; exit 1']\nprogress='percent'\nsuccess_codes=[0,1]");
        env.runner.spawn(1, plan(&a, 1, &input));
        let (o, seen) = env.finish(1);
        assert!(matches!(o, Outcome::Done { .. }), "{o:?}");
        assert!(seen.iter().any(|e| matches!(e, Event::Progress(1, f) if (*f - 0.6).abs() < 1e-6)), "{seen:?}");
    }

    #[test]
    fn directory_outputs() {
        let env = Env::new(1);
        let input = env.input("a.zip");
        let ok = spec("command=['touch','{out}/inside']\noutput='{dir}/{stem}'\noutput_dir=true");
        env.runner.spawn(1, plan(&ok, 1, &input));
        let Outcome::Done { outputs, .. } = env.finish(1).0 else { panic!() };
        assert!(outputs[0].join("inside").exists());
        let empty = spec("command=['true']\noutput='{dir}/{stem}-x'\noutput_dir=true");
        env.runner.spawn(2, plan(&empty, 2, &input));
        assert!(matches!(env.finish(2).0, Outcome::Failed(_)));
        assert_eq!(env.leftovers(), ["a", "a.zip"]);
    }

    #[test]
    fn oversized_capture_is_refused_not_truncated() {
        let env = Env::new(1);
        let input = env.input("a");
        let a = spec(&format!("command=['head','-c','{}','/dev/zero']\nstdout='shelf'", MAX_CAPTURE + 1));
        env.runner.spawn(1, plan(&a, 1, &input));
        assert_eq!(env.finish(1).0, Outcome::Failed("the output is larger than the 16 MiB limit".into()));
    }
}
