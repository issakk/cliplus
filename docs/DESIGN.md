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
| 同步 | 本地库+镜像库 LWW | — | 行级合并，依赖云盘文件同步，无需 HTTP 客户端 |
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
- 本地库 + 镜像库 + LWW 行级合并（详见 §5）
- 用户指定同步盘目录（OneDrive / Dropbox / Google Drive 等任意文件同步服务）
- 触发时机：启动时合并、退出时导出、写后防抖自动导出、手动触发

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

### 5.1 架构：本地库 + 镜像库 + LWW 合并

每台设备持有一份**本地工作库**（`app_data/clipsync.db`，开 WAL，可靠）。
用户可选一个云同步盘目录（OneDrive / Dropbox / Google Drive 等），其中存放**镜像库** `clipsync.db`。
设备不直接操作云盘上的数据库，而是通过启动合并 + 退出导出与镜像交换数据。

```
┌─────────────┐                     ┌─────────────┐
│   设备 A    │                     │   设备 B    │
│  本地库     │                     │  本地库     │
│  (app_data) │                     │  (app_data) │
└──────┬──────┘                     └──────┬──────┘
       │ merge_from / export_to            │ merge_from / export_to
       ▼                                   ▼
┌──────────────────────────────────────────────────┐
│     云同步盘目录 (OneDrive / Dropbox / …)         │
│     clipsync.db  (镜像库，普通文件，无 WAL)        │
└──────────────────────────────────────────────────┘
```

### 5.2 同步时机

- **启动时**：`merge_from(镜像)` — 把镜像里 `updated_at` 更新的行并入本地库
- **退出时**：`sync_with(镜像)` = `merge_from` + `export_to`
  - `export_to`：先防御性合并 → `PRAGMA wal_checkpoint(TRUNCATE)` → 整文件覆盖镜像 → 删除镜像的 `-wal`/`-shm`
- **写后防抖**：每次写库（复制/增删改）后投递信号，5 秒内无新写入则自动 `export_to`，避免频繁复制连击云盘
- **手动同步**：设置页"立即同步"按钮 → `sync_now` 命令

### 5.3 合并算法（LWW）

行级 Last-Writer-Wins，跨库用 SQLite `ATTACH` + `INSERT ... ON CONFLICT(id) DO UPDATE ... WHERE excluded.updated_at > clips.updated_at`。

```
fn merge_from(mirror_path):
    ATTACH mirror AS remote
    INSERT INTO clips SELECT * FROM remote.clips
        ON CONFLICT(id) DO UPDATE SET ... WHERE excluded.updated_at > clips.updated_at
    INSERT INTO snippets SELECT * FROM remote.snippets
        ON CONFLICT(id) DO UPDATE SET ... WHERE excluded.updated_at > snippets.updated_at
    DETACH remote
```

- 软删除以 `is_deleted=1` 的行传播（删除操作只更新 `is_deleted` 和 `updated_at`）
- `sync_meta`（设置项）不参与合并，保持各设备本地
- **不直接在云盘上开 SQLite**：避免 WAL/SHM 文件同步时序导致的数据不一致

### 5.4 配置

- 同步盘目录路径存于 `app_data/sync_dir.txt`
- Tauri 命令：`get_sync_dir` / `set_sync_dir`（设置后立即首次同步）/ `sync_now`
- `device_id` = `COMPUTERNAME`，写入每条 clip 的 `device_id` 字段，便于排查来源

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
- [x] 点击复制、置顶、删除、编辑（右键菜单编辑文本）

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
