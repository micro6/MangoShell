<script setup lang="ts">
import { computed, ref } from "vue";
import {
  ArrowDown,
  ArrowUp,
  ChevronDown,
  ChevronUp,
  RotateCw,
  Trash2,
  X,
} from "@lucide/vue";
import { Button } from "@/components/ui/button";
import { formatBytes, useTransfersStore, type TransferItem } from "@/stores/transfers";

const props = defineProps<{ tabId: string }>();
const transfers = useTransfersStore();
const collapsed = ref(false);

const items = computed(() => transfers.items.filter((i) => i.tabId === props.tabId));
const activeCount = computed(
  () => items.value.filter((i) => i.status === "running" || i.status === "pending").length,
);

const percent = (item: TransferItem) =>
  item.size > 0 ? Math.min(100, Math.round((item.transferred / item.size) * 100)) : 0;

const statusText: Record<TransferItem["status"], string> = {
  pending: "等待",
  running: "传输中",
  done: "完成",
  failed: "失败",
  cancelled: "已取消",
};
</script>

<template>
  <section v-if="items.length" class="queue" :class="{ 'queue--collapsed': collapsed }">
    <header class="head">
      <button type="button" class="toggle" @click="collapsed = !collapsed">
        <component :is="collapsed ? ChevronUp : ChevronDown" :size="13" />
        <span>传输队列</span>
        <span class="count">{{ items.length }}</span>
        <span v-if="activeCount" class="active">{{ activeCount }} 个进行中</span>
      </button>
      <Button
        variant="ghost"
        size="icon"
        class="h-6 w-6"
        title="清理已完成"
        @click="transfers.clearFinished()"
      >
        <Trash2 :size="13" />
      </Button>
    </header>

    <div v-if="!collapsed" class="list">
      <div v-for="item in items" :key="item.id" class="row">
        <component
          :is="item.direction === 'upload' ? ArrowUp : ArrowDown"
          :size="13"
          class="dir"
          :class="`dir--${item.direction}`"
        />
        <div class="main">
          <div class="line">
            <span class="name" :title="item.direction === 'upload' ? item.localPath : item.remotePath">
              {{ item.name }}
            </span>
            <span v-if="item.overwrite" class="badge" title="本地同名文件更长，将截断重写">覆盖</span>
            <span class="meta">
              {{ formatBytes(item.transferred) }} / {{ formatBytes(item.size) }}
              <template v-if="item.status === 'running' && item.speed > 0">
                · {{ formatBytes(item.speed) }}/s
              </template>
            </span>
          </div>
          <div class="track">
            <div
              class="bar"
              :class="`bar--${item.status}`"
              :style="{ width: `${item.status === 'done' ? 100 : percent(item)}%` }"
            />
          </div>
          <p v-if="item.error" class="error">{{ item.error }}</p>
        </div>
        <span class="status" :class="`status--${item.status}`">{{ statusText[item.status] }}</span>
        <div class="actions">
          <Button
            v-if="item.status === 'running' || item.status === 'pending'"
            variant="ghost"
            size="icon"
            class="h-6 w-6"
            title="取消"
            @click="transfers.cancel(item.id)"
          >
            <X :size="12" />
          </Button>
          <Button
            v-else-if="item.status === 'failed' || item.status === 'cancelled'"
            variant="ghost"
            size="icon"
            class="h-6 w-6"
            title="重试"
            @click="transfers.retry(item.id)"
          >
            <RotateCw :size="12" />
          </Button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped lang="scss">
.queue {
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--border);
  background: var(--card);
  flex: none;
  max-height: 180px;

  &--collapsed {
    max-height: none;
  }
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 26px;
  padding: 0 6px;
  flex: none;
}

.toggle {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--muted-foreground);
  cursor: default;
}

.count {
  padding: 0 5px;
  border-radius: 999px;
  background: var(--secondary);
  font-size: 11px;
}

.active {
  font-size: 11px;
  color: var(--primary);
}

.list {
  overflow-y: auto;
  padding: 0 6px 6px;
  @include scrollbar(8px);
}

.row {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 4px 0;
  border-top: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
}

.dir {
  margin-top: 2px;
  flex: none;

  &--upload {
    color: var(--primary);
  }

  &--download {
    color: var(--success);
  }
}

.main {
  flex: 1;
  min-width: 0;
}

.line {
  display: flex;
  align-items: baseline;
  gap: 6px;
}

.name {
  @include ellipsis;
  font-size: 12px;
}

.badge {
  flex: none;
  padding: 0 4px;
  border-radius: 3px;
  font-size: 10px;
  color: var(--warning);
  background: color-mix(in srgb, var(--warning) 14%, transparent);
}

.meta {
  margin-left: auto;
  font-size: 11px;
  color: var(--muted-foreground);
  white-space: nowrap;
}

.track {
  height: 3px;
  margin-top: 3px;
  border-radius: 999px;
  background: var(--border);
  overflow: hidden;
}

.bar {
  height: 100%;
  background: var(--primary);
  transition: width 120ms linear;

  &--done {
    background: var(--success);
  }

  &--failed {
    background: var(--destructive);
  }

  &--cancelled {
    background: var(--muted-foreground);
  }
}

.error {
  margin-top: 2px;
  font-size: 11px;
  color: var(--destructive);
  @include ellipsis;
}

.status {
  flex: none;
  font-size: 11px;
  color: var(--muted-foreground);

  &--running {
    color: var(--primary);
  }

  &--done {
    color: var(--success);
  }

  &--failed {
    color: var(--destructive);
  }
}

.actions {
  width: 24px;
  flex: none;
}
</style>
