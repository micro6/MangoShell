import { defineStore } from "pinia";
import { ref } from "vue";

export type TabKind = "terminal" | "sftp" | "forward" | "batch";
export type ConnState = "connecting" | "authenticating" | "connected" | "lost" | "failed";

export interface WorkspaceTab {
  id: string;
  title: string;
  kind: TabKind;
  state: ConnState;
  /** 关联的主机；终端标签必填 */
  hostId?: string;
}

/** 多窗口下标签 id 必须全局唯一，否则后端终端表会互相覆盖 */
const nextId = () => `tab-${crypto.randomUUID()}`;

/** 工作区只保存标签元数据，终端缓冲等重状态由 TerminalManager 持有 */
export const useWorkspaceStore = defineStore("workspace", () => {
  const tabs = ref<WorkspaceTab[]>([]);
  const activeId = ref<string | null>(null);
  const sidebarCollapsed = ref(false);

  function openTab(tab: Omit<WorkspaceTab, "id">) {
    // 同一主机的同类标签已存在则直接激活，避免重复
    const exist = tabs.value.find((t) =>
      tab.hostId
        ? t.hostId === tab.hostId && t.kind === tab.kind
        : t.title === tab.title && t.kind === tab.kind,
    );
    if (exist) {
      // 只有正常的标签才复用；failed / lost 说明连接已断，必须重建才能重连
      if (
        exist.state === "connected" ||
        exist.state === "connecting" ||
        exist.state === "authenticating"
      ) {
        activeId.value = exist.id;
        return exist;
      }
      tabs.value = tabs.value.filter((t) => t.id !== exist.id);
    }
    const created: WorkspaceTab = { ...tab, id: nextId() };
    tabs.value.push(created);
    activeId.value = created.id;
    return created;
  }

  function closeTab(id: string) {
    const index = tabs.value.findIndex((t) => t.id === id);
    if (index === -1) return;
    tabs.value.splice(index, 1);
    if (activeId.value === id) {
      activeId.value = tabs.value[Math.max(0, index - 1)]?.id ?? null;
    }
  }

  function setTabState(id: string, state: ConnState) {
    const tab = tabs.value.find((t) => t.id === id);
    if (tab) tab.state = state;
  }

  function activate(id: string) {
    activeId.value = id;
  }

  function toggleSidebar() {
    sidebarCollapsed.value = !sidebarCollapsed.value;
  }

  return {
    tabs,
    activeId,
    sidebarCollapsed,
    openTab,
    closeTab,
    setTabState,
    activate,
    toggleSidebar,
  };
});
