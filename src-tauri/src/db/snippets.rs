use rusqlite::params;
use uuid::Uuid;

use super::Database;

#[derive(serde::Serialize, Clone)]
pub struct Snippet {
    pub id: String,
    pub title: String,
    pub content: String,
    pub group_name: String,
    pub sort_order: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_snippet(row: &rusqlite::Row) -> rusqlite::Result<Snippet> {
    let bytes: Vec<u8> = row.get(0)?;
    Ok(Snippet {
        id: Uuid::from_slice(&bytes)
            .map(|u| u.to_string())
            .unwrap_or_default(),
        title: row.get(1)?,
        content: row.get(2)?,
        group_name: row.get(3)?,
        sort_order: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

impl Database {
    /// 插入新代码片段
    pub fn insert_snippet(
        &self,
        title: &str,
        content: &str,
        group_name: Option<&str>,
    ) -> Result<String, String> {
        let uuid = Uuid::now_v7();
        let id_bytes = uuid.as_bytes().to_vec();
        let now = chrono::Utc::now().timestamp_millis();
        let group = group_name.unwrap_or("");

        // 获取当前分组的最大 sort_order
        let max_order: i32 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order), 0) FROM snippets WHERE group_name = ?1",
                [group],
                |row| row.get(0),
            )
            .unwrap_or(0);

        self.conn
            .execute(
                "INSERT INTO snippets (id, title, content, group_name, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![id_bytes, title, content, group, max_order + 1, now],
            )
            .map_err(|e| e.to_string())?;

        Ok(uuid.to_string())
    }

    /// 查询代码片段列表
    pub fn get_snippets(&self, group_name: Option<&str>) -> Result<Vec<Snippet>, String> {
        let sql = match group_name {
            Some(g) if !g.is_empty() => {
                "SELECT id, title, content, group_name, sort_order, created_at, updated_at
                 FROM snippets WHERE group_name = ?1 ORDER BY sort_order, created_at DESC"
            }
            _ => {
                "SELECT id, title, content, group_name, sort_order, created_at, updated_at
                 FROM snippets ORDER BY group_name, sort_order, created_at DESC"
            }
        };

        let mut stmt = self.conn.prepare(sql).map_err(|e| e.to_string())?;

        let mut snippets = Vec::new();
        if let Some(g) = group_name {
            if !g.is_empty() {
                let mut rows = stmt
                    .query_map(params![g], row_to_snippet)
                    .map_err(|e| e.to_string())?;
                while let Some(row) = rows.next() {
                    snippets.push(row.map_err(|e| e.to_string())?);
                }
                return Ok(snippets);
            }
        }

        let mut rows = stmt
            .query_map([], row_to_snippet)
            .map_err(|e| e.to_string())?;
        while let Some(row) = rows.next() {
            snippets.push(row.map_err(|e| e.to_string())?);
        }
        Ok(snippets)
    }

    /// 更新代码片段
    pub fn update_snippet(
        &self,
        id: &str,
        title: &str,
        content: &str,
        group_name: Option<&str>,
    ) -> Result<(), String> {
        let uuid = Uuid::parse_str(id).map_err(|e| e.to_string())?;
        let id_bytes = uuid.as_bytes().to_vec();
        let now = chrono::Utc::now().timestamp_millis();

        let group = group_name.unwrap_or("");

        self.conn
            .execute(
                "UPDATE snippets SET title = ?1, content = ?2, group_name = ?3, updated_at = ?4 WHERE id = ?5",
                params![title, content, group, now, id_bytes],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// 删除代码片段
    pub fn delete_snippet(&self, id: &str) -> Result<(), String> {
        let uuid = Uuid::parse_str(id).map_err(|e| e.to_string())?;
        let id_bytes = uuid.as_bytes().to_vec();

        self.conn
            .execute("DELETE FROM snippets WHERE id = ?1", params![id_bytes])
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// 重排序代码片段
    pub fn reorder_snippet(&self, id: &str, sort_order: i32) -> Result<(), String> {
        let uuid = Uuid::parse_str(id).map_err(|e| e.to_string())?;
        let id_bytes = uuid.as_bytes().to_vec();
        let now = chrono::Utc::now().timestamp_millis();

        self.conn
            .execute(
                "UPDATE snippets SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
                params![sort_order, now, id_bytes],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }
}
