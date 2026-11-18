<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { toast } from "vue-sonner";
import { terminalManager } from "@/core/term/manager";
import { useWorkspaceStore, type WorkspaceTab } from "@/stores/workspace";

const props = defineProps<{ tab: WorkspaceTab; active: boolean }>();
const store = useWorkspaceStore();
const hostEl = ref<HTMLElement | null>(null);

onMounted(() => {
  if (!hostEl.value || !props.tab.hostId) return;

  store.setTabState(props.tab.id, "connecting");
  terminalManager.mount(props.tab.id, props.tab.hostId, hostEl.value, {
    onReady: () => store.setTabState(props.tab.id, "connected"),
    onExit: (error) => {
      store.setTabState(props.tab.id, error ? "failed" : "lost");
      if (error) toast.error("连接中断", { description: error });
    },
  });
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
  <div ref="hostEl" class="term-pane" />
</template>

<style scoped lang="scss">
.term-pane {
  position: absolute;
  inset: 0;
  padding: 4px 0 0 6px;
  // 终端区保留文本选择与复制
  user-select: text;
}
</style>
