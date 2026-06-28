use tauri::State;

use crate::db::clips::Clip;
use crate::db::snippets::Snippet;
use crate::AppState;

#[tauri::command]
pub fn get_clips(
    state: State<'_, AppState>,
    query: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<Clip>, String> {
    let db = state.db.lock().unwrap();
    db.get_clips(query.as_deref(), limit.unwrap_or(200))
}

#[tauri::command]
pub fn copy_clip(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    let text = db.get_clip_text(&id)?;
    if let Some(text) = text {
        unsafe {
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::DataExchange::{
                CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
            };
            use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

            OpenClipboard(None).map_err(|e| e.to_string())?;
            let _ = EmptyClipboard();

            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let size = wide.len() * 2;

            let h_global = GlobalAlloc(GMEM_MOVEABLE, size).map_err(|e| e.to_string())?;
            let ptr: *mut std::ffi::c_void = GlobalLock(h_global);
            if ptr.is_null() {
                CloseClipboard().ok();
                return Err("GlobalLock failed".into());
            }

            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
            let _ = GlobalUnlock(h_global);

            // HGLOBAL 和 HANDLE 都包装 *mut c_void，直接转换
            let h_handle = HANDLE(h_global.0);
            SetClipboardData(13, h_handle).map_err(|e| e.to_string())?;
            CloseClipboard().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn delete_clip(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.delete_clip(&id)
}

#[tauri::command]
pub fn toggle_pin(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.toggle_pin(&id)
}

#[tauri::command]
pub fn toggle_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            window.hide().map_err(|e| e.to_string())?;
        } else {
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_snippets(
    state: State<'_, AppState>,
    group_name: Option<String>,
) -> Result<Vec<Snippet>, String> {
    let db = state.db.lock().unwrap();
    db.get_snippets(group_name.as_deref())
}

#[tauri::command]
pub fn create_snippet(
    state: State<'_, AppState>,
    title: String,
    content: String,
    group_name: Option<String>,
) -> Result<String, String> {
    let db = state.db.lock().unwrap();
    db.insert_snippet(&title, &content, group_name.as_deref())
}

#[tauri::command]
pub fn update_snippet(
    state: State<'_, AppState>,
    id: String,
    title: String,
    content: String,
    group_name: Option<String>,
) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.update_snippet(&id, &title, &content, group_name.as_deref())
}

#[tauri::command]
pub fn delete_snippet_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.delete_snippet(&id)
}

#[tauri::command]
pub fn get_setting(state: State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    let db = state.db.lock().unwrap();
    db.get_setting(&key)
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.set_setting(&key, &value)
}

// ===== 同步命令 =====

#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> Result<crate::sync::types::SyncResult, String> {
    use crate::sync::{onedrive, webdav};
    use crate::sync::types::SyncBackend;

    // 读取同步配置
    let (backend, device_name, last_sync_ts) = {
        let db = state.db.lock().unwrap();
        let backend_str = db.get_setting("sync_backend")?.unwrap_or_default();
        let device = db.get_setting("device_name")?.unwrap_or_else(|| "unknown".into());
        let ts: i64 = db
            .get_setting("last_sync_ts")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let backend = match backend_str.as_str() {
            "onedrive" => SyncBackend::OneDrive,
            "webdav" => SyncBackend::WebDAV,
            _ => SyncBackend::None,
        };
        (backend, device, ts)
    };

    if backend == SyncBackend::None {
        return Err("未配置同步后端".into());
    }

    // 导出本地变更
    let entries = {
        let db = state.db.lock().unwrap();
        db.export_entries(last_sync_ts)?
    };

    let sync_content = crate::db::Database::generate_sync_file(&entries)?;

    // 上传到云端
    match &backend {
        SyncBackend::OneDrive => {
            let refresh_token = {
                let db = state.db.lock().unwrap();
                db.get_setting("onedrive_refresh_token")?.unwrap_or_default()
            };
            if refresh_token.is_empty() {
                return Err("OneDrive 未登录".into());
            }
            let mut client = onedrive::OneDriveClient::new(
                onedrive::DEFAULT_CLIENT_ID.to_string(),
                refresh_token,
            );
            client.upload_file("clipsync/sync.jsonl", &sync_content).await?;
        }
        SyncBackend::WebDAV => {
            let (url, user, pass) = {
                let db = state.db.lock().unwrap();
                let url = db.get_setting("webdav_url")?.unwrap_or_default();
                let user = db.get_setting("webdav_username")?.unwrap_or_default();
                let pass = db.get_setting("webdav_password")?.unwrap_or_default();
                (url, user, pass)
            };
            if url.is_empty() {
                return Err("WebDAV 未配置".into());
            }
            let client = webdav::WebDAVClient::new(url, user, pass);
            client.upload_file("clipsync/sync.jsonl", &sync_content).await?;
        }
        _ => {}
    }

    // 从云端下载并合并
    let remote_content: Option<String> = match &backend {
        SyncBackend::OneDrive => {
            let refresh_token = {
                let db = state.db.lock().unwrap();
                db.get_setting("onedrive_refresh_token")?.unwrap_or_default()
            };
            let mut client = onedrive::OneDriveClient::new(
                onedrive::DEFAULT_CLIENT_ID.to_string(),
                refresh_token,
            );
            client.download_file("clipsync/sync.jsonl").await?
        }
        SyncBackend::WebDAV => {
            let (url, user, pass) = {
                let db = state.db.lock().unwrap();
                let url = db.get_setting("webdav_url")?.unwrap_or_default();
                let user = db.get_setting("webdav_username")?.unwrap_or_default();
                let pass = db.get_setting("webdav_password")?.unwrap_or_default();
                (url, user, pass)
            };
            let client = webdav::WebDAVClient::new(url, user, pass);
            client.download_file("clipsync/sync.jsonl").await?
        }
        _ => None,
    };

    let mut result = crate::sync::types::SyncResult {
        pushed: entries.len() as u32,
        pulled: 0,
        merged: 0,
        errors: Vec::new(),
    };

    if let Some(content) = remote_content {
        let remote_entries = crate::db::Database::parse_sync_file(&content)?;
        result.pulled = remote_entries.len() as u32;

        let (merged, errors) = {
            let db = state.db.lock().unwrap();
            db.import_entries(&remote_entries, &device_name)?
        };
        result.merged = merged;
        result.errors = errors;
    }

    // 更新同步时间
    {
        let db = state.db.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis().to_string();
        db.set_setting("last_sync_ts", &now)?;
    }

    Ok(result)
}

#[tauri::command]
pub fn get_sync_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let db = state.db.lock().unwrap();
    let backend = db.get_setting("sync_backend")?.unwrap_or_default();
    let last_ts: i64 = db
        .get_setting("last_sync_ts")?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let device = db.get_setting("device_name")?.unwrap_or_else(|| "unknown".into());
    let onedrive_logged = db
        .get_setting("onedrive_refresh_token")?
        .map(|t| !t.is_empty())
        .unwrap_or(false);

    Ok(serde_json::json!({
        "backend": backend,
        "last_sync_ts": last_ts,
        "device_name": device,
        "onedrive_logged_in": onedrive_logged,
    }))
}

#[tauri::command]
pub async fn start_onedrive_login() -> Result<serde_json::Value, String> {
    use crate::sync::onedrive;
    let resp = onedrive::start_device_code_flow(onedrive::DEFAULT_CLIENT_ID).await?;
    Ok(serde_json::json!({
        "user_code": resp.user_code,
        "verification_uri": resp.verification_uri,
        "expires_in": resp.expires_in,
        "device_code": resp.device_code,
    }))
}

#[tauri::command]
pub async fn poll_onedrive_login(
    state: State<'_, AppState>,
    device_code: String,
) -> Result<serde_json::Value, String> {
    use crate::sync::onedrive;
    let resp = onedrive::poll_device_code(onedrive::DEFAULT_CLIENT_ID, &device_code).await?;

    // 保存 refresh_token
    let refresh_token = resp.refresh_token.ok_or("未获取到 refresh_token")?;
    let db = state.db.lock().unwrap();
    db.set_setting("onedrive_refresh_token", &refresh_token)?;
    db.set_setting("sync_backend", "onedrive")?;

    Ok(serde_json::json!({ "success": true }))
}

#[tauri::command]
pub fn cleanup_old_records(state: State<'_, AppState>, days: i64) -> Result<u32, String> {
    let db = state.db.lock().unwrap();
    db.cleanup_deleted(days)
}
