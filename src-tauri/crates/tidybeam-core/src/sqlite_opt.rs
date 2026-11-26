//! Safe SQLite optimization (VACUUM + WAL checkpoint) and selective browser
//! cookie/history cleaning using bundled rusqlite.

use rusqlite::{Connection, OpenFlags};
use std::{fs, path::Path};

/// Runs `PRAGMA wal_checkpoint(TRUNCATE)` and `VACUUM` on an unlocked SQLite DB,
/// returning bytes reclaimed.
pub fn vacuum_sqlite(db_path: &Path) -> Result<u64, String> {
    let before = fs::metadata(db_path).map_err(|e| e.to_string())?.len();
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Database is locked or unavailable: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_millis(500))
        .map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA secure_delete = ON; PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
        .map_err(|e| format!("Couldn't optimize database: {e}"))?;
    drop(conn);
    let after = fs::metadata(db_path).map(|m| m.len()).unwrap_or(before);
    Ok(before.saturating_sub(after))
}

/// Deletes cookies from a Chromium/Firefox SQLite cookie DB while preserving
/// any domain matching `whitelist_domains` (e.g. `github.com` matches `.github.com`).
pub fn clean_cookies_with_whitelist(
    db_path: &Path,
    whitelist_domains: &[String],
) -> Result<usize, String> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Cookie DB is locked: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_millis(500))
        .map_err(|e| e.to_string())?;

    let table: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('cookies', 'moz_cookies') LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    let Some(table_name) = table else {
        return Err("Unsupported cookie database schema".into());
    };
    let host_col = if table_name == "cookies" {
        "host_key"
    } else {
        "host"
    };

    let normalized: Vec<String> = whitelist_domains
        .iter()
        .map(|d| d.trim().trim_start_matches('.').to_lowercase())
        .filter(|d| !d.is_empty())
        .collect();

    let mut stmt = conn
        .prepare(&format!("SELECT rowid, {host_col} FROM {table_name}"))
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    let mut deleted = 0usize;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let _ = tx.execute("PRAGMA secure_delete = ON", []);
    {
        let mut del = tx
            .prepare(&format!("DELETE FROM {table_name} WHERE rowid = ?1"))
            .map_err(|e| e.to_string())?;
        for (rowid, host) in rows {
            let h = host.trim_start_matches('.').to_lowercase();
            let keep = normalized
                .iter()
                .any(|w| h == *w || h.ends_with(&format!(".{w}")));
            if !keep && del.execute([rowid]).is_ok() {
                deleted += 1;
            }
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    let _ = conn.execute_batch("VACUUM;");
    Ok(deleted)
}

/// Selectively deletes matching URLs from Firefox `places.sqlite` (or all if `domain` is None).
pub fn clean_firefox_history(places_db: &Path, domain: Option<&str>) -> Result<usize, String> {
    let conn = Connection::open_with_flags(
        places_db,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Firefox places.sqlite is locked: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_millis(500))
        .map_err(|e| e.to_string())?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let deleted = if let Some(dom) = domain.map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = format!("%{dom}%");
        let _ = tx.execute(
            "DELETE FROM moz_historyvisits WHERE place_id IN (SELECT id FROM moz_places WHERE url LIKE ?1)",
            [&pattern],
        );
        tx.execute(
            "DELETE FROM moz_places WHERE url LIKE ?1 AND foreign_count = 0",
            [&pattern],
        )
        .map_err(|e| e.to_string())?
    } else {
        let _ = tx.execute("DELETE FROM moz_historyvisits", []);
        tx.execute("DELETE FROM moz_places WHERE foreign_count = 0", [])
            .map_err(|e| e.to_string())?
    };
    tx.commit().map_err(|e| e.to_string())?;
    let _ = conn.execute_batch("VACUUM;");
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_whitelist_preserves_pinned_domains() {
        let dir = std::env::temp_dir().join(format!("tidybeam-sql-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("Cookies");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE cookies (host_key TEXT NOT NULL, name TEXT NOT NULL);
                 INSERT INTO cookies VALUES ('.github.com', 'session');
                 INSERT INTO cookies VALUES ('api.github.com', 'token');
                 INSERT INTO cookies VALUES ('.tracker.example', 'id');",
            )
            .unwrap();
        }
        let deleted = clean_cookies_with_whitelist(&db_path, &["github.com".into()]).unwrap();
        assert_eq!(deleted, 1);
        let conn = Connection::open(&db_path).unwrap();
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM cookies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 2);
        drop(conn);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn vacuum_sqlite_succeeds_on_valid_db() {
        let dir = std::env::temp_dir().join(format!("tidybeam-vac-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.sqlite");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE t (x TEXT); INSERT INTO t VALUES ('abc'); DELETE FROM t;",
            )
            .unwrap();
        }
        assert!(vacuum_sqlite(&db_path).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }
}
