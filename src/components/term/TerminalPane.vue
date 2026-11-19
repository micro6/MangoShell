<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { ChevronDown, ChevronUp, Circle, Search, X } from "@lucide/vue";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "vue-sonner";
import { Button } from "@/components/ui/button";
import { MAX_RECONNECT_ATTEMPTS, terminalManager } from "@/core/term/manager";
import { useWorkspaceStore, type WorkspaceTab } from "@/stores/workspace";

const props = defineProps<{ tab: WorkspaceTab; active: boolean }>();
const store = useWorkspaceStore();
const hostEl = ref<HTMLElement | null>(null);
const reconnecting = ref<{ attempt: number; delayMs: number } | null>(null);
const retryable = ref(false);
const lastError = ref<string | null>(null);
const stopping = ref(false);

const searchOpen = ref(false);
const searchInput = ref<HTMLInputElement | null>(null);
const query = ref("");
const caseSensitive = ref(false);
const regex = ref(false);
const regexInvalid = ref(false);
const resultIndex = ref(0);
const resultCount = ref(0);
const recording = ref(false);

const reconnectText = computed(() => {
  const current = reconnecting.value;
  if (!current) return "";
  if (current.attempt === 0) return "正在重新连接…";
  if (current.delayMs > 0) {
    return `连接已断开，${Math.ceil(current.delayMs / 1000)} 秒后进行第 ${current.attempt}/${MAX_RECONNECT_ATTEMPTS} 次重连`;
  }
  return `正在进行第 ${current.attempt}/${MAX_RECONNECT_ATTEMPTS} 次重连…`;
});

const searchStatus = computed(() => {
  if (!query.value) return "";
  if (regexInvalid.value) return "正则有误";
  return resultCount.value > 0 ? `${resultIndex.value + 1}/${resultCount.value}` : "无匹配";
});

function resetSearchResults() {
  resultIndex.value = 0;
  resultCount.value = 0;
}

function runSearch(incremental = true) {
  regexInvalid.value = false;
  if (!query.value) {
    terminalManager.clearSearch(props.tab.id);
    resetSearchResults();
    return;
  }
  if (regex.value) {
    try {
      new RegExp(query.value);
    } catch {
      regexInvalid.value = true;
      terminalManager.clearSearch(props.tab.id);
      resetSearchResults();
      return;
    }
  }
  terminalManager.searchNext(props.tab.id, query.value, {
    caseSensitive: caseSensitive.value,
    regex: regex.value,
    incremental,
  });
}

function findNext() {
  if (!query.value || regexInvalid.value) return;
  terminalManager.searchNext(props.tab.id, query.value, {
    caseSensitive: caseSensitive.value,
    regex: regex.value,
  });
}

function findPrevious() {
  if (!query.value || regexInvalid.value) return;
  terminalManager.searchPrevious(props.tab.id, query.value, {
    caseSensitive: caseSensitive.value,
    regex: regex.value,
  });
}

async function openSearch() {
  searchOpen.value = true;
  await nextTick();
  searchInput.value?.focus();
  searchInput.value?.select();
  if (query.value) runSearch(false);
}

function closeSearch() {
  searchOpen.value = false;
  query.value = "";
  regexInvalid.value = false;
  resetSearchResults();
  terminalManager.clearSearch(props.tab.id);
  terminalManager.focus(props.tab.id);
}

function onKeydown(event: KeyboardEvent) {
  if (!props.active) return;
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
    // 阻止 xterm 把 Ctrl+F 当作控制字符发给远端
    event.preventDefault();
    event.stopPropagation();
    void openSearch();
    return;
  }
  if (searchOpen.value && event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    closeSearch();
  }
}

watch([caseSensitive, regex], () => {
  if (searchOpen.value && query.value) runSearch(false);
});

function defaultLogName() {
  const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
  const title = props.tab.title.replace(/[\\/:*?"<>|]/g, "-");
  return `${title}-${stamp}.log`;
}

async function toggleRecording() {
  if (!recording.value) {
    const path = await save({
      title: "保存终端日志",
      defaultPath: defaultLogName(),
      filters: [{ name: "终端日志", extensions: ["log", "txt"] }],
    });
    if (!path) return;
    try {
      await terminalManager.startRecording(props.tab.id, path);
      recording.value = true;
      toast.success("已开始录制终端日志", { description: path });
    } catch (e) {
      toast.error("无法开始录制日志", { description: String(e) });
    }
    return;
  }

  const path = await terminalManager.stopRecording(props.tab.id);
  recording.value = false;
  if (path) toast.success("终端日志已保存", { description: path });
}

function retry() {
  terminalManager.retry(props.tab.id);
}

async function stopReconnect() {
  stopping.value = true;
  try {
    await terminalManager.stopReconnect(props.tab.id);
  } finally {
    stopping.value = false;
  }
}

onMounted(() => {
  window.addEventListener("keydown", onKeydown, true);
  if (!hostEl.value || !props.tab.hostId) return;

  store.setTabState(props.tab.id, "connecting");
  terminalManager.mount(props.tab.id, props.tab.hostId, hostEl.value, {
    onReady: () => {
      reconnecting.value = null;
      retryable.value = false;
      lastError.value = null;
      store.setTabState(props.tab.id, "connected");
    },
    onReconnecting: (attempt, delayMs) => {
      reconnecting.value = { attempt, delayMs };
      retryable.value = false;
      lastError.value = null;
      store.setTabState(props.tab.id, "connecting");
    },
    onExit: (error, canRetry) => {
      reconnecting.value = null;
      retryable.value = Boolean(canRetry);
      lastError.value = error ?? null;
      store.setTabState(props.tab.id, error ? "failed" : "lost");
      if (error) toast.error("连接中断", { description: error });
    },
    onSearchResults: (index, count) => {
      resultIndex.value = index;
      resultCount.value = count;
    },
  });
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown, true);
});

// 标签切回来时重新贴合容器，并让键盘焦点回到终端
watch(
  () => props.active,
  (active) => {
    if (!active) return;
    terminalManager.fit(props.tab.id);
    terminalManager.focus(props.tab.id);
  },
);
</script>

<template>
  <div class="term-root">
    <div ref="hostEl" class="term-pane" />
    <div class="term-tools">
      <button class="term-tool" title="搜索（Ctrl+F）" @click="openSearch">
        <Search :size="14" />
      </button>
      <button
        class="term-tool"
        :class="{ 'is-recording': recording }"
        :title="recording ? '停止录制日志' : '录制终端日志'"
        @click="toggleRecording"
      >
        <Circle :size="14" :fill="recording ? 'currentColor' : 'none'" />
      </button>
    </div>
    <div v-if="searchOpen" class="term-search" role="search">
      <input
        ref="searchInput"
        v-model="query"
        class="term-search-input"
        placeholder="搜索终端输出"
        spellcheck="false"
        @input="runSearch()"
        @keydown.enter.exact.prevent="findNext"
        @keydown.shift.enter.prevent="findPrevious"
      />
      <span v-if="query" class="term-search-count">{{ searchStatus }}</span>
      <button
        class="term-search-toggle"
        :class="{ active: caseSensitive }"
        title="区分大小写"
        @click="caseSensitive = !caseSensitive"
      >
        Aa
      </button>
      <button
        class="term-search-toggle"
        :class="{ active: regex }"
        title="正则表达式"
        @click="regex = !regex"
      >
        .*
      </button>
      <button class="term-search-btn" title="上一个（Shift+Enter）" @click="findPrevious">
        <ChevronUp :size="14" />
      </button>
      <button class="term-search-btn" title="下一个（Enter）" @click="findNext">
        <ChevronDown :size="14" />
      </button>
      <button class="term-search-btn" title="关闭（Esc）" @click="closeSearch">
        <X :size="14" />
      </button>
    </div>
    <div v-if="reconnecting" class="term-status" role="status">
      <span>{{ reconnectText }}</span>
      <Button variant="secondary" size="sm" :disabled="stopping" @click="stopReconnect">
        {{ stopping ? "停止中…" : "停止重连" }}
      </Button>
    </div>
    <div v-else-if="retryable" class="term-status" role="status">
      <span>{{ lastError ? "自动重连失败，可手动重试" : "已停止自动重连" }}</span>
      <Button variant="secondary" size="sm" @click="retry">重新连接</Button>
    </div>
  </div>
</template>

<style scoped lang="scss">
.term-root {
  position: absolute;
  inset: 0;
}

.term-pane {
  position: absolute;
  inset: 0;
  padding: 4px 0 0 6px;
  // 终端区保留文本选择与复制
  user-select: text;
}

.term-status {
  position: absolute;
  top: 12px;
  left: 50%;
  z-index: 10;
  display: flex;
  align-items: center;
  gap: 12px;
  max-width: calc(100% - 32px);
  padding: 8px 10px 8px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
  color: var(--card-foreground);
  box-shadow: 0 4px 16px rgb(0 0 0 / 12%);
  font-size: 12px;
  transform: translateX(-50%);

  :deep(button) {
    flex: none;
    height: 28px;
    font-size: 12px;
  }
}

.term-tools {
  position: absolute;
  top: 12px;
  right: 12px;
  z-index: 11;
  display: flex;
  gap: 2px;
  padding: 2px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
  color: var(--muted-foreground);
  box-shadow: 0 4px 16px rgb(0 0 0 / 12%);
}

.term-tool {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: calc(var(--radius) - 2px);

  &:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  &.is-recording {
    color: #dc2626;
  }
}

.term-search {
  position: absolute;
  top: 48px;
  right: 12px;
  z-index: 11;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 6px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--card);
  color: var(--card-foreground);
  box-shadow: 0 4px 16px rgb(0 0 0 / 12%);
}

.term-search-input {
  width: 180px;
  height: 26px;
  padding: 0 6px;
  border: 1px solid var(--border);
  border-radius: calc(var(--radius) - 2px);
  outline: none;
  background: var(--background);
  color: var(--foreground);
  font-size: 12px;

  &:focus-visible {
    border-color: var(--ring);
  }
}

.term-search-count {
  min-width: 42px;
  color: var(--muted-foreground);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  text-align: center;
}

.term-search-toggle,
.term-search-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: calc(var(--radius) - 2px);
  color: var(--muted-foreground);
  font-size: 11px;

  &:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }
}

.term-search-toggle.active {
  background: var(--accent);
  color: var(--accent-foreground);
}
</style>
