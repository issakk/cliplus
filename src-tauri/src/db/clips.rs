use rusqlite::params;
use uuid::Uuid;

use super::Database;

#[derive(serde::Serialize, Clone)]
pub struct Clip {
    pub id: String,
    pub content_text: Option<String>,
    pub content_type: String,
    pub source_app: Option<String>,
    pub is_pinned: i32,
    pub created_at: i64,
}

fn row_to_clip(row: &rusqlite::Row) -> rusqlite::Result<Clip> {
    let bytes: Vec<u8> = row.get(0)?;
    Ok(Clip {
        id: Uuid::from_slice(&bytes)
            .map(|u| u.to_string())
            .unwrap_or_default(),
        content_text: row.get(1)?,
        content_type: row.get(2)?,
        source_app: row.get(3)?,
        is_pinned: row.get(4)?,
        created_at: row.get(5)?,
    })
}

impl Database {
    /// 插入新剪切板条目
    pub fn insert_clip(
        &self,
        content_text: Option<&str>,
        content_rtf: Option<&str>,
        content_html: Option<&str>,
        content_image: Option<&[u8]>,
        content_type: &str,
        source_app: Option<&str>,
    ) -> Result<String, String> {
        let uuid = Uuid::now_v7();
        let id_bytes = uuid.as_bytes().to_vec();
        let now = chrono::Utc::now().timestamp_millis();

        // 去重：检查最近一条是否内容相同
        if let Some(text) = content_text {
            let dup: bool = self
                .conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM clips
                     WHERE content_text = ?1 AND content_type = 'text' AND is_deleted = 0
                     ORDER BY id DESC LIMIT 1",
                    [text],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if dup {
                return Ok(uuid.to_string());
            }
        }

        self.conn
            .execute(
                "INSERT INTO clips (id, content_text, content_rtf, content_html, content_image, content_type, source_app, device_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '', ?8, ?8)",
                params![
                    id_bytes,
                    content_text,
                    content_rtf,
                    content_html,
                    content_image,
                    content_type,
                    source_app,
                    now,
                ],
            )
            .map_err(|e| e.to_string())?;

        Ok(uuid.to_string())
    }

    /// 查询剪切板列表
    pub fn get_clips(&self, query: Option<&str>, limit: i64) -> Result<Vec<Clip>, String> {
        let (sql, has_query): (String, bool) = match query {
            Some(q) if !q.is_empty() => (
                format!(
                    "SELECT id, content_text, content_type, source_app, is_pinned, created_at
                     FROM clips
                     WHERE is_deleted = 0 AND content_text LIKE '%' || ?1 || '%'
                     ORDER BY is_pinned DESC, id DESC
                     LIMIT {limit}"
                ),
                true,
            ),
            _ => (
                format!(
                    "SELECT id, content_text, content_type, source_app, is_pinned, created_at
                     FROM clips
                     WHERE is_deleted = 0
                     ORDER BY is_pinned DESC, id DESC
                     LIMIT {limit}"
                ),
                false,
            ),
        };

        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;

        let mut clips = Vec::new();
        if has_query {
            let mut rows = stmt
                .query_map(params![query.unwrap()], row_to_clip)
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next() {
                clips.push(row.map_err(|e| e.to_string())?);
            }
        } else {
            let mut rows = stmt
                .query_map([], row_to_clip)
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next() {
                clips.push(row.map_err(|e| e.to_string())?);
            }
        }
        Ok(clips)
    }

    /// 获取单条剪切板的文本内容
    pub fn get_clip_text(&self, id: &str) -> Result<Option<String>, String> {
        let uuid = Uuid::parse_str(id).map_err(|e| e.to_string())?;
        let id_bytes = uuid.as_bytes().to_vec();

        self.conn
            .query_row(
                "SELECT content_text FROM clips WHERE id = ?1",
                params![id_bytes],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
    }

    /// 删除剪切板条目（软删除）
    pub fn delete_clip(&self, id: &str) -> Result<(), String> {
        let uuid = Uuid::parse_str(id).map_err(|e| e.to_string())?;
        let id_bytes = uuid.as_bytes().to_vec();
        let now = chrono::Utc::now().timestamp_millis();

        self.conn
            .execute(
                "UPDATE clips SET is_deleted = 1, updated_at = ?1 WHERE id = ?2",
                params![now, id_bytes],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// 切换置顶状态
    pub fn toggle_pin(&self, id: &str) -> Result<(), String> {
        let uuid = Uuid::parse_str(id).map_err(|e| e.to_string())?;
        let id_bytes = uuid.as_bytes().to_vec();
        let now = chrono::Utc::now().timestamp_millis();

        self.conn
            .execute(
                "UPDATE clips SET is_pinned = 1 - is_pinned, updated_at = ?1, version = version + 1 WHERE id = ?2",
                params![now, id_bytes],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }
}
