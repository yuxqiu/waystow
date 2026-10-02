//! `$XDG_CONFIG_HOME/waystow/config.toml`. Every key is optional; unknown keys
//! and bad values are errors, so typos surface at daemon start.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::paths;

/// The whole config file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The drop target at the screen edges.
    pub catcher: CatcherConfig,
    /// Look and feel.
    pub ui: UiConfig,
    /// Progress and screenshot pill.
    pub island: IslandConfig,
    /// Shelf behaviour.
    pub shelf: ShelfConfig,
    /// The Recent feed.
    pub recent: RecentConfig,
    /// The job runner.
    pub jobs: JobsConfig,
}

/// A screen edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    /// Left edge.
    Left,
    /// Right edge.
    Right,
    /// Top edge.
    Top,
    /// Bottom edge.
    Bottom,
}

impl Edge {
    /// Left or right: the strip is vertical.
    #[must_use]
    pub const fn is_vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// `[catcher]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CatcherConfig {
    /// Edges with a hot strip, on every monitor. Empty disables the catcher
    /// (use `waystow toggle`).
    pub edges: Vec<Edge>,
    /// Fraction of the edge the strip covers, centred.
    pub length: f64,
    /// Strip thickness in pixels.
    pub thickness: u32,
    /// Show a small handle on the first edge while the shelf has items.
    pub handle: bool,
}

impl Default for CatcherConfig {
    fn default() -> Self {
        Self { edges: vec![Edge::Right], length: 0.3, thickness: 2, handle: true }
    }
}

/// Light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follow the desktop (portal `color-scheme`).
    System,
    /// Always dark.
    Dark,
    /// Always light.
    Light,
}

/// `[ui]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    /// Light, dark or system.
    pub theme: Theme,
    /// `#rrggbb`; empty = the desktop's accent colour, then WayStow's default.
    pub accent: String,
    /// Shelf panel width in pixels.
    pub width: u32,
    /// Panel background opacity, 0.5–1.
    pub opacity: f64,
    /// Draw the panel shadow ourselves. Turn off when a compositor rule draws it.
    pub client_shadow: bool,
    /// Replace motion with fades.
    pub reduced_motion: bool,
    /// Font family for the panels; empty = the system font.
    pub font: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            accent: String::new(),
            width: 360,
            opacity: 0.94,
            client_shadow: true,
            reduced_motion: false,
            font: String::new(),
        }
    }
}

/// Where the island sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IslandPosition {
    /// Top centre.
    Top,
    /// Bottom centre.
    Bottom,
}

/// `[island]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IslandConfig {
    /// Show the island; when off, progress goes to notifications.
    pub enabled: bool,
    /// Top or bottom.
    pub position: IslandPosition,
    /// Gap to the screen edge (or bar) in pixels.
    pub margin: u32,
    /// Offer actions on new screenshots.
    pub screenshots: bool,
    /// Show downloads in progress.
    pub downloads: bool,
    /// Seconds a screenshot toast stays.
    pub toast_secs: u32,
}

impl Default for IslandConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: IslandPosition::Top,
            margin: 8,
            screenshots: true,
            downloads: true,
            toast_secs: 6,
        }
    }
}

/// `[shelf]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShelfConfig {
    /// How long unpinned items stay in "Temporary": `30m`, `1h`, `2d`, or `never`.
    /// Named collections never expire.
    pub expiry: String,
    /// Largest dropped or copied payload (raw image bytes, text) we stash, in MiB.
    /// Bigger payloads are refused, never cut.
    pub max_payload_mib: u32,
}

impl Default for ShelfConfig {
    fn default() -> Self {
        Self { expiry: "1h".into(), max_payload_mib: 64 }
    }
}

impl ShelfConfig {
    /// [`Self::expiry`] in seconds; `None` = never.
    ///
    /// # Errors
    ///
    /// Not a duration.
    pub fn expiry_secs(&self) -> anyhow::Result<Option<i64>> {
        parse_duration(&self.expiry).with_context(|| format!("shelf.expiry = {:?}", self.expiry))
    }

    /// [`Self::max_payload_mib`] in bytes.
    #[must_use]
    pub const fn max_payload(&self) -> u64 {
        self.max_payload_mib as u64 * 1024 * 1024
    }
}

/// `[recent]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecentConfig {
    /// Keep this many days of history.
    pub days: u32,
    /// Watch the XDG download directory.
    pub downloads: bool,
    /// Directories whose new and changed files count as recent (not recursive).
    pub watch: Vec<String>,
    /// Screenshot directories; empty = `$XDG_PICTURES_DIR/Screenshots`.
    pub screenshots: Vec<String>,
    /// Read `recently-used.xbel` for files apps opened.
    pub opened: bool,
    /// Note files copied to the clipboard.
    pub copied: bool,
    /// File names (exact) or `*.suffix` patterns to ignore.
    pub ignore: Vec<String>,
}

impl Default for RecentConfig {
    fn default() -> Self {
        Self {
            days: 7,
            downloads: true,
            watch: vec!["~/Desktop".into(), "~/Documents".into()],
            screenshots: Vec::new(),
            opened: true,
            copied: true,
            ignore: vec!["*.swp".into(), "*.tmp".into(), "*~".into(), ".DS_Store".into()],
        }
    }
}

impl RecentConfig {
    /// Watched directories, `~` expanded.
    #[must_use]
    pub fn watch_dirs(&self) -> Vec<PathBuf> {
        self.watch.iter().map(|d| paths::expand_tilde(d)).collect()
    }

    /// Screenshot directories, `~` expanded, with the default filled in.
    #[must_use]
    pub fn screenshot_dirs(&self) -> Vec<PathBuf> {
        if self.screenshots.is_empty() {
            vec![paths::user_dir("XDG_PICTURES_DIR", "Pictures").join("Screenshots")]
        } else {
            self.screenshots.iter().map(|d| paths::expand_tilde(d)).collect()
        }
    }
}

/// `[jobs]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobsConfig {
    /// Jobs run at once; 0 = half the CPUs (at least 1).
    pub concurrency: u32,
}

impl JobsConfig {
    /// The effective limit.
    #[must_use]
    pub fn limit(&self) -> usize {
        if self.concurrency > 0 {
            return self.concurrency as usize;
        }
        std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1))
    }
}

impl Config {
    /// Path of the config file.
    #[must_use]
    pub fn path() -> PathBuf {
        paths::config_dir().join("config.toml")
    }

    /// Load and validate the config file; a missing file means defaults.
    ///
    /// # Errors
    ///
    /// Unreadable file, bad TOML, unknown keys or invalid values.
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&Self::path())
    }

    /// [`Self::load`] from a given path.
    ///
    /// # Errors
    ///
    /// As [`Self::load`].
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    /// Parse and validate config text.
    ///
    /// # Errors
    ///
    /// Bad TOML, unknown keys or invalid values.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let c = &self.catcher;
        if !(0.05..=1.0).contains(&c.length) {
            bail!("catcher.length must be between 0.05 and 1.0, not {}", c.length);
        }
        if !(1..=16).contains(&c.thickness) {
            bail!("catcher.thickness must be 1–16 pixels, not {}", c.thickness);
        }
        let mut seen = Vec::new();
        for e in &c.edges {
            if seen.contains(e) {
                bail!("catcher.edges lists {e:?} twice");
            }
            seen.push(*e);
        }
        if !self.ui.accent.is_empty() && parse_hex(&self.ui.accent).is_none() {
            bail!("ui.accent must be #rrggbb, not {:?}", self.ui.accent);
        }
        if !(240..=720).contains(&self.ui.width) {
            bail!("ui.width must be 240–720 pixels, not {}", self.ui.width);
        }
        if !(0.5..=1.0).contains(&self.ui.opacity) {
            bail!("ui.opacity must be between 0.5 and 1.0, not {}", self.ui.opacity);
        }
        self.shelf.expiry_secs()?;
        if self.shelf.max_payload_mib == 0 {
            bail!("shelf.max_payload_mib must be at least 1");
        }
        if self.recent.days == 0 {
            bail!("recent.days must be at least 1");
        }
        if self.island.toast_secs == 0 {
            bail!("island.toast_secs must be at least 1");
        }
        Ok(())
    }
}

/// `#rrggbb` → (r, g, b).
#[must_use]
pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

/// `90s`, `30m`, `1h`, `2d` → seconds; `never` → `None`.
///
/// # Errors
///
/// Anything else.
pub fn parse_duration(s: &str) -> anyhow::Result<Option<i64>> {
    let s = s.trim();
    if s == "never" {
        return Ok(None);
    }
    let unit = s.chars().last().context("empty duration")?;
    let mult = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => bail!("expected a number followed by s, m, h or d (e.g. 1h), or \"never\""),
    };
    let n: i64 = s[..s.len() - 1].parse().context("expected a whole number before the unit, e.g. 30m")?;
    if n <= 0 {
        bail!("the duration must be positive");
    }
    Ok(Some(n * mult))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_default() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let e = Config::parse("[ui]\nthme = \"dark\"").unwrap_err();
        assert!(format!("{e:#}").contains("thme"), "{e:#}");
    }

    #[test]
    fn edges_are_configurable() {
        let c = Config::parse("[catcher]\nedges = [\"left\", \"bottom\"]").unwrap();
        assert_eq!(c.catcher.edges, vec![Edge::Left, Edge::Bottom]);
        assert!(Config::parse("[catcher]\nedges = [\"left\", \"left\"]").is_err());
        assert_eq!(Config::parse("[catcher]\nedges = []").unwrap().catcher.edges, Vec::<Edge>::new());
    }

    #[test]
    fn values_are_validated() {
        assert!(Config::parse("[ui]\naccent = \"red\"").is_err());
        assert!(Config::parse("[ui]\naccent = \"#7c6cf2\"").is_ok());
        assert!(Config::parse("[catcher]\nlength = 0").is_err());
        assert!(Config::parse("[shelf]\nexpiry = \"soon\"").is_err());
        assert!(Config::parse("[shelf]\nexpiry = \"0m\"").is_err());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("90s").unwrap(), Some(90));
        assert_eq!(parse_duration("30m").unwrap(), Some(1800));
        assert_eq!(parse_duration("1h").unwrap(), Some(3600));
        assert_eq!(parse_duration("2d").unwrap(), Some(172_800));
        assert_eq!(parse_duration("never").unwrap(), None);
        assert!(parse_duration("h").is_err());
        assert!(parse_duration("1w").is_err());
    }

    #[test]
    fn hex() {
        assert_eq!(parse_hex("#7c6cf2"), Some((0x7c, 0x6c, 0xf2)));
        assert_eq!(parse_hex("7c6cf2"), None);
        assert_eq!(parse_hex("#7c6cfz"), None);
    }
}
