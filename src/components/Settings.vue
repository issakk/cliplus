<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

const props = defineProps<{ theme: "dark" | "light"; uiFont: string; clipFont: string }>();
const emit = defineEmits<{
  "update:theme": [value: "dark" | "light"];
  "update:ui-font": [value: string];
  "update:clip-font": [value: string];
}>();

const hotkey = ref("Ctrl+Shift+V");
const autoStart = ref(false);
const cleanupMessage = ref("");
const isListeningHotkey = ref(false);

// 同步盘目录
const syncDir = ref("");

// 系统字体列表（从后端读取），UI 字体额外含"系统默认"选项
const uiFontOptions = ref<string[]>([]);
const clipFontOptions = ref<string[]>([]);

function onUiFontChange(e: Event) {
  emit("update:ui-font", (e.target as HTMLSelectElement).value);
}

function onClipFontChange(e: Event) {
  emit("update:clip-font", (e.target as HTMLSelectElement).value);
}

async function loadSettings() {
  const h = await invoke<string | null>("get_setting", { key: "hotkey" });
  if (h) hotkey.value = h;

  const a = await invoke<string | null>("get_setting", { key: "auto_start" });
  autoStart.value = a === "true";

  try {
    syncDir.value = await invoke<string>("get_sync_dir") || "";
  } catch {}

  // 读取系统字体列表
  try {
    const fonts = await invoke<string[]>("list_system_fonts");
    // UI 字体：前置"系统默认"选项
    uiFontOptions.value = ["系统默认", ...fonts];
    // 剪切板字体：前置"系统默认"选项
    clipFontOptions.value = ["系统默认", ...fonts];
  } catch {}
}

async function saveSetting(key: string, value: string) {
  await invoke("set_setting", { key, value });
}

function onHotkeyInput(e: KeyboardEvent) {
  if (!isListeningHotkey.value) return;
  e.preventDefault();
  e.stopPropagation();

  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  if (e.metaKey) parts.push("Meta");

  const ignored = [
    "Control", "Shift", "Alt", "Meta",
    "Ctrl", "Shift", "Alt", "Meta",
  ];
  if (!ignored.includes(e.key)) {
    parts.push(e.key.length === 1 ? e.key.toUpperCase() : e.key);
  }

  if (parts.length >= 2) {
    hotkey.value = parts.join("+");
    isListeningHotkey.value = false;
    saveSetting("hotkey", hotkey.value);
    // 注册新的全局快捷键
    invoke("register_hotkey", { hotkey: hotkey.value }).catch((e) => {
      cleanupMessage.value = "快捷键注册失败: " + e;
      setTimeout(() => (cleanupMessage.value = ""), 5000);
    });
  }
}

function startListenHotkey() {
  isListeningHotkey.value = true;
}

function stopListenHotkey() {
  isListeningHotkey.value = false;
}

async function onAutoStartChange() {
  await saveSetting("auto_start", String(autoStart.value));
}

async function onThemeChange(t: "dark" | "light") {
  emit("update:theme", t);
}

async function chooseSyncDir() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择同步盘目录（如 OneDrive / Dropbox）",
  });
  if (!selected) return;

  try {
    const newPath = await invoke<string>("set_sync_dir", { path: selected });
    syncDir.value = newPath;
    cleanupMessage.value = "已设置同步目录并完成首次同步: " + newPath;
  } catch (e: any) {
    cleanupMessage.value = "设置失败: " + e;
  }
  setTimeout(() => (cleanupMessage.value = ""), 5000);
}

async function syncNow() {
  try {
    const msg = await invoke<string>("sync_now");
    cleanupMessage.value = msg;
  } catch (e: any) {
    cleanupMessage.value = "同步失败: " + e;
  }
  setTimeout(() => (cleanupMessage.value = ""), 5000);
}

async function cleanupOldRecords() {
  try {
    await invoke("cleanup_old_records", { days: 30 });
    cleanupMessage.value = "已清除 30 天前的记录";
  } catch {
    cleanupMessage.value = "清理失败";
  }
  setTimeout(() => (cleanupMessage.value = ""), 3000);
}

onMounted(() => {
  loadSettings();
});
</script>

<template>
  <div class="settings">
    <div class="settings-header">
      <h2>设置</h2>
    </div>

    <div class="settings-body">
      <!-- 快捷键 -->
      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">全局快捷键</span>
          <span class="label-desc">按下组合键修改</span>
        </div>
        <div class="setting-control">
          <div
            class="hotkey-display"
            :class="{ listening: isListeningHotkey }"
            tabindex="0"
            @click="startListenHotkey"
            @keydown="onHotkeyInput"
            @blur="stopListenHotkey"
          >
            {{ isListeningHotkey ? "请按下快捷键..." : hotkey }}
          </div>
        </div>
      </div>

      <!-- 开机自启动 -->
      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">开机自启动</span>
          <span class="label-desc">系统启动时自动运行</span>
        </div>
        <div class="setting-control">
          <label class="toggle">
            <input
              type="checkbox"
              v-model="autoStart"
              @change="onAutoStartChange"
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
      </div>

      <!-- 主题切换 -->
      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">主题</span>
          <span class="label-desc">界面配色方案</span>
        </div>
        <div class="setting-control">
          <div class="segment">
            <button
              :class="{ active: props.theme === 'light' }"
              @click="onThemeChange('light')"
            >
              浅色
            </button>
            <button
              :class="{ active: props.theme === 'dark' }"
              @click="onThemeChange('dark')"
            >
              深色
            </button>
          </div>
        </div>
      </div>

      <!-- 界面字体 -->
      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">界面字体</span>
          <span class="label-desc">按钮、标签、设置等 UI 文字</span>
        </div>
        <div class="setting-control">
          <select
            class="font-select"
            :value="props.uiFont"
            @change="onUiFontChange"
          >
            <option
              v-for="name in uiFontOptions"
              :key="name"
              :value="name"
            >
              {{ name }}
            </option>
          </select>
        </div>
      </div>

      <!-- 剪切板字体 -->
      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">剪切板字体</span>
          <span class="label-desc">剪切板内容与编辑区域文字</span>
        </div>
        <div class="setting-control">
          <select
            class="font-select"
            :value="props.clipFont"
            @change="onClipFontChange"
          >
            <option
              v-for="name in clipFontOptions"
              :key="name"
              :value="name"
            >
              {{ name }}
            </option>
          </select>
        </div>
      </div>

      <!-- 分隔线 -->
      <div class="setting-divider"></div>

      <!-- 同步目录 -->
      <div class="setting-item" style="flex-direction: column; align-items: stretch;">
        <div class="setting-label" style="margin-bottom: 8px;">
          <span class="label-text">同步目录</span>
          <span class="label-desc">选择同步盘目录（如 OneDrive / Dropbox），数据库镜像将存放在此目录实现多设备同步</span>
        </div>
        <div class="db-path-row">
          <span class="db-path-text" :title="syncDir">{{ syncDir || "未设置（仅本地）" }}</span>
          <button class="btn-primary" @click="chooseSyncDir">更改目录</button>
          <button v-if="syncDir" class="btn-secondary" @click="syncNow">立即同步</button>
        </div>
      </div>

      <!-- 分隔线 -->
      <div class="setting-divider"></div>

      <!-- 数据库清理 -->
      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">数据清理</span>
          <span class="label-desc">清除超过 30 天的历史记录</span>
        </div>
        <div class="setting-control">
          <button class="btn-danger" @click="cleanupOldRecords">
            立即清理
          </button>
        </div>
      </div>

      <div v-if="cleanupMessage" class="cleanup-message">
        {{ cleanupMessage }}
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings {
  height: 100vh;
  overflow-y: auto;
  background: var(--bg);
  color: var(--text);
}

.settings-header {
  position: sticky;
  top: 0;
  z-index: 1;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border);
  background: var(--bg);
}

.settings-header h2 {
  font-size: 16px;
  font-weight: 600;
}

.settings-body {
  padding: 8px 0;
}

.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 20px;
  min-height: 52px;
}

.setting-item:hover {
  background: var(--bg-secondary);
}

.setting-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.label-text {
  font-size: 14px;
  font-weight: 500;
}

.label-desc {
  font-size: 12px;
  color: var(--text-dim);
}

.setting-control {
  flex-shrink: 0;
  margin-left: 16px;
}

.setting-divider {
  height: 1px;
  background: var(--border);
  margin: 8px 20px;
}

/* 快捷键显示 */
.hotkey-display {
  padding: 6px 14px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 6px;
  font-size: 13px;
  font-family: "Consolas", "Courier New", monospace;
  cursor: pointer;
  min-width: 140px;
  text-align: center;
  transition: border-color 0.15s;
  outline: none;
  user-select: none;
}

.hotkey-display:hover {
  border-color: var(--accent);
}

.hotkey-display:focus {
  border-color: var(--accent);
}

.hotkey-display.listening {
  border-color: var(--accent);
  background: var(--surface);
}

/* 开关 */
.toggle {
  position: relative;
  display: inline-block;
  width: 42px;
  height: 24px;
  cursor: pointer;
}

.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle-slider {
  position: absolute;
  inset: 0;
  background: var(--surface);
  border-radius: 12px;
  transition: background 0.2s;
}

.toggle-slider::before {
  content: "";
  position: absolute;
  width: 18px;
  height: 18px;
  left: 3px;
  bottom: 3px;
  background: var(--text);
  border-radius: 50%;
  transition: transform 0.2s;
}

.toggle input:checked + .toggle-slider {
  background: var(--accent);
}

.toggle input:checked + .toggle-slider::before {
  transform: translateX(18px);
  background: var(--bg);
}

/* 分段选择器 */
.segment {
  display: flex;
  background: var(--bg-secondary);
  border-radius: 6px;
  overflow: hidden;
  border: 1px solid var(--border);
}

.segment button {
  padding: 6px 12px;
  background: transparent;
  border: none;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
  white-space: nowrap;
}

.segment button:not(:last-child) {
  border-right: 1px solid var(--border);
}

.segment button:hover {
  color: var(--text);
}

.segment button.active {
  background: var(--accent);
  color: var(--bg);
  font-weight: 500;
}

/* 字体下拉选择 */
.font-select {
  padding: 6px 10px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-size: 13px;
  font-family: var(--ui-font);
  cursor: pointer;
  outline: none;
  min-width: 160px;
  transition: border-color 0.15s;
}

.font-select:hover {
  border-color: var(--accent);
}

.font-select:focus {
  border-color: var(--accent);
}

/* 数据库路径 */
.db-path-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.db-path-text {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  font-family: "Consolas", "Courier New", monospace;
  color: var(--text-dim);
  background: var(--bg-secondary);
  padding: 6px 10px;
  border-radius: 6px;
  border: 1px solid var(--border);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 危险按钮 */
.btn-danger {
  padding: 6px 14px;
  background: transparent;
  border: 1px solid var(--red);
  border-radius: 6px;
  color: var(--red);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}

.btn-danger:hover {
  background: var(--red);
  color: var(--bg);
}

/* 清理提示 */
.cleanup-message {
  padding: 8px 20px 12px;
  font-size: 13px;
  color: var(--green);
}

/* 主要按钮 */
.btn-primary {
  padding: 6px 14px;
  background: var(--accent);
  border: none;
  border-radius: 6px;
  color: var(--bg);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s;
  white-space: nowrap;
}

.btn-primary:hover {
  opacity: 0.85;
}

.btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 次要按钮 */
.btn-secondary {
  padding: 6px 14px;
  background: transparent;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s;
  white-space: nowrap;
}

.btn-secondary:hover {
  opacity: 0.7;
}
</style>
