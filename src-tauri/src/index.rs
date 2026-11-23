//! Local SQLite/FTS5 name index with persistent roots and live filesystem watchers.

use crate::platform::{self, FileEntry};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

const SCHEMA_VERSION: i64 = 2;
const COMMIT_EVERY: usize = 25_000;

pub struct IndexDb {
    read: Mutex<Connection>,
    write: Mutex<Connection>,
}

#[derive(Default)]
pub struct IndexWatchers {
    active: Mutex<HashMap<PathBuf, RecommendedWatcher>>,
}

#[derive(Serialize)]
pub struct IndexStatus {
    pub root: String,
    pub indexed: u64,
    pub watching: bool,
    pub watch_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRoot {
    pub root: String,
    pub watch: bool,
    pub watching: bool,
    pub indexed: u64,
    pub indexed_at: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn configure(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA busy_timeout=5000;",
    )
}

fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    // The index is a rebuildable cache, so older layouts are dropped rather than migrated.
    connection.execute_batch(&format!(
        "DROP TABLE IF EXISTS files_fts; DROP TABLE IF EXISTS files; DROP TABLE IF EXISTS roots;
        CREATE TABLE files (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            extension TEXT NOT NULL,
            is_dir INTEGER NOT NULL,
            size INTEGER NOT NULL,
            modified INTEGER NOT NULL,
            hidden INTEGER NOT NULL,
            generation INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX idx_files_extension ON files(extension);
        CREATE TABLE roots (root TEXT PRIMARY KEY, watch INTEGER NOT NULL DEFAULT 1, indexed_at INTEGER NOT NULL DEFAULT 0);
        CREATE VIRTUAL TABLE files_fts USING fts5(name, content='files', content_rowid='id', tokenize='unicode61 remove_diacritics 2', prefix='2 3');
        CREATE TRIGGER files_ai AFTER INSERT ON files BEGIN
            INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
        END;
        CREATE TRIGGER files_ad AFTER DELETE ON files BEGIN
            INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
        END;
        CREATE TRIGGER files_au AFTER UPDATE OF name ON files WHEN old.name IS NOT new.name BEGIN
            INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
            INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
        END;
        PRAGMA user_version = {SCHEMA_VERSION};"
    ))
}

/// Bounds for every path inside `root` (and `root` itself) for an indexed range scan.
fn range(root: &str) -> (String, String) {
    let sep = std::path::MAIN_SEPARATOR;
    let prefix = if root.ends_with(['/', '\\']) {
        root.to_string()
    } else {
        format!("{root}{sep}")
    };
    let end = format!("{prefix}\u{10FFFF}");
    (prefix, end)
}

const IN_ROOT: &str = "(path = ?1 OR (path >= ?2 AND path < ?3))";

impl IndexDb {
    pub fn open(file: &Path) -> Result<Self, String> {
        let write = Connection::open(file).map_err(|e| e.to_string())?;
        configure(&write).map_err(|e| e.to_string())?;
        migrate(&write).map_err(|e| e.to_string())?;
        let read = Connection::open(file).map_err(|e| e.to_string())?;
        configure(&read).map_err(|e| e.to_string())?;
        Ok(Self {
            read: Mutex::new(read),
            write: Mutex::new(write),
        })
    }

    fn count(&self, root: &str) -> Result<u64, String> {
        let (prefix, end) = range(root);
        let connection = self.read.lock().map_err(|e| e.to_string())?;
        connection
            .query_row(
                &format!("SELECT COUNT(*) FROM files WHERE {IN_ROOT}"),
                params![root, prefix, end],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
    }

    pub fn roots(&self, watchers: &IndexWatchers) -> Result<Vec<IndexRoot>, String> {
        let rows = {
            let connection = self.read.lock().map_err(|e| e.to_string())?;
            let mut statement = connection
                .prepare("SELECT root, watch, indexed_at FROM roots ORDER BY root")
                .map_err(|e| e.to_string())?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, u64>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        let active = watchers.active.lock().map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|(root, watch, indexed_at)| {
                Ok(IndexRoot {
                    indexed: self.count(&root)?,
                    watching: active.contains_key(Path::new(&root)),
                    root,
                    watch,
                    indexed_at,
                })
            })
            .collect()
    }

    /// Walks `root`, upserting every entry and removing rows that no longer exist.
    pub fn index_root(&self, root: &Path) -> Result<u64, String> {
        let root_text = platform::path_string(root);
        let (prefix, end) = range(&root_text);
        let generation = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(1);
        let mut connection = self.write.lock().map_err(|e| e.to_string())?;
        connection
            .execute("INSERT INTO roots(root, watch, indexed_at) VALUES (?1, 1, 0) ON CONFLICT(root) DO NOTHING", [&root_text])
            .map_err(|e| e.to_string())?;
        let mut count = 0u64;
        let mut pending = 0usize;
        let mut transaction = connection.transaction().map_err(|e| e.to_string())?;
        for item in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let file_type = item.file_type();
            if !file_type.is_file() && !file_type.is_dir() {
                continue;
            }
            let Ok(metadata) = item.metadata() else {
                continue;
            };
            let entry = platform::entry_from_metadata(item.path(), &metadata);
            upsert(&transaction, &entry, generation)?;
            count += 1;
            pending += 1;
            if pending >= COMMIT_EVERY {
                transaction.commit().map_err(|e| e.to_string())?;
                transaction = connection.transaction().map_err(|e| e.to_string())?;
                pending = 0;
            }
        }
        transaction
            .execute(
                &format!("DELETE FROM files WHERE {IN_ROOT} AND generation <> ?4"),
                params![root_text, prefix, end, generation],
            )
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "UPDATE roots SET indexed_at = ?2 WHERE root = ?1",
                params![root_text, now()],
            )
            .map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(count)
    }

    fn apply_changes(&self, root: &Path, changes: &HashMap<PathBuf, bool>) -> Result<(), String> {
        let mut connection = self.write.lock().map_err(|e| e.to_string())?;
        let transaction = connection.transaction().map_err(|e| e.to_string())?;
        for (path, walk) in changes {
            if !path.starts_with(root) {
                continue;
            }
            let path_text = platform::path_string(path);
            let metadata = std::fs::symlink_metadata(path);
            match metadata {
                Ok(meta) if !meta.file_type().is_symlink() => {
                    upsert(&transaction, &platform::entry_from_metadata(path, &meta), 0)?;
                    // Only newly created or renamed folders need their contents walked.
                    if meta.is_dir() && *walk {
                        for item in WalkDir::new(path)
                            .min_depth(1)
                            .follow_links(false)
                            .into_iter()
                            .filter_map(Result::ok)
                        {
                            let file_type = item.file_type();
                            if (file_type.is_file() || file_type.is_dir())
                                && let Ok(meta) = item.metadata()
                            {
                                upsert(
                                    &transaction,
                                    &platform::entry_from_metadata(item.path(), &meta),
                                    0,
                                )?;
                            }
                        }
                    }
                }
                _ => {
                    let (prefix, end) = range(&path_text);
                    transaction
                        .execute(
                            &format!("DELETE FROM files WHERE {IN_ROOT}"),
                            params![path_text, prefix, end],
                        )
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        transaction.commit().map_err(|e| e.to_string())
    }

    pub fn remove_root(&self, root: &str) -> Result<(), String> {
        let (prefix, end) = range(root);
        let mut connection = self.write.lock().map_err(|e| e.to_string())?;
        let transaction = connection.transaction().map_err(|e| e.to_string())?;
        transaction
            .execute("DELETE FROM roots WHERE root = ?1", [root])
            .map_err(|e| e.to_string())?;
        // Keep rows that another, enclosing root still covers.
        let covered: Option<String> = transaction
            .query_row(
                "SELECT root FROM roots WHERE ?1 = root OR substr(?1, 1, length(root)) = root LIMIT 1",
                [root],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if covered.is_none() {
            transaction
                .execute(
                    &format!("DELETE FROM files WHERE {IN_ROOT}"),
                    params![root, prefix, end],
                )
                .map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())
    }

    pub fn set_watch(&self, root: &str, watch: bool) -> Result<(), String> {
        let connection = self.write.lock().map_err(|e| e.to_string())?;
        connection
            .execute(
                "UPDATE roots SET watch = ?2 WHERE root = ?1",
                params![root, watch],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn search(
        &self,
        within: Option<&str>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FileEntry>, String> {
        let mut terms = Vec::new();
        let mut extension = String::new();
        let mut min_size = 0u64;
        let mut modified_after = 0u64;
        let mut kind: Option<bool> = None;
        for token in query.split_whitespace() {
            let lower = token.to_lowercase();
            if let Some(value) = lower.strip_prefix("ext:") {
                extension = value.trim_start_matches('.').into();
            } else if let Some(value) = lower.strip_prefix("size:>") {
                min_size = parse_size(value).unwrap_or(0);
            } else if lower == "modified:today" {
                modified_after = now().saturating_sub(86_400);
            } else if lower == "modified:week" {
                modified_after = now().saturating_sub(7 * 86_400);
            } else if lower == "type:folder" {
                kind = Some(true);
            } else if lower == "type:file" {
                kind = Some(false);
            } else if token.chars().any(char::is_alphanumeric) {
                terms.push(format!("\"{}\"*", token.replace('"', "\"\"")));
            }
        }
        let (root, prefix, end) = match within {
            Some(root) => {
                let (prefix, end) = range(root);
                (Some(root.to_string()), prefix, end)
            }
            None => (None, String::new(), String::new()),
        };
        let filters = "(?1 IS NULL OR f.path = ?1 OR (f.path >= ?2 AND f.path < ?3)) AND (?4 = '' OR f.extension = ?4) AND f.size >= ?5 AND f.modified >= ?6 AND (?7 IS NULL OR f.is_dir = ?7)";
        let limit = limit.clamp(1, 500) as i64;
        let connection = self.read.lock().map_err(|e| e.to_string())?;
        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<FileEntry> {
            let path: String = row.get(1)?;
            Ok(FileEntry {
                name: row.get(0)?,
                is_dir: row.get(2)?,
                size: row.get(3)?,
                modified: row.get(4)?,
                extension: row.get(5)?,
                hidden: row.get(6)?,
                is_symlink: false,
                executable: false,
                path,
            })
        };
        let rows = if terms.is_empty() {
            let sql = format!(
                "SELECT f.name, f.path, f.is_dir, f.size, f.modified, f.extension, f.hidden FROM files f WHERE {filters} ORDER BY f.modified DESC LIMIT ?8"
            );
            let mut statement = connection.prepare_cached(&sql).map_err(|e| e.to_string())?;
            statement
                .query_map(
                    params![
                        root,
                        prefix,
                        end,
                        extension,
                        min_size,
                        modified_after,
                        kind,
                        limit
                    ],
                    map,
                )
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
        } else {
            let sql = format!(
                "SELECT f.name, f.path, f.is_dir, f.size, f.modified, f.extension, f.hidden FROM files_fts s JOIN files f ON f.id = s.rowid WHERE files_fts MATCH ?9 AND {filters} ORDER BY f.is_dir DESC, length(f.name), f.name COLLATE NOCASE LIMIT ?8"
            );
            let mut statement = connection.prepare_cached(&sql).map_err(|e| e.to_string())?;
            statement
                .query_map(
                    params![
                        root,
                        prefix,
                        end,
                        extension,
                        min_size,
                        modified_after,
                        kind,
                        limit,
                        terms.join(" AND ")
                    ],
                    map,
                )
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
        };
        rows.map_err(|e| e.to_string())
    }

    pub fn status(&self, root: &Path, watchers: &IndexWatchers) -> Result<IndexStatus, String> {
        let root_text = platform::path_string(root);
        // A folder inside an indexed root is searchable too.
        Ok(IndexStatus {
            indexed: self.count(&root_text)?,
            watching: watchers.covers(root),
            watch_error: None,
            root: root_text,
        })
    }
}

fn upsert(connection: &Connection, entry: &FileEntry, generation: i64) -> Result<(), String> {
    let mut statement = connection
        .prepare_cached(
            "INSERT INTO files(path, name, extension, is_dir, size, modified, hidden, generation)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(path) DO UPDATE SET name = excluded.name, extension = excluded.extension,
                is_dir = excluded.is_dir, size = excluded.size, modified = excluded.modified,
                hidden = excluded.hidden, generation = excluded.generation",
        )
        .map_err(|e| e.to_string())?;
    statement
        .execute(params![
            entry.path,
            entry.name,
            entry.extension,
            entry.is_dir,
            entry.size,
            entry.modified,
            entry.hidden,
            generation
        ])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn parse_size(input: &str) -> Option<u64> {
    let value = input.trim();
    let split = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let number = value[..split].parse::<f64>().ok()?;
    let multiplier = match value[split..].trim() {
        "" | "b" => 1.0,
        "k" | "kb" | "kib" => 1024.0,
        "m" | "mb" | "mib" => 1024.0_f64.powi(2),
        "g" | "gb" | "gib" => 1024.0_f64.powi(3),
        "t" | "tb" | "tib" => 1024.0_f64.powi(4),
        _ => return None,
    };
    Some((number * multiplier).min(u64::MAX as f64) as u64)
}

impl IndexWatchers {
    pub fn covers(&self, path: &Path) -> bool {
        self.active
            .lock()
            .map(|active| {
                active
                    .keys()
                    .any(|root| platform::same_or_descendant(path, root))
            })
            .unwrap_or(false)
    }

    pub fn stop(&self, root: &Path) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(root);
        }
    }

    pub fn start(&self, db: Arc<IndexDb>, root: PathBuf) -> Result<(), String> {
        if self
            .active
            .lock()
            .map_err(|e| e.to_string())?
            .contains_key(&root)
        {
            return Ok(());
        }
        let (sender, receiver) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })
        .map_err(|e| watch_error_message(&e))?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| watch_error_message(&e))?;
        let thread_root = root.clone();
        std::thread::Builder::new()
            .name("tidybeam-index-watch".into())
            .spawn(move || run_watch(db, thread_root, receiver))
            .map_err(|e| e.to_string())?;
        self.active
            .lock()
            .map_err(|e| e.to_string())?
            .insert(root, watcher);
        Ok(())
    }
}

fn watch_error_message(error: &notify::Error) -> String {
    match error.kind {
        notify::ErrorKind::MaxFilesWatch => {
            if cfg!(target_os = "linux") {
                "This folder has more subfolders than Linux allows watching. Raise fs.inotify.max_user_watches, or index a smaller folder.".into()
            } else {
                "The system limit for watched folders was reached.".into()
            }
        }
        _ => format!("Live updates couldn't start: {error}"),
    }
}

fn run_watch(
    db: Arc<IndexDb>,
    root: PathBuf,
    receiver: mpsc::Receiver<notify::Result<notify::Event>>,
) {
    while let Ok(first) = receiver.recv() {
        let mut changes = HashMap::<PathBuf, bool>::new();
        let mut rescan = false;
        let mut take = |event: notify::Result<notify::Event>,
                        changes: &mut HashMap<PathBuf, bool>| {
            let Ok(event) = event else { return };
            if event.need_rescan() {
                rescan = true;
            }
            let walk = matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Modify(notify::event::ModifyKind::Name(_))
                    | EventKind::Any
                    | EventKind::Other
            );
            for path in event.paths {
                let flag = changes.entry(path).or_insert(false);
                *flag |= walk;
            }
        };
        take(first, &mut changes);
        loop {
            match receiver.recv_timeout(Duration::from_millis(200)) {
                Ok(event) => take(event, &mut changes),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        let result = if rescan {
            db.index_root(&root).map(|_| ())
        } else {
            db.apply_changes(&root, &changes)
        };
        if let Err(error) = result {
            eprintln!("TidyBeam index update failed: {error}");
        }
    }
}

/// Restores watchers and refreshes every saved root in the background after launch.
pub fn restore(db: Arc<IndexDb>, watchers: Arc<IndexWatchers>) {
    let _ = std::thread::Builder::new()
        .name("tidybeam-index-restore".into())
        .spawn(move || {
            let roots = match db.roots(&watchers) {
                Ok(roots) => roots,
                Err(error) => {
                    eprintln!("TidyBeam couldn't read index roots: {error}");
                    return;
                }
            };
            let mut seen = HashSet::new();
            for root in roots {
                let path = PathBuf::from(&root.root);
                if !path.is_dir() || !seen.insert(root.root.clone()) {
                    continue;
                }
                if root.watch
                    && let Err(error) = watchers.start(db.clone(), path.clone())
                {
                    eprintln!("TidyBeam couldn't watch {}: {error}", root.root);
                }
                // Catch up on changes made while the app was closed.
                if let Err(error) = db.index_root(&path) {
                    eprintln!("TidyBeam couldn't refresh {}: {error}", root.root);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_searches_and_sweeps() {
        let dir = std::env::temp_dir().join(format!("tidybeam-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("tree").join("docs")).unwrap();
        std::fs::write(dir.join("tree").join("docs").join("Launch plan.pdf"), b"x").unwrap();
        std::fs::write(dir.join("tree").join("notes.md"), b"y").unwrap();
        let db = IndexDb::open(&dir.join("index.sqlite3")).unwrap();
        let root = platform::canonical(&dir.join("tree")).unwrap();
        assert_eq!(db.index_root(&root).unwrap(), 4);
        let hits = db.search(None, "laun", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Launch plan.pdf");
        assert_eq!(
            db.search(Some(&platform::path_string(&root)), "ext:md", 10)
                .unwrap()
                .len(),
            1
        );
        std::fs::remove_file(root.join("notes.md")).unwrap();
        db.index_root(&root).unwrap();
        assert_eq!(db.search(None, "notes", 10).unwrap().len(), 0);
        db.remove_root(&platform::path_string(&root)).unwrap();
        assert_eq!(db.search(None, "launch", 10).unwrap().len(), 0);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("500mb"), Some(500 * 1024 * 1024));
        assert_eq!(parse_size("1.5g"), Some((1.5 * 1024f64.powi(3)) as u64));
        assert_eq!(parse_size("abc"), None);
    }
}
