<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import SnippetPanel from "./components/SnippetPanel.vue";
import Settings from "./components/Settings.vue";

interface Clip {
  id: string;
  content_text: string;
  content_type: string;
  source_app: string;
  is_pinned: number;
  created_at: number;
}

type Tab = "clips" | "snippets" | "settings";

const activeTab = ref<Tab>("clips");
const theme = ref<"dark" | "light">("dark");

// 剪切板状态
const clips = ref<Clip[]>([]);
const searchQuery = ref("");
const selectedIds = ref<Set<string>>(new Set());
const lastSelectedId = ref<string | null>(null); // Shift 范围选择锚点

// 右键菜单状态
const contextMenu = ref<{ x: number; y: number; clip: Clip } | null>(null);

async function loadClips() {
  clips.value = await invoke("get_clips", {
    query: searchQuery.value || null,
    limit: 200,
  });
}

// 单击：选中该项（Ctrl 切换多选 / Shift 范围多选）
function onClickClip(clip: Clip, e: MouseEvent) {
  if (e.shiftKey && lastSelectedId.value) {
    // Shift+点击：从锚点到当前项的范围选择
    const ids = clips.value.map((c) => c.id);
    const anchorIdx = ids.indexOf(lastSelectedId.value);
    const currentIdx = ids.indexOf(clip.id);
    if (anchorIdx !== -1 && currentIdx !== -1) {
      const [start, end] =
        anchorIdx <= currentIdx
          ? [anchorIdx, currentIdx]
          : [currentIdx, anchorIdx];
      const rangeIds = ids.slice(start, end + 1);
      selectedIds.value = new Set(rangeIds);
    }
  } else if (e.ctrlKey || e.metaKey) {
    // Ctrl+点击：切换选中状态
    const s = new Set(selectedIds.value);
    if (s.has(clip.id)) {
      s.delete(clip.id);
    } else {
      s.add(clip.id);
    }
    selectedIds.value = s;
    lastSelectedId.value = clip.id;
  } else {
    // 普通点击：仅选中当前项
    selectedIds.value = new Set([clip.id]);
    lastSelectedId.value = clip.id;
  }
}

// 双击：复制并粘贴到上一个活动窗口
async function onDoubleClickClip(clip: Clip) {
  await invoke("copy_clip", { id: clip.id });
  // 后端处理：隐藏窗口 → 等焦点转移 → Ctrl+V
  await invoke("paste_to_active_window");
}

// 右键菜单
function onContextMenu(clip: Clip, e: MouseEvent) {
  e.preventDefault();
  // 如果右键的项不在选中范围内，单独选中它
  if (!selectedIds.value.has(clip.id)) {
    selectedIds.value = new Set([clip.id]);
    lastSelectedId.value = clip.id;
  }
  contextMenu.value = { x: e.clientX, y: e.clientY, clip };
}

function closeContextMenu() {
  contextMenu.value = null;
}

async function ctxCopy() {
  if (!contextMenu.value) return;
  await copySelected();
  closeContextMenu();
}

async function ctxPaste() {
  if (!contextMenu.value) return;
  await onDoubleClickClip(contextMenu.value.clip);
  closeContextMenu();
}

async function ctxTogglePin() {
  if (!contextMenu.value) return;
  await togglePin(contextMenu.value.clip.id);
  closeContextMenu();
}

async function ctxDelete() {
  if (!contextMenu.value) return;
  // 多选时批量删除
  const ids = selectedIds.value.has(contextMenu.value.clip.id)
    ? [...selectedIds.value]
    : [contextMenu.value.clip.id];
  for (const id of ids) {
    await deleteClip(id);
  }
  closeContextMenu();
}

// 复制选中项到剪切板（多选时合并文本）
async function copySelected() {
  const ids = selectedIds.value;
  if (ids.size === 0) return;

  if (ids.size === 1) {
    const id = [...ids][0];
    await invoke("copy_clip", { id });
  } else {
    // 多选：按选中顺序拼接文本
    const texts = clips.value
      .filter((c) => ids.has(c.id) && c.content_text)
      .map((c) => c.content_text);
    const merged = texts.join("\n");
    // 设置 suppress 避免合并文本被插入列表
    await invoke("suppress_next_clip");
    const { writeText } = await import(
      "@tauri-apps/plugin-clipboard-manager"
    );
    await writeText(merged);
  }
}

async function deleteClip(id: string) {
  await invoke("delete_clip", { id });
  selectedIds.value.delete(id);
  await loadClips();
}

async function togglePin(id: string) {
  await invoke("toggle_pin", { id });
  await loadClips();
}

function formatTime(ts: number) {
  return new Date(ts).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function truncate(text: string, max: number = 120) {
  if (!text) return "";
  return text.length > max ? text.slice(0, max) + "…" : text;
}

function onSearch() {
  loadClips();
}

// 全局键盘：Ctrl+C 复制选中项
function onGlobalKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    closeContextMenu();
    return;
  }
  if (e.ctrlKey && e.key === "c" && selectedIds.value.size > 0) {
    // 如果焦点在搜索框且有选中文本，不拦截
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;
    e.preventDefault();
    copySelected();
  }
}

// 主题切换
function applyTheme(t: "dark" | "light") {
  document.documentElement.setAttribute("data-theme", t);
  invoke("set_setting", { key: "theme", value: t });
}

watch(theme, (t) => applyTheme(t));

// 切换标签时刷新数据
function switchTab(tab: Tab) {
  activeTab.value = tab;
  if (tab === "clips") loadClips();
}

async function hideWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  getCurrentWindow().hide();
}

let unlisten: (() => void) | null = null;

onMounted(async () => {
  // 加载主题设置
  const savedTheme = await invoke<string | null>("get_setting", { key: "theme" });
  if (savedTheme === "light") theme.value = "light";

  await loadClips();

  const { listen } = await import("@tauri-apps/api/event");
  unlisten = await listen("clipboard-changed", () => {
    if (activeTab.value === "clips") loadClips();
  });

  document.addEventListener("keydown", onGlobalKeydown);
  document.addEventListener("contextmenu", (e) => e.preventDefault());
});

onUnmounted(() => {
  unlisten?.();
  document.removeEventListener("keydown", onGlobalKeydown);
});
</script>

<template>
  <div class="app">
    <!-- 标签页导航（整个区域可拖拽移动窗口） -->
    <nav class="tabs" data-tauri-drag-region>
      <button
        :class="{ active: activeTab === 'clips' }"
        @click="switchTab('clips')"
      >
        📋 剪切板
      </button>
      <button
        :class="{ active: activeTab === 'snippets' }"
        @click="switchTab('snippets')"
      >
        📝 片段
      </button>
      <button
        :class="{ active: activeTab === 'settings' }"
        @click="switchTab('settings')"
      >
        ⚙️ 设置
      </button>
      <button class="btn-close" @click="hideWindow" title="关闭（最小化到托盘）">
        ✕
      </button>
    </nav>

    <!-- 剪切板标签页 -->
    <template v-if="activeTab === 'clips'">
      <div class="search-bar">
        <input
          v-model="searchQuery"
          type="text"
          placeholder="搜索剪切板..."
          @input="onSearch"
          autofocus
        />
      </div>

      <div v-if="selectedIds.size > 1" class="action-bar">
        <span class="action-count">已选 {{ selectedIds.size }} 项</span>
        <button class="btn-action" @click="copySelected">📋 复制</button>
      </div>

      <div class="clip-list">
        <div
          v-for="clip in clips"
          :key="clip.id"
          class="clip-item"
          :class="{
            pinned: clip.is_pinned,
            selected: selectedIds.has(clip.id),
          }"
          @click="onClickClip(clip, $event)"
          @dblclick="onDoubleClickClip(clip)"
          @contextmenu="onContextMenu(clip, $event)"
        >
          <div class="clip-content">
            <span v-if="clip.content_type === 'text'" class="clip-text">
              {{ truncate(clip.content_text) }}
            </span>
            <span v-else-if="clip.content_type === 'image'" class="clip-image">
              🖼️ 图片
            </span>
            <span v-else class="clip-other">
              📋 {{ clip.content_type }}
            </span>
          </div>
          <div class="clip-meta">
            <span class="clip-app" v-if="clip.source_app">{{
              clip.source_app
            }}</span>
            <span class="clip-time">{{ formatTime(clip.created_at) }}</span>
            <button
              class="btn-icon"
              @click.stop="togglePin(clip.id)"
              :title="clip.is_pinned ? '取消置顶' : '置顶'"
            >
              {{ clip.is_pinned ? "📌" : "📍" }}
            </button>
            <button
              class="btn-icon"
              @click.stop="deleteClip(clip.id)"
              title="删除"
            >
              🗑️
            </button>
          </div>
        </div>

        <div v-if="clips.length === 0" class="empty">
          {{ searchQuery ? "没有匹配的剪切板" : "暂无剪切板记录" }}
        </div>
      </div>

      <!-- 右键菜单遮罩 -->
      <div v-if="contextMenu" class="ctx-overlay" @click="closeContextMenu" @contextmenu.prevent="closeContextMenu"></div>
      <!-- 右键菜单 -->
      <div
        v-if="contextMenu"
        class="context-menu"
        :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
      >
        <div class="ctx-item" @click="ctxCopy">📋 复制</div>
        <div class="ctx-item" @click="ctxPaste">📌 复制并粘贴</div>
        <div class="ctx-item" @click="ctxTogglePin">
          {{ contextMenu.clip.is_pinned ? "📍 取消置顶" : "📌 置顶" }}
        </div>
        <div class="ctx-sep"></div>
        <div class="ctx-item ctx-danger" @click="ctxDelete">🗑️ 删除</div>
      </div>
    </template>

    <!-- 片段标签页 -->
    <SnippetPanel v-if="activeTab === 'snippets'" />

    <!-- 设置标签页 -->
    <Settings
      v-if="activeTab === 'settings'"
      :theme="theme"
      @update:theme="theme = $event"
    />
  </div>
</template>

<style>
/* ===== 深色主题（默认） ===== */
:root {
  --bg: #1e1e2e;
  --bg-secondary: #313244;
  --text: #cdd6f4;
  --text-dim: #a6adc8;
  --accent: #89b4fa;
  --surface: #45475a;
  --border: #585b70;
  --red: #f38ba8;
  --green: #a6e3a1;
}

/* ===== 浅色主题 ===== */
:root[data-theme="light"] {
  --bg: #eff1f5;
  --bg-secondary: #e6e9ef;
  --text: #4c4f69;
  --text-dim: #6c6f85;
  --accent: #1e66f5;
  --surface: #ccd0da;
  --border: #bcc0cc;
  --red: #d20f39;
  --green: #40a02b;
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: "Segoe UI", system-ui, sans-serif;
  background: var(--bg);
  color: var(--text);
  overflow: hidden;
  user-select: none;
}

.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

/* ===== 标签页导航 ===== */
.tabs {
  display: flex;
  border-bottom: 1px solid var(--border);
  background: var(--bg-secondary);
  flex-shrink: 0;
  align-items: center;
}

.tabs button {
  flex: 1;
  padding: 8px 0;
  background: none;
  border: none;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
  border-bottom: 2px solid transparent;
  transition: all 0.15s;
}

.tabs button:hover {
  color: var(--text);
  background: var(--bg);
}

.tabs button.active {
  color: var(--accent);
  border-bottom-color: var(--accent);
}

.btn-close {
  flex: none !important;
  width: 36px;
  font-size: 14px;
  border-left: 1px solid var(--border) !important;
  border-bottom: none !important;
}

.btn-close:hover {
  color: var(--red) !important;
  background: var(--bg-secondary) !important;
}

/* ===== 搜索栏 ===== */
.search-bar {
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.search-bar input {
  width: 100%;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-size: 14px;
  outline: none;
}

.search-bar input:focus {
  border-color: var(--accent);
}

/* ===== 剪切板列表 ===== */
.clip-list {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0;
}

.clip-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 12px;
  cursor: pointer;
  border-left: 3px solid transparent;
  transition: background 0.1s;
}

.clip-item:hover,
.clip-item.selected {
  background: var(--bg-secondary);
}

.clip-item.selected {
  border-left-color: var(--accent);
}

.clip-item.pinned {
  border-left-color: var(--green);
}

.clip-content {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  line-height: 1.4;
}

.clip-text {
  white-space: pre-wrap;
  word-break: break-all;
}

.clip-image {
  color: var(--text-dim);
}

.clip-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: 8px;
  flex-shrink: 0;
}

.clip-app {
  font-size: 11px;
  color: var(--text-dim);
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.clip-time {
  font-size: 11px;
  color: var(--text-dim);
}

.btn-icon {
  background: none;
  border: none;
  cursor: pointer;
  font-size: 14px;
  padding: 2px 4px;
  border-radius: 4px;
  opacity: 0.5;
  transition: opacity 0.1s;
}

.btn-icon:hover {
  opacity: 1;
  background: var(--surface);
}

.empty {
  text-align: center;
  padding: 40px 20px;
  color: var(--text-dim);
  font-size: 14px;
}

/* ===== 多选操作栏 ===== */
.action-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 12px;
  background: var(--accent);
  color: var(--bg);
  font-size: 13px;
  font-weight: 500;
  flex-shrink: 0;
}

.action-count {
  font-size: 12px;
}

.btn-action {
  padding: 4px 12px;
  background: var(--bg);
  color: var(--accent);
  border: none;
  border-radius: 4px;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
}

.btn-action:hover {
  opacity: 0.85;
}

/* 右键菜单 */
.ctx-overlay {
  position: fixed;
  inset: 0;
  z-index: 999;
}

.context-menu {
  position: fixed;
  z-index: 1000;
  min-width: 160px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 4px 0;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
}

.ctx-item {
  padding: 6px 14px;
  font-size: 13px;
  color: var(--text);
  cursor: pointer;
  white-space: nowrap;
}

.ctx-item:hover {
  background: var(--surface);
}

.ctx-danger {
  color: var(--red);
}

.ctx-sep {
  height: 1px;
  background: var(--border);
  margin: 4px 0;
}
</style>
