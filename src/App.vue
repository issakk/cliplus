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
const selectedIndex = ref(0);

async function loadClips() {
  clips.value = await invoke("get_clips", {
    query: searchQuery.value || null,
    limit: 200,
  });
}

async function selectClip(clip: Clip) {
  await invoke("copy_clip", { id: clip.id });
}

async function deleteClip(id: string) {
  await invoke("delete_clip", { id });
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
});

onUnmounted(() => {
  unlisten?.();
});
</script>

<template>
  <div class="app">
    <!-- 标签页导航 -->
    <nav class="tabs">
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

      <div class="clip-list">
        <div
          v-for="(clip, index) in clips"
          :key="clip.id"
          class="clip-item"
          :class="{
            pinned: clip.is_pinned,
            selected: index === selectedIndex,
          }"
          @click="selectClip(clip)"
          @mouseenter="selectedIndex = index"
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
</style>
