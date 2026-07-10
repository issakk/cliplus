use std::path::{Path, PathBuf};

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
    let db = state.db.lock();
    db.get_clips(query.as_deref(), limit.unwrap_or(200))
}

#[tauri::command]
pub fn copy_clip(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock();
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
pub fn delete_clip(app: tauri::AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock();
    let r = db.delete_clip(&id);
    drop(db);
    if r.is_ok() {
        crate::sync_scheduler::schedule();
    }
    r
}

#[tauri::command]
pub fn toggle_pin(app: tauri::AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock();
    let r = db.toggle_pin(&id);
    drop(db);
    if r.is_ok() {
        crate::sync_scheduler::schedule();
    }
    r
}

#[tauri::command]
pub fn update_clip(app: tauri::AppHandle, state: State<'_, AppState>, id: String, content: String) -> Result<(), String> {
    let db = state.db.lock();
    let r = db.update_clip_text(&id, &content);
    drop(db);
    if r.is_ok() {
        crate::sync_scheduler::schedule();
    }
    r
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
    let db = state.db.lock();
    db.get_snippets(group_name.as_deref())
}

#[tauri::command]
pub fn create_snippet(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    title: String,
    content: String,
    group_name: Option<String>,
) -> Result<String, String> {
    let db = state.db.lock();
    let r = db.insert_snippet(&title, &content, group_name.as_deref());
    drop(db);
    if r.is_ok() {
        crate::sync_scheduler::schedule();
    }
    r
}

#[tauri::command]
pub fn update_snippet(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    title: String,
    content: String,
    group_name: Option<String>,
) -> Result<(), String> {
    let db = state.db.lock();
    let r = db.update_snippet(&id, &title, &content, group_name.as_deref());
    drop(db);
    if r.is_ok() {
        crate::sync_scheduler::schedule();
    }
    r
}

#[tauri::command]
pub fn delete_snippet_cmd(app: tauri::AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock();
    let r = db.delete_snippet(&id);
    drop(db);
    if r.is_ok() {
        crate::sync_scheduler::schedule();
    }
    r
}

#[tauri::command]
pub fn get_setting(state: State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    let db = state.db.lock();
    db.get_setting(&key)
}

#[tauri::command]
pub fn set_setting(app: tauri::AppHandle, state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    let db = state.db.lock();
    let r = db.set_setting(&key, &value);
    drop(db);
    if r.is_ok() {
        crate::sync_scheduler::schedule();
    }
    r
}

#[tauri::command]
pub fn cleanup_old_records(app: tauri::AppHandle, state: State<'_, AppState>, days: i64) -> Result<u32, String> {
    let db = state.db.lock();
    let r = db.cleanup_deleted(days);
    drop(db);
    if r.is_ok() {
        crate::sync_scheduler::schedule();
    }
    r
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
// ===== 同步目录管理 =====

/// 读取配置文件中的同步盘目录
fn read_sync_dir_config(app_data_dir: &Path) -> Result<Option<String>, String> {
    let cfg_path = app_data_dir.join("sync_dir.txt");
    if cfg_path.exists() {
        let content = std::fs::read_to_string(&cfg_path).map_err(|e| e.to_string())?;
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(None)
}

#[tauri::command]
pub fn get_sync_dir(state: State<'_, AppState>) -> Result<Option<String>, String> {
    read_sync_dir_config(&state.app_data_dir)
}

/// 设置同步盘目录：写入配置、立即合并+导出、更新 AppState.mirror_path
#[tauri::command]
pub fn set_sync_dir(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let target_dir = PathBuf::from(&path);
    if !target_dir.is_dir() {
        return Err("目标目录不存在".into());
    }

    let mirror_path = target_dir.join("clipsync.db");

    // 保存配置（与启动时读取的路径保持一致：app_data_dir）
    std::fs::create_dir_all(&state.app_data_dir).map_err(|e| e.to_string())?;
    std::fs::write(
        state.app_data_dir.join("sync_dir.txt"),
        target_dir.to_string_lossy().as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    // 立即同步一次：合并镜像入本地 → 导出本地到镜像
    {
        let db = state.db.lock();
        let stats = db.sync_with(&mirror_path)?;
        log::info!(
            "设置同步目录后完成同步：clips {} 行，snippets {} 行",
            stats.clips,
            stats.snippets
        );
    }

    // 更新 AppState
    *state.mirror_path.lock() = Some(mirror_path.clone());

    Ok(mirror_path.to_string_lossy().to_string())
}

/// 手动触发一次同步（合并 + 导出）
#[tauri::command]
pub fn sync_now(state: State<'_, AppState>) -> Result<String, String> {
    let mp = state.mirror_path.lock().clone();
    let mirror_path = mp.ok_or("未配置同步目录")?;
    let db = state.db.lock();
    let stats = db.sync_with(&mirror_path)?;
    Ok(format!(
        "同步完成：合并 clips {} 行，snippets {} 行",
        stats.clips, stats.snippets
    ))
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

/// 列出系统已安装字体名（去重、排序）
#[tauri::command]
pub fn list_system_fonts() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        use std::collections::BTreeSet;
        use windows::Win32::Graphics::Gdi::{
            EnumFontFamiliesExW, LOGFONTW, NEWTEXTMETRICEXW, FONT_RESOURCE_FILE_HANDLE,
            GetDC, ReleaseDC,
        };
        use windows::Win32::Foundation::HWND;

        let mut fonts: BTreeSet<String> = BTreeSet::new();
        let fonts_ptr = &mut fonts as *mut BTreeSet<String>;

        extern "system" fn callback(
            _elf: *const LOGFONTW,
            _ntm: *const NEWTEXTMETRICEXW,
            _font_type: u32,
            lparam: usize,
        ) -> i32 {
            // 从 LOGFONTW 读取字体名
            let lf = unsafe { &*_elf };
            // lfFaceName 是 [u16; 32]，以 null 结尾
            let mut end = 0;
            for i in 0..32 {
                if lf.lfFaceName[i] == 0 {
                    end = i;
                    break;
                }
                end = i + 1;
            }
            let name = String::from_utf16_lossy(&lf.lfFaceName[..end]);
            let set = unsafe { &mut *(lparam as *mut BTreeSet<String>) };
            if !name.is_empty() && !name.starts_with('@') {
                set.insert(name);
            }
            1 // 继续枚举
        }

        unsafe {
            let hdc = GetDC(HWND(std::ptr::null_mut()));
            if hdc.is_invalid() {
                return Err("GetDC 失败".into());
            }

            let mut lf = LOGFONTW::default();
            lf.lfCharSet = 0; // DEFAULT_CHARSET = 0 → 枚举所有字符集的字体

            EnumFontFamiliesExW(
                hdc,
                &lf,
                Some(callback),
                fonts_ptr as usize,
                0,
            )
            .map_err(|e| format!("EnumFontFamiliesExW 失败: {}", e))?;

            ReleaseDC(HWND(std::ptr::null_mut()), hdc);

            Ok(fonts.into_iter().collect())
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("仅支持 Windows".into())
    }
}
