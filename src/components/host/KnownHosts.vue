<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ShieldCheck, Trash2 } from "@lucide/vue";
import { toast } from "vue-sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface KnownHostRecord {
  host: string;
  port: number;
  keyType: string;
  fingerprint: string;
  addedAt: number;
}

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ "update:open": [value: boolean] }>();

const items = ref<KnownHostRecord[]>([]);
const loading = ref(false);

async function load() {
  loading.value = true;
  try {
    items.value = await invoke<KnownHostRecord[]>("known_host_list");
  } catch (e) {
    toast.error("读取已知主机失败", { description: String(e) });
  } finally {
    loading.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) void load();
  },
);

async function remove(item: KnownHostRecord) {
  try {
    await invoke("known_host_delete", {
      host: item.host,
      port: item.port,
      keyType: item.keyType,
    });
    items.value = items.value.filter(
      (i) => !(i.host === item.host && i.port === item.port && i.keyType === item.keyType),
    );
    toast.success("已移除信任记录，下次连接将重新校验");
  } catch (e) {
    toast.error("删除失败", { description: String(e) });
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="(value) => emit('update:open', value)">
    <DialogContent class="known-hosts w-[560px] max-w-[92vw]">
      <DialogHeader>
        <DialogTitle class="text-[14px]">已知主机</DialogTitle>
        <DialogDescription class="text-[12px]">
          首次连接会自动记录主机密钥；删除后下次连接重新按首次信任处理
        </DialogDescription>
      </DialogHeader>

      <div class="list">
        <div v-if="loading" class="empty">正在加载…</div>
        <div v-else-if="!items.length" class="empty">
          <ShieldCheck :size="20" class="empty-icon" />
          <p>还没有已知主机，连接成功后会自动记录</p>
        </div>
        <template v-else>
          <div
            v-for="item in items"
            :key="`${item.host}:${item.port}:${item.keyType}`"
            class="item"
          >
            <div class="item-main">
              <div class="item-title">
                <span class="addr">{{ item.host }}:{{ item.port }}</span>
                <span class="type">{{ item.keyType }}</span>
              </div>
              <div class="fp" :title="item.fingerprint">{{ item.fingerprint }}</div>
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              class="h-7 w-7"
              title="删除信任记录"
              @click="remove(item)"
            >
              <Trash2 :size="13" />
            </Button>
          </div>
        </template>
      </div>
    </DialogContent>
  </Dialog>
</template>

<style scoped lang="scss">
.known-hosts {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 360px;
  overflow-y: auto;
  @include scrollbar(8px);
}

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 28px 12px;
  font-size: 12px;
  color: var(--muted-foreground);
  text-align: center;
}

.empty-icon {
  color: var(--muted-foreground);
}

.item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: var(--radius);

  &:hover {
    background: var(--secondary);
  }
}

.item-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.item-title {
  display: flex;
  align-items: center;
  gap: 6px;
}

.addr {
  font-size: 12px;
  @include ellipsis;
}

.type {
  font-size: 10px;
  color: var(--muted-foreground);
  border: 1px solid var(--border);
  border-radius: 4px;
  padding: 0 4px;
  flex: none;
}

.fp {
  font-family: ui-monospace, monospace;
  font-size: 11px;
  color: var(--muted-foreground);
  @include ellipsis;
}
</style>
