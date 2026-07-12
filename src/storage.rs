// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryItem {
    pub id: i64,
    pub content: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordOutcome {
    Inserted(i64),
    IgnoredBlank,
}

pub struct Storage {
    connection: Connection,
    max_items: usize,
}

impl Storage {
    pub fn open(path: &Path, max_items: usize) -> Result<Self> {
        let connection = Connection::open(path)
            .with_context(|| format!("无法打开历史数据库 {}", path.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 content TEXT NOT NULL,
                 content_hash BLOB NOT NULL UNIQUE,
                 created_at_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS history_created_idx
                 ON history(id DESC);
             CREATE TABLE IF NOT EXISTS settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );",
        )?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("无法设置数据库权限 {}", path.display()))?;

        Ok(Self {
            connection,
            max_items,
        })
    }

    pub fn record(&mut self, content: &str) -> Result<RecordOutcome> {
        if content.trim().is_empty() {
            return Ok(RecordOutcome::IgnoredBlank);
        }

        let hash = blake3::hash(content.as_bytes());
        let timestamp = now_millis();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM history WHERE content_hash = ?1",
            params![hash.as_bytes().as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO history(content, content_hash, created_at_ms)
             VALUES (?1, ?2, ?3)",
            params![content, hash.as_bytes().as_slice(), timestamp],
        )?;
        let id = transaction.last_insert_rowid();
        transaction.execute(
            "DELETE FROM history
             WHERE id NOT IN (
                 SELECT id FROM history ORDER BY id DESC LIMIT ?1
             )",
            params![self.max_items as i64],
        )?;
        transaction.commit()?;
        Ok(RecordOutcome::Inserted(id))
    }

    pub fn list(&self) -> Result<Vec<HistoryItem>> {
        let mut statement = self.connection.prepare(
            "SELECT id, content, created_at_ms
             FROM history
             ORDER BY id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(HistoryItem {
                id: row.get(0)?,
                content: row.get(1)?,
                created_at_ms: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get(&self, id: i64) -> Result<Option<HistoryItem>> {
        self.connection
            .query_row(
                "SELECT id, content, created_at_ms FROM history WHERE id = ?1",
                params![id],
                |row| {
                    Ok(HistoryItem {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        created_at_ms: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        Ok(self
            .connection
            .execute("DELETE FROM history WHERE id = ?1", params![id])?
            > 0)
    }

    pub fn clear(&self) -> Result<usize> {
        let changed = self.connection.execute("DELETE FROM history", [])?;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(changed)
    }

    pub fn count(&self) -> Result<usize> {
        let count = self
            .connection
            .query_row("SELECT COUNT(*) FROM history", [], |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(count as usize)
    }

    pub fn is_paused(&self) -> Result<bool> {
        let value = self
            .connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'paused'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(matches!(value.as_deref(), Some("true")))
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        self.connection.execute(
            "INSERT INTO settings(key, value) VALUES ('paused', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![if paused { "true" } else { "false" }],
        )?;
        Ok(())
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::{RecordOutcome, Storage};

    fn storage(max_items: usize) -> (tempfile::TempDir, Storage) {
        let temp = tempfile::tempdir().unwrap();
        let store = Storage::open(&temp.path().join("history.sqlite3"), max_items).unwrap();
        (temp, store)
    }

    #[test]
    fn records_exact_text_and_ignores_blank_content() {
        let (_temp, mut store) = storage(10);
        assert_eq!(store.record(" \n\t ").unwrap(), RecordOutcome::IgnoredBlank);
        store.record("  first\nline\0  ").unwrap();

        let items = store.list().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].content, "  first\nline\0  ");
    }

    #[test]
    fn duplicate_moves_to_front_and_limit_trims_oldest() {
        let (_temp, mut store) = storage(2);
        store.record("alpha").unwrap();
        store.record("beta").unwrap();
        store.record("alpha").unwrap();
        store.record("gamma").unwrap();

        let contents: Vec<_> = store
            .list()
            .unwrap()
            .into_iter()
            .map(|item| item.content)
            .collect();
        assert_eq!(contents, ["gamma", "alpha"]);
    }

    #[test]
    fn delete_clear_and_pause_are_persistent() {
        let (temp, mut store) = storage(10);
        let id = match store.record("secret").unwrap() {
            RecordOutcome::Inserted(id) => id,
            RecordOutcome::IgnoredBlank => unreachable!(),
        };
        assert!(store.delete(id).unwrap());
        assert!(!store.delete(id).unwrap());
        store.record("one").unwrap();
        store.record("two").unwrap();
        assert_eq!(store.clear().unwrap(), 2);
        store.set_paused(true).unwrap();
        drop(store);

        let reopened = Storage::open(&temp.path().join("history.sqlite3"), 10).unwrap();
        assert!(reopened.is_paused().unwrap());
        assert_eq!(reopened.count().unwrap(), 0);
    }
}
