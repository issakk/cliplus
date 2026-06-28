use std::ffi::CString;
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, OpenClipboard,
    RemoveClipboardFormatListener,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExA, DefWindowProcA, DispatchMessageA, GetMessageA, RegisterClassA,
    TranslateMessage, HWND_MESSAGE, MSG, WNDCLASSA, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLIPBOARDUPDATE, WM_DESTROY,
};

/// 启动剪切板监听（在独立线程中运行）
pub fn start_monitor(app: AppHandle) {
    APP_HANDLE.set(app).ok();
    std::thread::spawn(move || unsafe { run_clipboard_listener() });
}

// 全局 AppHandle 存储（使用 OnceLock 线程安全）
static APP_HANDLE: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

fn get_app_handle() -> Option<&'static AppHandle> {
    APP_HANDLE.get()
}

unsafe fn run_clipboard_listener() {

    let class_name = CString::new("ClipSyncListener").unwrap();

    let wc = WNDCLASSA {
        lpfnWndProc: Some(clipboard_wnd_proc),
        lpszClassName: windows::core::PCSTR(class_name.as_ptr() as *const u8),
        ..Default::default()
    };

    let _ = RegisterClassA(&wc);

    let title = CString::new("ClipSync").unwrap();
    let hwnd = CreateWindowExA(
        WINDOW_EX_STYLE::default(),
        windows::core::PCSTR(class_name.as_ptr() as *const u8),
        windows::core::PCSTR(title.as_ptr() as *const u8),
        WINDOW_STYLE::default(),
        0,
        0,
        0,
        0,
        HWND(HWND_MESSAGE.0),
        None,
        None,
        None,
    )
    .expect("无法创建消息窗口");

    AddClipboardFormatListener(hwnd).expect("无法注册剪切板监听");

    let mut msg = MSG::default();
    while GetMessageA(&mut msg, None, 0, 0).into() {
        let _ = TranslateMessage(&msg);
        DispatchMessageA(&msg);
    }

    RemoveClipboardFormatListener(hwnd).ok();
}

unsafe extern "system" fn clipboard_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            handle_clipboard_update();
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}

fn handle_clipboard_update() {
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }

        let content_text: Option<String>;

        // 尝试获取 Unicode 文本 (CF_UNICODETEXT = 13)
        match GetClipboardData(13) {
            Ok(handle) => {
                let hglobal = windows::Win32::Foundation::HGLOBAL(handle.0);
                let ptr: *mut std::ffi::c_void = GlobalLock(hglobal);
                if !ptr.is_null() {
                    let wide_slice = std::slice::from_raw_parts(ptr as *const u16, 4096);
                    let len = wide_slice
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(wide_slice.len());
                    content_text = Some(String::from_utf16_lossy(&wide_slice[..len]));
                    let _ = GlobalUnlock(hglobal);
                } else {
                    content_text = None;
                }
            }
            Err(_) => {
                content_text = None;
            }
        }

        CloseClipboard().ok();

        let text = match content_text {
            Some(t) if !t.is_empty() => t,
            _ => return,
        };

        // 写入数据库并通知前端
        if let Some(app) = get_app_handle() {
            let state = app.state::<crate::AppState>();
            let db = state.db.lock().unwrap();
            if let Ok(id) = db.insert_clip(Some(&text), None, None, None, "text", None) {
                log::debug!("新剪切板条目: {}", id);
                let _ = app.emit("clipboard-changed", ());
            }
        }
    }
}
