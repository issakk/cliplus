use serde::{Deserialize, Serialize};

/// 同步操作类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SyncOp {
    Upsert,
    Delete,
}

/// 同步条目（JSON-lines 格式的每一行）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEntry {
    pub op: SyncOp,
    /// 条目 ID (UUIDv7 的 base64)
    pub id: String,
    /// 条目类型: "clip" 或 "snippet"
    #[serde(rename = "type")]
    pub entry_type: String,
    /// 操作时间戳 (ms)
    pub ts: i64,
    /// 设备名称
    pub device: String,
    /// 条目数据（Upsert 时有值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// 同步配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// 同步后端: "onedrive" 或 "webdav"
    pub backend: SyncBackend,
    /// 设备名称
    pub device_name: String,
    /// 上次同步时间戳
    pub last_sync_ts: i64,
    /// 上次同步文件 hash
    pub last_sync_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SyncBackend {
    OneDrive,
    WebDAV,
    None,
}

/// OneDrive OAuth 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneDriveConfig {
    pub client_id: String,
    pub refresh_token: String,
    pub access_token: String,
    pub token_expires_at: i64,
}

/// WebDAV 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDAVConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

/// 同步结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub pushed: u32,
    pub pulled: u32,
    pub merged: u32,
    pub errors: Vec<String>,
}
