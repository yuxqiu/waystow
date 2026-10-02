//! The SQLite store: collections, shelf items, jobs and the Recent feed.
//! One connection, owned by the daemon's main thread.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use rusqlite::{Connection, OptionalExtension, Row, params};
use waystow_core::model::{
    Collection, Content, Item, Job, JobStatus, Recent, RecentKind, Source, TEMPORARY, Timestamp, Undo,
};

const SCHEMA: &[&str] = &[r#"
CREATE TABLE collection (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    emoji TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);
INSERT INTO collection (id, name, emoji, created_at) VALUES (1, 'Temporary', '', 0);
CREATE TABLE item (
    id INTEGER PRIMARY KEY,
    collection INTEGER NOT NULL REFERENCES collection(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    mime TEXT NOT NULL,
    size INTEGER,
    source TEXT NOT NULL,
    added_at INTEGER NOT NULL,
    expires_at INTEGER,
    pinned INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX item_collection ON item(collection);
CREATE TABLE job (
    id INTEGER PRIMARY KEY,
    action TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    inputs TEXT NOT NULL,
    outputs TEXT NOT NULL DEFAULT '[]',
    error TEXT,
    undo TEXT NOT NULL DEFAULT '{"undo":"nothing"}',
    started_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE TABLE recent (
    path TEXT NOT NULL,
    kind TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (path, kind)
);
CREATE INDEX recent_at ON recent(at);
"#];

/// A shelf item before it has an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewItem {
    /// Target collection.
    pub collection: i64,
    /// What it holds.
    pub content: Content,
    /// MIME type.
    pub mime: String,
    /// Size in bytes.
    pub size: Option<u64>,
    /// Where it came from.
    pub source: Source,
}

/// The database.
pub struct Store {
    conn: Connection,
    /// Lifetime of unpinned items in Temporary; `None` = never expire.
    pub temporary_expiry: Option<i64>,
}

impl Store {
    /// Open (creating and migrating) the database at `path`.
    ///
    /// # Errors
    ///
    /// The file can't be opened or migrated.
    pub fn open(path: &Path, temporary_expiry: Option<i64>) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::init(conn, temporary_expiry)
    }

    /// An in-memory database, for tests.
    ///
    /// # Errors
    ///
    /// SQLite fails to start.
    pub fn in_memory(temporary_expiry: Option<i64>) -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?, temporary_expiry)
    }

    fn init(conn: Connection, temporary_expiry: Option<i64>) -> anyhow::Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 2000;")?;
        let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(version <= SCHEMA.len(), "the database is from a newer WayStow (schema {version})");
        for (i, step) in SCHEMA.iter().enumerate().skip(version) {
            conn.execute_batch(&format!("BEGIN; {step} PRAGMA user_version = {}; COMMIT;", i + 1))
                .with_context(|| format!("migrating the database to schema {}", i + 1))?;
        }
        Ok(Self { conn, temporary_expiry })
    }

    // ── Collections ──────────────────────────────────────────────────────

    /// All collections, Temporary first, then by creation.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn collections(&self) -> anyhow::Result<Vec<Collection>> {
        let mut st =
            self.conn.prepare("SELECT id, name, emoji FROM collection ORDER BY id = 1 DESC, created_at, id")?;
        let rows = st.query_map([], |r| Ok(Collection { id: r.get(0)?, name: r.get(1)?, emoji: r.get(2)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Find a collection by name (case-insensitive); `None` name = Temporary.
    ///
    /// # Errors
    ///
    /// No such collection.
    pub fn collection_id(&self, name: Option<&str>) -> anyhow::Result<i64> {
        let Some(name) = name else { return Ok(TEMPORARY) };
        self.conn
            .query_row("SELECT id FROM collection WHERE name = ?1 COLLATE NOCASE", [name.trim()], |r| r.get(0))
            .optional()?
            .with_context(|| format!("no collection named {name:?}"))
    }

    /// Create a collection.
    ///
    /// # Errors
    ///
    /// Empty or duplicate name.
    pub fn new_collection(&self, name: &str, emoji: &str, now: Timestamp) -> anyhow::Result<Collection> {
        let name = name.trim();
        ensure!(!name.is_empty(), "the collection name is empty");
        ensure!(self.collection_id(Some(name)).is_err(), "a collection named {name:?} already exists");
        self.conn.execute(
            "INSERT INTO collection (name, emoji, created_at) VALUES (?1, ?2, ?3)",
            params![name, emoji, now],
        )?;
        Ok(Collection { id: self.conn.last_insert_rowid(), name: name.into(), emoji: emoji.into() })
    }

    /// Delete a collection; returns its items (so the caller removes stash files).
    ///
    /// # Errors
    ///
    /// Trying to delete Temporary.
    pub fn delete_collection(&self, id: i64) -> anyhow::Result<Vec<Item>> {
        ensure!(id != TEMPORARY, "the Temporary collection can't be deleted");
        let items = self.items(Some(id))?;
        let n = self.conn.execute("DELETE FROM collection WHERE id = ?1", [id])?;
        ensure!(n == 1, "no such collection");
        Ok(items)
    }

    // ── Items ────────────────────────────────────────────────────────────

    /// When an unpinned item placed in `collection` now should expire.
    #[must_use]
    pub fn expiry_for(&self, collection: i64, now: Timestamp) -> Option<Timestamp> {
        (collection == TEMPORARY).then_some(self.temporary_expiry.map(|s| now + s)).flatten()
    }

    /// Add an item. A file already in the collection is moved to the top
    /// (and its expiry renewed) instead of being added twice.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn add(&self, new: &NewItem, now: Timestamp) -> anyhow::Result<Item> {
        let (kind, value) = encode(&new.content);
        let expires = self.expiry_for(new.collection, now);
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM item WHERE collection = ?1 AND kind = ?2 AND value = ?3",
                params![new.collection, kind, value],
                |r| r.get(0),
            )
            .optional()?;
        let id = if let Some(id) = existing {
            self.conn.execute(
                "UPDATE item SET added_at = ?2, expires_at = CASE WHEN pinned THEN NULL ELSE ?3 END,
                 mime = ?4, size = ?5 WHERE id = ?1",
                params![id, now, expires, new.mime, new.size.map(|s| s as i64)],
            )?;
            id
        } else {
            self.conn.execute(
                "INSERT INTO item (collection, kind, value, mime, size, source, added_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    new.collection,
                    kind,
                    value,
                    new.mime,
                    new.size.map(|s| s as i64),
                    new.source.as_str(),
                    now,
                    expires
                ],
            )?;
            self.conn.last_insert_rowid()
        };
        self.item(id)?.context("item vanished")
    }

    /// One item.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn item(&self, id: i64) -> anyhow::Result<Option<Item>> {
        Ok(self.conn.query_row(&format!("{ITEM_SELECT} WHERE id = ?1"), [id], row_item).optional()?)
    }

    /// Items, newest first; all collections when `collection` is `None`.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn items(&self, collection: Option<i64>) -> anyhow::Result<Vec<Item>> {
        let mut st = self
            .conn
            .prepare(&format!("{ITEM_SELECT} WHERE ?1 IS NULL OR collection = ?1 ORDER BY added_at DESC, id DESC"))?;
        let rows = st.query_map([collection], row_item)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Remove items; returns what was removed.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn remove(&self, ids: &[i64]) -> anyhow::Result<Vec<Item>> {
        let mut gone = Vec::new();
        for &id in ids {
            if let Some(item) = self.item(id)? {
                self.conn.execute("DELETE FROM item WHERE id = ?1", [id])?;
                gone.push(item);
            }
        }
        Ok(gone)
    }

    /// Remove every unpinned item of a collection.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn clear(&self, collection: i64) -> anyhow::Result<Vec<Item>> {
        let ids: Vec<i64> = self.items(Some(collection))?.into_iter().filter(|i| !i.pinned).map(|i| i.id).collect();
        self.remove(&ids)
    }

    /// Pin (never expires) or unpin (expiry restarts now).
    ///
    /// # Errors
    ///
    /// No such item.
    pub fn set_pinned(&self, id: i64, pinned: bool, now: Timestamp) -> anyhow::Result<Item> {
        let item = self.item(id)?.context("no such item")?;
        let expires = if pinned { None } else { self.expiry_for(item.collection, now) };
        self.conn
            .execute("UPDATE item SET pinned = ?2, expires_at = ?3 WHERE id = ?1", params![id, pinned, expires])?;
        self.item(id)?.context("item vanished")
    }

    /// Move items to another collection (expiry follows the target's policy).
    ///
    /// # Errors
    ///
    /// No such collection.
    pub fn move_items(&self, ids: &[i64], collection: i64, now: Timestamp) -> anyhow::Result<()> {
        let exists: bool =
            self.conn.query_row("SELECT EXISTS(SELECT 1 FROM collection WHERE id = ?1)", [collection], |r| r.get(0))?;
        ensure!(exists, "no such collection");
        let expires = self.expiry_for(collection, now);
        for &id in ids {
            // An identical item already in the target wins; drop the mover.
            let Some(item) = self.item(id)? else { continue };
            let (kind, value) = encode(&item.content);
            let dup: Option<i64> = self
                .conn
                .query_row(
                    "SELECT id FROM item WHERE collection = ?1 AND kind = ?2 AND value = ?3",
                    params![collection, kind, value],
                    |r| r.get(0),
                )
                .optional()?;
            if dup.is_some_and(|d| d != id) {
                self.conn.execute("DELETE FROM item WHERE id = ?1", [id])?;
                continue;
            }
            self.conn.execute(
                "UPDATE item SET collection = ?2, expires_at = CASE WHEN pinned THEN NULL ELSE ?3 END WHERE id = ?1",
                params![id, collection, expires],
            )?;
        }
        Ok(())
    }

    /// Remove expired items; returns them.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn expire(&self, now: Timestamp) -> anyhow::Result<Vec<Item>> {
        let mut st = self
            .conn
            .prepare(&format!("{ITEM_SELECT} WHERE pinned = 0 AND expires_at IS NOT NULL AND expires_at <= ?1"))?;
        let due: Vec<Item> = st.query_map([now], row_item)?.collect::<Result<_, _>>()?;
        let ids: Vec<i64> = due.iter().map(|i| i.id).collect();
        self.remove(&ids)
    }

    /// The soonest expiry, so the daemon can sleep until then.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn next_expiry(&self) -> anyhow::Result<Option<Timestamp>> {
        Ok(self.conn.query_row("SELECT MIN(expires_at) FROM item WHERE pinned = 0", [], |r| r.get(0))?)
    }

    /// Is this stash file still referenced by any item?
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn stash_in_use(&self, path: &Path) -> anyhow::Result<bool> {
        let (_, value) = encode(&Content::Stash(path.to_path_buf()));
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM item WHERE kind = 'stash' AND value = ?1)",
            [value],
            |r| r.get(0),
        )?)
    }

    // ── Jobs ─────────────────────────────────────────────────────────────

    /// Record a new queued job; returns its id.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn new_job(&self, action: &str, title: &str, inputs: &[PathBuf], now: Timestamp) -> anyhow::Result<i64> {
        self.conn.execute(
            "INSERT INTO job (action, title, status, inputs, started_at) VALUES (?1, ?2, 'queued', ?3, ?4)",
            params![action, title, serde_json::to_string(inputs)?, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Save a job's state.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn save_job(&self, job: &Job) -> anyhow::Result<()> {
        let n = self.conn.execute(
            "UPDATE job SET status = ?2, outputs = ?3, error = ?4, undo = ?5, finished_at = ?6 WHERE id = ?1",
            params![
                job.id,
                job.status.as_str(),
                serde_json::to_string(&job.outputs)?,
                job.error,
                serde_json::to_string(&job.undo)?,
                job.finished_at
            ],
        )?;
        if n != 1 {
            bail!("no job {}", job.id);
        }
        Ok(())
    }

    /// One job.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn job(&self, id: i64) -> anyhow::Result<Option<Job>> {
        Ok(self.conn.query_row(&format!("{JOB_SELECT} WHERE id = ?1"), [id], row_job).optional()?)
    }

    /// Jobs, newest first.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn jobs(&self, limit: u32) -> anyhow::Result<Vec<Job>> {
        let mut st = self.conn.prepare(&format!("{JOB_SELECT} ORDER BY id DESC LIMIT ?1"))?;
        let rows = st.query_map([limit], row_job)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Mark jobs left running by a previous daemon as failed.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn fail_orphaned_jobs(&self, now: Timestamp) -> anyhow::Result<usize> {
        Ok(self.conn.execute(
            "UPDATE job SET status = 'failed', error = 'WayStow stopped while this was running', finished_at = ?1
             WHERE status IN ('queued', 'running')",
            [now],
        )?)
    }

    /// Forget all but the newest `keep` jobs.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn prune_jobs(&self, keep: u32) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM job WHERE id NOT IN (SELECT id FROM job ORDER BY id DESC LIMIT ?1)
             AND status NOT IN ('queued', 'running')",
            [keep],
        )?;
        Ok(())
    }

    // ── Recent ───────────────────────────────────────────────────────────

    /// Note a recent file (keeps the latest time per path and kind).
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn add_recent(&self, path: &Path, kind: RecentKind, at: Timestamp) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO recent (path, kind, at) VALUES (?1, ?2, ?3)
             ON CONFLICT (path, kind) DO UPDATE SET at = MAX(at, excluded.at)",
            params![path_text(path), kind.as_str(), at],
        )?;
        Ok(())
    }

    /// Recent files, newest first, one row per path (its latest kind).
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn recent(&self, kind: Option<RecentKind>, limit: u32) -> anyhow::Result<Vec<Recent>> {
        let mut st = self.conn.prepare(
            "SELECT path, kind, at FROM recent r
             WHERE (?1 IS NULL AND at = (SELECT MAX(at) FROM recent r2 WHERE r2.path = r.path)) OR kind = ?1
             GROUP BY path ORDER BY at DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![kind.map(RecentKind::as_str), limit], |r| {
            let kind: String = r.get(1)?;
            Ok(Recent {
                path: PathBuf::from(r.get::<_, String>(0)?),
                kind: RecentKind::parse(&kind).unwrap_or(RecentKind::Modified),
                at: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Forget recent entries older than `before`, and those for a path that's gone.
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn prune_recent(&self, before: Timestamp) -> anyhow::Result<()> {
        self.conn.execute("DELETE FROM recent WHERE at < ?1", [before])?;
        Ok(())
    }

    /// Drop a path from Recent (it was deleted or moved).
    ///
    /// # Errors
    ///
    /// SQLite errors.
    pub fn forget_recent(&self, path: &Path) -> anyhow::Result<()> {
        self.conn.execute("DELETE FROM recent WHERE path = ?1", [path_text(path)])?;
        Ok(())
    }
}

fn path_text(p: &Path) -> String {
    // Paths are stored as text; non-UTF-8 paths round-trip through their URI form.
    p.to_str().map_or_else(|| waystow_core::uri::from_path(p), str::to_owned)
}

fn path_from_text(s: String) -> PathBuf {
    if s.starts_with("file://") {
        waystow_core::uri::to_path(&s).unwrap_or_else(|| PathBuf::from(s))
    } else {
        PathBuf::from(s)
    }
}

fn encode(c: &Content) -> (&'static str, String) {
    match c {
        Content::File(p) => ("file", path_text(p)),
        Content::Stash(p) => ("stash", path_text(p)),
        Content::Text(t) => ("text", t.clone()),
        Content::Url(u) => ("url", u.clone()),
    }
}

const ITEM_SELECT: &str =
    "SELECT id, collection, kind, value, mime, size, source, added_at, expires_at, pinned FROM item";

fn row_item(r: &Row<'_>) -> rusqlite::Result<Item> {
    let kind: String = r.get(2)?;
    let value: String = r.get(3)?;
    let content = match kind.as_str() {
        "stash" => Content::Stash(path_from_text(value)),
        "text" => Content::Text(value),
        "url" => Content::Url(value),
        _ => Content::File(path_from_text(value)),
    };
    let source: String = r.get(6)?;
    Ok(Item {
        id: r.get(0)?,
        collection: r.get(1)?,
        content,
        mime: r.get(4)?,
        size: r.get::<_, Option<i64>>(5)?.map(|s| s as u64),
        source: Source::parse(&source).unwrap_or(Source::Cli),
        added_at: r.get(7)?,
        expires_at: r.get(8)?,
        pinned: r.get(9)?,
    })
}

const JOB_SELECT: &str =
    "SELECT id, action, title, status, inputs, outputs, error, undo, started_at, finished_at FROM job";

fn row_job(r: &Row<'_>) -> rusqlite::Result<Job> {
    let status: String = r.get(3)?;
    let json = |i: usize| -> rusqlite::Result<String> { r.get(i) };
    Ok(Job {
        id: r.get(0)?,
        action: r.get(1)?,
        title: r.get(2)?,
        status: JobStatus::parse(&status).unwrap_or(JobStatus::Failed),
        progress: None,
        inputs: serde_json::from_str(&json(4)?).unwrap_or_default(),
        outputs: serde_json::from_str(&json(5)?).unwrap_or_default(),
        error: r.get(6)?,
        undo: serde_json::from_str(&json(7)?).unwrap_or(Undo::Nothing),
        started_at: r.get(8)?,
        finished_at: r.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(collection: i64, p: &str) -> NewItem {
        NewItem {
            collection,
            content: Content::File(p.into()),
            mime: "image/png".into(),
            size: Some(3),
            source: Source::Drag,
        }
    }

    #[test]
    fn only_temporary_expires() {
        let s = Store::in_memory(Some(3600)).unwrap();
        let work = s.new_collection("Work", "💼", 0).unwrap();
        let a = s.add(&file(TEMPORARY, "/a.png"), 100).unwrap();
        let b = s.add(&file(work.id, "/b.png"), 100).unwrap();
        assert_eq!(a.expires_at, Some(3700));
        assert_eq!(b.expires_at, None);
        assert_eq!(s.next_expiry().unwrap(), Some(3700));
        assert!(s.expire(3699).unwrap().is_empty());
        assert_eq!(s.expire(3700).unwrap(), vec![a]);
        assert_eq!(s.items(None).unwrap(), vec![b]);
    }

    #[test]
    fn pinned_items_stay() {
        let s = Store::in_memory(Some(10)).unwrap();
        let a = s.add(&file(TEMPORARY, "/a.png"), 0).unwrap();
        let a = s.set_pinned(a.id, true, 5).unwrap();
        assert_eq!(a.expires_at, None);
        assert!(s.expire(1000).unwrap().is_empty());
        assert!(s.clear(TEMPORARY).unwrap().is_empty(), "clear keeps pinned items");
        let a = s.set_pinned(a.id, false, 2000).unwrap();
        assert_eq!(a.expires_at, Some(2010), "unpinning restarts the clock");
    }

    #[test]
    fn re_adding_a_file_bumps_it() {
        let s = Store::in_memory(Some(10)).unwrap();
        let a = s.add(&file(TEMPORARY, "/a.png"), 0).unwrap();
        s.add(&file(TEMPORARY, "/b.png"), 1).unwrap();
        let again = s.add(&file(TEMPORARY, "/a.png"), 2).unwrap();
        assert_eq!(again.id, a.id);
        assert_eq!(again.expires_at, Some(12));
        let names: Vec<_> = s.items(None).unwrap().iter().map(Item::display_name).collect();
        assert_eq!(names, ["a.png", "b.png"]);
    }

    #[test]
    fn moving_changes_expiry_and_merges_duplicates() {
        let s = Store::in_memory(Some(10)).unwrap();
        let w = s.new_collection("Upload", "", 0).unwrap();
        let a = s.add(&file(TEMPORARY, "/a.png"), 0).unwrap();
        s.move_items(&[a.id], w.id, 5).unwrap();
        assert_eq!(s.item(a.id).unwrap().unwrap().expires_at, None);
        let a2 = s.add(&file(TEMPORARY, "/a.png"), 6).unwrap();
        s.move_items(&[a2.id], w.id, 7).unwrap();
        assert_eq!(s.items(Some(w.id)).unwrap().len(), 1);
        assert!(s.items(Some(TEMPORARY)).unwrap().is_empty());
        assert!(s.move_items(&[a.id], 99, 8).is_err());
    }

    #[test]
    fn collections() {
        let s = Store::in_memory(None).unwrap();
        assert!(s.new_collection(" ", "", 0).is_err());
        let w = s.new_collection("Email to Alice", "✉", 0).unwrap();
        assert!(s.new_collection("email to alice", "", 0).is_err());
        assert_eq!(s.collection_id(Some("EMAIL TO ALICE")).unwrap(), w.id);
        assert_eq!(s.collection_id(None).unwrap(), TEMPORARY);
        assert!(s.collection_id(Some("nope")).is_err());
        s.add(&file(w.id, "/x"), 0).unwrap();
        assert_eq!(s.delete_collection(w.id).unwrap().len(), 1);
        assert!(s.items(None).unwrap().is_empty(), "items go with their collection");
        assert!(s.delete_collection(TEMPORARY).is_err());
        assert_eq!(s.collections().unwrap()[0].name, "Temporary");
    }

    #[test]
    fn jobs_round_trip_and_orphans_fail() {
        let s = Store::in_memory(None).unwrap();
        let id = s.new_job("image.convert", "Convert", &["/a.png".into()], 5).unwrap();
        let mut j = s.job(id).unwrap().unwrap();
        assert_eq!(j.status, JobStatus::Queued);
        j.status = JobStatus::Done;
        j.outputs = vec!["/a.webp".into()];
        j.undo = Undo::TrashOutputs;
        j.finished_at = Some(6);
        s.save_job(&j).unwrap();
        assert_eq!(s.job(id).unwrap().unwrap(), j);
        let k = s.new_job("x", "X", &[], 7).unwrap();
        assert_eq!(s.fail_orphaned_jobs(8).unwrap(), 1);
        assert_eq!(s.job(k).unwrap().unwrap().status, JobStatus::Failed);
        assert_eq!(s.jobs(10).unwrap().len(), 2);
        s.prune_jobs(1).unwrap();
        assert_eq!(s.jobs(10).unwrap().len(), 1);
    }

    #[test]
    fn recent_keeps_latest_per_path() {
        let s = Store::in_memory(None).unwrap();
        s.add_recent(Path::new("/d/a"), RecentKind::Downloaded, 10).unwrap();
        s.add_recent(Path::new("/d/a"), RecentKind::Opened, 20).unwrap();
        s.add_recent(Path::new("/d/b"), RecentKind::Screenshot, 15).unwrap();
        s.add_recent(Path::new("/d/b"), RecentKind::Screenshot, 5).unwrap();
        let all = s.recent(None, 10).unwrap();
        assert_eq!(
            all.iter().map(|r| (r.path.to_str().unwrap(), r.kind, r.at)).collect::<Vec<_>>(),
            [("/d/a", RecentKind::Opened, 20), ("/d/b", RecentKind::Screenshot, 15)]
        );
        assert_eq!(s.recent(Some(RecentKind::Downloaded), 10).unwrap().len(), 1);
        s.prune_recent(16).unwrap();
        assert_eq!(s.recent(None, 10).unwrap().len(), 1);
        s.forget_recent(Path::new("/d/a")).unwrap();
        assert!(s.recent(None, 10).unwrap().is_empty());
    }

    #[test]
    fn reopening_keeps_data_and_rejects_newer_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("sub/w.db");
        Store::open(&db, None).unwrap().add(&file(TEMPORARY, "/a"), 0).unwrap();
        assert_eq!(Store::open(&db, None).unwrap().items(None).unwrap().len(), 1);
        Connection::open(&db).unwrap().execute_batch("PRAGMA user_version = 99").unwrap();
        assert!(Store::open(&db, None).is_err());
    }

    #[test]
    fn stash_references() {
        let s = Store::in_memory(None).unwrap();
        let n = NewItem { content: Content::Stash("/st/x.png".into()), ..file(TEMPORARY, "") };
        let it = s.add(&n, 0).unwrap();
        assert!(s.stash_in_use(Path::new("/st/x.png")).unwrap());
        s.remove(&[it.id]).unwrap();
        assert!(!s.stash_in_use(Path::new("/st/x.png")).unwrap());
    }
}
