//! `file://` URIs and `text/uri-list`.

use std::fmt::Write as _;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

/// `/a b` → `file:///a%20b`.
#[must_use]
pub fn from_path(path: &Path) -> String {
    let mut out = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// `file:///a%20b` (or `file://localhost/…`) → `/a b`; `None` for other schemes
/// and remote hosts.
#[must_use]
pub fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

/// Lines of a `text/uri-list` body, without comments and blank lines.
#[must_use]
pub fn parse_list(body: &str) -> Vec<String> {
    body.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned).collect()
}

/// A `text/uri-list` body for these paths (CRLF-terminated, as the spec says).
#[must_use]
pub fn list_of(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| from_path(p) + "\r\n").collect()
}

/// Looks like a web address.
#[must_use]
pub fn is_web(s: &str) -> bool {
    let s = s.trim();
    (s.starts_with("https://") || s.starts_with("http://")) && !s.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for p in ["/a b/c.png", "/ünï/%x", "/q?#[]"] {
            let u = from_path(Path::new(p));
            assert!(!u[7..].contains(' ') && !u[7..].contains('#'), "{u}");
            assert_eq!(to_path(&u).unwrap(), PathBuf::from(p));
        }
    }

    #[test]
    fn parsing() {
        assert_eq!(to_path("file://localhost/x%20y"), Some(PathBuf::from("/x y")));
        assert_eq!(to_path("file://host/x"), None);
        assert_eq!(to_path("https://x/y"), None);
        assert_eq!(to_path("file:///bad%zz"), None);
        assert_eq!(to_path("file:///trunc%2"), None);
        assert_eq!(parse_list("# c\r\nfile:///a\r\n\r\nfile:///b\n"), ["file:///a", "file:///b"]);
        assert!(is_web(" https://example.com/x "));
        assert!(!is_web("https://a b"));
    }
}
