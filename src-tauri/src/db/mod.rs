pub mod clips;
pub mod settings;
pub mod snippets;

use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct Database {
    pub conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;

        // 性能优化
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| e.to_string())?;

        let db = Database { conn };
        db.migrate()?;
        Ok(db)
    }

    /// 物理删除已被软删除超过指定天数的 clip 记录
    pub fn cleanup_deleted(&self, days: i64) -> Result<u32, String> {
        let cutoff = Utc::now().timestamp_millis() - days * 86_400_000;

        let count = self
            .conn
            .execute(
                "DELETE FROM clips WHERE is_deleted = 1 AND updated_at < ?1",
                params![cutoff],
            )
            .map_err(|e| e.to_string())?;

        Ok(count as u32)
    }

    fn migrate(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS clips (
                    id BLOB PRIMARY KEY,
                    content_text TEXT,
                    content_rtf TEXT,
                    content_html TEXT,
                    content_image BLOB,
                    content_type TEXT NOT NULL,
                    source_app TEXT,
                    is_pinned INTEGER DEFAULT 0,
                    is_deleted INTEGER DEFAULT 0,
                    device_id TEXT NOT NULL DEFAULT '',
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    version INTEGER DEFAULT 1
                );

                CREATE INDEX IF NOT EXISTS idx_clips_created ON clips(id DESC);
                CREATE INDEX IF NOT EXISTS idx_clips_pinned ON clips(is_pinned, id DESC);
                CREATE INDEX IF NOT EXISTS idx_clips_updated ON clips(updated_at);
                CREATE INDEX IF NOT EXISTS idx_clips_deleted ON clips(is_deleted);

                CREATE TABLE IF NOT EXISTS snippets (
                    id BLOB PRIMARY KEY,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    group_name TEXT DEFAULT '',
                    sort_order INTEGER DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_snippets_sort ON snippets(group_name, sort_order);

                CREATE TABLE IF NOT EXISTS sync_meta (
                    key TEXT PRIMARY KEY,
                    value TEXT
                );
                ",
            )
            .map_err(|e| e.to_string())?;

        // FTS5 全文搜索（需要单独处理，因为 IF NOT EXISTS 语法不同）
        let has_fts: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='clips_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !has_fts {
            self.conn
                .execute_batch(
                    "
                    CREATE VIRTUAL TABLE clips_fts USING fts5(
                        content_text,
                        content='clips',
                        content_rowid='rowid'
                    );

                    CREATE TRIGGER clips_fts_insert AFTER INSERT ON clips BEGIN
                        INSERT INTO clips_fts(rowid, content_text) VALUES (new.rowid, new.content_text);
                    END;

                    CREATE TRIGGER clips_fts_delete AFTER DELETE ON clips BEGIN
                        INSERT INTO clips_fts(clips_fts, rowid, content_text)
                        VALUES('delete', old.rowid, old.content_text);
                    END;

                    CREATE TRIGGER clips_fts_update AFTER UPDATE ON clips BEGIN
                        INSERT INTO clips_fts(clips_fts, rowid, content_text)
                        VALUES('delete', old.rowid, old.content_text);
                        INSERT INTO clips_fts(rowid, content_text) VALUES (new.rowid, new.content_text);
                    END;
                    ",
                )
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}
