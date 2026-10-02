//! File names: split into stem and extension, and pick names that never
//! overwrite anything (`foo.webp`, then `foo (1).webp`, …).

use std::path::{Path, PathBuf};

/// `(stem, extension)` of a file name. A leading dot is part of the stem
/// (`.bashrc` has no extension) and only the last extension is split off,
/// except for `.tar.*` archives.
#[must_use]
pub fn split_name(name: &str) -> (&str, &str) {
    for compound in [".tar.gz", ".tar.xz", ".tar.zst", ".tar.bz2"] {
        if name.len() > compound.len() && name.to_ascii_lowercase().ends_with(compound) {
            let at = name.len() - compound.len();
            return (&name[..at], &name[at + 1..]);
        }
    }
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(i) => (&name[..i], &name[i + 1..]),
    }
}

/// The `n`th candidate for `path`: `n = 0` is `path` itself, then
/// `stem (n).ext`.
#[must_use]
pub fn numbered(path: &Path, n: u32) -> PathBuf {
    if n == 0 {
        return path.to_path_buf();
    }
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = split_name(&name);
    let new = if ext.is_empty() { format!("{stem} ({n})") } else { format!("{stem} ({n}).{ext}") };
    path.with_file_name(new)
}

/// The first of `path`, `stem (1).ext`, `stem (2).ext`… for which `taken`
/// is false.
#[must_use]
#[allow(clippy::maybe_infinite_iter)] // ends at the first free name
pub fn unique(path: &Path, taken: impl Fn(&Path) -> bool) -> PathBuf {
    (0..).map(|n| numbered(path, n)).find(|p| !taken(p)).expect("an unbounded range always yields a free name")
}

/// A hidden temporary sibling of `out` that keeps its extension, so tools
/// that pick a format from the extension (ImageMagick, ffmpeg) still work.
#[must_use]
pub fn temp_sibling(out: &Path, job: i64) -> PathBuf {
    let name = out.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = split_name(&name);
    let tmp = if ext.is_empty() { format!(".{stem}.waystow-{job}") } else { format!(".{stem}.waystow-{job}.{ext}") };
    out.with_file_name(tmp)
}

/// Is `name` one of our temporary files?
#[must_use]
pub fn is_temp(name: &str) -> bool {
    name.starts_with('.') && name.contains(".waystow-")
}

/// Is `name` a valid single file name (no separators, not `.`/`..`, not empty)?
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits() {
        assert_eq!(split_name("photo.png"), ("photo", "png"));
        assert_eq!(split_name("archive.tar.gz"), ("archive", "tar.gz"));
        assert_eq!(split_name(".bashrc"), (".bashrc", ""));
        assert_eq!(split_name("README"), ("README", ""));
        assert_eq!(split_name("a.b.c"), ("a.b", "c"));
    }

    #[test]
    fn unique_never_reuses_a_taken_name() {
        let taken = ["/d/foo.webp", "/d/foo (1).webp"];
        let p = unique(Path::new("/d/foo.webp"), |p| taken.contains(&p.to_str().unwrap()));
        assert_eq!(p, PathBuf::from("/d/foo (2).webp"));
        assert_eq!(unique(Path::new("/d/dir"), |p| p == Path::new("/d/dir")), PathBuf::from("/d/dir (1)"));
    }

    #[test]
    fn temp_keeps_the_extension() {
        let t = temp_sibling(Path::new("/d/foo.webp"), 7);
        assert_eq!(t, PathBuf::from("/d/.foo.waystow-7.webp"));
        assert!(is_temp(t.file_name().unwrap().to_str().unwrap()));
        assert!(!is_temp("foo.webp"));
    }

    #[test]
    fn names() {
        assert!(is_valid_name("a b.txt"));
        assert!(!is_valid_name("../x"));
        assert!(!is_valid_name(".."));
        assert!(!is_valid_name(""));
    }
}
