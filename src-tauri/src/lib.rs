mod clipboard;
mod commands;
mod db;
mod sync;
mod tray;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager;

/// 全局数据库连接
pub struct AppState {
    pub db: Mutex<db::Database>,
    /// 数据库文件所在目录（用于 lock 文件）
    pub db_dir: PathBuf,
}

/// 配置文件路径（存储数据库路径）
fn config_path(app_data_dir: &PathBuf) -> PathBuf {
    app_data_dir.join("db_path.txt")
}

/// 读取用户配置的数据库路径
fn read_db_path(app_data_dir: &PathBuf) -> Option<PathBuf> {
    let cfg = config_path(app_data_dir);
    let content = std::fs::read_to_string(&cfg).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = PathBuf::from(trimmed);
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

/// lock 文件路径
fn lock_path(db_dir: &PathBuf) -> PathBuf {
    db_dir.join(".clipsync.lock")
}

/// 创建文件锁
fn create_lock(db_dir: &PathBuf) -> Result<(), String> {
    let lp = lock_path(db_dir);
    let device = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".into());
    let pid = std::process::id();
    let content = format!("{}:{}", device, pid);
    std::fs::write(&lp, content).map_err(|e| e.to_string())?;
    Ok(())
}

/// 检查是否有其他实例在使用数据库
fn check_lock(db_dir: &PathBuf) -> Result<Option<String>, String> {
    let lp = lock_path(db_dir);
    if !lp.exists() {
        return Ok(None);
    }

    // 检查 lock 文件年龄，超过 120 秒视为过期
    let meta = std::fs::metadata(&lp).map_err(|e| e.to_string())?;
    let modified = meta
        .modified()
        .map_err(|e| e.to_string())?
        .elapsed()
        .unwrap_or(std::time::Duration::from_secs(999));

    if modified > std::time::Duration::from_secs(120) {
        // 过期，删除
        std::fs::remove_file(&lp).ok();
        return Ok(None);
    }

    let content = std::fs::read_to_string(&lp).unwrap_or_default();
    Ok(Some(content))
}

/// 删除文件锁
fn remove_lock(db_dir: &PathBuf) {
    let _ = std::fs::remove_file(lock_path(db_dir));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("无法获取应用数据目录");
            std::fs::create_dir_all(&app_dir).ok();

            // 确定数据库路径
            let db_path = if let Some(p) = read_db_path(&app_dir) {
                p
            } else {
                app_dir.join("clipsync.db")
            };

            let db_dir = db_path.parent().unwrap_or(&app_dir).to_path_buf();

            // 检查文件锁
            if let Ok(Some(lock_info)) = check_lock(&db_dir) {
                log::warn!("数据库可能正被其他实例使用: {}", lock_info);
            }

            // 创建文件锁
            create_lock(&db_dir).ok();

            let database = db::Database::open(&db_path).expect("无法打开数据库");

            app.manage(AppState {
                db: Mutex::new(database),
                db_dir,
            });

            // 隐藏窗口在任务栏的图标（仅通过托盘操作）
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_skip_taskbar(true);
            }

            // 从数据库读取快捷键并注册
            {
                let state = app.state::<AppState>();
                let db = state.db.lock().unwrap();
                let hotkey = db
                    .get_setting("hotkey")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "Ctrl+Shift+V".into());

                let app_handle = app.handle().clone();
                if let Err(e) = commands::register_hotkey_inner(&app_handle, &hotkey) {
                    log::warn!("快捷键注册失败: {}", e);
                }
            }

            // 启动剪切板监听
            let handle = app.handle().clone();
            clipboard::start_monitor(handle);

            // 创建系统托盘
            tray::create_tray(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_clips,
            commands::copy_clip,
            commands::delete_clip,
            commands::toggle_pin,
            commands::toggle_window,
            commands::get_snippets,
            commands::create_snippet,
            commands::update_snippet,
            commands::delete_snippet_cmd,
            commands::get_setting,
            commands::set_setting,
            commands::cleanup_old_records,
            commands::get_db_path,
            commands::set_db_path,
            commands::register_hotkey,
        ])
        .build(tauri::generate_context!())
        .expect("运行 Tauri 应用失败")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // 退出时删除文件锁
                let state = app_handle.state::<AppState>();
                remove_lock(&state.db_dir);
            }
        });
}
