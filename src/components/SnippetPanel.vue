<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";

interface Snippet {
  id: string;
  title: string;
  content: string;
  group_name: string;
  sort_order: number;
  created_at: number;
}

const snippets = ref<Snippet[]>([]);
const selectedId = ref<string | null>(null);
const filterGroup = ref<string>("");
const isNew = ref(false);

// 编辑表单
const form = ref({
  title: "",
  content: "",
  group_name: "",
});

// 所有分组（去重）
const groups = computed(() => {
  const set = new Set(snippets.value.map((s) => s.group_name).filter(Boolean));
  return Array.from(set).sort();
});

// 按分组过滤后的列表
const filteredSnippets = computed(() => {
  if (!filterGroup.value) return snippets.value;
  return snippets.value.filter((s) => s.group_name === filterGroup.value);
});

async function loadSnippets() {
  snippets.value = await invoke("get_snippets", {
    groupName: null,
  });
}

function selectSnippet(snippet: Snippet) {
  selectedId.value = snippet.id;
  isNew.value = false;
  form.value = {
    title: snippet.title,
    content: snippet.content,
    group_name: snippet.group_name,
  };
}

function startNew() {
  selectedId.value = null;
  isNew.value = true;
  form.value = {
    title: "",
    content: "",
    group_name: filterGroup.value || "",
  };
}

async function save() {
  if (!form.value.title.trim()) return;

  if (isNew.value) {
    await invoke("create_snippet", {
      title: form.value.title,
      content: form.value.content,
      groupName: form.value.group_name,
    });
  } else if (selectedId.value) {
    await invoke("update_snippet", {
      id: selectedId.value,
      title: form.value.title,
      content: form.value.content,
      groupName: form.value.group_name,
    });
  }

  await loadSnippets();
  isNew.value = false;

  // 保存后重新选中刚编辑的片段
  if (!isNew.value && selectedId.value) {
    const updated = snippets.value.find((s) => s.id === selectedId.value);
    if (updated) selectSnippet(updated);
  }
}

async function deleteSnippet() {
  if (!selectedId.value) return;
  await invoke("delete_snippet_cmd", { id: selectedId.value });
  selectedId.value = null;
  isNew.value = false;
  form.value = { title: "", content: "", group_name: "" };
  await loadSnippets();
}

async function copySnippet(snippet: Snippet) {
  await navigator.clipboard.writeText(snippet.content);
}

function truncate(text: string, max: number = 40) {
  if (!text) return "";
  return text.length > max ? text.slice(0, max) + "..." : text;
}

onMounted(() => {
  loadSnippets();
});
</script>

<template>
  <div class="snippet-panel">
    <!-- 左侧：片段列表 -->
    <div class="sidebar">
      <!-- 分组过滤 -->
      <div class="group-filter">
        <select v-model="filterGroup" class="group-select">
          <option value="">全部分组</option>
          <option v-for="g in groups" :key="g" :value="g">{{ g }}</option>
        </select>
      </div>

      <!-- 新建按钮 -->
      <button class="btn-new" @click="startNew">+ 新建片段</button>

      <!-- 片段列表 -->
      <div class="snippet-list">
        <div
          v-for="snippet in filteredSnippets"
          :key="snippet.id"
          class="snippet-item"
          :class="{ selected: snippet.id === selectedId }"
          @click="selectSnippet(snippet)"
          @dblclick="copySnippet(snippet)"
          title="双击复制内容"
        >
          <div class="snippet-title">{{ truncate(snippet.title) }}</div>
          <div class="snippet-group" v-if="snippet.group_name">
            {{ snippet.group_name }}
          </div>
        </div>

        <div v-if="filteredSnippets.length === 0" class="empty">
          {{ filterGroup ? "该分组无片段" : "暂无片段" }}
        </div>
      </div>
    </div>

    <!-- 右侧：编辑区 -->
    <div class="editor">
      <template v-if="selectedId || isNew">
        <div class="editor-header">
          <span class="editor-title">{{ isNew ? "新建片段" : "编辑片段" }}</span>
          <div class="editor-actions">
            <button class="btn-save" @click="save" :disabled="!form.title.trim()">
              保存
            </button>
            <button
              v-if="!isNew && selectedId"
              class="btn-delete"
              @click="deleteSnippet"
            >
              删除
            </button>
          </div>
        </div>

        <div class="editor-fields">
          <div class="field">
            <label>标题</label>
            <input
              v-model="form.title"
              type="text"
              placeholder="片段标题"
              class="input"
            />
          </div>

          <div class="field">
            <label>分组</label>
            <input
              v-model="form.group_name"
              type="text"
              placeholder="分组名称"
              class="input"
              list="group-suggestions"
            />
            <datalist id="group-suggestions">
              <option v-for="g in groups" :key="g" :value="g" />
            </datalist>
          </div>

          <div class="field field-grow">
            <label>内容</label>
            <textarea
              v-model="form.content"
              placeholder="片段内容..."
              class="textarea"
            />
          </div>
        </div>
      </template>

      <div v-else class="editor-empty">
        <span>选择一个片段进行编辑，或新建片段</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.snippet-panel {
  display: flex;
  height: 100vh;
  background: var(--bg);
  color: var(--text);
}

/* 左侧边栏 */
.sidebar {
  width: 200px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border);
  background: var(--bg);
}

.group-filter {
  padding: 8px;
  border-bottom: 1px solid var(--border);
}

.group-select {
  width: 100%;
  padding: 6px 8px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text);
  font-size: 13px;
  outline: none;
  cursor: pointer;
}

.group-select:focus {
  border-color: var(--accent);
}

.btn-new {
  margin: 8px;
  padding: 6px 0;
  background: var(--accent);
  color: var(--bg);
  border: none;
  border-radius: 4px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
}

.btn-new:hover {
  opacity: 0.9;
}

.snippet-list {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0;
}

.snippet-item {
  padding: 8px 12px;
  cursor: pointer;
  border-left: 3px solid transparent;
  transition: background 0.1s;
}

.snippet-item:hover {
  background: var(--bg-secondary);
}

.snippet-item.selected {
  background: var(--bg-secondary);
  border-left-color: var(--accent);
}

.snippet-title {
  font-size: 13px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.snippet-group {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 2px;
}

.empty {
  text-align: center;
  padding: 40px 12px;
  color: var(--text-dim);
  font-size: 13px;
}

/* 右侧编辑区 */
.editor {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.editor-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 16px;
  border-bottom: 1px solid var(--border);
}

.editor-title {
  font-size: 14px;
  font-weight: 600;
}

.editor-actions {
  display: flex;
  gap: 8px;
}

.btn-save {
  padding: 5px 16px;
  background: var(--green);
  color: var(--bg);
  border: none;
  border-radius: 4px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
}

.btn-save:disabled {
  opacity: 0.4;
  cursor: default;
}

.btn-save:hover:not(:disabled) {
  opacity: 0.9;
}

.btn-delete {
  padding: 5px 12px;
  background: var(--red);
  color: var(--bg);
  border: none;
  border-radius: 4px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
}

.btn-delete:hover {
  opacity: 0.9;
}

.editor-fields {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 16px;
  gap: 12px;
  overflow: hidden;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.field-grow {
  flex: 1;
  min-height: 0;
}

.field label {
  font-size: 12px;
  color: var(--text-dim);
  font-weight: 600;
}

.input {
  padding: 8px 10px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text);
  font-size: 13px;
  outline: none;
}

.input:focus {
  border-color: var(--accent);
}

.textarea {
  flex: 1;
  padding: 10px;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text);
  font-size: 13px;
  font-family: "Cascadia Code", "Fira Code", "Consolas", monospace;
  line-height: 1.5;
  resize: none;
  outline: none;
}

.textarea:focus {
  border-color: var(--accent);
}

.editor-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-dim);
  font-size: 14px;
}
</style>
