<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "vue-sonner";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Button } from "@/components/ui/button";
import FilePane from "./FilePane.vue";
import TransferQueue from "./TransferQueue.vue";
import { useTransfersStore } from "@/stores/transfers";
import { useWorkspaceStore, type WorkspaceTab } from "@/stores/workspace";

interface FileRow {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
}

const props = defineProps<{ tab: WorkspaceTab; active: boolean }>();
const store = useWorkspaceStore();
const transfers = useTransfersStore();

const ready = ref(false);
const error = ref("");
const localPath = ref("");
const remotePath = ref("");
const localPane = ref<InstanceType<typeof FilePane> | null>(null);
const remotePane = ref<InstanceType<typeof FilePane> | null>(null);

async function open() {
  if (!props.tab.hostId) return;
  error.value = "";
  try {
    const home = await invoke<string>("local_home");
    const opened = await invoke<{ home: string }>("sftp_open", {
      tabId: props.tab.id,
      hostId: props.tab.hostId,
      password: null,
    });
    localPath.value = home;
    remotePath.value = opened.home;
    store.setTabState(props.tab.id, "connected");
    ready.value = true;
  } catch (e) {
    error.value = String(e);
    store.setTabState(props.tab.id, "failed");
    toast.error("SFTP 打开失败", { description: error.value });
  }
}

onMounted(() => {
  void open();
});

onUnmounted(() => {
  void transfers.cancelTab(props.tab.id);
  void invoke("sftp_close", { tabId: props.tab.id }).catch(() => {});
});

// 传输完成后刷新两侧列表
watch(
  () => transfers.items.filter((i) => i.tabId === props.tab.id && i.status === "done").length,
  (count, prev) => {
    if (count > (prev ?? 0)) {
      localPane.value?.refresh();
      remotePane.value?.refresh();
    }
  },
);

function joinLocal(dir: string, name: string) {
  const sep = dir.includes("\\") ? "\\" : "/";
  return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`;
}

function joinRemote(dir: string, name: string) {
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

function handleUpload(rows: FileRow[]) {
  transfers.enqueue(
    rows.map((row) => ({
      tabId: props.tab.id,
      direction: "upload" as const,
      localPath: row.path,
      remotePath: joinRemote(remotePath.value, row.name),
      size: 0,
    })),
  );
}

function handleDownload(rows: FileRow[]) {
  transfers.enqueue(
    rows.map((row) => {
      const existing = localPane.value?.findFile(row.name) ?? null;
      return {
        tabId: props.tab.id,
        direction: "download" as const,
        localPath: joinLocal(localPath.value, row.name),
        remotePath: row.path,
        size: row.size,
        // 本地文件比远端更长时，后端会截断重写
        overwrite: Boolean(existing) && existing!.size > row.size,
      };
    }),
  );
}
</script>

<template>
  <div class="sftp-pane">
    <div v-if="error" class="loading">
      <p class="loading-error">{{ error }}</p>
      <Button variant="secondary" size="sm" class="h-7 text-[12px]" @click="open">重试</Button>
    </div>
    <div v-else-if="!ready" class="loading">正在打开 SFTP 会话…</div>

    <template v-else>
      <ResizablePanelGroup direction="horizontal" class="panes">
        <ResizablePanel :default-size="50" :min-size="20">
          <FilePane
            ref="localPane"
            v-model:path="localPath"
            side="local"
            :tab-id="tab.id"
            @upload="handleUpload"
            @download="handleDownload"
          />
        </ResizablePanel>
        <ResizableHandle with-handle />
        <ResizablePanel :default-size="50" :min-size="20">
          <FilePane
            ref="remotePane"
            v-model:path="remotePath"
            side="remote"
            :tab-id="tab.id"
            @upload="handleUpload"
            @download="handleDownload"
          />
        </ResizablePanel>
      </ResizablePanelGroup>

      <TransferQueue :tab-id="tab.id" />
    </template>
  </div>
</template>

<style scoped lang="scss">
.sftp-pane {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
  user-select: none;
}

.panes {
  flex: 1;
  min-height: 0;
}

.loading {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  font-size: 12px;
  color: var(--muted-foreground);
}

.loading-error {
  max-width: 60%;
  text-align: center;
  color: var(--destructive);
  word-break: break-word;
}
</style>
