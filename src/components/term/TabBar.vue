<script setup lang="ts">
import { FolderOpen, Waypoints, X } from "@lucide/vue";
import { useWorkspaceStore, type WorkspaceTab } from "@/stores/workspace";

const store = useWorkspaceStore();

const stateClass = (tab: WorkspaceTab) => {
  switch (tab.state) {
    case "connected":
      return "bg-[var(--success)]";
    case "connecting":
    case "authenticating":
      return "bg-[var(--warning)] animate-pulse";
    case "failed":
      return "bg-[var(--destructive)]";
    default:
      return "bg-[var(--muted-foreground)]";
  }
};
</script>

<template>
  <div class="tabbar">
    <div class="brand">MangoShell</div>

    <div class="tabs">
      <button
        v-for="tab in store.tabs"
        :key="tab.id"
        type="button"
        class="tab"
        :class="{ 'tab--active': tab.id === store.activeId }"
        :title="`${tab.title}（${tab.kind}）`"
        @click="store.activate(tab.id)"
        @auxclick.middle.prevent="store.closeTab(tab.id)"
      >
        <span class="dot" :class="stateClass(tab)" />
        <FolderOpen v-if="tab.kind === 'sftp'" :size="12" class="kind" />
        <Waypoints v-else-if="tab.kind === 'forward'" :size="12" class="kind" />
        <span class="label">{{ tab.title }}</span>
        <span class="close" @click.stop="store.closeTab(tab.id)">
          <X :size="12" />
        </span>
      </button>
    </div>
  </div>
</template>

<style lang="scss" scoped>
.tabbar {
  display: flex;
  align-items: center;
  height: $tabbar-height;
  padding: 0 8px;
  gap: 8px;
  border-bottom: 1px solid var(--border);
  background: var(--card);
  flex: none;
}

.brand {
  font-weight: 600;
  font-size: 12px;
  letter-spacing: 0.2px;
  color: var(--muted-foreground);
  padding-right: 4px;
}

.tabs {
  display: flex;
  align-items: stretch;
  gap: 2px;
  overflow-x: auto;
  scrollbar-width: none;

  &::-webkit-scrollbar {
    display: none;
  }
}

.tab {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  max-width: 200px;
  padding: 0 6px 0 8px;
  border-radius: var(--radius);
  color: var(--muted-foreground);
  background: transparent;
  cursor: default;

  &:hover {
    background: var(--secondary);
  }

  &--active {
    background: var(--secondary);
    color: var(--foreground);
  }
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  flex: none;
}

.kind {
  flex: none;
  color: var(--muted-foreground);
}

.label {
  @include ellipsis;
  font-size: 12px;
}

.close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  border-radius: 3px;
  opacity: 0;

  &:hover {
    background: var(--border);
  }

  .tab:hover &,
  .tab--active & {
    opacity: 1;
  }
}
</style>
