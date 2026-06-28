# ClipSync — 设计文档

> Windows 剪切板管理器 + OneDrive/WebDAV 多设备同步

## 1. 产品定位

对标 ClipDiary，做简洁实用的剪切板管理器（Ditto 风格），核心差异：支持 OneDrive / WebDAV 云同步。

**目标用户场景：** 两台电脑交替使用（工作机 + 家用机），同一时间只有一台活跃，需要同步剪切板历史和常用片段。

## 2. 技术栈

| 层 | 选型 | 版本 | 理由 |
|---|---|---|---|
| 后端语言 | Rust | stable | 性能好、内存安全、直接调 Win32 API |
| 应用框架 | Tauri | 2.x | 轻量（vs Electron）、内置 system-tray / global-shortcut |
| 前端框架 | Vue 3 | 3.x | 生态成熟、Tauri 官方模板支持、组合式 API 灵活 |
| 构建工具 | Vite | 6.x | 快速 HMR、Tauri 默认集成 |
| 数据库 | SQLite (rusqlite) | — | 单文件、可靠、零部署 |
| HTTP 客户端 | reqwest | — | OneDrive Graph API / WebDAV 请求 |
| 异步运行时 | tokio | — | Tauri 2.0 默认运行时 |
| ID 生成 | uuid v7 | RFC 9562 | 时间有序、去中心化、无需协调设备号 |

**内存预估：** WebView2 ~30MB + Rust 进程 ~5MB ≈ **35MB**

## 3. 核心功能

### 3.1 剪切板监听
- 使用 Windows API `AddClipboardFormatListener` 注册监听
- 支持格式：纯文本、RTF、HTML、图片（BMP/PNG）
- 复制时自动存入 SQLite，去重（连续相同内容不重复记录）
- 来源应用追踪（`GetClipboardOwner` 获取窗口标题）

### 3.2 全局快捷键
- 默认 `Ctrl+Shift+V` 呼出/隐藏主窗口
- Tauri `global-shortcut` 插件，原生实现
- 可在设置中自定义

### 3.3 历史列表
- 虚拟滚动（`@tanstack/vue-virtual`），支持万级条目流畅滚动
- 按时间倒序（UUIDv7 的 BLOB 排序 = 时间序）
- 搜索：FTS5 全文搜索，支持中文分词（可选 jieba 分词）
- 操作：点击复制、置顶、删除、编辑

### 3.4 片段 (Snippets)
- 常用文本模板管理
- 支持分组、排序
- 一键插入到当前焦点窗口

### 3.5 同步
- 支持 OneDrive（Microsoft Graph API）和 WebDAV 两种后端
- JSON-lines 变更日志格式
- LWW (Last-Writer-Wins) 合并策略
- 触发时机：启动时、退出时、手动触发、可选定时（5 分钟）

### 3.6 系统托盘
- 最小化到托盘，后台常驻
- 托盘菜单：显示主窗口、同步、设置、退出

## 4. 数据模型

### 4.1 ID 方案：UUID v7

```
UUIDv7 结构 (128 bits):
| 48 bits 毫秒时间戳 | 4 bits version | 12 bits rand_a | 2 bits variant | 62 bits rand_b |
```

- RFC 9562 标准，时间有序
- SQLite 中存为 **BLOB (16字节)**，索引紧凑
- 两台设备独立生成，零协调，天然不冲突
- 应用层维护 `device_id` TEXT 字段存友好名称（如 "书房电脑"）

### 4.2 表结构

```sql
-- 剪切板条目
CREATE TABLE clips (
    id BLOB PRIMARY KEY,                -- UUIDv7, 16 bytes
    content_text TEXT,                   -- 纯文本内容
    content_rtf TEXT,                    -- RTF 富文本
    content_html TEXT,                   -- HTML 内容
    content_image BLOB,                  -- 图片数据 (PNG)
    content_type TEXT NOT NULL,          -- 'text' | 'rtf' | 'html' | 'image'
    source_app TEXT,                     -- 来源应用窗口标题
    is_pinned INTEGER DEFAULT 0,        -- 置顶标记
    is_deleted INTEGER DEFAULT 0,       -- 软删除（同步用）
    device_id TEXT NOT NULL,             -- 设备友好名称
    created_at INTEGER NOT NULL,         -- Unix timestamp ms（冗余，方便查询）
    updated_at INTEGER NOT NULL,         -- Unix timestamp ms
    version INTEGER DEFAULT 1            -- 乐观锁版本号
);

-- 片段
CREATE TABLE snippets (
    id BLOB PRIMARY KEY,                -- UUIDv7
    title TEXT NOT NULL,                 -- 片段标题
    content TEXT NOT NULL,               -- 片段内容
    group_name TEXT DEFAULT '',          -- 分组名
    sort_order INTEGER DEFAULT 0,       -- 排序权重
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- 同步元数据
CREATE TABLE sync_meta (
    key TEXT PRIMARY KEY,
    value TEXT
);
-- 存储：last_sync_timestamp, last_sync_file_hash, device_id 等
```

### 4.3 索引

```sql
-- 时间排序（UUIDv7 BLOB 比较 = 时间序）
CREATE INDEX idx_clips_id ON clips(id DESC);

-- 置顶 + 时间
CREATE INDEX idx_clips_pinned ON clips(is_pinned, id DESC);

-- 同步：找"上次同步后变更的条目"
CREATE INDEX idx_clips_updated ON clips(updated_at);

-- 软删除过滤
CREATE INDEX idx_clips_deleted ON clips(is_deleted);

-- 片段排序
CREATE INDEX idx_snippets_sort ON snippets(group_name, sort_order);
```

### 4.4 全文搜索

```sql
CREATE VIRTUAL TABLE clips_fts USING fts5(
    content_text,
    content='clips',
    content_rowid='rowid'
);

-- 自动维护 FTS 索引的触发器
CREATE TRIGGER clips_fts_insert AFTER INSERT ON clips BEGIN
    INSERT INTO clips_fts(rowid, content_text) VALUES (new.rowid, new.content_text);
END;

CREATE TRIGGER clips_fts_delete AFTER DELETE ON clips BEGIN
    INSERT INTO clips_fts(clips_fts, rowid, content_text)
    VALUES('delete', old.rowid, old.content_text);
END;

CREATE TRIGGER clips_fts_update AFTER UPDATE ON clips BEGIN
    INSERT INTO clips_fts(clips_fts, rowid, content_text)
    VALUES('delete', old.rowid, old.content_text);
    INSERT INTO clips_fts(rowid, content_text) VALUES (new.rowid, new.content_text);
END;
```

## 5. 同步方案

### 5.1 架构

```
┌─────────────┐     ┌─────────────┐
│   设备 A    │     │   设备 B    │
│  SQLite DB  │     │  SQLite DB  │
└──────┬──────┘     └──────┬──────┘
       │                    │
       │  export/import     │
       ▼                    ▼
┌──────────────────────────────────┐
│         云存储 (OneDrive / WebDAV)        │
│         clipsync/sync.jsonl              │
└──────────────────────────────────┘
```

### 5.2 同步文件格式

`sync.jsonl` — 每行一条变更记录：

```jsonl
{"op":"upsert","id":"AQID...base64...","data":{...},"ts":1719500000000,"device":"书房电脑"}
{"op":"delete","id":"AQID...base64...","ts":1719500001000,"device":"书房电脑"}
{"op":"upsert","id":"AQID...base64...","data":{...},"ts":1719500002000,"device":"笔记本"}
```

- `op`: `upsert`（插入或更新）或 `delete`（软删除）
- `id`: UUIDv7 的 Base64 编码
- `data`: 完整 clip 数据（不含图片 BLOB，图片单独同步）
- `ts`: 操作时间戳
- `device`: 设备友好名称

**图片同步：** 图片不放在 jsonl 里，单独以 `clipsync/images/{uuid}.png` 存储在云盘。

### 5.3 合并算法

```
fn merge(local_db, remote_log):
    last_sync = local_db.get_meta("last_sync_ts")

    for entry in remote_log where entry.ts > last_sync:
        local = local_db.get(entry.id)

        match (local, entry.op):
            (None, "upsert")      → INSERT entry.data
            (Some, "upsert")      → if entry.ts > local.updated_at → UPDATE
            (Some, "delete")      → if entry.ts > local.updated_at → SET is_deleted=1
            (None, "delete")      → SKIP (已经不存在)

    local_db.set_meta("last_sync_ts", now())
```

**冲突策略：** LWW (Last-Writer-Wins)，`updated_at` 大的胜出。
对于两台设备交替使用的场景，几乎不会出现真正的冲突。

### 5.4 OneDrive 集成

- **认证：** OAuth 2.0 Device Code Flow（无需内嵌 client secret）
- **API：** Microsoft Graph API
  - 文件存储在 `/me/drive/special/approot/clipsync/`
  - PUT 上传、GET 下载、GET delta 增量查询
- **Token 管理：** refresh_token 存在本地加密配置中

### 5.5 WebDAV 集成

- 标准 WebDAV（RFC 4918）
- 支持 Nextcloud / Synology / 坚果云等
- 配置项：URL + 用户名 + 密码（密码存本地加密配置）

## 6. 安全

- SQLite 数据库可选 AES-256 加密（SQLCipher）
- 云同步凭据（OAuth token / WebDAV 密码）使用 Windows DPAPI 加密存储
- 剪切板中的敏感数据（密码等）可设置排除规则（按来源应用 / 内容匹配）

## 7. 项目结构

```
F:\cliplus\
├── docs/
│   └── DESIGN.md               ← 本文档
├── src-tauri/
│   ├── src/
│   │   ├── main.rs             # Tauri 入口，插件注册
│   │   ├── lib.rs              # 模块导出
│   │   ├── clipboard/
│   │   │   ├── mod.rs
│   │   │   ├── monitor.rs      # Windows 剪切板监听
│   │   │   ├── types.rs        # Clip / Snippet 数据类型
│   │   │   └── image.rs        # 图片处理（压缩、格式转换）
│   │   ├── db/
│   │   │   ├── mod.rs
│   │   │   ├── connection.rs   # 连接池、迁移
│   │   │   ├── clips.rs        # Clips CRUD
│   │   │   ├── snippets.rs     # Snippets CRUD
│   │   │   └── search.rs       # FTS5 搜索
│   │   ├── sync/
│   │   │   ├── mod.rs
│   │   │   ├── engine.rs       # 同步引擎（导出/导入/合并）
│   │   │   ├── onedrive.rs     # OneDrive Graph API 客户端
│   │   │   ├── webdav.rs       # WebDAV 客户端
│   │   │   └── types.rs        # SyncEntry / SyncConfig
│   │   ├── commands.rs         # Tauri invoke 命令（前端调用）
│   │   ├── hotkey.rs           # 全局快捷键管理
│   │   └── tray.rs             # 系统托盘
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── build.rs
├── src/                         # Vue 3 前端
│   ├── App.vue
│   ├── main.ts
│   ├── components/
│   │   ├── ClipList.vue        # 历史列表（虚拟滚动）
│   │   ├── ClipItem.vue        # 单条剪切板项
│   │   ├── SearchBar.vue       # 搜索栏
│   │   ├── SnippetPanel.vue    # 片段面板
│   │   ├── Settings.vue        # 设置页
│   │   ├── SyncStatus.vue      # 同步状态指示器
│   │   └── TrayMenu.vue        # 托盘菜单（Tauri 内置）
│   ├── composables/
│   │   ├── useClips.ts         # 剪切板数据 composable
│   │   ├── useSearch.ts        # 搜索 composable
│   │   └── useSync.ts          # 同步状态 composable
│   ├── stores/
│   │   ├── clips.ts            # Pinia store
│   │   └── settings.ts
│   ├── utils/
│   │   ├── tauri.ts            # Tauri invoke 封装
│   │   └── format.ts           # 内容格式化 / 截断
│   └── assets/
│       └── styles/
│           └── main.css
├── package.json
├── vite.config.ts
├── tsconfig.json
└── .gitignore
```

## 8. 实施计划

### Phase 1: 项目骨架 + 本地核心
- [ ] 初始化 Tauri 2 + Vue 3 项目
- [ ] SQLite 数据库 + 迁移脚本
- [ ] 剪切板监听（text / rtf / html / image）
- [ ] 全局快捷键 `Ctrl+Shift+V`
- [ ] 系统托盘 + 后台运行
- [ ] 前端：ClipList + SearchBar + ClipItem
- [ ] 点击复制、置顶、删除

### Phase 2: 片段 + 设置
- [ ] SnippetPanel CRUD
- [ ] 设置页面（快捷键、开机启动、同步配置）
- [ ] 深色 / 浅色主题

### Phase 3: 同步
- [ ] 同步引擎（jsonl 导出 / 导入 / 合并）
- [ ] OneDrive 集成（OAuth Device Code Flow + Graph API）
- [ ] WebDAV 集成
- [ ] 同步状态 UI

### Phase 4: 优化
- [ ] 图片压缩存储
- [ ] 数据库定期清理（超过 N 条 / 超过 30 天）
- [ ] 懒加载内容（列表只加载元数据，展开时加载全文）
- [ ] FTS5 中文分词优化

## 9. 构建

```bash
# 开发
npm run tauri dev

# 构建安装包
npm run tauri build
# 输出: src-tauri/target/release/bundle/
```
