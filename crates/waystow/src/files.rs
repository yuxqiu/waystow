//! Files: MIME detection, the stash (files WayStow owns), and turning
//! snippets into files when a command needs one.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use gtk::gio;
use gtk::prelude::*;
use waystow_core::model::{Content, Item};
use waystow_core::{mime, naming, paths};

/// MIME type of shelf snippets.
pub const SNIPPET: &str = "text/x-snippet";
/// MIME type of shelf web addresses.
pub const URL: &str = "text/x-uri";

/// MIME type and size of a file or folder (sniffed, as file managers do).
pub fn probe(path: &Path) -> anyhow::Result<(String, Option<u64>)> {
    let file = gio::File::for_path(path);
    let info = file
        .query_info(
            "standard::content-type,standard::size,standard::type",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .with_context(|| format!("{} isn't readable", path.display()))?;
    if info.file_type() == gio::FileType::Directory {
        return Ok((mime::DIRECTORY.into(), None));
    }
    let mime = info
        .content_type()
        .and_then(|ct| gio::content_type_get_mime_type(&ct))
        .map_or_else(|| "application/octet-stream".to_owned(), |m| m.to_string());
    Ok((mime, u64::try_from(info.size()).ok()))
}

/// Human-readable type name ("PNG image").
pub fn describe(mime_type: &str) -> String {
    match mime_type {
        SNIPPET => "Text snippet".into(),
        URL => "Link".into(),
        m => gio::content_type_from_mime_type(m)
            .map_or_else(|| m.to_owned(), |ct| gio::content_type_get_description(&ct).to_string()),
    }
}

/// Icon for a MIME type.
pub fn icon(mime_type: &str) -> gio::Icon {
    match mime_type {
        SNIPPET => gio::ThemedIcon::new("text-x-generic").upcast(),
        URL => gio::ThemedIcon::new("text-html").upcast(),
        m => gio::content_type_from_mime_type(m)
            .map_or_else(|| gio::ThemedIcon::new("text-x-generic").upcast(), |ct| gio::content_type_get_icon(&ct)),
    }
}

/// The stash directory for today (created).
fn stash_today() -> anyhow::Result<PathBuf> {
    let dir = paths::stash_dir().join(glib_date());
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

fn glib_date() -> String {
    gtk::glib::DateTime::now_local()
        .and_then(|d| d.format("%Y-%m-%d"))
        .map_or_else(|_| "stash".into(), |s| s.to_string())
}

/// `14-02-33`.
pub fn clock_stamp() -> String {
    gtk::glib::DateTime::now_local().and_then(|d| d.format("%H-%M-%S")).map_or_else(|_| "now".into(), |s| s.to_string())
}

/// File extension for an image MIME type.
pub fn image_ext(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        _ => "png",
    }
}

/// Write bytes into the stash as `name` (made unique); returns the path.
pub fn stash_bytes(name: &str, bytes: &[u8]) -> anyhow::Result<PathBuf> {
    if !naming::is_valid_name(name) {
        bail!("bad file name {name:?}");
    }
    let path = naming::unique(&stash_today()?.join(name), Path::exists);
    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Is this path inside the stash?
pub fn in_stash(path: &Path) -> bool {
    path.starts_with(paths::stash_dir())
}

/// Delete a stash file once nothing references it (never touches anything else).
pub fn delete_stash_file(path: &Path) {
    if in_stash(path) && !path.components().any(|c| c == std::path::Component::ParentDir) {
        let _ = std::fs::remove_file(path);
        // Tidy the day directory when it empties.
        if let Some(dir) = path.parent()
            && dir != paths::stash_dir()
        {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Directory for snippets materialized as files for command actions.
fn materialized_dir() -> PathBuf {
    paths::stash_dir().join("materialized")
}

/// Remove all materialized snippet files (at daemon start).
pub fn clear_materialized() {
    let _ = std::fs::remove_dir_all(materialized_dir());
}

/// The file a command should read for this item: the file itself, or the
/// snippet/link text written to a stash file.
pub fn input_path(item: &Item) -> anyhow::Result<PathBuf> {
    match &item.content {
        Content::File(p) | Content::Stash(p) => {
            if !p.exists() {
                bail!("{} no longer exists", p.display());
            }
            Ok(p.clone())
        }
        Content::Text(t) | Content::Url(t) => {
            let dir = materialized_dir();
            std::fs::create_dir_all(&dir)?;
            let stem = if item.mime == URL { "link" } else { "snippet" };
            let path = dir.join(format!("{stem}-{}.txt", item.id));
            std::fs::write(&path, t)?;
            Ok(path)
        }
    }
}

/// A file name for a snippet ("First words.txt").
pub fn snippet_file_name(text: &str) -> String {
    let title: String = waystow_core::model::snippet_title(text)
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '…'))
        .take(40)
        .collect();
    let title = title.trim();
    if title.is_empty() { format!("Snippet {}.txt", clock_stamp()) } else { format!("{title}.txt") }
}

/// Human-readable size.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else if v < 10.0 {
        format!("{v:.1} {}", UNITS[u])
    } else {
        format!("{v:.0} {}", UNITS[u])
    }
}

/// "just now", "4 min ago", "2 h ago", "3 d ago".
pub fn ago(secs: i64) -> String {
    match secs {
        s if s < 45 => "just now".into(),
        s if s < 3600 => format!("{} min ago", (s + 30) / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

/// "52 min left", "3 h left".
pub fn left(secs: i64) -> String {
    match secs {
        s if s < 60 => "less than a minute left".into(),
        s if s <= 3540 => format!("{} min left", (s + 59) / 60),
        s if s < 86_400 => format!("{} h left", (s + 1800) / 3600),
        s => format!("{} d left", (s + 43_200) / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_times() {
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1_500), "1.5 KB");
        assert_eq!(human_size(12_000_000), "12 MB");
        assert_eq!(ago(10), "just now");
        assert_eq!(ago(240), "4 min ago");
        assert_eq!(ago(7300), "2 h ago");
        assert_eq!(left(3000), "50 min left");
        assert_eq!(left(30), "less than a minute left");
        assert_eq!(left(3599), "1 h left", "never \"60 min\" next to \"1 h\"");
    }

    #[test]
    fn snippet_names() {
        assert_eq!(snippet_file_name("Hello: world/again\nmore"), "Hello worldagain.txt");
        assert!(snippet_file_name("   ").starts_with("Snippet "));
    }
}
