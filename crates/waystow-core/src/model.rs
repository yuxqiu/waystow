//! Things on the shelf, shared by the store, the IPC protocol and the UI.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Seconds since the Unix epoch.
pub type Timestamp = i64;

/// Now, in [`Timestamp`] seconds.
#[must_use]
pub fn now() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The id of the always-present "Temporary" collection.
pub const TEMPORARY: i64 = 1;

/// A named group of shelf items. Only [`TEMPORARY`] expires; named collections
/// persist until cleared (DECISIONS #9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    /// Row id; [`TEMPORARY`] for the default collection.
    pub id: i64,
    /// Display name.
    pub name: String,
    /// One emoji or symbol shown on the chip.
    pub emoji: String,
}

/// What an item holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Content {
    /// A reference to a file or directory the user owns; never deleted by us.
    File(PathBuf),
    /// A file WayStow wrote into the stash; deleted when its item goes.
    Stash(PathBuf),
    /// A text snippet.
    Text(String),
    /// A web address.
    Url(String),
}

impl Content {
    /// The file behind this item, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::File(p) | Self::Stash(p) => Some(p),
            Self::Text(_) | Self::Url(_) => None,
        }
    }
}

/// Where an item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Dropped on the shelf.
    Drag,
    /// Taken from the clipboard.
    Clipboard,
    /// A new screenshot.
    Screenshot,
    /// `waystow add`.
    Cli,
    /// The output of an action.
    Action,
    /// Moved over from the Recent list.
    Recent,
}

impl Source {
    /// Stable name used in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Drag => "drag",
            Self::Clipboard => "clipboard",
            Self::Screenshot => "screenshot",
            Self::Cli => "cli",
            Self::Action => "action",
            Self::Recent => "recent",
        }
    }

    /// Inverse of [`Self::as_str`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "drag" => Self::Drag,
            "clipboard" => Self::Clipboard,
            "screenshot" => Self::Screenshot,
            "cli" => Self::Cli,
            "action" => Self::Action,
            "recent" => Self::Recent,
            _ => return None,
        })
    }
}

/// One thing on the shelf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Row id.
    pub id: i64,
    /// Owning collection.
    pub collection: i64,
    /// What it holds.
    pub content: Content,
    /// MIME type (`inode/directory` for folders, `text/plain` for snippets).
    pub mime: String,
    /// Size in bytes, if known.
    pub size: Option<u64>,
    /// Where it came from.
    pub source: Source,
    /// When it was added.
    pub added_at: Timestamp,
    /// When it leaves the shelf; `None` = never.
    pub expires_at: Option<Timestamp>,
    /// Pinned items never expire.
    pub pinned: bool,
}

impl Item {
    /// The name to show: the file name, or the start of a snippet.
    #[must_use]
    pub fn display_name(&self) -> String {
        match &self.content {
            Content::File(p) | Content::Stash(p) => {
                p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
            }
            Content::Text(t) => snippet_title(t),
            Content::Url(u) => u.clone(),
        }
    }
}

/// The first line of a snippet, cut to 48 characters.
#[must_use]
pub fn snippet_title(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let mut out: String = line.chars().take(48).collect();
    if line.chars().count() > 48 {
        out.push('…');
    }
    out
}

/// Kinds of "recent".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecentKind {
    /// A finished download.
    Downloaded,
    /// A new file in a watched directory.
    Created,
    /// A changed file in a watched directory.
    Modified,
    /// Opened by an app (recently-used.xbel).
    Opened,
    /// A new screenshot.
    Screenshot,
    /// Copied to the clipboard as a file.
    Copied,
}

impl RecentKind {
    /// Every kind, in tab order.
    pub const ALL: [Self; 6] =
        [Self::Downloaded, Self::Screenshot, Self::Copied, Self::Opened, Self::Created, Self::Modified];

    /// Stable name used in the database and CLI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Downloaded => "downloaded",
            Self::Created => "created",
            Self::Modified => "modified",
            Self::Opened => "opened",
            Self::Screenshot => "screenshot",
            Self::Copied => "copied",
        }
    }

    /// Inverse of [`Self::as_str`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// A recently touched file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    /// The file.
    pub path: PathBuf,
    /// How it became recent.
    pub kind: RecentKind,
    /// When.
    pub at: Timestamp,
}

/// Lifecycle of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// Waiting for a free slot.
    Queued,
    /// Running.
    Running,
    /// Finished with outputs (possibly none, for clipboard actions).
    Done,
    /// The tool failed or produced nothing.
    Failed,
    /// Stopped by the user.
    Cancelled,
    /// Done, then undone.
    Undone,
}

impl JobStatus {
    /// Stable name used in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Undone => "undone",
        }
    }

    /// Inverse of [`Self::as_str`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Queued, Self::Running, Self::Done, Self::Failed, Self::Cancelled, Self::Undone]
            .into_iter()
            .find(|k| k.as_str() == s)
    }

    /// No further transitions except undo.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled | Self::Undone)
    }
}

/// How to reverse a finished job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "undo", rename_all = "snake_case")]
pub enum Undo {
    /// Nothing to undo (clipboard and reveal actions).
    Nothing,
    /// Move these outputs to the Trash.
    TrashOutputs,
    /// Rename `to` back to `from`.
    Rename {
        /// The original path.
        from: PathBuf,
        /// The new path.
        to: PathBuf,
    },
    /// Restore these files from the Trash.
    RestoreTrashed {
        /// Original paths of the trashed files.
        paths: Vec<PathBuf>,
    },
}

/// A job, as shown in history and progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// Row id.
    pub id: i64,
    /// The action's id.
    pub action: String,
    /// The action's title, kept so history survives action file edits.
    pub title: String,
    /// Status.
    pub status: JobStatus,
    /// 0.0–1.0 when the tool reports it.
    pub progress: Option<f32>,
    /// Inputs.
    pub inputs: Vec<PathBuf>,
    /// Outputs, once done.
    pub outputs: Vec<PathBuf>,
    /// The error, for failed jobs.
    pub error: Option<String>,
    /// How to undo it.
    pub undo: Undo,
    /// Started (queued) at.
    pub started_at: Timestamp,
    /// Finished at.
    pub finished_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_titles() {
        assert_eq!(snippet_title("\n  hello world  \nmore"), "hello world");
        assert_eq!(snippet_title(&"x".repeat(60)).chars().count(), 49);
        assert_eq!(snippet_title(""), "");
    }

    #[test]
    fn names_round_trip() {
        for k in RecentKind::ALL {
            assert_eq!(RecentKind::parse(k.as_str()), Some(k));
        }
        for s in [Source::Drag, Source::Clipboard, Source::Screenshot, Source::Cli, Source::Action, Source::Recent] {
            assert_eq!(Source::parse(s.as_str()), Some(s));
        }
        assert_eq!(JobStatus::parse("undone"), Some(JobStatus::Undone));
    }
}
