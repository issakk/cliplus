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
├── lib.rs           # Tauri Builder, 插件注册, 命令注册, 文件锁
├── commands.rs      # 所有 #[tauri::command] 函数
├── clipboard/mod.rs # Windows 剪切板监听 (AddClipboardFormatListener)
├── tray.rs          # 系统托盘
├── db/
│   ├── mod.rs       # Database 结构体, 迁移, FTS5, cleanup_deleted
│   ├── clips.rs     # Clips CRUD
│   ├── snippets.rs  # Snippets CRUD
│   └── settings.rs  # Settings (get_setting/set_setting, 复用 sync_meta 表)
└── sync/            # (已清空, 同步改为数据库直接放同步盘)

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
- **同步**: sync engine 函数是 `impl Database` 的关联函数, 非 `&self` 方法

## 数据库表

- `clips` — 剪切板条目 (id BLOB PK, content_text, content_type, is_pinned, is_deleted, updated_at)
- `snippets` — 文本片段 (id BLOB PK, title, content, group_name, sort_order)
- `sync_meta` — 同步元数据 + 设置项 (key TEXT PK, value TEXT)
- `clips_fts` — FTS5 全文搜索虚拟表 (自动维护触发器)

## 同步架构

数据库直接放到同步盘目录（OneDrive / Dropbox / Google Drive 等），由客户端自动同步文件。

```
用户在设置页选择同步盘目录 → 数据库文件迁移到该目录
                ↓
OneDrive/Dropbox 客户端自动同步 clipsync.db
                ↓
其他设备启动时读取 db_path.txt 打开同一个数据库
```

文件锁 (`.clipsync.lock`) 防止多设备同时写入。锁超过 120 秒自动过期。

## 待完成

- [ ] FTS5 中文分词优化
- [ ] Release 构建优化 (图标替换)
