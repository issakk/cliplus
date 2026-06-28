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
