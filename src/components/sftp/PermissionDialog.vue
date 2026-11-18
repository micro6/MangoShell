<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const props = defineProps<{ open: boolean; tabId: string; path: string; mode: number }>();
const emit = defineEmits<{ "update:open": [value: boolean]; saved: [] }>();

const value = ref("");
const error = ref("");
const saving = ref(false);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    value.value = props.mode.toString(8).padStart(3, "0");
    error.value = "";
  },
);

async function submit() {
  const text = value.value.trim();
  if (!/^[0-7]{3,4}$/.test(text)) {
    error.value = "请输入 3~4 位八进制权限，例如 644、755、1777";
    return;
  }

  saving.value = true;
  try {
    await invoke("sftp_chmod", {
      tabId: props.tabId,
      path: props.path,
      mode: Number.parseInt(text, 8),
    });
    emit("saved");
    emit("update:open", false);
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="(next) => emit('update:open', next)">
    <DialogContent class="max-w-[320px]">
      <DialogHeader>
        <DialogTitle class="text-[14px]">修改权限</DialogTitle>
        <DialogDescription class="path text-[12px]">{{ path }}</DialogDescription>
      </DialogHeader>

      <form class="body" @submit.prevent="submit">
        <Input
          v-model="value"
          class="h-8 font-mono text-[12px]"
          placeholder="644"
          autofocus
          @keydown.enter.prevent="submit"
        />
        <p class="hint">八进制模式，例如 644（文件）、755（目录）</p>
        <p v-if="error" class="error">{{ error }}</p>

        <DialogFooter>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            class="h-8 text-[12px]"
            @click="emit('update:open', false)"
          >
            取消
          </Button>
          <Button type="submit" size="sm" class="h-8 text-[12px]" :disabled="saving">
            {{ saving ? "保存中…" : "保存" }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>

<style scoped lang="scss">
.path {
  @include ellipsis;
}

.body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.hint {
  font-size: 11px;
  color: var(--muted-foreground);
}

.error {
  font-size: 12px;
  color: var(--destructive);
}
</style>
