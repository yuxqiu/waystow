//! Runs every built-in command action on generated fixtures, with the real
//! tools. Actions whose tools are missing (e.g. in the Nix build sandbox) are
//! skipped and listed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use waystow_core::action::{self, Action, Stdout};
use waystow_jobs::{Event, Outcome, Runner};

fn run(cmd: &mut Command) -> bool {
    cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Fixture files by MIME type, made with the same tools the actions use.
/// In preference order: the first matching MIME type feeds an action.
type Fixtures = Vec<(&'static str, Vec<PathBuf>)>;

fn fixtures(dir: &Path) -> Fixtures {
    let mut f = Fixtures::new();
    let mut add = |mime: &'static str, path: PathBuf| match f.iter_mut().find(|(m, _)| *m == mime) {
        Some((_, v)) => v.push(path),
        None => f.push((mime, vec![path])),
    };
    let p = |n: &str| dir.join(n);
    if action::on_path("magick") {
        let png = p("text.png");
        if run(Command::new("magick")
            .args([
                "-size",
                "640x160",
                "xc:white",
                "-fill",
                "black",
                "-pointsize",
                "64",
                "-annotate",
                "+20+100",
                "Hello WayStow",
            ])
            .arg(&png))
        {
            add("image/png", png);
        }
        let jpg = p("photo.jpg");
        if run(Command::new("magick").args(["-size", "320x200", "plasma:fractal"]).arg(&jpg)) {
            add("image/jpeg", jpg);
        }
        for (n, page) in [("a.pdf", "One"), ("b.pdf", "Two")] {
            let pdf = p(n);
            if run(Command::new("magick")
                .args(["-size", "595x842", "xc:white", "-pointsize", "72", "-annotate", "+60+200", page])
                .arg(&pdf))
            {
                add("application/pdf", pdf);
            }
        }
    }
    if action::on_path("ffmpeg") {
        let mp4 = p("clip.mp4");
        if run(Command::new("ffmpeg")
            .args([
                "-v",
                "quiet",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=2:size=320x180:rate=15",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=2",
                "-pix_fmt",
                "yuv420p",
                "-shortest",
            ])
            .arg(&mp4))
        {
            add("video/mp4", mp4);
        }
        let webm = p("clip.webm");
        if run(Command::new("ffmpeg")
            .args([
                "-v",
                "quiet",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=160x90:rate=10",
                "-c:v",
                "libvpx",
                "-b:v",
                "200k",
            ])
            .arg(&webm))
        {
            add("video/webm", webm);
        }
        let mp3 = p("tone.mp3");
        if run(Command::new("ffmpeg").args(["-v", "quiet", "-f", "lavfi", "-i", "sine=duration=2"]).arg(&mp3)) {
            add("audio/mpeg", mp3);
        }
    }
    std::fs::write(p("data.json"), r#"{"b":[1,2],"a":{"c":true}}"#).unwrap();
    add("application/json", p("data.json"));
    std::fs::write(p("one.txt"), "alpha\nbeta\n").unwrap();
    std::fs::write(p("two.txt"), "alpha\ngamma\n").unwrap();
    add("text/plain", p("one.txt"));
    add("text/plain", p("two.txt"));
    std::fs::write(p("snippet.txt"), "https://example.com/waystow").unwrap();
    add("text/x-uri", p("snippet.txt"));
    if action::on_path("bsdtar") {
        let zip = p("bundle.zip");
        if run(Command::new("bsdtar").args(["-a", "-cf"]).arg(&zip).arg("-C").arg(dir).args(["./one.txt", "./two.txt"]))
        {
            add("application/zip", zip);
        }
    }
    f
}

fn finish(rx: &async_channel::Receiver<Event>, rt: &tokio::runtime::Runtime, job: i64) -> Outcome {
    rt.block_on(async {
        loop {
            let e = tokio::time::timeout(Duration::from_secs(120), rx.recv()).await.expect("job timed out").unwrap();
            if let Event::Finished(j, o) = e
                && j == job
            {
                return o;
            }
        }
    })
}

#[test]
fn every_builtin_command_action_works() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixtures(dir.path());
    let actions = action::parse(action::BUILTIN).unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let (tx, rx) = async_channel::unbounded();
    let runner = Runner::new(4, tx, rt.handle().clone());
    let mut ran = Vec::new();
    let mut skipped = Vec::new();
    let mut failures = Vec::new();
    for (n, a) in actions.iter().enumerate() {
        // Internal actions need the desktop; GUIs and terminals need a human.
        if a.internal.is_some() || a.detach || a.interactive {
            continue;
        }
        if !a.requires.iter().all(|r| action::on_path(r)) {
            skipped.push(a.id.clone());
            continue;
        }
        let Some(inputs) = pick(a, &fx) else {
            skipped.push(format!("{} (no fixture)", a.id));
            continue;
        };
        let job = n as i64 + 1;
        let plan = a
            .plan(job, &inputs, &a.defaults(), |_| true, Path::exists, dir.path())
            .unwrap_or_else(|e| panic!("{}: {e:#}", a.id));
        runner.spawn(job, plan);
        match finish(&rx, &rt, job) {
            Outcome::Done { outputs, stdout } => {
                if a.id == "image.ocr" && !stdout.as_deref().unwrap_or_default().contains("Hello") {
                    failures.push(format!("image.ocr: didn't read \"Hello\" (got {stdout:?})"));
                }
                let ok = match (a.output.is_some(), a.stdout) {
                    (true, _) => outputs.len() == 1 && outputs[0].exists(),
                    (false, Stdout::Clipboard | Stdout::Shelf) => stdout.is_some_and(|s| !s.trim().is_empty()),
                    (false, _) => true,
                };
                if !ok {
                    failures.push(format!("{}: finished without a usable result", a.id));
                }
            }
            other => failures.push(format!("{}: {other:?}", a.id)),
        }
        ran.push(a.id.clone());
    }
    eprintln!("ran {} actions: {}", ran.len(), ran.join(", "));
    eprintln!("skipped: {}", skipped.join(", "));
    assert!(failures.is_empty(), "failed actions:\n{}", failures.join("\n"));
}

/// Inputs from the fixtures that the action applies to.
fn pick(a: &Action, fx: &Fixtures) -> Option<Vec<PathBuf>> {
    for (mime, files) in fx {
        let want = match a.arity {
            action::Arity::Each => 1,
            action::Arity::Pair => 2,
            action::Arity::All => a.min_inputs.max(1),
        };
        if files.len() < want {
            continue;
        }
        let mimes = vec![*mime; want];
        if a.applies_to(&mimes) {
            return Some(files[..want].to_vec());
        }
    }
    None
}
