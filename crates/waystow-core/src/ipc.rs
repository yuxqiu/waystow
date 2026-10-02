//! CLI ⇄ daemon protocol: one JSON request line, one JSON response line.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{Collection, Item, Job, Recent, RecentKind};

/// A command from the CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// Open the shelf, or close it if open.
    Toggle,
    /// Open the shelf.
    Show,
    /// Close the shelf.
    Hide,
    /// Put files on the shelf.
    Add {
        /// Absolute paths.
        paths: Vec<PathBuf>,
        /// Collection name; default "Temporary".
        collection: Option<String>,
    },
    /// Put text (a snippet, or a URL) on the shelf.
    AddText {
        /// The text.
        text: String,
        /// Collection name; default "Temporary".
        collection: Option<String>,
    },
    /// Put the current clipboard on the shelf.
    AddClipboard {
        /// Collection name; default "Temporary".
        collection: Option<String>,
    },
    /// Shelf contents.
    List {
        /// Only this collection.
        collection: Option<String>,
    },
    /// Take items off the shelf (never deletes the files they point to).
    Remove {
        /// Item ids.
        ids: Vec<i64>,
    },
    /// Empty a collection ("Temporary" by default), keeping pinned items.
    Clear {
        /// Collection name.
        collection: Option<String>,
    },
    /// Pin or unpin an item.
    Pin {
        /// Item id.
        id: i64,
        /// Pinned items never expire.
        pinned: bool,
    },
    /// Move items into a collection.
    Move {
        /// Item ids.
        ids: Vec<i64>,
        /// Target collection name.
        collection: String,
    },
    /// Create a collection.
    NewCollection {
        /// Name.
        name: String,
        /// Emoji or symbol.
        emoji: Option<String>,
    },
    /// Delete a collection and its items.
    DeleteCollection {
        /// Name.
        name: String,
    },
    /// Actions that apply to these items.
    Actions {
        /// Item ids.
        ids: Vec<i64>,
    },
    /// Run an action on items or paths.
    Run {
        /// Action id.
        action: String,
        /// Item ids.
        ids: Vec<i64>,
        /// Extra absolute paths (not on the shelf).
        paths: Vec<PathBuf>,
        /// Param values.
        params: BTreeMap<String, String>,
    },
    /// Recent jobs, newest first.
    Jobs {
        /// How many.
        limit: u32,
    },
    /// Stop a running or queued job.
    Cancel {
        /// Job id.
        job: i64,
    },
    /// Undo a finished job.
    Undo {
        /// Job id.
        job: i64,
    },
    /// Recent files, newest first.
    Recent {
        /// Only this kind.
        kind: Option<RecentKind>,
        /// How many.
        limit: u32,
    },
    /// Daemon status.
    Status,
    /// Re-read config and action files.
    Reload,
    /// Stop the daemon.
    Quit,
}

/// An action, as offered for a selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionInfo {
    /// Id.
    pub id: String,
    /// Title.
    pub title: String,
    /// Icon name.
    pub icon: String,
    /// In the primary row.
    pub primary: bool,
    /// Has params the user may set.
    pub has_params: bool,
}

/// Daemon status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Program version.
    pub version: String,
    /// Shelf panel phase, e.g. `idle`.
    pub panel: String,
    /// Items on the shelf.
    pub items: usize,
    /// Running or queued jobs.
    pub jobs: usize,
    /// Monitors with a catcher.
    pub monitors: usize,
    /// Actions loaded (available / total).
    pub actions: (usize, usize),
}

/// The daemon's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    /// Done.
    Ok,
    /// Items were added.
    Added {
        /// New item ids.
        ids: Vec<i64>,
    },
    /// Shelf contents.
    Items {
        /// Collections, Temporary first.
        collections: Vec<Collection>,
        /// Items, newest first.
        items: Vec<Item>,
    },
    /// Applicable actions.
    Actions {
        /// Primary first.
        actions: Vec<ActionInfo>,
    },
    /// Jobs were queued.
    Started {
        /// Job ids.
        jobs: Vec<i64>,
    },
    /// Job history.
    Jobs {
        /// Newest first.
        jobs: Vec<Job>,
    },
    /// Recent files.
    Recent {
        /// Newest first.
        recent: Vec<Recent>,
    },
    /// Daemon status.
    Status(Status),
    /// The request failed.
    Error {
        /// What went wrong.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format() {
        let r = Request::Add { paths: vec!["/a".into()], collection: None };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, r#"{"cmd":"add","paths":["/a"],"collection":null}"#);
        assert_eq!(serde_json::from_str::<Request>(&s).unwrap(), r);
        assert_eq!(serde_json::to_string(&Request::Toggle).unwrap(), r#"{"cmd":"toggle"}"#);
        let e = Response::Error { message: "x".into() };
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"status":"error","message":"x"}"#);
    }
}
