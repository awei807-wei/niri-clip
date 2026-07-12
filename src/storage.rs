// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::content::{ClipboardContent, ContentFormat, ContentKind};

const MAX_THUMBNAIL_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryItem {
    pub id: i64,
    pub kind: ContentKind,
    pub mime_type: String,
    pub title: String,
    pub search_text: String,
    pub byte_len: usize,
    pub thumbnail: Option<Vec<u8>>,
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
        let mut connection = Connection::open(path)
            .with_context(|| format!("无法打开历史数据库 {}", path.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 content BLOB NOT NULL,
                 content_hash BLOB NOT NULL UNIQUE,
                 kind TEXT NOT NULL DEFAULT 'text',
                 mime_type TEXT NOT NULL DEFAULT 'text/plain;charset=utf-8',
                 title TEXT NOT NULL DEFAULT '',
                 search_text TEXT NOT NULL DEFAULT '',
                 byte_len INTEGER NOT NULL DEFAULT 0,
                 thumbnail BLOB,
                 created_at_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS history_created_idx
                 ON history(id DESC);
             CREATE TABLE IF NOT EXISTS settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );",
        )?;
        migrate_history_schema(&mut connection)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("无法设置数据库权限 {}", path.display()))?;

        Ok(Self {
            connection,
            max_items,
        })
    }

    pub fn record(
        &mut self,
        content: &ClipboardContent,
        thumbnail: Option<&[u8]>,
    ) -> Result<RecordOutcome> {
        if content.is_blank() {
            return Ok(RecordOutcome::IgnoredBlank);
        }

        let hash = content.fingerprint();
        let title = truncate_chars(&content.display_title(), 240);
        let search_text = content.search_text();
        let thumbnail = thumbnail.filter(|bytes| bytes.len() <= MAX_THUMBNAIL_BYTES);
        let timestamp = now_millis();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM history WHERE content_hash = ?1",
            params![hash.as_bytes().as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO history(
                 content, content_hash, kind, mime_type, title, search_text,
                 byte_len, thumbnail, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                content.bytes.as_slice(),
                hash.as_bytes().as_slice(),
                content.kind.as_str(),
                &content.mime_type,
                title,
                search_text,
                content.bytes.len() as i64,
                thumbnail,
                timestamp
            ],
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
            "SELECT id, kind, mime_type, title, search_text, byte_len,
                    thumbnail, created_at_ms
             FROM history
             ORDER BY id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<Vec<u8>>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (id, kind, mime_type, title, search_text, byte_len, thumbnail, created_at_ms) =
                row?;
            Ok(HistoryItem {
                id,
                kind: ContentKind::from_db(&kind)
                    .ok_or_else(|| anyhow!("历史 {id} 的内容种类无效: {kind}"))?,
                mime_type,
                title,
                search_text,
                byte_len: usize::try_from(byte_len.max(0)).unwrap_or(usize::MAX),
                thumbnail,
                created_at_ms,
            })
        })
        .collect()
    }

    /// Loads one full payload on demand; list queries never include this BLOB.
    pub fn get_content(&self, id: i64) -> Result<Option<ClipboardContent>> {
        let stored = self
            .connection
            .query_row(
                "SELECT kind, mime_type, CAST(content AS BLOB)
                 FROM history WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()?;
        stored
            .map(|(kind, mime_type, bytes)| {
                let kind = ContentKind::from_db(&kind)
                    .ok_or_else(|| anyhow!("历史 {id} 的内容种类无效: {kind}"))?;
                ClipboardContent::new(ContentFormat { kind, mime_type }, bytes)
                    .ok_or_else(|| anyhow!("历史 {id} 的内容与 MIME 不一致"))
            })
            .transpose()
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

fn migrate_history_schema(connection: &mut Connection) -> Result<()> {
    let columns = history_columns(connection)?;
    let migrations = [
        (
            "kind",
            "ALTER TABLE history ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'",
        ),
        (
            "mime_type",
            "ALTER TABLE history ADD COLUMN mime_type TEXT NOT NULL DEFAULT 'text/plain;charset=utf-8'",
        ),
        (
            "title",
            "ALTER TABLE history ADD COLUMN title TEXT NOT NULL DEFAULT ''",
        ),
        (
            "search_text",
            "ALTER TABLE history ADD COLUMN search_text TEXT NOT NULL DEFAULT ''",
        ),
        (
            "byte_len",
            "ALTER TABLE history ADD COLUMN byte_len INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "thumbnail",
            "ALTER TABLE history ADD COLUMN thumbnail BLOB",
        ),
    ];
    let legacy = migrations.iter().any(|(name, _)| !columns.contains(*name));
    for (name, statement) in migrations {
        if !columns.contains(name) {
            connection.execute_batch(statement)?;
        }
    }
    connection.execute_batch(
        "UPDATE history
         SET title = CAST(content AS TEXT),
             search_text = CAST(content AS TEXT),
             byte_len = length(CAST(content AS BLOB))
         WHERE kind = 'text' AND (title = '' OR search_text = '' OR byte_len = 0);",
    )?;
    if legacy {
        migrate_legacy_hashes(connection)?;
    }
    Ok(())
}

fn history_columns(connection: &Connection) -> Result<HashSet<String>> {
    let mut statement = connection.prepare("PRAGMA table_info(history)")?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    names
        .collect::<rusqlite::Result<HashSet<_>>>()
        .map_err(Into::into)
}

fn migrate_legacy_hashes(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction()?;
    let records = {
        let mut statement = transaction.prepare(
            "SELECT id, kind, mime_type, CAST(content AS BLOB) FROM history ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (id, _, _, _) in &records {
        transaction.execute(
            "UPDATE history SET content_hash = ?1 WHERE id = ?2",
            params![format!("legacy-{id}").as_bytes(), id],
        )?;
    }
    for (id, kind, mime_type, bytes) in records {
        let kind = ContentKind::from_db(&kind)
            .ok_or_else(|| anyhow!("历史 {id} 的内容种类无效: {kind}"))?;
        let content = ClipboardContent::new(ContentFormat { kind, mime_type }, bytes)
            .ok_or_else(|| anyhow!("历史 {id} 无法迁移为类型化内容"))?;
        let hash = content.fingerprint();
        transaction.execute(
            "UPDATE history SET content_hash = ?1 WHERE id = ?2",
            params![hash.as_bytes().as_slice(), id],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut output = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        output.push('…');
    }
    output
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests;
