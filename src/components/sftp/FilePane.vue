<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowUp,
  Download,
  File,
  Folder,
  FolderPlus,
  LoaderCircle,
  Pencil,
  RefreshCw,
  Shield,
  Trash2,
  Upload,
} from "@lucide/vue";
import { toast } from "vue-sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import PermissionDialog from "./PermissionDialog.vue";
import { formatBytes } from "@/stores/transfers";

interface FileRow {
  name: string;
  path: string;
  isDir: boolean;
  isSymlink?: boolean;
  size: number;
  mode: number;
  mtime: number;
}

interface RemoteEntry extends FileRow {
  isSymlink: boolean;
}

interface LocalEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  mtime: number;
}

interface LocalDir {
  path: string;
  parent: string | null;
  entries: LocalEntry[];
}

const props = defineProps<{ side: "local" | "remote"; tabId: string; path: string }>();
const emit = defineEmits<{
  "update:path": [value: string];
  upload: [rows: FileRow[]];
  download: [rows: FileRow[]];
}>();

const isRemote = computed(() => props.side === "remote");
const rows = ref<FileRow[]>([]);
const parentPath = ref<string | null>(null);
const draft = ref(props.path);
const loading = ref(false);
const error = ref("");
const selected = ref<string[]>([]);

const selectedRows = computed(() => rows.value.filter((r) => selected.value.includes(r.path)));
const selectedFiles = computed(() => selectedRows.value.filter((r) => !r.isDir));

/** 远端 POSIX 父目录 */
function remoteParent(path: string) {
  const trimmed = path.replace(/\/+$/, "");
  const index = trimmed.lastIndexOf("/");
  return index <= 0 ? "/" : trimmed.slice(0, index);
}

function joinRemote(dir: string, name: string) {
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

function octal(mode: number) {
  return mode.toString(8).padStart(4, "0").slice(-4);
}

function formatTime(mtime: number) {
  if (!mtime) return "—";
  const date = new Date(mtime * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

async function load(path: string) {
  if (!path) return;
  loading.value = true;
  error.value = "";
  try {
    if (isRemote.value) {
      const list = await invoke<RemoteEntry[]>("sftp_list", { tabId: props.tabId, path });
      rows.value = list;
      parentPath.value = remoteParent(path);
    } else {
      const dir = await invoke<LocalDir>("local_list", { path });
      rows.value = dir.entries.map((e) => ({
        name: e.name,
        path: e.path,
        isDir: e.isDir,
        size: e.size,
        mode: 0,
        mtime: e.mtime,
      }));
      parentPath.value = dir.parent;
    }
    selected.value = [];
  } catch (e) {
    rows.value = [];
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

watch(
  () => props.path,
  (path) => {
    draft.value = path;
    void load(path);
  },
  { immediate: true },
);

function navigate(path: string) {
  const target = path.trim();
  if (!target) return;
  if (target === props.path) {
    void load(target);
    return;
  }
  emit("update:path", target);
}

function refresh() {
  void load(props.path);
}

/** 查找当前目录下的同名文件，供下载时判断是否会覆盖 */
function findFile(name: string) {
  return rows.value.find((r) => !r.isDir && r.name === name) ?? null;
}

defineExpose({ refresh, findFile });

// 多选：Ctrl / Cmd 切换，普通点击单选
function onRowClick(event: MouseEvent, row: FileRow) {
  if (event.ctrlKey || event.metaKey) {
    selected.value = selected.value.includes(row.path)
      ? selected.value.filter((p) => p !== row.path)
      : [...selected.value, row.path];
  } else {
    selected.value = [row.path];
  }
}

function onActivate(row: FileRow) {
  if (row.isDir) {
    navigate(row.path);
    return;
  }
  if (isRemote.value) emit("download", [row]);
  else emit("upload", [row]);
}

function transferSelected() {
  if (!selectedFiles.value.length) return;
  if (isRemote.value) emit("download", selectedFiles.value);
  else emit("upload", selectedFiles.value);
}

const menu = ref<{ x: number; y: number; row: FileRow } | null>(null);

function openMenu(event: MouseEvent, row: FileRow) {
  if (!selected.value.includes(row.path)) selected.value = [row.path];
  menu.value = { x: event.clientX, y: event.clientY, row };
}

function closeMenu() {
  menu.value = null;
}

// 提示输入框：新建目录 / 重命名共用
const prompt = reactive({
  open: false,
  mode: "mkdir" as "mkdir" | "rename",
  value: "",
  target: "",
  error: "",
});

function openPrompt(mode: "mkdir" | "rename", row?: FileRow) {
  prompt.mode = mode;
  prompt.value = mode === "rename" ? (row?.name ?? "") : "";
  prompt.target = row?.path ?? "";
  prompt.error = "";
  prompt.open = true;
}

async function submitPrompt() {
  const name = prompt.value.trim();
  if (!name) {
    prompt.error = "名称不能为空";
    return;
  }
  try {
    if (prompt.mode === "mkdir") {
      await invoke("sftp_mkdir", { tabId: props.tabId, path: joinRemote(props.path, name) });
    } else {
      await invoke("sftp_rename", {
        tabId: props.tabId,
        from: prompt.target,
        to: joinRemote(remoteParent(prompt.target), name),
      });
    }
    prompt.open = false;
    refresh();
  } catch (e) {
    prompt.error = String(e);
  }
}

// 权限编辑
const permission = reactive({ open: false, path: "", mode: 0 });

function openPermission(row: FileRow) {
  permission.path = row.path;
  permission.mode = row.mode;
  permission.open = true;
}

// 删除确认：目标在打开时快照，弹窗关闭不会清掉，避免确认回调拿到空数组
const deleteTargets = ref<FileRow[]>([]);
const deleteOpen = ref(false);

function askDelete(rows: FileRow[]) {
  deleteTargets.value = rows;
  deleteOpen.value = true;
}

async function confirmDelete() {
  const targets = [...deleteTargets.value];
  deleteOpen.value = false;
  for (const row of targets) {
    try {
      await invoke("sftp_delete", { tabId: props.tabId, path: row.path, isDir: row.isDir });
    } catch (e) {
      toast.error(`删除「${row.name}」失败`, { description: String(e) });
    }
  }
  refresh();
}

onMounted(() => document.addEventListener("click", closeMenu));
onUnmounted(() => document.removeEventListener("click", closeMenu));
</script>

<template>
  <div class="pane">
    <div class="bar">
      <Button
        variant="ghost"
        size="icon"
        class="h-6 w-6"
        title="上一级"
        :disabled="!parentPath"
        @click="parentPath && navigate(parentPath)"
      >
        <ArrowUp :size="13" />
      </Button>
      <form class="path-form" @submit.prevent="navigate(draft)">
        <Input v-model="draft" class="h-6 font-mono text-[11px]" spellcheck="false" />
      </form>
      <Button variant="ghost" size="icon" class="h-6 w-6" title="刷新" @click="refresh">
        <RefreshCw :size="13" />
      </Button>
      <Button
        v-if="isRemote"
        variant="ghost"
        size="icon"
        class="h-6 w-6"
        title="新建目录"
        @click="openPrompt('mkdir')"
      >
        <FolderPlus :size="13" />
      </Button>
      <Button
        v-if="selectedFiles.length"
        variant="secondary"
        size="sm"
        class="h-6 text-[11px]"
        @click="transferSelected"
      >
        <component :is="isRemote ? Download : Upload" :size="12" />
        {{ isRemote ? "下载" : "上传" }}
      </Button>
    </div>

    <div class="table-wrap">
      <table v-show="!loading" class="table">
        <thead>
          <tr>
            <th>名称</th>
            <th class="col-size">大小</th>
            <th class="col-time">修改时间</th>
            <th v-if="isRemote" class="col-mode">权限</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in rows"
            :key="row.path"
            :class="{ 'row--selected': selected.includes(row.path) }"
            @click="onRowClick($event, row)"
            @dblclick="onActivate(row)"
            @contextmenu.prevent="openMenu($event, row)"
          >
            <td class="col-name">
              <div class="name-cell">
                <component :is="row.isDir ? Folder : File" :size="13" class="icon" />
                <span class="name">{{ row.name }}</span>
                <span v-if="row.isSymlink" class="badge" title="符号链接">link</span>
              </div>
            </td>
            <td class="col-size">{{ row.isDir ? "—" : formatBytes(row.size) }}</td>
            <td class="col-time">{{ formatTime(row.mtime) }}</td>
            <td v-if="isRemote" class="col-mode">{{ row.mode ? octal(row.mode) : "—" }}</td>
          </tr>
        </tbody>
      </table>

      <div v-if="loading" class="state">
        <LoaderCircle :size="14" class="spin" />
        <span>加载中…</span>
      </div>
      <div v-else-if="error" class="state state--error">{{ error }}</div>
      <div v-else-if="!rows.length" class="state">空目录</div>
    </div>

    <div v-if="menu" class="menu" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }" @click.stop>
      <button
        v-if="!menu.row.isDir"
        type="button"
        class="menu-item"
        @click="
          isRemote ? emit('download', [menu!.row]) : emit('upload', [menu!.row]);
          closeMenu();
        "
      >
        <component :is="isRemote ? Download : Upload" :size="12" />
        {{ isRemote ? "下载" : "上传" }}
      </button>
      <button
        v-if="isRemote"
        type="button"
        class="menu-item"
        @click="
          openPrompt('rename', menu!.row);
          closeMenu();
        "
      >
        <Pencil :size="12" />重命名
      </button>
      <button
        v-if="isRemote"
        type="button"
        class="menu-item"
        @click="
          openPermission(menu!.row);
          closeMenu();
        "
      >
        <Shield :size="12" />权限
      </button>
      <button
        v-if="isRemote"
        type="button"
        class="menu-item"
        @click="
          askDelete([menu!.row]);
          closeMenu();
        "
      >
        <Trash2 :size="12" />删除
      </button>
      <button
        type="button"
        class="menu-item"
        @click="
          refresh();
          closeMenu();
        "
      >
        <RefreshCw :size="12" />刷新
      </button>
    </div>

    <Dialog :open="prompt.open" @update:open="(value) => (prompt.open = value)">
      <DialogContent class="max-w-[320px]">
        <DialogHeader>
          <DialogTitle class="text-[14px]">{{
            prompt.mode === "mkdir" ? "新建目录" : "重命名"
          }}</DialogTitle>
        </DialogHeader>
        <form class="prompt-body" @submit.prevent="submitPrompt">
          <Input v-model="prompt.value" class="h-8 text-[12px]" autofocus />
          <p v-if="prompt.error" class="error">{{ prompt.error }}</p>
          <DialogFooter>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              class="h-8 text-[12px]"
              @click="prompt.open = false"
            >
              取消
            </Button>
            <Button type="submit" size="sm" class="h-8 text-[12px]">确定</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>

    <PermissionDialog
      v-if="isRemote"
      v-model:open="permission.open"
      :tab-id="tabId"
      :path="permission.path"
      :mode="permission.mode"
      @saved="refresh"
    />

    <AlertDialog :open="deleteOpen" @update:open="(value) => (deleteOpen = value)">
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle class="text-[14px]">删除确认</AlertDialogTitle>
          <AlertDialogDescription class="text-[12px]">
            确定删除{{ deleteTargets[0]?.isDir ? "目录" : "文件" }}「{{
              deleteTargets[0]?.name
            }}」吗？目录必须为空。
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel class="h-8 text-[12px]">取消</AlertDialogCancel>
          <AlertDialogAction class="h-8 text-[12px]" @click="confirmDelete">删除</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </div>
</template>

<style scoped lang="scss">
.pane {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-width: 0;
}

.bar {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 30px;
  padding: 0 6px;
  border-bottom: 1px solid var(--border);
  flex: none;
}

.path-form {
  flex: 1;
  min-width: 0;
}

.table-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: auto;
  @include scrollbar(8px);
}

.table {
  flex: none;
  width: 100%;
  border-collapse: collapse;
  table-layout: fixed;
  font-size: 12px;

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    height: 24px;
    padding: 0 6px;
    text-align: left;
    font-weight: 500;
    font-size: 11px;
    color: var(--muted-foreground);
    background: var(--card);
    border-bottom: 1px solid var(--border);
  }

  td {
    height: 24px;
    padding: 0 6px;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 50%, transparent);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  tbody tr:hover {
    background: var(--secondary);
  }
}

.row--selected {
  background: color-mix(in srgb, var(--primary) 14%, transparent);
}

.col-name {
  overflow: hidden;
}

.name-cell {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
}

.icon {
  flex: none;
  color: var(--muted-foreground);
}

.name {
  @include ellipsis;
}

.badge {
  flex: none;
  padding: 0 4px;
  border-radius: 3px;
  font-size: 10px;
  color: var(--muted-foreground);
  background: var(--secondary);
}

.col-size {
  width: 80px;
  text-align: right;
}

.col-time {
  width: 124px;
}

.col-mode {
  width: 58px;
  font-family: var(--font-mono, monospace);
}

// 加载 / 空目录 / 错误统一居中展示，避免提示掉在表格底部
.state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 16px 12px;
  font-size: 12px;
  color: var(--muted-foreground);

  &--error {
    color: var(--destructive);
    text-align: center;
    word-break: break-word;
  }
}

.spin {
  animation: spin 0.9s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.menu {
  position: fixed;
  z-index: 50;
  min-width: 128px;
  padding: 4px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--popover, var(--card));
  box-shadow: 0 8px 24px rgb(0 0 0 / 18%);
}

.menu-item {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  height: 26px;
  padding: 0 8px;
  border-radius: calc(var(--radius) - 2px);
  font-size: 12px;
  text-align: left;
  color: var(--foreground);
  cursor: default;

  &:hover {
    background: var(--secondary);
  }
}

.prompt-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.error {
  font-size: 12px;
  color: var(--destructive);
}
</style>
