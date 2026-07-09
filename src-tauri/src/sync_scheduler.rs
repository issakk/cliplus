//! 防抖同步调度器：写库后等 DEBOUNCE 窗口内无新写入，再导出本地库到镜像。
//!
//! 避免每次复制都整文件覆盖镜像（export_to = checkpoint + fs::copy），
//! 把短时间内连续写入合并为一次导出，兼顾实时性与云盘客户端负载。
//!
//! 用法：启动时 `init(app)`，每次写库后 `schedule()`。

use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

/// 防抖窗口：最后一次写入后等多久再导出（秒）。
const DEBOUNCE: Duration = Duration::from_secs(5);

/// 单例调度器：持有发往 worker 线程的信号通道。
struct Scheduler {
    tx: mpsc::Sender<()>,
}

static SCHEDULER: OnceLock<Scheduler> = OnceLock::new();

/// 启动时调用一次：创建 worker 线程。AppHandle 为运行期输入，故用 OnceLock。
pub fn init(app: AppHandle) {
    SCHEDULER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<()>();
        thread::spawn(move || worker(app, rx));
        Scheduler { tx }
    });
}

/// 任何写库操作后调用：投递一个同步信号。窗口内多次调用只触发一次导出。
pub fn schedule() {
    if let Some(s) = SCHEDULER.get() {
        let _ = s.tx.send(());
    }
}

/// worker：等首个信号 → 进入防抖窗口（窗口内每收到新信号就重置）→
/// 窗口安静满 DEBOUNCE 后执行一次 export_to → 回到等信号。
fn worker(app: AppHandle, rx: mpsc::Receiver<()>) {
    loop {
        // 等第一个信号；通道关闭则退出
        if rx.recv().is_err() {
            return;
        }
        // 防抖：窗口内每收到新信号重置计时
        loop {
            match rx.recv_timeout(DEBOUNCE) {
                Ok(()) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        do_export(&app);
    }
}

fn do_export(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let mp = state.mirror_path.lock().clone();
    let Some(mirror_path) = mp else {
        return; // 未配置同步目录，跳过
    };
    let db = state.db.lock();
    if let Err(e) = db.export_to(&mirror_path) {
        log::warn!("防抖同步导出失败: {}", e);
    }
}