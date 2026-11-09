//! Data layer: folders (tree, top level = categories), snippets, tags in one SQLite file.

use phrasex_engine::{Mode, Snippet};
use rusqlite::{params, Connection, Row};

pub use rusqlite::{Error, Result};

pub mod sync;

/// Schema migrations, applied in order; `PRAGMA user_version` = number applied.
const MIGRATIONS: &[&str] = &[V1, V2, V3, V4, V5, V6];

/// SQL for "now" as sortable UTC text with milliseconds (the sync clock).
const NOW: &str = "strftime('%Y-%m-%d %H:%M:%f', 'now')";
/// SQL for a new random id shared by every PC that syncs the item.
const NEW_UID: &str = "lower(hex(randomblob(16)))";

/// Cloud sync: stable ids across PCs, edit times with milliseconds for folders and phrases,
/// and tombstones (written by triggers, so cascaded deletes count too).
const V6: &str = "
ALTER TABLE folder ADD COLUMN uid TEXT;
ALTER TABLE folder ADD COLUMN updated_at TEXT NOT NULL DEFAULT '1970-01-01 00:00:00.000';
ALTER TABLE snippet ADD COLUMN uid TEXT;
UPDATE folder SET uid = lower(hex(randomblob(16))), updated_at = strftime('%Y-%m-%d %H:%M:%f', 'now');
UPDATE snippet SET uid = lower(hex(randomblob(16))), updated_at = substr(updated_at || '.000', 1, 23);
CREATE UNIQUE INDEX folder_uid ON folder(uid);
CREATE UNIQUE INDEX snippet_uid ON snippet(uid);
CREATE TABLE tombstone (uid TEXT PRIMARY KEY, deleted_at TEXT NOT NULL);
CREATE TRIGGER snippet_tombstone AFTER DELETE ON snippet BEGIN
  INSERT OR REPLACE INTO tombstone VALUES (OLD.uid, strftime('%Y-%m-%d %H:%M:%f', 'now'));
END;
CREATE TRIGGER folder_tombstone AFTER DELETE ON folder BEGIN
  INSERT OR REPLACE INTO tombstone VALUES (OLD.uid, strftime('%Y-%m-%d %H:%M:%f', 'now'));
END;
";

/// Several phrases may share a trigger (the user picks one from a menu): drop UNIQUE(trigger).
/// SQLite can't drop a constraint, so the table is rebuilt (foreign keys are off during migrations).
const V5: &str = "
CREATE TABLE snippet_new (
  id             INTEGER PRIMARY KEY,
  folder_id      INTEGER REFERENCES folder(id) ON DELETE CASCADE,
  name           TEXT NOT NULL,
  trigger        TEXT,                     -- NULL = only from menu/search; may repeat
  body           TEXT NOT NULL,
  mode           TEXT NOT NULL DEFAULT 'word' CHECK (mode IN ('word', 'immediate')),
  case_sensitive INTEGER NOT NULL DEFAULT 0,
  enabled        INTEGER NOT NULL DEFAULT 1,
  updated_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  hotkey         TEXT,
  uses           INTEGER NOT NULL DEFAULT 0
);
INSERT INTO snippet_new (id, folder_id, name, trigger, body, mode, case_sensitive, enabled, updated_at, hotkey, uses)
  SELECT id, folder_id, name, trigger, body, mode, case_sensitive, enabled, updated_at, hotkey, uses FROM snippet;
DROP TABLE snippet;
ALTER TABLE snippet_new RENAME TO snippet;
CREATE INDEX snippet_folder ON snippet(folder_id);
CREATE INDEX snippet_trigger ON snippet(trigger);
";

/// Deleted phrases/folders, kept as an export snapshot (JSON) so they can be restored.
const V4: &str = "
CREATE TABLE trash (
  id         INTEGER PRIMARY KEY,
  label      TEXT NOT NULL,
  is_folder  INTEGER NOT NULL,
  parent_id  INTEGER,              -- where it was; no FK: the folder may be gone too
  payload    TEXT NOT NULL,
  deleted_at TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
);
";

/// Days a deleted item stays in the trash before it is dropped for good.
pub const TRASH_DAYS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    pub id: i64,
    pub label: String,
    pub is_folder: bool,
    pub deleted_at: String,
}

const V3: &str = "
CREATE TABLE clip (id INTEGER PRIMARY KEY, text TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
ALTER TABLE snippet ADD COLUMN uses INTEGER NOT NULL DEFAULT 0;
";

/// How many clipboard items are kept.
pub const CLIP_LIMIT: i64 = 200;

const V2: &str = "
ALTER TABLE snippet ADD COLUMN hotkey TEXT;
CREATE TABLE setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

const V1: &str = "
CREATE TABLE folder (
  id        INTEGER PRIMARY KEY,
  parent_id INTEGER REFERENCES folder(id) ON DELETE CASCADE,
  name      TEXT NOT NULL
);
CREATE TABLE snippet (
  id             INTEGER PRIMARY KEY,
  folder_id      INTEGER REFERENCES folder(id) ON DELETE CASCADE,
  name           TEXT NOT NULL,
  trigger        TEXT UNIQUE,              -- NULL = only from menu/search
  body           TEXT NOT NULL,
  mode           TEXT NOT NULL DEFAULT 'word' CHECK (mode IN ('word', 'immediate')),
  case_sensitive INTEGER NOT NULL DEFAULT 0,
  enabled        INTEGER NOT NULL DEFAULT 1,
  updated_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE tag (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE);
CREATE TABLE snippet_tag (
  snippet_id INTEGER NOT NULL REFERENCES snippet(id) ON DELETE CASCADE,
  tag_id     INTEGER NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
  PRIMARY KEY (snippet_id, tag_id)
);
CREATE INDEX snippet_folder ON snippet(folder_id);
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
}

/// A stored snippet. `id == 0` means "not saved yet".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub folder_id: Option<i64>,
    pub name: String,
    pub trigger: Option<String>,
    pub body: String,
    pub mode: Mode,
    pub case_sensitive: bool,
    pub enabled: bool,
    /// Global shortcut that inserts this snippet, e.g. "Ctrl+Shift+F".
    pub hotkey: Option<String>,
}

impl Entry {
    pub fn new(folder_id: Option<i64>, name: &str, trigger: &str, body: &str) -> Self {
        Self {
            id: 0,
            folder_id,
            name: name.into(),
            trigger: (!trigger.is_empty()).then(|| trigger.into()),
            body: body.into(),
            mode: Mode::WordEnd,
            case_sensitive: false,
            enabled: true,
            hotkey: None,
        }
    }
}

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Opens `%APPDATA%\PhraseX\phrasex.db`, seeding a few examples on first run.
    pub fn open_default() -> std::result::Result<(Self, std::path::PathBuf), Box<dyn std::error::Error>> {
        let dir = std::path::PathBuf::from(std::env::var_os("APPDATA").ok_or("APPDATA not set")?).join("PhraseX");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("phrasex.db");
        let store = Self::open(&path)?;
        if store.entries()?.is_empty() && store.folders()?.is_empty() {
            let f = Some(store.add_folder(None, "Esempi")?);
            let immediate = |name, trigger, body| Entry { mode: Mode::Immediate, ..Entry::new(f, name, trigger, body) };
            for e in [
                Entry::new(f, "By the way", "btw", "by the way"),
                Entry::new(f, "Firma", "firma", "Cordiali saluti\nPhrase X"),
                immediate("Email", ";mail", "nome@example.com"),
                immediate("Data di oggi", ";data", "{#date}"),
                immediate("Ora", ";ora", "{#time HH:mm:ss}"),
                immediate("Appunti", ";cb", "Appunti: «{#clipboard}»"),
            ] {
                store.save(&e)?;
            }
            // untouched examples: any synced library wins over them (see sync::Library::is_pristine)
            store.db.execute_batch(&format!(
                "UPDATE folder SET updated_at = '{0}'; UPDATE snippet SET updated_at = '{0}';",
                sync::EPOCH
            ))?;
        }
        Ok((store, path))
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut db: Connection) -> Result<Self> {
        // Off while migrating: rebuilding a table must not cascade-delete its children (tags).
        db.pragma_update(None, "foreign_keys", false)?;
        let version: usize = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            let tx = db.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i + 1)?;
            tx.commit()?;
        }
        db.pragma_update(None, "foreign_keys", true)?;
        Ok(Self { db })
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        let mut st = self.db.prepare_cached("SELECT value FROM setting WHERE key = ?1")?;
        let mut rows = st.query([key])?;
        rows.next()?.map(|r| r.get(0)).transpose()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.db
            .execute("INSERT INTO setting (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", [key, value])
            .map(drop)
    }

    pub fn add_folder(&self, parent_id: Option<i64>, name: &str) -> Result<i64> {
        self.db.execute(
            &format!("INSERT INTO folder (parent_id, name, uid, updated_at) VALUES (?1, ?2, {NEW_UID}, {NOW})"),
            params![parent_id, name],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn rename_folder(&self, id: i64, name: &str) -> Result<()> {
        self.db.execute(&format!("UPDATE folder SET name = ?2, updated_at = {NOW} WHERE id = ?1"), params![id, name]).map(drop)
    }

    /// Moves a folder under `parent` (None = top level). Refuses to move it inside itself.
    /// Returns false when the move would create a loop.
    pub fn move_folder(&self, id: i64, parent: Option<i64>) -> Result<bool> {
        if creates_loop(&self.db, id, parent)? {
            return Ok(false);
        }
        self.db.execute(&format!("UPDATE folder SET parent_id = ?2, updated_at = {NOW} WHERE id = ?1"), params![id, parent])?;
        Ok(true)
    }

    /// Deletes the folder with its subfolders and snippets.
    pub fn delete_folder(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM folder WHERE id = ?1", [id]).map(drop)
    }

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let mut st = self.db.prepare("SELECT id, parent_id, name FROM folder ORDER BY name COLLATE NOCASE")?;
        let rows = st.query_map([], |r| Ok(Folder { id: r.get(0)?, parent_id: r.get(1)?, name: r.get(2)? }))?;
        rows.collect()
    }

    /// Inserts (`id == 0`) or updates; returns the id. Fails on a duplicate trigger.
    pub fn save(&self, e: &Entry) -> Result<i64> {
        let mode = match e.mode {
            Mode::WordEnd => "word",
            Mode::Immediate => "immediate",
        };
        let p = params![e.folder_id, e.name, e.trigger, e.body, mode, e.case_sensitive, e.enabled, e.hotkey];
        if e.id == 0 {
            self.db.execute(
                &format!(
                    "INSERT INTO snippet (folder_id, name, trigger, body, mode, case_sensitive, enabled, hotkey, uid, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, {NEW_UID}, {NOW})"
                ),
                p,
            )?;
            return Ok(self.db.last_insert_rowid());
        }
        self.db.execute(
            &format!(
                "UPDATE snippet SET folder_id = ?1, name = ?2, trigger = ?3, body = ?4, mode = ?5,
                 case_sensitive = ?6, enabled = ?7, hotkey = ?8, updated_at = {NOW} WHERE id = ?9"
            ),
            params![e.folder_id, e.name, e.trigger, e.body, mode, e.case_sensitive, e.enabled, e.hotkey, e.id],
        )?;
        Ok(e.id)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM snippet WHERE id = ?1", [id]).map(drop)
    }

    pub fn entries(&self) -> Result<Vec<Entry>> {
        let mut st = self.db.prepare(
            "SELECT id, folder_id, name, trigger, body, mode, case_sensitive, enabled, hotkey
             FROM snippet ORDER BY name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], entry)?;
        rows.collect()
    }

    /// Replaces the tags of a snippet (created on the fly, case-insensitive).
    pub fn set_tags(&mut self, snippet_id: i64, tags: &[&str]) -> Result<()> {
        let tx = self.db.transaction()?;
        write_tags(&tx, snippet_id, tags)?;
        tx.execute("DELETE FROM tag WHERE id NOT IN (SELECT tag_id FROM snippet_tag)", [])?;
        tx.commit()
    }

    pub fn tags(&self, snippet_id: i64) -> Result<Vec<String>> {
        let mut st = self.db.prepare(
            "SELECT t.name FROM tag t JOIN snippet_tag st ON st.tag_id = t.id
             WHERE st.snippet_id = ?1 ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = st.query_map([snippet_id], |r| r.get(0))?;
        rows.collect()
    }

    /// Consistent copy of the whole database (safe while the app runs).
    pub fn backup_to(&self, path: &std::path::Path) -> Result<()> {
        self.db.execute("VACUUM INTO ?1", [path.to_string_lossy()]).map(drop)
    }

    /// Remembers a copied text (most recent first; an existing duplicate moves to the top).
    pub fn add_clip(&self, text: &str) -> Result<()> {
        self.db.execute("DELETE FROM clip WHERE text = ?1", [text])?;
        self.db.execute("INSERT INTO clip (text) VALUES (?1)", [text])?;
        self.db
            .execute("DELETE FROM clip WHERE id NOT IN (SELECT id FROM clip ORDER BY id DESC LIMIT ?1)", [CLIP_LIMIT])
            .map(drop)
    }

    /// (id, text), most recent first.
    pub fn clips(&self) -> Result<Vec<(i64, String)>> {
        let mut st = self.db.prepare("SELECT id, text FROM clip ORDER BY id DESC")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    pub fn delete_clip(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM clip WHERE id = ?1", [id]).map(drop)
    }

    pub fn clear_clips(&self) -> Result<()> {
        self.db.execute("DELETE FROM clip", []).map(drop)
    }

    /// Puts a deleted item's snapshot in the trash.
    pub fn add_trash(&self, label: &str, is_folder: bool, parent_id: Option<i64>, payload: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO trash (label, is_folder, parent_id, payload) VALUES (?1, ?2, ?3, ?4)",
            params![label, is_folder, parent_id, payload],
        )?;
        self.purge_trash()
    }

    /// Drops items deleted more than `TRASH_DAYS` days ago (deleted_at is local time, like 'now','localtime').
    fn purge_trash(&self) -> Result<()> {
        self.db
            .execute("DELETE FROM trash WHERE deleted_at < datetime('now', 'localtime', ?1)", [format!("-{TRASH_DAYS} days")])
            .map(drop)
    }

    /// Most recently deleted first (expired items purged first).
    pub fn trash(&self) -> Result<Vec<TrashItem>> {
        self.purge_trash()?;
        let mut st = self.db.prepare("SELECT id, label, is_folder, deleted_at FROM trash ORDER BY id DESC")?;
        let rows = st.query_map([], |r| Ok(TrashItem { id: r.get(0)?, label: r.get(1)?, is_folder: r.get(2)?, deleted_at: r.get(3)? }))?;
        rows.collect()
    }

    /// Removes an item from the trash and returns (original parent if it still exists, payload).
    pub fn take_trash(&self, id: i64) -> Result<Option<(Option<i64>, String)>> {
        let row: Option<(Option<i64>, String)> = self
            .db
            .query_row(
                "SELECT (SELECT f.id FROM folder f WHERE f.id = t.parent_id), payload FROM trash t WHERE t.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map(Some)
            .or_else(|e| if e == Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })?;
        self.db.execute("DELETE FROM trash WHERE id = ?1", [id])?;
        Ok(row)
    }

    pub fn clear_trash(&self) -> Result<()> {
        self.db.execute("DELETE FROM trash", []).map(drop)
    }

    /// Counts one use of a phrase (statistics).
    pub fn count_use(&self, id: i64) -> Result<()> {
        self.db.execute("UPDATE snippet SET uses = uses + 1 WHERE id = ?1", [id]).map(drop)
    }

    /// Counts a typed expansion: `typed` is the abbreviation as typed, maybe followed by its delimiter.
    pub fn count_use_typed(&self, typed: &str) -> Result<()> {
        let without_delimiter: String = typed.chars().take(typed.chars().count().saturating_sub(1)).collect();
        self.db
            .execute(
                "UPDATE snippet SET uses = uses + 1 WHERE trigger IS NOT NULL AND (lower(trigger) = lower(?1) OR lower(trigger) = lower(?2))",
                [typed, without_delimiter.as_str()],
            )
            .map(drop)
    }

    /// (phrase id, uses), only used phrases, most used first.
    pub fn usage(&self) -> Result<Vec<(i64, i64)>> {
        let mut st = self.db.prepare("SELECT id, uses FROM snippet WHERE uses > 0 ORDER BY uses DESC")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// Enabled snippets that have a trigger, ready for the expansion engine.
    pub fn engine_snippets(&self) -> Result<Vec<Snippet>> {
        Ok(self
            .entries()?
            .into_iter()
            .filter(|e| e.enabled)
            .filter_map(|e| {
                Some(Snippet { id: e.id, trigger: e.trigger?, body: e.body, mode: e.mode, case_sensitive: e.case_sensitive })
            })
            .collect())
    }
}

/// True when putting folder `id` under `parent` would make it its own ancestor.
fn creates_loop(db: &Connection, id: i64, parent: Option<i64>) -> Result<bool> {
    let mut cursor = parent;
    while let Some(p) = cursor {
        if p == id {
            return Ok(true);
        }
        cursor = db.query_row("SELECT parent_id FROM folder WHERE id = ?1", [p], |r| r.get(0))?;
    }
    Ok(false)
}

/// Replaces a snippet's tags (orphan tags are left for the caller to drop).
fn write_tags<S: AsRef<str>>(db: &Connection, snippet_id: i64, tags: &[S]) -> Result<()> {
    db.execute("DELETE FROM snippet_tag WHERE snippet_id = ?1", [snippet_id])?;
    for t in tags.iter().map(|t| t.as_ref().trim()).filter(|t| !t.is_empty()) {
        db.execute("INSERT OR IGNORE INTO tag (name) VALUES (?1)", [t])?;
        db.execute("INSERT OR IGNORE INTO snippet_tag SELECT ?1, id FROM tag WHERE name = ?2", params![snippet_id, t])?;
    }
    Ok(())
}

fn entry(r: &Row) -> Result<Entry> {
    let mode: String = r.get(5)?;
    Ok(Entry {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        name: r.get(2)?,
        trigger: r.get(3)?,
        body: r.get(4)?,
        mode: if mode == "immediate" { Mode::Immediate } else { Mode::WordEnd },
        case_sensitive: r.get(6)?,
        enabled: r.get(7)?,
        hotkey: r.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_and_engine_view() {
        let s = Store::open_in_memory().unwrap();
        let f = s.add_folder(None, "Lavoro").unwrap();
        let mut e = Entry::new(Some(f), "Saluto", "ciao", "Ciao, come stai?");
        e.id = s.save(&e).unwrap();
        e.body = "Buongiorno!".into();
        e.mode = Mode::Immediate;
        s.save(&e).unwrap();
        s.save(&Entry::new(Some(f), "Solo menu", "", "x")).unwrap();
        s.save(&Entry { enabled: false, ..Entry::new(None, "Spento", "off", "x") }).unwrap();

        assert_eq!(s.entries().unwrap().len(), 3);
        assert_eq!(s.entries().unwrap()[0], e);
        let snippets = s.engine_snippets().unwrap();
        assert_eq!(snippets.len(), 1);
        assert_eq!((snippets[0].trigger.as_str(), snippets[0].mode), ("ciao", Mode::Immediate));

        s.delete(e.id).unwrap();
        assert_eq!(s.entries().unwrap().len(), 2);
    }

    #[test]
    fn v5_allows_shared_triggers_and_keeps_tags() {
        // a database as it was before V5
        let db = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..4] {
            db.execute_batch(sql).unwrap();
        }
        db.pragma_update(None, "user_version", 4).unwrap();
        db.execute_batch(
            "INSERT INTO snippet (id, name, trigger, body, hotkey, uses) VALUES (7, 'a', 'btw', 'x', 'Ctrl+Alt+B', 3);
             INSERT INTO tag (id, name) VALUES (1, 'mail');
             INSERT INTO snippet_tag VALUES (7, 1);",
        )
        .unwrap();
        let mut s = Store::init(db).unwrap();
        assert_eq!(s.tags(7).unwrap(), ["mail"], "rebuild must not cascade-delete tags");
        assert_eq!(s.usage().unwrap(), [(7, 3)]);
        assert_eq!(s.entries().unwrap()[0].hotkey.as_deref(), Some("Ctrl+Alt+B"));
        s.save(&Entry::new(None, "b", "btw", "y")).unwrap();
        assert_eq!(s.engine_snippets().unwrap().iter().filter(|e| e.trigger == "btw").count(), 2);
        s.set_tags(7, &[]).unwrap(); // foreign keys back on: deleting still works
        s.delete(7).unwrap();
        assert_eq!(s.entries().unwrap().len(), 1);
    }

    #[test]
    fn delete_folder_cascades() {
        let s = Store::open_in_memory().unwrap();
        let root = s.add_folder(None, "Root").unwrap();
        let sub = s.add_folder(Some(root), "Sub").unwrap();
        s.save(&Entry::new(Some(sub), "n", "t", "b")).unwrap();
        s.save(&Entry::new(None, "keep", "k", "b")).unwrap();
        s.delete_folder(root).unwrap();
        assert!(s.folders().unwrap().is_empty());
        assert_eq!(s.entries().unwrap().len(), 1);
    }

    #[test]
    fn trash_keeps_parent_only_if_it_still_exists() {
        let s = Store::open_in_memory().unwrap();
        let a = s.add_folder(None, "A").unwrap();
        s.add_trash("x", false, Some(a), "{1}").unwrap();
        s.add_trash("y", true, Some(a), "{2}").unwrap();
        let items = s.trash().unwrap();
        assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["y", "x"]);
        assert!(items[0].is_folder);
        assert_eq!(s.take_trash(items[1].id).unwrap(), Some((Some(a), "{1}".into())));
        assert_eq!(s.take_trash(items[1].id).unwrap(), None, "already taken");
        s.delete_folder(a).unwrap();
        assert_eq!(s.take_trash(items[0].id).unwrap(), Some((None, "{2}".into())));
        s.add_trash("old", false, None, "{}").unwrap();
        s.add_trash("new", false, None, "{}").unwrap();
        s.db.execute("UPDATE trash SET deleted_at = datetime('now', 'localtime', '-31 days') WHERE label = 'old'", []).unwrap();
        let left: Vec<String> = s.trash().unwrap().into_iter().map(|t| t.label).collect();
        assert_eq!(left, ["new"], "older than TRASH_DAYS is purged");
        s.clear_trash().unwrap();
        assert!(s.trash().unwrap().is_empty());
    }

    #[test]
    fn move_folder_refuses_loops() {
        let s = Store::open_in_memory().unwrap();
        let a = s.add_folder(None, "A").unwrap();
        let b = s.add_folder(Some(a), "B").unwrap();
        let c = s.add_folder(Some(b), "C").unwrap();
        assert!(!s.move_folder(a, Some(c)).unwrap(), "A inside its own grandchild");
        assert!(!s.move_folder(a, Some(a)).unwrap());
        assert!(s.move_folder(c, None).unwrap());
        assert_eq!(s.folders().unwrap().iter().find(|f| f.id == c).unwrap().parent_id, None);
        assert!(s.move_folder(a, Some(c)).unwrap(), "now allowed: C is no longer under A");
    }

    #[test]
    fn tags_replace_and_cleanup() {
        let mut s = Store::open_in_memory().unwrap();
        let id = s.save(&Entry::new(None, "n", "t", "b")).unwrap();
        s.set_tags(id, &["clienti", "Email", " "]).unwrap();
        assert_eq!(s.tags(id).unwrap(), ["clienti", "Email"]);
        s.set_tags(id, &["email"]).unwrap();
        assert_eq!(s.tags(id).unwrap(), ["Email"]); // same tag, case-insensitive
        let orphans: i64 = s.db.query_row("SELECT COUNT(*) FROM tag", [], |r| r.get(0)).unwrap();
        assert_eq!(orphans, 1);
    }

    #[test]
    fn settings_and_hotkey() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.setting("lang").unwrap(), None);
        s.set_setting("lang", "it").unwrap();
        s.set_setting("lang", "en").unwrap();
        assert_eq!(s.setting("lang").unwrap().as_deref(), Some("en"));
        let e = Entry { hotkey: Some("Ctrl+Shift+F".into()), ..Entry::new(None, "n", "", "b") };
        s.save(&e).unwrap();
        assert_eq!(s.entries().unwrap()[0].hotkey.as_deref(), Some("Ctrl+Shift+F"));
    }

    #[test]
    fn clipboard_history_dedup_and_limit() {
        let s = Store::open_in_memory().unwrap();
        s.add_clip("a").unwrap();
        s.add_clip("b").unwrap();
        s.add_clip("a").unwrap();
        let texts: Vec<String> = s.clips().unwrap().into_iter().map(|c| c.1).collect();
        assert_eq!(texts, ["a", "b"]);
        for i in 0..CLIP_LIMIT + 5 {
            s.add_clip(&i.to_string()).unwrap();
        }
        assert_eq!(s.clips().unwrap().len() as i64, CLIP_LIMIT);
        s.clear_clips().unwrap();
        assert!(s.clips().unwrap().is_empty());
    }

    #[test]
    fn usage_counts() {
        let s = Store::open_in_memory().unwrap();
        let a = s.save(&Entry::new(None, "a", "a", "x")).unwrap();
        let b = s.save(&Entry::new(None, "b", "b", "x")).unwrap();
        s.count_use(b).unwrap();
        s.count_use(b).unwrap();
        s.count_use(a).unwrap();
        assert_eq!(s.usage().unwrap(), [(b, 2), (a, 1)]);
    }

    #[test]
    fn persists_on_disk() {
        let path = std::env::temp_dir().join(format!("phrasex-test-{}.db", std::process::id()));
        {
            let s = Store::open(&path).unwrap();
            s.save(&Entry::new(None, "n", "t", "b")).unwrap();
        }
        assert_eq!(Store::open(&path).unwrap().entries().unwrap().len(), 1);
        std::fs::remove_file(path).unwrap();
    }
}
