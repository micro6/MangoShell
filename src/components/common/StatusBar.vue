<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

interface AppHealth {
  version: string;
  dbPath: string;
  schemaVersion: number;
  hostCount: number;
}

const health = ref<AppHealth | null>(null);
const error = ref<string | null>(null);

onMounted(async () => {
  try {
    health.value = await invoke<AppHealth>("app_health");
  } catch (e) {
    error.value = String(e);
  }
});
</script>

<template>
  <footer class="statusbar">
    <span v-if="health" class="item">
      <span class="dot" />
      就绪 · v{{ health.version }} · 主机 {{ health.hostCount }} 台 · schema v{{ health.schemaVersion }}
    </span>
    <span v-else-if="error" class="item item--error">后端未就绪：{{ error }}</span>
    <span v-else class="item">初始化中…</span>
  </footer>
</template>

<style lang="scss" scoped>
.statusbar {
  display: flex;
  align-items: center;
  height: $statusbar-height;
  padding: 0 10px;
  border-top: 1px solid var(--border);
  background: var(--card);
  font-size: 11px;
  color: var(--muted-foreground);
  flex: none;
}

.item {
  display: flex;
  align-items: center;
  gap: 6px;
  @include ellipsis;

  &--error {
    color: var(--destructive);
  }
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  background: var(--success);
  flex: none;
}
</style>
