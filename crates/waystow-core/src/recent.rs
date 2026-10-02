//! Rules for the Recent feed: which file events count, partial downloads,
//! ignore patterns, and `recently-used.xbel`.

use std::path::{Path, PathBuf};

use quick_xml::events::Event;

use crate::model::Timestamp;
use crate::naming;

/// Suffixes browsers and download tools use while a file is still arriving.
pub const PARTIAL_SUFFIXES: [&str; 5] = [".part", ".crdownload", ".download", ".partial", ".opdownload"];

/// The final name of a partial download (`foo.iso.part` → `foo.iso`).
#[must_use]
pub fn partial_target(name: &str) -> Option<&str> {
    PARTIAL_SUFFIXES.iter().find_map(|s| name.strip_suffix(s)).filter(|n| !n.is_empty())
}

/// Should a file with this name be left out of Recent?
#[must_use]
pub fn ignored(name: &str, patterns: &[String]) -> bool {
    if name.starts_with('.') || naming::is_temp(name) || partial_target(name).is_some() {
        return true;
    }
    patterns.iter().any(|p| {
        p.strip_prefix('*').map_or_else(
            || p.strip_suffix('*').map_or_else(|| p == name, |prefix| name.starts_with(prefix)),
            |suffix| name.ends_with(suffix),
        )
    })
}

/// Bookmarks in `recently-used.xbel`: `(path, last modified or visited)`,
/// local files only.
#[must_use]
pub fn parse_xbel(xml: &str) -> Vec<(PathBuf, Timestamp)> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if e.name().as_ref() == b"bookmark" => {
                let mut href = None;
                let mut when: Timestamp = 0;
                for attr in e.attributes().flatten() {
                    let value = attr.unescape_value().map(std::borrow::Cow::into_owned).unwrap_or_default();
                    match attr.key.as_ref() {
                        b"href" => href = crate::uri::to_path(&value),
                        b"added" | b"modified" | b"visited" => {
                            if let Some(t) = parse_iso8601(&value) {
                                when = when.max(t);
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(p) = href.filter(|_| when > 0) {
                    out.push((p, when));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// `2026-10-01T12:34:56.123Z` (UTC) → seconds since the epoch.
#[must_use]
pub fn parse_iso8601(s: &str) -> Option<Timestamp> {
    let s = s.trim().strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let time = time.split('.').next()?;
    let mut t = time.split(':').map(|x| x.parse::<i64>().ok());
    let (h, mi, se) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + se)
}

/// Howard Hinnant's days-from-civil.
const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Is `path` directly inside one of `dirs`?
#[must_use]
pub fn in_any(path: &Path, dirs: &[PathBuf]) -> bool {
    path.parent().is_some_and(|p| dirs.iter().any(|d| d == p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_downloads() {
        assert_eq!(partial_target("fedora.iso.part"), Some("fedora.iso"));
        assert_eq!(partial_target("Unconfirmed 1.crdownload"), Some("Unconfirmed 1"));
        assert_eq!(partial_target(".part"), None);
        assert_eq!(partial_target("report.pdf"), None);
    }

    #[test]
    fn ignore_rules() {
        let pats = vec!["*.swp".to_string(), "node_modules".to_string(), "~lock*".to_string()];
        assert!(ignored(".hidden", &pats));
        assert!(ignored("a.swp", &pats));
        assert!(ignored("node_modules", &pats));
        assert!(ignored("~lock.doc#", &pats));
        assert!(ignored(".x.waystow-3.png", &pats));
        assert!(ignored("x.iso.part", &pats));
        assert!(!ignored("report.pdf", &pats));
    }

    #[test]
    fn iso_dates() {
        assert_eq!(parse_iso8601("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso8601("2026-10-01T12:00:00.123456Z"), Some(1_790_856_000));
        assert_eq!(parse_iso8601("2000-02-29T00:00:00Z"), Some(951_782_400));
        assert_eq!(parse_iso8601("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_iso8601("2026-10-01T00:00:00"), None);
    }

    #[test]
    fn xbel() {
        let xml = r#"<?xml version="1.0"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
  <bookmark href="file:///home/u/a%20b.pdf" added="2026-10-01T10:00:00Z" modified="2026-10-01T11:00:00Z" visited="2026-10-01T12:00:00Z">
    <info/>
  </bookmark>
  <bookmark href="https://example.com" added="2026-10-01T10:00:00Z"/>
  <bookmark href="file:///x" added="garbage"/>
</xbel>"#;
        let v = parse_xbel(xml);
        assert_eq!(v, vec![(PathBuf::from("/home/u/a b.pdf"), 1_790_856_000)]);
    }

    #[test]
    fn directly_inside() {
        let dirs = [PathBuf::from("/d")];
        assert!(in_any(Path::new("/d/x"), &dirs));
        assert!(!in_any(Path::new("/d/sub/x"), &dirs));
    }
}
