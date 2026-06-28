use base64::Engine;
use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

use super::types::{SyncEntry, SyncOp};
use crate::db::Database;

/// UUID 字节 -> base64 编码的短 ID
fn uuid_to_b64(uuid: &Uuid) -> String {
    base64::engine::general_purpose::STANDARD.encode(uuid.as_bytes())
}

/// base64 编码的 ID -> UUID 字节
fn b64_to_uuid_bytes(b64: &str) -> Result<Vec<u8>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())?;
    if bytes.len() != 16 {
        return Err("invalid uuid base64 length".into());
    }
    Ok(bytes)
}

impl Database {
    /// 导出自上次同步以来变更的条目
    pub fn export_entries(&self, since_ts: i64) -> Result<Vec<SyncEntry>, String> {
        let mut entries = Vec::new();

        // ---- clips ----
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, content_text, content_type, source_app, is_pinned, is_deleted,
                            created_at, updated_at
                     FROM clips WHERE updated_at > ?1",
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map(params![since_ts], |row| {
                    let id_bytes: Vec<u8> = row.get(0)?;
                    let content_text: Option<String> = row.get(1)?;
                    let content_type: String = row.get(2)?;
                    let source_app: Option<String> = row.get(3)?;
                    let is_pinned: i32 = row.get(4)?;
                    let is_deleted: i32 = row.get(5)?;
                    let created_at: i64 = row.get(6)?;
                    let updated_at: i64 = row.get(7)?;
                    Ok((
                        id_bytes,
                        content_text,
                        content_type,
                        source_app,
                        is_pinned,
                        is_deleted,
                        created_at,
                        updated_at,
                    ))
                })
                .map_err(|e| e.to_string())?;

            for row in rows {
                let (id_bytes, content_text, content_type, source_app, is_pinned, is_deleted, created_at, updated_at) =
                    row.map_err(|e| e.to_string())?;

                let uuid = Uuid::from_slice(&id_bytes).map_err(|e| e.to_string())?;
                let b64_id = uuid_to_b64(&uuid);

                let op = if is_deleted == 1 {
                    SyncOp::Delete
                } else {
                    SyncOp::Upsert
                };

                let data = if op == SyncOp::Upsert {
                    Some(serde_json::json!({
                        "content_text": content_text,
                        "content_type": content_type,
                        "source_app": source_app,
                        "is_pinned": is_pinned,
                        "is_deleted": is_deleted,
                        "created_at": created_at,
                    }))
                } else {
                    None
                };

                entries.push(SyncEntry {
                    op,
                    id: b64_id,
                    entry_type: "clip".into(),
                    ts: updated_at,
                    device: String::new(), // 调用方填充
                    data,
                });
            }
        }

        // ---- snippets ----
        {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, title, content, group_name, sort_order, created_at, updated_at
                     FROM snippets WHERE updated_at > ?1",
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map(params![since_ts], |row| {
                    let id_bytes: Vec<u8> = row.get(0)?;
                    let title: String = row.get(1)?;
                    let content: String = row.get(2)?;
                    let group_name: String = row.get(3)?;
                    let sort_order: i32 = row.get(4)?;
                    let created_at: i64 = row.get(5)?;
                    let updated_at: i64 = row.get(6)?;
                    Ok((
                        id_bytes, title, content, group_name, sort_order, created_at, updated_at,
                    ))
                })
                .map_err(|e| e.to_string())?;

            for row in rows {
                let (id_bytes, title, content, group_name, sort_order, created_at, updated_at) =
                    row.map_err(|e| e.to_string())?;

                let uuid = Uuid::from_slice(&id_bytes).map_err(|e| e.to_string())?;
                let b64_id = uuid_to_b64(&uuid);

                entries.push(SyncEntry {
                    op: SyncOp::Upsert,
                    id: b64_id,
                    entry_type: "snippet".into(),
                    ts: updated_at,
                    device: String::new(),
                    data: Some(serde_json::json!({
                        "title": title,
                        "content": content,
                        "group_name": group_name,
                        "sort_order": sort_order,
                        "created_at": created_at,
                    })),
                });
            }
        }

        Ok(entries)
    }

    /// 导入远程同步条目，返回 (合并数量, 错误列表)
    pub fn import_entries(
        &self,
        entries: &[SyncEntry],
        device_name: &str,
    ) -> Result<(u32, Vec<String>), String> {
        let mut merged: u32 = 0;
        let mut errors: Vec<String> = Vec::new();

        for entry in entries {
            let result = self.import_single_entry(entry, device_name);
            match result {
                Ok(true) => merged += 1,
                Ok(false) => { /* 无变化，跳过 */ }
                Err(e) => errors.push(format!("{}: {}", entry.id, e)),
            }
        }

        Ok((merged, errors))
    }

    /// 导入单条同步条目，返回 true 表示发生了合并
    fn import_single_entry(&self, entry: &SyncEntry, device_name: &str) -> Result<bool, String> {
        let id_bytes = b64_to_uuid_bytes(&entry.id)?;

        match entry.entry_type.as_str() {
            "clip" => self.import_clip_entry(entry, &id_bytes, device_name),
            "snippet" => self.import_snippet_entry(entry, &id_bytes, device_name),
            _ => Err(format!("unknown entry type: {}", entry.entry_type)),
        }
    }

    fn import_clip_entry(
        &self,
        entry: &SyncEntry,
        id_bytes: &[u8],
        device_name: &str,
    ) -> Result<bool, String> {
        // 查询本地是否存在
        let local_updated_at: Option<i64> = self
            .conn
            .query_row(
                "SELECT updated_at FROM clips WHERE id = ?1",
                params![id_bytes],
                |row| row.get(0),
            )
            .ok();

        match (&entry.op, local_updated_at) {
            // Upsert + 本地不存在 -> INSERT
            (SyncOp::Upsert, None) => {
                let data = entry
                    .data
                    .as_ref()
                    .ok_or("upsert entry missing data")?;

                let content_text = data["content_text"].as_str();
                let content_type = data["content_type"]
                    .as_str()
                    .unwrap_or("text");
                let source_app = data["source_app"].as_str();
                let is_pinned = data["is_pinned"].as_i64().unwrap_or(0) as i32;
                let is_deleted = data["is_deleted"].as_i64().unwrap_or(0) as i32;
                let created_at = data["created_at"].as_i64().unwrap_or(entry.ts);

                self.conn
                    .execute(
                        "INSERT INTO clips (id, content_text, content_type, source_app, is_pinned, is_deleted, device_id, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            id_bytes,
                            content_text,
                            content_type,
                            source_app,
                            is_pinned,
                            is_deleted,
                            device_name,
                            created_at,
                            entry.ts,
                        ],
                    )
                    .map_err(|e| e.to_string())?;

                Ok(true)
            }

            // Upsert + 本地已存在 + remote 更新 -> UPDATE（不更新 image 字段）
            (SyncOp::Upsert, Some(local_ts)) if entry.ts > local_ts => {
                let data = entry
                    .data
                    .as_ref()
                    .ok_or("upsert entry missing data")?;

                let content_text = data["content_text"].as_str();
                let content_type = data["content_type"]
                    .as_str()
                    .unwrap_or("text");
                let source_app = data["source_app"].as_str();
                let is_pinned = data["is_pinned"].as_i64().unwrap_or(0) as i32;
                let is_deleted = data["is_deleted"].as_i64().unwrap_or(0) as i32;

                self.conn
                    .execute(
                        "UPDATE clips SET content_text = ?1, content_type = ?2, source_app = ?3,
                                is_pinned = ?4, is_deleted = ?5, updated_at = ?6
                         WHERE id = ?7",
                        params![
                            content_text,
                            content_type,
                            source_app,
                            is_pinned,
                            is_deleted,
                            entry.ts,
                            id_bytes,
                        ],
                    )
                    .map_err(|e| e.to_string())?;

                Ok(true)
            }

            // Delete + 本地已存在 -> 软删除
            (SyncOp::Delete, Some(_)) => {
                self.conn
                    .execute(
                        "UPDATE clips SET is_deleted = 1, updated_at = ?1 WHERE id = ?2",
                        params![entry.ts, id_bytes],
                    )
                    .map_err(|e| e.to_string())?;

                Ok(true)
            }

            // Delete + 本地不存在 -> 忽略
            (SyncOp::Delete, None) => Ok(false),

            // Upsert + 本地已存在 + 本地更新 -> 忽略（本地更新）
            (SyncOp::Upsert, Some(_)) => Ok(false),
        }
    }

    fn import_snippet_entry(
        &self,
        entry: &SyncEntry,
        id_bytes: &[u8],
        _device_name: &str,
    ) -> Result<bool, String> {
        let local_updated_at: Option<i64> = self
            .conn
            .query_row(
                "SELECT updated_at FROM snippets WHERE id = ?1",
                params![id_bytes],
                |row| row.get(0),
            )
            .ok();

        match (&entry.op, local_updated_at) {
            // Upsert + 本地不存在 -> INSERT
            (SyncOp::Upsert, None) => {
                let data = entry
                    .data
                    .as_ref()
                    .ok_or("upsert entry missing data")?;

                let title = data["title"].as_str().unwrap_or("");
                let content = data["content"].as_str().unwrap_or("");
                let group_name = data["group_name"].as_str().unwrap_or("");
                let sort_order = data["sort_order"].as_i64().unwrap_or(0) as i32;
                let created_at = data["created_at"].as_i64().unwrap_or(entry.ts);

                self.conn
                    .execute(
                        "INSERT INTO snippets (id, title, content, group_name, sort_order, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            id_bytes, title, content, group_name, sort_order, created_at, entry.ts,
                        ],
                    )
                    .map_err(|e| e.to_string())?;

                Ok(true)
            }

            // Upsert + 本地已存在 + remote 更新 -> UPDATE
            (SyncOp::Upsert, Some(local_ts)) if entry.ts > local_ts => {
                let data = entry
                    .data
                    .as_ref()
                    .ok_or("upsert entry missing data")?;

                let title = data["title"].as_str().unwrap_or("");
                let content = data["content"].as_str().unwrap_or("");
                let group_name = data["group_name"].as_str().unwrap_or("");
                let sort_order = data["sort_order"].as_i64().unwrap_or(0) as i32;

                self.conn
                    .execute(
                        "UPDATE snippets SET title = ?1, content = ?2, group_name = ?3,
                                sort_order = ?4, updated_at = ?5
                         WHERE id = ?6",
                        params![title, content, group_name, sort_order, entry.ts, id_bytes],
                    )
                    .map_err(|e| e.to_string())?;

                Ok(true)
            }

            // Delete + 本地已存在 -> 物理删除
            (SyncOp::Delete, Some(_)) => {
                self.conn
                    .execute("DELETE FROM snippets WHERE id = ?1", params![id_bytes])
                    .map_err(|e| e.to_string())?;

                Ok(true)
            }

            // Delete + 本地不存在 -> 忽略
            (SyncOp::Delete, None) => Ok(false),

            // Upsert + 本地已存在 + 本地更新 -> 忽略
            (SyncOp::Upsert, Some(_)) => Ok(false),
        }
    }

    /// 将同步条目序列化为 JSON-lines 格式字符串
    pub fn generate_sync_file(entries: &[SyncEntry]) -> Result<String, String> {
        let mut lines = Vec::with_capacity(entries.len());
        for entry in entries {
            let line =
                serde_json::to_string(entry).map_err(|e| e.to_string())?;
            lines.push(line);
        }
        Ok(lines.join("\n"))
    }

    /// 解析 JSON-lines 格式内容为同步条目列表
    pub fn parse_sync_file(content: &str) -> Result<Vec<SyncEntry>, String> {
        let mut entries = Vec::new();
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: SyncEntry =
                serde_json::from_str(trimmed).map_err(|e| format!("line {}: {}", i + 1, e))?;
            entries.push(entry);
        }
        Ok(entries)
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
}
