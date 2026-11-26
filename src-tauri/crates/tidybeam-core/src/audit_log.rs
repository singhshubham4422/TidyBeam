//! Local SQLite audit log of scans, cleanups, shreds, and boosts.

use rusqlite::{Connection, params};
use serde::Serialize;
use std::{
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub timestamp: u64,
    pub action: String,
    pub summary: String,
    pub bytes_reclaimed: u64,
    pub items_count: u64,
}

pub struct AuditLogDb {
    conn: Mutex<Connection>,
}

impl AuditLogDb {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS audit_log (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 timestamp INTEGER NOT NULL,
                 action TEXT NOT NULL,
                 summary TEXT NOT NULL,
                 bytes_reclaimed INTEGER NOT NULL,
                 items_count INTEGER NOT NULL
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn record(&self, action: &str, summary: &str, bytes_reclaimed: u64, items_count: u64) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO audit_log(timestamp, action, summary, bytes_reclaimed, items_count) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![ts, action, summary, bytes_reclaimed, items_count],
            );
        }
    }

    pub fn list(&self, limit: usize) -> Vec<AuditEntry> {
        let Ok(conn) = self.conn.lock() else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, timestamp, action, summary, bytes_reclaimed, items_count FROM audit_log ORDER BY id DESC LIMIT ?1",
        ) else {
            return Vec::new();
        };
        stmt.query_map([limit as i64], |r| {
            Ok(AuditEntry {
                id: r.get(0)?,
                timestamp: r.get(1)?,
                action: r.get(2)?,
                summary: r.get(3)?,
                bytes_reclaimed: r.get(4)?,
                items_count: r.get(5)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_log_records_and_lists() {
        let dir = std::env::temp_dir().join(format!("tidybeam-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = AuditLogDb::open(&dir.join("audit.sqlite3")).unwrap();
        db.record("cleanup", "Cleaned temp files", 4096, 3);
        let items = db.list(10);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].action, "cleanup");
        assert_eq!(items[0].bytes_reclaimed, 4096);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
