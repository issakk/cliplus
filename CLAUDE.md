# ClipSync

Windows 剪切板管理器 + OneDrive/WebDAV 多设备同步。

## 技术栈

- **后端**: Rust + Tauri 2.0
- **前端**: Vue 3 + TypeScript + Vite
- **数据库**: SQLite (rusqlite, WAL 模式)
- **ID**: UUID v7 (BLOB 16字节, 时间有序)
- **同步**: JSON-lines 变更日志 + LWW 合并

## 项目结构

```
src-tauri/src/
├── main.rs          # 入口, 调用 lib::run()
├── lib.rs           # Tauri Builder, 插件注册, 命令注册, 文件锁, 启动合并/退出导出
├── commands.rs      # 所有 #[tauri::command] 函数
├── clipboard/mod.rs # Windows 剪切板监听 (AddClipboardFormatListener)
├── tray.rs          # 系统托盘
├── db/
│   ├── mod.rs       # Database 结构体, 迁移, FTS5, cleanup_deleted
│   ├── clips.rs     # Clips CRUD (含 device_id, 去重)
│   ├── snippets.rs  # Snippets CRUD
│   ├── settings.rs  # Settings (get_setting/set_setting, 复用 sync_meta 表)
│   └── sync.rs      # 同步引擎: merge_from (LWW) + export_to + sync_with

src/
├── main.ts          # Vue 入口
├── App.vue          # 标签页导航 + 剪切板列表 + 主题
└── components/
    ├── SnippetPanel.vue  # 片段管理 (列表+编辑)
    └── Settings.vue      # 设置页 (快捷键/主题/同步/清理)
```

## 构建

需要 MSVC Build Tools。使用 `build.bat` 自动设置环境：

```cmd
build.bat                # debug 构建
build.bat --release      # release 构建
```

或在 Developer Command Prompt 中：

```cmd
cd src-tauri && cargo build
```

前端开发：

```cmd
npm run dev              # Vite dev server
npm run tauri dev        # Tauri dev (自动启动 Vite + Rust 编译)
```

## 代码约定

- **错误处理**: `.map_err(|e| e.to_string())?` 统一用 String
- **数据库**: `rusqlite::params![]` 参数绑定, `impl Database` 块
- **ID 生成**: `uuid::Uuid::now_v7()`, 存为 BLOB (`uuid.as_bytes().to_vec()`)
- **时间戳**: `chrono::Utc::now().timestamp_millis()` (i64 毫秒)
- **Tauri 命令**: 在 `commands.rs` 定义, 在 `lib.rs` 的 `generate_handler![]` 注册
- **前端调用**: `invoke("command_name", { param })` (camelCase 参数名)
- **CSS 变量**: 使用 `--bg`, `--text`, `--accent` 等全局变量, 支持深色/浅色主题
- **同步**: `db::sync` 模块的 `merge_from`/`export_to`/`sync_with` 是 `impl Database` 的 `&self` 方法

## 验证约束

- **不要编译**：本仓库改动后不要运行 `cargo build` / `cargo check` / `npm run tauri dev`。改动直接落盘提交，由用户自行构建验证。

## 数据库表

- `clips` — 剪切板条目 (id BLOB PK, content_text, content_type, is_pinned, is_deleted, updated_at)
- `snippets` — 文本片段 (id BLOB PK, title, content, group_name, sort_order)
- `sync_meta` — 同步元数据 + 设置项 (key TEXT PK, value TEXT)
- `clips_fts` — FTS5 全文搜索虚拟表 (自动维护触发器)

## 同步架构

本地库 + 镜像库 + LWW 合并。每台设备持有一份本地工作库（app_data/clipsync.db，开 WAL），
用户可选一个云同步盘目录，其中存放镜像库 clipsync.db。

```
启动: 本地库 merge_from(镜像)  ← 把镜像里 updated_at 更新的行并入本地
              ↓
正常使用本地库（剪切板监听、增删改都写本地）
              ↓
退出/手动同步: 本地库 sync_with(镜像) = merge_from + export_to
              export_to: checkpoint(TRUNCATE) → 整文件覆盖镜像 → 删除镜像 -wal/-shm
```

- **LWW (Last-Writer-Wins)**: 按 `updated_at` 取大者，软删除以 `is_deleted=1` 传播
- **device_id**: 每条 clip 记录来源设备名 (COMPUTERNAME)，便于排查
- **不直接操作云盘上的 db**：本地库始终在 app_data，避免 WAL 文件在云盘上的同步时序问题
- 文件锁 (`.clipsync.lock`) 防本机多实例，120 秒过期
- 配置: `app_data/sync_dir.txt` 存同步盘目录路径；`get_sync_dir`/`set_sync_dir`/`sync_now` 命令

## 待完成

- [ ] FTS5 中文分词优化
- [ ] Release 构建优化 (图标替换)
