<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { FolderOpen, FolderPlus, PanelLeft, Plug, Upload, Waypoints } from "@lucide/vue";
import { toast } from "vue-sonner";
import HostTree from "@/components/host/HostTree.vue";
import HostEditor from "@/components/host/HostEditor.vue";
import KeyManager from "@/components/host/KeyManager.vue";
import KnownHosts from "@/components/host/KnownHosts.vue";
import QuickConnect, { type ConnectPayload } from "@/components/host/QuickConnect.vue";
import StatusBar from "@/components/common/StatusBar.vue";
import ForwardPane from "@/components/forward/ForwardPane.vue";
import SftpPane from "@/components/sftp/SftpPane.vue";
import TabBar from "@/components/term/TabBar.vue";
import TerminalPane from "@/components/term/TerminalPane.vue";
import { Button } from "@/components/ui/button";
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
import { Toaster } from "@/components/ui/sonner";
import { terminalManager } from "@/core/term/manager";
import { useHostsStore, type HostRecord } from "@/stores/hosts";
import { useWorkspaceStore } from "@/stores/workspace";

const store = useWorkspaceStore();
const hosts = useHostsStore();
const activeTab = computed(() => store.tabs.find((t) => t.id === store.activeId) ?? null);

/** 只有已连接的终端标签才能开 SFTP，复用同一条 SSH 连接 */
const canOpenSftp = computed(
  () =>
    activeTab.value?.kind === "terminal" &&
    activeTab.value.state === "connected" &&
    Boolean(activeTab.value.hostId),
);

function openSftp() {
  const tab = activeTab.value;
  if (!tab?.hostId || tab.kind !== "terminal") return;
  store.openTab({
    title: `${tab.title} · SFTP`,
    kind: "sftp",
    state: "connected",
    hostId: tab.hostId,
  });
}

/** 隧道属于主机，已连接的标签都可以打开 */
const canOpenForward = computed(
  () => Boolean(activeTab.value?.hostId) && activeTab.value?.state === "connected",
);

function openForward() {
  const tab = activeTab.value;
  if (!tab?.hostId) return;
  const alias = hosts.items.find((h) => h.id === tab.hostId)?.alias ?? tab.title;
  store.openTab({
    title: `${alias} · 隧道`,
    kind: "forward",
    state: "connected",
    hostId: tab.hostId,
  });
}

const sheetOpen = ref(false);
const preset = ref<HostRecord | null>(null);
const editorOpen = ref(false);
const editorHost = ref<HostRecord | null>(null);
const keysOpen = ref(false);
const knownHostsOpen = ref(false);
const deleteTarget = ref<HostRecord | null>(null);

function openCreate() {
  preset.value = null;
  sheetOpen.value = true;
}

function openConnect(host: HostRecord) {
  preset.value = host;
  sheetOpen.value = true;
}

function openEditor(host: HostRecord | null) {
  editorHost.value = host;
  editorOpen.value = true;
}

/** 密钥主机直接建连，不再弹密码框 */
function handleConnect(host: HostRecord) {
  if (host.authType === "key") {
    store.openTab({
      title: host.alias,
      kind: "terminal",
      state: "connecting",
      hostId: host.id,
    });
    return;
  }
  openConnect(host);
}

function handleHostSaved(host: HostRecord) {
  toast.success("主机已保存", { description: `${host.username}@${host.host}:${host.port}` });
}

async function confirmDeleteHost() {
  const host = deleteTarget.value;
  if (!host) return;
  try {
    await hosts.remove(host.id);
    // 关闭该主机的全部标签，实例与通道由 TerminalManager 释放
    for (const tab of store.tabs.filter((t) => t.hostId === host.id)) {
      store.closeTab(tab.id);
    }
    toast.success("主机已删除");
  } catch (e) {
    toast.error("删除主机失败", { description: String(e) });
  } finally {
    deleteTarget.value = null;
  }
}

async function handleSubmit(payload: ConnectPayload) {
  let host = preset.value;
  if (!host) {
    try {
      host = await invoke<HostRecord>("host_save", {
        id: null,
        alias: payload.alias,
        host: payload.host,
        port: payload.port,
        username: payload.username,
        authType: "password",
        keyId: null,
      });
    } catch (e) {
      toast.error("保存主机失败", { description: String(e) });
      return;
    }
    void hosts.load().catch(() => {});
  }

  if (payload.password) terminalManager.setPassword(host.id, payload.password);
  const before = store.tabs.length;
  store.openTab({
    title: host.alias,
    kind: "terminal",
    state: "connecting",
    hostId: host.id,
  });
  // 已存在同主机标签时不会新建，密码不应残留
  if (store.tabs.length === before) terminalManager.clearPassword(host.id);

  sheetOpen.value = false;
}

// 标签关闭后释放对应的 xterm 实例；SFTP 由组件卸载时自行关闭会话
watch(
  () => store.tabs.map((t) => ({ id: t.id, kind: t.kind })),
  (tabs, oldTabs) => {
    for (const old of oldTabs ?? []) {
      if (old.kind === "terminal" && !tabs.some((t) => t.id === old.id)) {
        terminalManager.dispose(old.id);
      }
    }
  },
);

// Ctrl+W 关闭当前标签，符合终端类工具的通用习惯
function onKeydown(e: KeyboardEvent) {
  if (e.ctrlKey && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "w" && store.activeId) {
    e.preventDefault();
    store.closeTab(store.activeId);
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div class="app">
    <TabBar @new-session="openCreate" />

    <div class="body">
      <HostTree
        v-if="!store.sidebarCollapsed"
        @create="openEditor(null)"
        @connect="handleConnect"
        @edit="openEditor"
        @delete="deleteTarget = $event"
        @manage-keys="keysOpen = true"
        @manage-known-hosts="knownHostsOpen = true"
      />

      <main class="workspace">
        <div class="workspace-toolbar">
          <Button
            variant="ghost"
            size="icon"
            class="h-7 w-7"
            title="折叠侧栏"
            @click="store.toggleSidebar()"
          >
            <PanelLeft :size="14" />
          </Button>
          <span class="crumb">{{ activeTab ? activeTab.title : "未打开会话" }}</span>
          <Button
            variant="ghost"
            size="icon"
            class="h-7 w-7"
            title="端口转发"
            :disabled="!canOpenForward"
            @click="openForward"
          >
            <Waypoints :size="14" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            class="ml-auto h-7 w-7"
            title="打开 SFTP 文件管理"
            :disabled="!canOpenSftp"
            @click="openSftp"
          >
            <FolderOpen :size="14" />
          </Button>
          <Button variant="ghost" size="icon" class="h-7 w-7" title="快速连接" @click="openCreate">
            <Plug :size="14" />
          </Button>
        </div>

        <div v-if="store.tabs.length" class="term-stack">
          <template v-for="tab in store.tabs" :key="tab.id">
            <TerminalPane
              v-if="tab.kind === 'terminal'"
              v-show="tab.id === store.activeId"
              :tab="tab"
              :active="tab.id === store.activeId"
            />
            <SftpPane
              v-else-if="tab.kind === 'sftp'"
              v-show="tab.id === store.activeId"
              :tab="tab"
              :active="tab.id === store.activeId"
            />
            <ForwardPane
              v-else-if="tab.kind === 'forward'"
              v-show="tab.id === store.activeId"
              :tab="tab"
              :active="tab.id === store.activeId"
            />
          </template>
        </div>

        <section v-else class="placeholder">
          <Plug :size="22" class="placeholder-icon" />
          <p class="placeholder-title">开始连接</p>
          <p class="placeholder-hint">输入主机信息，建立第一个 SSH 会话</p>
          <div class="actions">
            <Button size="sm" class="h-7 text-[12px]" @click="openEditor(null)">
              <FolderPlus :size="13" />新建主机
            </Button>
            <Button size="sm" variant="secondary" class="h-7 text-[12px]" disabled>
              <Upload :size="13" />导入 ~/.ssh/config
            </Button>
          </div>
        </section>
      </main>
    </div>

    <StatusBar />

    <QuickConnect v-model:open="sheetOpen" :preset="preset" @submit="handleSubmit" />
    <HostEditor v-model:open="editorOpen" :preset="editorHost" @saved="handleHostSaved" />
    <KeyManager v-model:open="keysOpen" />
    <KnownHosts v-model:open="knownHostsOpen" />

    <AlertDialog
      :open="Boolean(deleteTarget)"
      @update:open="(value) => (!value ? (deleteTarget = null) : null)"
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle class="text-[14px]">删除主机</AlertDialogTitle>
          <AlertDialogDescription class="text-[12px]">
            确定删除「{{ deleteTarget?.alias }}」吗？该主机的已打开标签会一并关闭。
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel class="h-8 text-[12px]">取消</AlertDialogCancel>
          <AlertDialogAction class="h-8 text-[12px]" @click="confirmDeleteHost"
            >删除</AlertDialogAction
          >
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>

    <Toaster position="top-right" />
  </div>
</template>

<style lang="scss" scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
  background: var(--background);
}

.body {
  display: flex;
  flex: 1;
  min-height: 0;
}

.workspace {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
}

.workspace-toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 32px;
  padding: 0 8px;
  border-bottom: 1px solid var(--border);
  flex: none;
}

.crumb {
  font-size: 12px;
  color: var(--muted-foreground);
  @include ellipsis;
}

.term-stack {
  position: relative;
  flex: 1;
  min-height: 0;
}

.placeholder {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  user-select: text;
}

.placeholder-icon {
  color: var(--muted-foreground);
}

.placeholder-title {
  font-size: 13px;
  font-weight: 500;
}

.placeholder-hint {
  font-size: 12px;
  color: var(--muted-foreground);
}

.actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}
</style>
