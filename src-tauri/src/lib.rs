mod clipboard;
mod commands;
mod db;
mod sync_scheduler;
mod tray;

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

/// 同步盘目录配置文件名（存放在 app_data 下，内容为同步盘绝对路径）
const SYNC_DIR_CFG: &str = "sync_dir.txt";

/// 全局应用状态
pub struct AppState {
    pub db: Mutex<db::Database>,
    /// 应用数据目录（Tauri app_data_dir），存放本地库与同步目录配置
    pub app_data_dir: PathBuf,
    /// 镜像数据库路径（同步盘目录下的 clipsync.db），None 表示未配置同步
    pub mirror_path: parking_lot::Mutex<Option<PathBuf>>,
    /// 当前设备友好名称（COMPUTERNAME），写入每条 clip 的 device_id
    pub device_id: String,
    /// 自身写剪切板时跳过监听，避免循环插入
    pub suppress_clip: std::sync::atomic::AtomicBool,
}

/// 同步盘目录配置路径
fn sync_dir_cfg_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(SYNC_DIR_CFG)
}

/// 读取用户配置的同步盘目录
fn read_sync_dir(app_data_dir: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(sync_dir_cfg_path(app_data_dir)).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = PathBuf::from(trimmed);
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// 设备友好名称
fn device_id() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 已有实例在运行：弹友好消息框，并把已运行实例的主窗口显示并聚焦
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_skip_taskbar(true);
                let _ = window.show();
                let _ = window.set_focus();
            }
            tauri_plugin_dialog::MessageDialogBuilder::new(
                app.dialog().clone(),
                "ClipSync",
                "ClipSync 已在运行中，请勿重复启动。",
            )
            .kind(tauri_plugin_dialog::MessageDialogKind::Info)
            .show(|_| {});
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("无法获取应用数据目录");
            std::fs::create_dir_all(&app_dir).ok();

            // 本地数据库始终在 app_data 下（可靠、开 WAL）
            let db_path = app_dir.join("clipsync.db");

            let database = db::Database::open(&db_path).expect("无法打开数据库");

            // 同步盘目录（可选）
            let sync_dir = read_sync_dir(&app_dir);
            let mirror_path = sync_dir.map(|d| d.join("clipsync.db"));

            // 启动合并：把镜像里较新的行并入本地
            if let Some(mp) = &mirror_path {
                match database.merge_from(mp) {
                    Ok(stats) => log::info!(
                        "启动合并完成：clips {} 行，snippets {} 行",
                        stats.clips,
                        stats.snippets
                    ),
                    Err(e) => log::warn!("启动合并失败: {}", e),
                }
            }
            app.manage(AppState {
                db: Mutex::new(database),
                app_data_dir: app_dir.clone(),
                mirror_path: Mutex::new(mirror_path),
                device_id: device_id(),
                suppress_clip: std::sync::atomic::AtomicBool::new(false),
            });

            // 隐藏窗口在任务栏的图标（仅通过托盘操作）
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_skip_taskbar(true);

                // 关闭时隐藏到托盘，不退出
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_clone.hide();
                    }
                });
            }

            // 从数据库读取快捷键并注册
            {
                let state = app.state::<AppState>();
                let db = state.db.lock();
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

            // 启动防抖同步调度器（写库后自动 export_to）
            sync_scheduler::init(app.handle().clone());

            // 创建系统托盘
            tray::create_tray(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_clips,
            commands::copy_clip,
            commands::delete_clip,
            commands::toggle_pin,
            commands::update_clip,
            commands::toggle_window,
            commands::get_snippets,
            commands::create_snippet,
            commands::update_snippet,
            commands::delete_snippet_cmd,
            commands::get_setting,
            commands::set_setting,
            commands::cleanup_old_records,
            commands::get_sync_dir,
            commands::set_sync_dir,
            commands::sync_now,
            commands::register_hotkey,
            commands::paste_to_active_window,
            commands::suppress_next_clip,
        ])
        .build(tauri::generate_context!())
        .expect("运行 Tauri 应用失败")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                let state = app_handle.state::<AppState>();
                // 退出前导出本地库到镜像
                let mp = state.mirror_path.lock().clone();
                if let Some(mp) = mp {
                    let db = state.db.lock();
                    if let Err(e) = db.export_to(&mp) {
                        log::warn!("退出导出失败: {}", e);
                    }
                }
            }
        });
}