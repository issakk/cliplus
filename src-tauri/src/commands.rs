use std::path::PathBuf;

use tauri::{Manager, State};

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
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        let focused = window.is_focused().unwrap_or(false);
        if visible && focused {
            window.hide().map_err(|e| e.to_string())?;
        } else {
            window.set_skip_taskbar(true).ok();
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

#[tauri::command]
pub fn cleanup_old_records(state: State<'_, AppState>, days: i64) -> Result<u32, String> {
    let db = state.db.lock().unwrap();
    db.cleanup_deleted(days)
}

// ===== 剪切板控制 =====

/// 设置 suppress 标志，下次剪切板变化时跳过监听（用于程序自身写剪切板）
#[tauri::command]
pub fn suppress_next_clip(state: State<'_, AppState>) {
    state
        .suppress_clip
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

// ===== 快捷键管理 =====

/// 注册快捷键（供 lib.rs 启动时和 register_hotkey 命令共用）
pub fn register_hotkey_inner(app: &tauri::AppHandle, hotkey: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{
        GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
    };

    // 注销旧快捷键
    let _ = app.global_shortcut().unregister_all();

    // 解析快捷键字符串，如 "Ctrl+Shift+V"
    let parts: Vec<&str> = hotkey.split('+').map(|s| s.trim()).collect();
    if parts.len() < 2 {
        return Err("快捷键至少需要 2 个组合键".into());
    }

    let mut modifiers = Modifiers::empty();
    let mut code = None;

    for part in &parts {
        match part.to_uppercase().as_str() {
            "CTRL" | "CONTROL" => modifiers |= Modifiers::CONTROL,
            "SHIFT" => modifiers |= Modifiers::SHIFT,
            "ALT" => modifiers |= Modifiers::ALT,
            "META" | "SUPER" | "WIN" => modifiers |= Modifiers::META,
            key => {
                code = Some(parse_key_code(key)?);
            }
        }
    }

    let code = code.ok_or("缺少普通按键")?;
    let shortcut = Shortcut::new(Some(modifiers), code);

    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let visible = window.is_visible().unwrap_or(false);
                    let focused = window.is_focused().unwrap_or(false);
                    if visible && focused {
                        // 窗口可见且有焦点 → 隐藏
                        let _ = window.hide();
                    } else {
                        // 窗口隐藏，或可见但无焦点 → 置顶并聚焦
                        let _ = window.set_skip_taskbar(true);
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        })
        .map_err(|e| format!("注册快捷键失败: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn register_hotkey(app: tauri::AppHandle, hotkey: String) -> Result<(), String> {
    register_hotkey_inner(&app, &hotkey)
}

fn parse_key_code(key: &str) -> Result<tauri_plugin_global_shortcut::Code, String> {
    use tauri_plugin_global_shortcut::Code;
    match key {
        "A" => Ok(Code::KeyA),
        "B" => Ok(Code::KeyB),
        "C" => Ok(Code::KeyC),
        "D" => Ok(Code::KeyD),
        "E" => Ok(Code::KeyE),
        "F" => Ok(Code::KeyF),
        "G" => Ok(Code::KeyG),
        "H" => Ok(Code::KeyH),
        "I" => Ok(Code::KeyI),
        "J" => Ok(Code::KeyJ),
        "K" => Ok(Code::KeyK),
        "L" => Ok(Code::KeyL),
        "M" => Ok(Code::KeyM),
        "N" => Ok(Code::KeyN),
        "O" => Ok(Code::KeyO),
        "P" => Ok(Code::KeyP),
        "Q" => Ok(Code::KeyQ),
        "R" => Ok(Code::KeyR),
        "S" => Ok(Code::KeyS),
        "T" => Ok(Code::KeyT),
        "U" => Ok(Code::KeyU),
        "V" => Ok(Code::KeyV),
        "W" => Ok(Code::KeyW),
        "X" => Ok(Code::KeyX),
        "Y" => Ok(Code::KeyY),
        "Z" => Ok(Code::KeyZ),
        "0" => Ok(Code::Digit0),
        "1" => Ok(Code::Digit1),
        "2" => Ok(Code::Digit2),
        "3" => Ok(Code::Digit3),
        "4" => Ok(Code::Digit4),
        "5" => Ok(Code::Digit5),
        "6" => Ok(Code::Digit6),
        "7" => Ok(Code::Digit7),
        "8" => Ok(Code::Digit8),
        "9" => Ok(Code::Digit9),
        "SPACE" => Ok(Code::Space),
        "F1" => Ok(Code::F1),
        "F2" => Ok(Code::F2),
        "F3" => Ok(Code::F3),
        "F4" => Ok(Code::F4),
        "F5" => Ok(Code::F5),
        "F6" => Ok(Code::F6),
        "F7" => Ok(Code::F7),
        "F8" => Ok(Code::F8),
        "F9" => Ok(Code::F9),
        "F10" => Ok(Code::F10),
        "F11" => Ok(Code::F11),
        "F12" => Ok(Code::F12),
        _ => Err(format!("不支持的按键: {}", key)),
    }
}

// ===== 数据库路径管理 =====

/// 读取配置文件中的数据库路径
fn read_db_path_config() -> Result<String, String> {
    let app_dir = dirs::data_local_dir()
        .ok_or("无法获取数据目录")?
        .join("clipsync");
    let cfg_path = app_dir.join("db_path.txt");
    if cfg_path.exists() {
        let content = std::fs::read_to_string(&cfg_path).map_err(|e| e.to_string())?;
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    Ok(app_dir.join("clipsync.db").to_string_lossy().to_string())
}

#[tauri::command]
pub fn get_db_path() -> Result<String, String> {
    read_db_path_config()
}

#[tauri::command]
pub fn set_db_path(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let target_dir = PathBuf::from(&path);
    if !target_dir.exists() {
        return Err("目标目录不存在".into());
    }

    let new_db_path = target_dir.join("clipsync.db");

    // 如果目标已存在数据库，检查 lock
    if new_db_path.exists() {
        let lock_file = target_dir.join(".clipsync.lock");
        if lock_file.exists() {
            if let Ok(meta) = std::fs::metadata(&lock_file) {
                let age = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .unwrap_or(std::time::Duration::from_secs(999));
                if age < std::time::Duration::from_secs(120) {
                    return Err("目标数据库正被其他设备使用，请稍后再试".into());
                }
            }
        }
    }

    // 获取当前数据库路径
    let old_db_path = read_db_path_config()?;
    let old_path = PathBuf::from(&old_db_path);

    // 如果源和目标相同，直接返回
    if old_path == new_db_path {
        return Ok(new_db_path.to_string_lossy().to_string());
    }

    // 复制数据库文件（包括 WAL 和 SHM）
    if old_path.exists() {
        std::fs::copy(&old_path, &new_db_path).map_err(|e| format!("复制数据库失败: {}", e))?;
    }
    for suffix in &["-wal", "-shm"] {
        let src = PathBuf::from(format!("{}{}", old_path.display(), suffix));
        let dst = PathBuf::from(format!("{}{}", new_db_path.display(), suffix));
        if src.exists() {
            std::fs::copy(&src, &dst).ok();
        }
    }

    // 保存新路径到配置文件
    let app_dir = dirs::data_local_dir()
        .ok_or("无法获取数据目录")?
        .join("clipsync");
    std::fs::write(
        app_dir.join("db_path.txt"),
        new_db_path.to_string_lossy().as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    // 关闭旧连接，打开新连接
    {
        let mut db = state.db.lock().unwrap();
        *db = crate::db::Database::open(&new_db_path)?;
    }

    // 删除旧 lock，创建新 lock
    let _ = std::fs::remove_file(state.db_dir.join(".clipsync.lock"));
    let device = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".into());
    let pid = std::process::id();
    let lock_file = target_dir.join(".clipsync.lock");
    std::fs::write(&lock_file, format!("{}:{}", device, pid)).ok();

    Ok(new_db_path.to_string_lossy().to_string())
}

/// 隐藏窗口 → 等焦点转移 → 模拟 Ctrl+V 粘贴到上一个活动窗口
#[tauri::command]
pub fn paste_to_active_window(app: tauri::AppHandle) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;

    // 1. 隐藏 ClipSync 窗口
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    // 2. 等待焦点转移到上一个窗口
    std::thread::sleep(std::time::Duration::from_millis(200));

    // 3. 模拟 Ctrl+V
    unsafe {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_V,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_V,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }

    Ok(())
}
