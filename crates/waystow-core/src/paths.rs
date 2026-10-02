//! XDG locations. Every directory honours its `XDG_*` variable, so tests and
//! headless runs point WayStow somewhere other than the user's home.

use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if Path::new(&v).is_absolute() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

/// `$XDG_CONFIG_HOME/waystow`.
#[must_use]
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("waystow")
}

/// `$XDG_STATE_HOME/waystow`: the database and the stash.
#[must_use]
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("waystow")
}

/// `$XDG_CACHE_HOME/waystow`: thumbnails we generate.
#[must_use]
pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("waystow")
}

/// `$XDG_DATA_HOME`.
#[must_use]
pub fn data_home() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}

/// Files WayStow owns: dropped image bytes, clipboard images, saved snippets.
#[must_use]
pub fn stash_dir() -> PathBuf {
    state_dir().join("stash")
}

/// The daemon socket. Falls back to `/tmp` (made private by the daemon) when
/// `XDG_RUNTIME_DIR` is unset.
#[must_use]
pub fn socket() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").filter(|v| Path::new(v).is_absolute()).map_or_else(
        || {
            // SAFETY: getuid has no preconditions and never fails.
            let uid = unsafe { libc::getuid() };
            PathBuf::from(format!("/tmp/waystow-{uid}.sock"))
        },
        |d| PathBuf::from(d).join("waystow.sock"),
    )
}

/// Expand a leading `~/` to `$HOME`.
#[must_use]
pub fn expand_tilde(p: &str) -> PathBuf {
    p.strip_prefix("~/").map_or_else(|| if p == "~" { home() } else { PathBuf::from(p) }, |rest| home().join(rest))
}

/// `xdg-user-dirs` lookup (`XDG_DOWNLOAD_DIR`, `XDG_PICTURES_DIR`…) from
/// `user-dirs.dirs`, falling back to `~/<fallback>`.
#[must_use]
pub fn user_dir(key: &str, fallback: &str) -> PathBuf {
    let file = xdg("XDG_CONFIG_HOME", ".config").join("user-dirs.dirs");
    std::fs::read_to_string(file).ok().and_then(|s| parse_user_dir(&s, key)).unwrap_or_else(|| home().join(fallback))
}

fn parse_user_dir(contents: &str, key: &str) -> Option<PathBuf> {
    contents.lines().find_map(|line| {
        let value = line.trim().strip_prefix(key)?.trim_start().strip_prefix('=')?;
        let value = value.trim().trim_matches('"');
        let path = value
            .strip_prefix("$HOME")
            .map_or_else(|| PathBuf::from(value), |rest| home().join(rest.trim_start_matches('/')));
        path.is_absolute().then_some(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_dirs_file_is_parsed() {
        let s = "# comment\nXDG_DOWNLOAD_DIR=\"$HOME/Downloads\"\nXDG_PICTURES_DIR=\"/data/pics\"\n";
        assert_eq!(parse_user_dir(s, "XDG_DOWNLOAD_DIR"), Some(home().join("Downloads")));
        assert_eq!(parse_user_dir(s, "XDG_PICTURES_DIR"), Some(PathBuf::from("/data/pics")));
        assert_eq!(parse_user_dir(s, "XDG_MUSIC_DIR"), None);
    }

    #[test]
    fn tilde() {
        assert_eq!(expand_tilde("~/x"), home().join("x"));
        assert_eq!(expand_tilde("/abs"), PathBuf::from("/abs"));
    }
}
