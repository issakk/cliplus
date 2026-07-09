//! 同步：本地库 + 镜像库 + LWW 合并。
//!
//! 每台机器持有一份本地工作库（app_data/clipsync.db，可靠、开 WAL）。
//! 用户可选一个云同步目录，其中放置镜像库 clipsync.db。
//!
//! - 启动：`merge_from(mirror)` 把镜像里较新的行并入本地。
//! - 写入后 / 退出 / 定时：`export_to(mirror)` 先防御性合并，再 checkpoint +
//!   整文件覆盖镜像，并删除镜像的 -wal/-shm（避免云盘只同步主文件）。
//!
//! LWW：按 `updated_at` 取大者；软删除以 `is_deleted=1` 行传播。
//! `device_id` 逐行记录来源，便于人肉排查；不参与合并判定。

use std::path::Path;

use super::Database;

#[derive(Debug, Default, Clone)]
pub struct MergeStats {
    pub clips: usize,
    pub snippets: usize,
}

/// clips 合并第一步：导入 remote 中本地不存在的行。
/// 用 INSERT OR IGNORE，主键冲突的行跳过（由第二步 UPDATE 处理）。
const CLIPS_MERGE_INSERT: &str = "
INSERT OR IGNORE INTO clips (
    id, content_text, content_rtf, content_html, content_image,
    content_type, source_app, is_pinned, is_deleted, device_id,
    created_at, updated_at, version
)
SELECT
    id, content_text, content_rtf, content_html, content_image,
    content_type, source_app, is_pinned, is_deleted, device_id,
    created_at, updated_at, version
FROM remote.clips;
";

/// clips 合并第二步：用 remote 中 updated_at 更新的行覆盖本地（LWW）。
/// 用 UPDATE...FROM join，避免标量子查询在 ATTACH 库上的相关性行为问题。
const CLIPS_MERGE_UPDATE: &str = "
UPDATE clips SET
    content_text  = r.content_text,
    content_rtf   = r.content_rtf,
    content_html  = r.content_html,
    content_image = r.content_image,
    content_type  = r.content_type,
    source_app    = r.source_app,
    is_pinned     = r.is_pinned,
    is_deleted    = r.is_deleted,
    device_id     = r.device_id,
    created_at    = r.created_at,
    updated_at    = r.updated_at,
    version       = r.version
FROM remote.clips AS r
WHERE clips.id = r.id AND r.updated_at > clips.updated_at;
";

/// snippets 合并第一步：导入 remote 中本地不存在的行。
const SNIPPETS_MERGE_INSERT: &str = "
INSERT OR IGNORE INTO snippets (
    id, title, content, group_name, sort_order, created_at, updated_at
)
SELECT
    id, title, content, group_name, sort_order, created_at, updated_at
FROM remote.snippets;
";

/// snippets 合并第二步：用 remote 中 updated_at 更新的行覆盖本地（LWW）。
const SNIPPETS_MERGE_UPDATE: &str = "
UPDATE snippets SET
    title      = r.title,
    content    = r.content,
    group_name = r.group_name,
    sort_order = r.sort_order,
    created_at = r.created_at,
    updated_at = r.updated_at
FROM remote.snippets AS r
WHERE snippets.id = r.id AND r.updated_at > snippets.updated_at;
";

impl Database {
    /// 把 `mirror_path` 数据库合并进本地（LWW）。
    /// mirror 不存在或为空则无操作。`sync_meta`（设置项）不参与合并，保持本地。
    pub fn merge_from(&self, mirror_path: &Path) -> Result<MergeStats, String> {
        if !mirror_path.exists() {
            return Ok(MergeStats::default());
        }
        // 先把本地 WAL 落盘，避免与 ATTACH 交叉时的状态不一致。
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| e.to_string())?;

        let path_str = mirror_path
            .to_str()
            .ok_or("镜像数据库路径含非 UTF-8 字符")?;
        // 单引号转义
        let escaped = path_str.replace('\'', "''");
        self.conn
            .execute(&format!("ATTACH DATABASE '{}' AS remote", escaped), [])
            .map_err(|e| format!("ATTACH 镜像失败: {}", e))?;

        let result = (|| -> Result<MergeStats, String> {
            // 跨库事务用 unchecked_transaction：rusqlite 的 transaction() 在 ATTACH 后
            // 的 busy 重试语义不适用，这里无需它。
            let tx = self
                .conn
                .unchecked_transaction()
                .map_err(|e| e.to_string())?;

            // 探测 remote 库 schema 是否与本地兼容：旧版本镜像库（缺 version 等列）
            // 会导致两步合并列数不匹配。schema 不兼容时返回明确错误，避免静默
            // 跳过后被 export_to 整文件覆盖而丢失镜像数据。
            let cols: Vec<String> = tx
                .prepare(
                    "SELECT name FROM pragma_table_info('clips', 'remote') ORDER BY cid",
                )
                .map_err(|e| e.to_string())?
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .filter_map(|c| c.ok())
                .collect();
            let need = ["id", "content_text", "content_type", "is_pinned",
                        "is_deleted", "device_id", "created_at", "updated_at",
                        "version"];
            let clips = if cols.is_empty() {
                0
            } else {
                for c in &need {
                    if !cols.iter().any(|rc| rc == c) {
                        return Err(format!("镜像库 clips 表缺少列 `{}`，请删除旧镜像库后重新同步", c));
                    }
                }
                let inserted = tx
                    .execute(CLIPS_MERGE_INSERT, [])
                    .map_err(|e| format!("clips 合并失败（导入）: {}", e))?;
                let updated = tx
                    .execute(CLIPS_MERGE_UPDATE, [])
                    .map_err(|e| format!("clips 合并失败（更新）: {}", e))?;
                inserted + updated
            };

            let cols_s: Vec<String> = tx
                .prepare(
                    "SELECT name FROM pragma_table_info('snippets', 'remote') ORDER BY cid",
                )
                .map_err(|e| e.to_string())?
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .filter_map(|c| c.ok())
                .collect();
            let need_s = ["id", "title", "content", "group_name", "sort_order",
                          "created_at", "updated_at"];
            let snippets = if cols_s.is_empty() {
                0
            } else {
                for c in &need_s {
                    if !cols_s.iter().any(|rc| rc == c) {
                        return Err(format!("镜像库 snippets 表缺少列 `{}`，请删除旧镜像库后重新同步", c));
                    }
                }
                let inserted = tx
                    .execute(SNIPPETS_MERGE_INSERT, [])
                    .map_err(|e| format!("snippets 合并失败（导入）: {}", e))?;
                let updated = tx
                    .execute(SNIPPETS_MERGE_UPDATE, [])
                    .map_err(|e| format!("snippets 合并失败（更新）: {}", e))?;
                inserted + updated
            };
            tx.commit().map_err(|e| e.to_string())?;
            Ok(MergeStats { clips, snippets })
        })();

        // 无论合并成功与否都要 DETACH，避免连接残留
        let _ = self.conn.execute("DETACH DATABASE remote", []);
        result
    }

    /// 导出本地库到 `mirror_path`。
    /// 步骤：防御性 merge_from（吸纳镜像里尚未并入本地的写入）→ checkpoint →
    /// 整文件覆盖 → 删除镜像的 -wal/-shm。
    pub fn export_to(&self, mirror_path: &Path) -> Result<(), String> {
        // 1. 防御性合并：若镜像有本地没有的新行（例如上次导出后别的设备写过，
        //    而本设备尚未启动合并），先并入本地，避免整文件覆盖丢失。
        let _ = self.merge_from(mirror_path);

        // 2. checkpoint，把 WAL 合进主文件，保证复制出的是完整快照
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| e.to_string())?;

        // 3. 整文件覆盖
        let local_path = self
            .conn
            .path()
            .ok_or("无法获取本地数据库路径")?;
        std::fs::create_dir_all(mirror_path.parent().unwrap_or(Path::new(".")))
            .map_err(|e| e.to_string())?;
        std::fs::copy(local_path, mirror_path)
            .map_err(|e| format!("导出（复制）失败: {}", e))?;

        // 4. 清除镜像可能残留的 -wal/-shm，云盘客户端对这类临时文件同步行为不一致
        for suffix in &["-wal", "-shm"] {
            let p = format!("{}{}", mirror_path.display(), suffix);
            let _ = std::fs::remove_file(&p);
        }
        Ok(())
    }

    /// 一次完整同步：合并镜像入本地，再把本地导出回镜像。
    /// 导出本身已含防御性合并，这里显式 merge 一次以便返回 stats 并保证本地最新。
    pub fn sync_with(&self, mirror_path: &Path) -> Result<MergeStats, String> {
        let stats = self.merge_from(mirror_path)?;
        self.export_to(mirror_path)?;
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tempfile::tempdir;

    fn open(tmp: &Path, name: &str) -> Database {
        Database::open(&tmp.join(name)).unwrap()
    }

    #[test]
    fn merge_imports_new_and_newer_rows() {
        let tmp = tempdir().unwrap();
        let local = open(tmp.path(), "local.db");
        let a = open(tmp.path(), "a.db");

        // a 写入一条
        let id = a.insert_clip(Some("hello"), None, None, None, "text", Some("app"), "A").unwrap();
        // 本地不存在该 id → merge 应导入
        let stats = local.merge_from(&tmp.path().join("a.db")).unwrap();
        assert_eq!(stats.clips, 1);
        let text = local.get_clip_text(&id).unwrap();
        assert_eq!(text.as_deref(), Some("hello"));

        // 本地修改该行（更新 updated_at）
        std::thread::sleep(std::time::Duration::from_millis(5));
        // 用 toggle_pin 触发 updated_at 推进
        local.toggle_pin(&id).unwrap();
        // a 也修改同一行，但更晚
        std::thread::sleep(std::time::Duration::from_millis(5));
        // a 直接改 content：用一条新插入再删旧的简单方式不易，这里用 toggle_pin 推进 a 的 updated_at
        a.toggle_pin(&id).unwrap();
        // 再 merge：a 的 updated_at 更大 → 覆盖本地
        let stats2 = local.merge_from(&tmp.path().join("a.db")).unwrap();
        assert!(stats2.clips >= 1, "应更新该行");
    }

    #[test]
    fn merge_propagates_soft_delete() {
        let tmp = tempdir().unwrap();
        let local = open(tmp.path(), "local.db");
        let a = open(tmp.path(), "a.db");

        let id = local
            .insert_clip(Some("to-delete"), None, None, None, "text", None, "L")
            .unwrap();
        // a 也有一条同 id？不，a 需要拿到该 id 才能 delete。
        // 先从 local 导出到 a，再在 a 删，再 merge 回。
        local.export_to(&tmp.path().join("a.db")).unwrap();
        let a = open(tmp.path(), "a.db");
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.delete_clip(&id).unwrap();
        // merge a → local，应把 is_deleted 传播
        let stats = local.merge_from(&tmp.path().join("a.db")).unwrap();
        assert!(stats.clips >= 1);
        // get_clips 不返回已删除项
        let clips = local.get_clips(None, 100).unwrap();
        assert!(!clips.iter().any(|c| c.id == id));
    }

    #[test]
    fn export_then_merge_roundtrip() {
        let tmp = tempdir().unwrap();
        let a = open(tmp.path(), "a.db");
        let _id1 = a.insert_clip(Some("x1"), None, None, None, "text", None, "A").unwrap();
        a.export_to(&tmp.path().join("mirror.db")).unwrap();

        let b = open(tmp.path(), "b.db");
        let stats = b.merge_from(&tmp.path().join("mirror.db")).unwrap();
        assert_eq!(stats.clips, 1);
        // b 现在有 x1
        let clips = b.get_clips(None, 100).unwrap();
        assert_eq!(clips.len(), 1);
    }

    #[test]
    fn merge_missing_mirror_is_noop() {
        let tmp = tempdir().unwrap();
        let local = open(tmp.path(), "local.db");
        let stats = local.merge_from(&tmp.path().join("nope.db")).unwrap();
        assert_eq!(stats.clips, 0);
        assert_eq!(stats.snippets, 0);
    }
}