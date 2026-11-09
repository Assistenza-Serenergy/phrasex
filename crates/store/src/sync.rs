//! Cloud sync data: the whole library as one document keyed by stable uids, merged per item
//! (the most recent edit wins, deletions travel as tombstones).

use super::{creates_loop, write_tags, Store};
use rusqlite::{params, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Edit time of the untouched examples seeded on first run: anything synced is newer.
pub const EPOCH: &str = "1970-01-01 00:00:00.000";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Library {
    pub folders: Vec<SyncFolder>,
    pub snippets: Vec<SyncSnippet>,
    pub tombstones: Vec<Tombstone>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SyncFolder {
    pub uid: String,
    pub parent: Option<String>,
    pub name: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SyncSnippet {
    pub uid: String,
    pub folder: Option<String>,
    pub name: String,
    pub trigger: Option<String>,
    pub body: String,
    pub immediate: bool,
    pub case_sensitive: bool,
    pub enabled: bool,
    pub hotkey: Option<String>,
    pub tags: Vec<String>,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Tombstone {
    pub uid: String,
    pub deleted_at: String,
}

trait Stamped {
    fn uid(&self) -> &str;
    fn stamp(&self) -> &str;
}
impl Stamped for SyncFolder {
    fn uid(&self) -> &str {
        &self.uid
    }
    fn stamp(&self) -> &str {
        &self.updated_at
    }
}
impl Stamped for SyncSnippet {
    fn uid(&self) -> &str {
        &self.uid
    }
    fn stamp(&self) -> &str {
        &self.updated_at
    }
}

impl Library {
    /// Only the examples seeded on first run, never edited: a new PC drops them for the synced library.
    pub fn is_pristine(&self) -> bool {
        self.folders.iter().all(|f| f.updated_at == EPOCH) && self.snippets.iter().all(|s| s.updated_at == EPOCH)
    }
}

/// Per item, the newest version wins; a deletion newer than every version removes the item.
/// Output is sorted by uid, so equal libraries compare equal. Ties keep `a`.
/// ponytail: tombstones are never pruned (~70 bytes each); prune old ones if the file ever gets big.
pub fn merge(a: &Library, b: &Library) -> Library {
    let mut tombs: BTreeMap<String, String> = BTreeMap::new();
    for t in a.tombstones.iter().chain(&b.tombstones) {
        let d = tombs.entry(t.uid.clone()).or_default();
        if t.deleted_at > *d {
            d.clone_from(&t.deleted_at);
        }
    }
    let folders = newest(&a.folders, &b.folders, &mut tombs);
    let snippets = newest(&a.snippets, &b.snippets, &mut tombs);
    let tombstones = tombs.into_iter().map(|(uid, deleted_at)| Tombstone { uid, deleted_at }).collect();
    Library { folders, snippets, tombstones }
}

fn newest<T: Stamped + Clone>(a: &[T], b: &[T], tombs: &mut BTreeMap<String, String>) -> Vec<T> {
    let mut best: BTreeMap<&str, &T> = BTreeMap::new();
    for x in a.iter().chain(b) {
        match best.get(x.uid()) {
            Some(y) if y.stamp() >= x.stamp() => {}
            _ => {
                best.insert(x.uid(), x);
            }
        }
    }
    let mut out = Vec::new();
    for x in best.into_values() {
        match tombs.get(x.uid()) {
            Some(d) if d.as_str() > x.stamp() => {}
            found => {
                if found.is_some() {
                    tombs.remove(x.uid()); // edited after the deletion: it lives on
                }
                out.push(x.clone());
            }
        }
    }
    out
}

impl Store {
    /// The library as the sync document (local ids replaced by uids).
    pub fn sync_snapshot(&self) -> Result<Library> {
        let mut st = self.db.prepare(
            "SELECT f.uid, p.uid, f.name, f.updated_at FROM folder f LEFT JOIN folder p ON p.id = f.parent_id ORDER BY f.uid",
        )?;
        let folders = st
            .query_map([], |r| Ok(SyncFolder { uid: r.get(0)?, parent: r.get(1)?, name: r.get(2)?, updated_at: r.get(3)? }))?
            .collect::<Result<_>>()?;
        let mut st = self.db.prepare(
            "SELECT s.id, s.uid, f.uid, s.name, s.trigger, s.body, s.mode, s.case_sensitive, s.enabled, s.hotkey, s.updated_at
             FROM snippet s LEFT JOIN folder f ON f.id = s.folder_id ORDER BY s.uid",
        )?;
        let rows: Vec<(i64, SyncSnippet)> = st
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    SyncSnippet {
                        uid: r.get(1)?,
                        folder: r.get(2)?,
                        name: r.get(3)?,
                        trigger: r.get(4)?,
                        body: r.get(5)?,
                        immediate: r.get::<_, String>(6)? == "immediate",
                        case_sensitive: r.get(7)?,
                        enabled: r.get(8)?,
                        hotkey: r.get(9)?,
                        tags: vec![],
                        updated_at: r.get(10)?,
                    },
                ))
            })?
            .collect::<Result<_>>()?;
        let snippets = rows.into_iter().map(|(id, s)| Ok(SyncSnippet { tags: self.tags(id)?, ..s })).collect::<Result<_>>()?;
        let mut st = self.db.prepare("SELECT uid, deleted_at FROM tombstone ORDER BY uid")?;
        let tombstones =
            st.query_map([], |r| Ok(Tombstone { uid: r.get(0)?, deleted_at: r.get(1)? }))?.collect::<Result<_>>()?;
        Ok(Library { folders, snippets, tombstones })
    }

    /// Makes the local library exactly `lib` (one transaction). Keeps local usage counts.
    /// Parents that are gone, or that would make a loop, put the item at the top level.
    pub fn apply_sync(&mut self, lib: &Library) -> Result<()> {
        let tx = self.db.transaction()?;
        let mut ids: HashMap<&str, i64> = HashMap::new();
        for f in &lib.folders {
            tx.execute(
                "INSERT INTO folder (uid, name, updated_at, parent_id) VALUES (?1, ?2, ?3, NULL)
                 ON CONFLICT(uid) DO UPDATE SET name = ?2, updated_at = ?3, parent_id = NULL",
                params![f.uid, f.name, f.updated_at],
            )?;
            ids.insert(&f.uid, tx.query_row("SELECT id FROM folder WHERE uid = ?1", [&f.uid], |r| r.get(0))?);
        }
        for f in &lib.folders {
            let (id, parent) = (ids[f.uid.as_str()], f.parent.as_deref().and_then(|p| ids.get(p)).copied());
            if parent.is_some() && !creates_loop(&tx, id, parent)? {
                tx.execute("UPDATE folder SET parent_id = ?2 WHERE id = ?1", params![id, parent])?;
            }
        }
        for s in &lib.snippets {
            let folder = s.folder.as_deref().and_then(|u| ids.get(u)).copied();
            let mode = if s.immediate { "immediate" } else { "word" };
            tx.execute(
                "INSERT INTO snippet (uid, folder_id, name, trigger, body, mode, case_sensitive, enabled, hotkey, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(uid) DO UPDATE SET folder_id = ?2, name = ?3, trigger = ?4, body = ?5, mode = ?6,
                   case_sensitive = ?7, enabled = ?8, hotkey = ?9, updated_at = ?10",
                params![s.uid, folder, s.name, s.trigger, s.body, mode, s.case_sensitive, s.enabled, s.hotkey, s.updated_at],
            )?;
            let id: i64 = tx.query_row("SELECT id FROM snippet WHERE uid = ?1", [&s.uid], |r| r.get(0))?;
            write_tags(&tx, id, &s.tags)?;
        }
        // everything not in `lib` goes: phrases first, so no folder cascade can reach a live one
        let live: HashSet<&str> = lib.folders.iter().map(|f| f.uid.as_str()).chain(lib.snippets.iter().map(|s| s.uid.as_str())).collect();
        for table in ["snippet", "folder"] {
            let uids: Vec<String> =
                tx.prepare(&format!("SELECT uid FROM {table}"))?.query_map([], |r| r.get(0))?.collect::<Result<_>>()?;
            for uid in uids.iter().filter(|u| !live.contains(u.as_str())) {
                tx.execute(&format!("DELETE FROM {table} WHERE uid = ?1"), [uid])?;
            }
        }
        tx.execute("DELETE FROM tag WHERE id NOT IN (SELECT tag_id FROM snippet_tag)", [])?;
        tx.execute("DELETE FROM tombstone", [])?; // the triggers above wrote local ones: keep the merged set only
        for t in &lib.tombstones {
            tx.execute("INSERT INTO tombstone (uid, deleted_at) VALUES (?1, ?2)", params![t.uid, t.deleted_at])?;
        }
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entry;

    fn uid_of(s: &Store, name: &str) -> String {
        s.sync_snapshot().unwrap().snippets.into_iter().find(|x| x.name == name).unwrap().uid
    }

    #[test]
    fn deletes_leave_tombstones_even_when_cascaded() {
        let s = Store::open_in_memory().unwrap();
        let f = s.add_folder(None, "F").unwrap();
        s.save(&Entry::new(Some(f), "a", "a", "x")).unwrap();
        s.save(&Entry::new(None, "b", "b", "x")).unwrap();
        let snap = s.sync_snapshot().unwrap();
        assert!(snap.folders[0].uid.len() == 32 && snap.tombstones.is_empty());
        assert!(snap.snippets.iter().all(|x| x.updated_at.len() == 23), "ms precision: {:?}", snap.snippets);
        let a_uid = uid_of(&s, "a");
        s.delete_folder(f).unwrap();
        let gone: Vec<String> = s.sync_snapshot().unwrap().tombstones.into_iter().map(|t| t.uid).collect();
        assert!(gone.contains(&a_uid) && gone.contains(&snap.folders[0].uid), "cascade wrote tombstones: {gone:?}");
    }

    #[test]
    fn two_pcs_converge_newest_edit_wins_and_deletions_travel() {
        let mut pc1 = Store::open_in_memory().unwrap();
        let mut pc2 = Store::open_in_memory().unwrap();
        let f = pc1.add_folder(None, "Lavoro").unwrap();
        let mut keep = Entry::new(Some(f), "keep", "k", "v1");
        keep.id = pc1.save(&keep).unwrap();
        let tags = pc1.save(&Entry::new(Some(f), "tagged", "t", "x")).unwrap();
        pc1.set_tags(tags, &["mail"]).unwrap();
        let gone = pc1.save(&Entry::new(None, "gone", "g", "x")).unwrap();

        // PC1 uploads, PC2 (empty) downloads
        let cloud = merge(&pc1.sync_snapshot().unwrap(), &Library::default());
        pc2.apply_sync(&merge(&pc2.sync_snapshot().unwrap(), &cloud)).unwrap();
        assert_eq!(pc2.sync_snapshot().unwrap(), cloud, "PC2 has the whole library");
        assert_eq!(pc2.tags(pc2.entries().unwrap().iter().find(|e| e.name == "tagged").unwrap().id).unwrap(), ["mail"]);
        assert_eq!(pc2.folders().unwrap()[0].name, "Lavoro");

        // PC1 deletes "gone"; later PC2 edits "keep" and adds a phrase
        std::thread::sleep(std::time::Duration::from_millis(5));
        pc1.delete(gone).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut k2 = pc2.entries().unwrap().into_iter().find(|e| e.name == "keep").unwrap();
        k2.body = "v2".into();
        pc2.save(&k2).unwrap();
        pc2.save(&Entry::new(None, "new on pc2", "n", "x")).unwrap();

        let cloud = merge(&pc1.sync_snapshot().unwrap(), &cloud); // PC1 syncs
        pc1.apply_sync(&cloud).unwrap();
        let cloud = merge(&pc2.sync_snapshot().unwrap(), &cloud); // PC2 syncs
        pc2.apply_sync(&cloud).unwrap();
        let cloud = merge(&pc1.sync_snapshot().unwrap(), &cloud); // PC1 again
        pc1.apply_sync(&cloud).unwrap();

        for pc in [&pc1, &pc2] {
            let names: Vec<(String, String)> = pc.entries().unwrap().into_iter().map(|e| (e.name, e.body)).collect();
            assert_eq!(
                names,
                [("keep".into(), "v2".into()), ("new on pc2".into(), "x".into()), ("tagged".into(), "x".into())]
            );
        }
        assert_eq!(pc1.sync_snapshot().unwrap(), pc2.sync_snapshot().unwrap());

        // a deletion older than an edit loses
        let uid = uid_of(&pc1, "tagged");
        let mut lib = pc1.sync_snapshot().unwrap();
        lib.tombstones.push(Tombstone { uid: uid.clone(), deleted_at: EPOCH.into() });
        let merged = merge(&lib, &Library::default());
        assert!(merged.snippets.iter().any(|s| s.uid == uid) && !merged.tombstones.iter().any(|t| t.uid == uid));
    }

    #[test]
    fn folder_deleted_elsewhere_keeps_newer_phrase_at_top_level() {
        let pc1 = Store::open_in_memory().unwrap();
        let mut pc2 = Store::open_in_memory().unwrap();
        let f = pc1.add_folder(None, "F").unwrap();
        pc1.save(&Entry::new(Some(f), "old", "o", "x")).unwrap();
        let cloud = merge(&pc1.sync_snapshot().unwrap(), &Library::default());
        pc2.apply_sync(&cloud).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(5)); // a deletion in the same ms as the edit loses
        pc1.delete_folder(f).unwrap(); // takes "old" with it
        std::thread::sleep(std::time::Duration::from_millis(5));
        let f2 = pc2.folders().unwrap()[0].id;
        pc2.save(&Entry::new(Some(f2), "added later", "a", "x")).unwrap();

        let cloud = merge(&pc1.sync_snapshot().unwrap(), &cloud);
        let cloud = merge(&pc2.sync_snapshot().unwrap(), &cloud);
        pc2.apply_sync(&cloud).unwrap();
        assert!(pc2.folders().unwrap().is_empty());
        let left = pc2.entries().unwrap();
        assert_eq!((left.len(), left[0].name.as_str(), left[0].folder_id), (1, "added later", None));
    }

    #[test]
    fn crossed_folder_moves_never_hide_folders() {
        let mut s = Store::open_in_memory().unwrap();
        s.add_folder(None, "A").unwrap();
        s.add_folder(None, "B").unwrap();
        let mut lib = s.sync_snapshot().unwrap();
        let (ua, ub) = (lib.folders[0].uid.clone(), lib.folders[1].uid.clone());
        lib.folders[0].parent = Some(ub); // PC1 moved A under B, PC2 moved B under A
        lib.folders[1].parent = Some(ua);
        s.apply_sync(&lib).unwrap();
        let roots = s.folders().unwrap().into_iter().filter(|f| f.parent_id.is_none()).count();
        assert_eq!(roots, 1, "one of the two stays at the top level");
    }

    #[test]
    fn pristine_seed_and_v6_on_old_database() {
        let s = Store::open_in_memory().unwrap();
        s.save(&Entry::new(None, "n", "t", "b")).unwrap();
        assert!(!s.sync_snapshot().unwrap().is_pristine());
        s.db.execute_batch(&format!("UPDATE snippet SET updated_at = '{EPOCH}'")).unwrap();
        assert!(s.sync_snapshot().unwrap().is_pristine());

        // a V5 database gets uids and ms timestamps
        let db = rusqlite::Connection::open_in_memory().unwrap();
        for sql in &crate::MIGRATIONS[..5] {
            db.execute_batch(sql).unwrap();
        }
        db.pragma_update(None, "user_version", 5).unwrap();
        db.execute_batch("INSERT INTO folder (id, name) VALUES (1, 'F'); INSERT INTO snippet (folder_id, name, body, updated_at) VALUES (1, 'a', 'x', '2026-11-08 10:00:00');").unwrap();
        let s = Store::init(db).unwrap();
        let lib = s.sync_snapshot().unwrap();
        assert_eq!(lib.snippets[0].updated_at, "2026-11-08 10:00:00.000");
        assert_eq!(lib.snippets[0].folder.as_ref(), Some(&lib.folders[0].uid));
    }
}
