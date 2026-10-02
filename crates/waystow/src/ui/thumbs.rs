//! Thumbnails: generated off the main thread (images via gdk-pixbuf, videos
//! via ffmpeg, PDFs via pdftoppm), cached on disk and in memory. Results are
//! keyed by path and mtime, so a stale result can't land on a changed file.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use gtk::{gdk, gdk_pixbuf, gio, glib};
use waystow_core::{paths, uri};

type Key = (PathBuf, i64, i32);
/// Someone waiting for a thumbnail.
type Waiter = Box<dyn FnOnce(Option<gdk::Texture>)>;

thread_local! {
    static CACHE: RefCell<HashMap<Key, Option<gdk::Texture>>> = RefCell::new(HashMap::new());
    static PENDING: RefCell<HashMap<Key, Vec<Waiter>>> = RefCell::new(HashMap::new());
}

fn mtime_of(p: &Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// Can we make a thumbnail for this MIME type?
pub fn supported(mime: &str) -> bool {
    mime.starts_with("image/") || mime.starts_with("video/") || mime == "application/pdf"
}

/// Load a thumbnail at most `size` px on its longer side. `done` runs on the
/// main thread, immediately when cached.
pub fn load(path: &Path, mime: &str, size: i32, done: impl FnOnce(Option<gdk::Texture>) + 'static) {
    if !supported(mime) {
        done(None);
        return;
    }
    let key: Key = (path.to_path_buf(), mtime_of(path), size);
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        done(hit);
        return;
    }
    let first = PENDING.with(|p| {
        let mut p = p.borrow_mut();
        let waiters = p.entry(key.clone()).or_default();
        waiters.push(Box::new(done));
        waiters.len() == 1
    });
    if !first {
        return;
    }
    let (path, mime) = (path.to_path_buf(), mime.to_owned());
    glib::spawn_future_local(async move {
        let k = key.clone();
        let bytes = gio::spawn_blocking(move || generate(&k.0, &mime, k.1, k.2)).await.ok().flatten();
        let texture = bytes.and_then(|b| gdk::Texture::from_bytes(&glib::Bytes::from_owned(b)).ok());
        CACHE.with(|c| {
            let mut c = c.borrow_mut();
            if c.len() > 600 {
                c.clear();
            }
            c.insert(key.clone(), texture.clone());
        });
        let waiters = PENDING.with(|p| p.borrow_mut().remove(&key)).unwrap_or_default();
        for w in waiters {
            w(texture.clone());
        }
        let _ = path;
    });
}

/// PNG bytes of a thumbnail (from a cache or freshly made).
fn generate(path: &Path, mime: &str, mtime: i64, size: i32) -> Option<Vec<u8>> {
    let uri = uri::from_path(path);
    let md5 = glib::compute_checksum_for_string(glib::ChecksumType::Md5, &uri)?.to_string();
    // The shared freedesktop cache (made by file managers), if it's fresh.
    let shared = xdg_cache().join(if size > 128 { "large" } else { "normal" }).join(format!("{md5}.png"));
    if mtime_of(&shared) >= mtime
        && let Ok(b) = std::fs::read(&shared)
    {
        return Some(b);
    }
    let ours = paths::cache_dir().join("thumbs").join(format!("{md5}-{mtime}-{size}.png"));
    if let Ok(b) = std::fs::read(&ours) {
        return Some(b);
    }
    let bytes = if mime.starts_with("image/") {
        image(path, size)
    } else if mime.starts_with("video/") {
        video(path, size)
    } else {
        pdf(path, size)
    }?;
    if let Some(dir) = ours.parent() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&ours, &bytes);
    }
    Some(bytes)
}

fn xdg_cache() -> PathBuf {
    paths::cache_dir().parent().map_or_else(|| PathBuf::from("/tmp"), |p| p.join("thumbnails"))
}

fn image(path: &Path, size: i32) -> Option<Vec<u8>> {
    // gdk-pixbuf first; formats without a loader (WebP, AVIF, HEIC…) via ImageMagick, then ffmpeg.
    let pixbuf = || {
        let pb = gdk_pixbuf::Pixbuf::from_file_at_scale(path, size, size, true).ok()?;
        let pb = pb.apply_embedded_orientation().unwrap_or(pb);
        pb.save_to_bufferv("png", &[]).ok()
    };
    let geometry = format!("{size}x{size}>");
    pixbuf()
        .or_else(|| {
            let mut arg = std::ffi::OsString::from(path.as_os_str());
            arg.push("[0]");
            run_png(Command::new("magick").arg(arg).args(["-auto-orient", "-thumbnail", &geometry, "png:-"]))
        })
        .or_else(|| video(path, size))
}

fn run_png(cmd: &mut Command) -> Option<Vec<u8>> {
    let out = cmd.stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    (out.status.success() && out.stdout.starts_with(b"\x89PNG")).then_some(out.stdout)
}

fn video(path: &Path, size: i32) -> Option<Vec<u8>> {
    let scale = format!("scale='min({size},iw)':-2");
    let at = |seek: &str| {
        run_png(Command::new("ffmpeg").args(["-nostdin", "-v", "quiet", "-ss", seek, "-i"]).arg(path).args([
            "-frames:v",
            "1",
            "-vf",
            &scale,
            "-f",
            "image2",
            "-c:v",
            "png",
            "pipe:1",
        ]))
    };
    at("1").or_else(|| at("0"))
}

fn pdf(path: &Path, size: i32) -> Option<Vec<u8>> {
    run_png(
        Command::new("pdftoppm")
            .args(["-png", "-singlefile", "-f", "1", "-l", "1", "-scale-to", &size.to_string()])
            .arg(path),
    )
}
