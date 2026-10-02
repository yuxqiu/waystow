//! MIME pattern matching for action filters.

/// Does `mime` match `pattern`? Patterns are exact (`application/pdf`),
/// a type wildcard (`image/*`) or `*` for anything.
#[must_use]
pub fn matches(pattern: &str, mime: &str) -> bool {
    if pattern == "*" || pattern == "*/*" {
        return true;
    }
    pattern.strip_suffix("/*").map_or_else(
        || pattern.eq_ignore_ascii_case(mime),
        |major| mime.split_once('/').is_some_and(|(m, _)| m.eq_ignore_ascii_case(major)),
    )
}

/// Does `mime` match any of `patterns`?
#[must_use]
pub fn matches_any(patterns: &[String], mime: &str) -> bool {
    patterns.iter().any(|p| matches(p, mime))
}

/// Directories.
pub const DIRECTORY: &str = "inode/directory";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(matches("image/*", "image/png"));
        assert!(!matches("image/*", "video/mp4"));
        assert!(matches("application/pdf", "application/pdf"));
        assert!(!matches("application/pdf", "application/pdfx"));
        assert!(matches("*", "inode/directory"));
        assert!(!matches("image/*", "imagex"));
    }
}
