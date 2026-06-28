mod clipboard;
mod commands;
mod db;
mod sync;
mod tray;

use std::sync::Mutex;

use tauri::Manager;

/// 全局数据库连接
pub struct AppState {
    pub db: Mutex<db::Database>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // 初始化数据库
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("无法获取应用数据目录");
            std::fs::create_dir_all(&app_dir).ok();
            let db_path = app_dir.join("clipsync.db");
            let database = db::Database::open(&db_path).expect("无法打开数据库");

            app.manage(AppState {
                db: Mutex::new(database),
            });

            // 注册全局快捷键 Ctrl+Shift+V
            {
                use tauri_plugin_global_shortcut::{
                    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
                };

                let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyV);
                let app_handle = app.handle().clone();
                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(move |_app, sc, event| {
                            if sc == &shortcut && event.state() == ShortcutState::Pressed {
                                if let Some(window) = app_handle.get_webview_window("main") {
                                    if window.is_visible().unwrap_or(false) {
                                        let _ = window.hide();
                                    } else {
                                        let _ = window.show();
                                        let _ = window.set_focus();
                                    }
                                }
                            }
                        })
                        .build(),
                )?;
                app.global_shortcut().register(shortcut)?;
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
            commands::sync_now,
            commands::get_sync_status,
            commands::start_onedrive_login,
            commands::poll_onedrive_login,
            commands::cleanup_old_records,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用失败");
}
