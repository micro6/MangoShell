<script setup lang="ts">
import { ref, watch } from "vue";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Copy, FileKey, KeyRound, Trash2, Upload } from "@lucide/vue";
import { toast } from "vue-sonner";
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
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useKeysStore, type KeyRecord } from "@/stores/keys";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ "update:open": [value: boolean] }>();

const keys = useKeysStore();

const filePath = ref("");
const name = ref("");
const passphrase = ref("");
const formOpen = ref(false);
const busy = ref(false);
const deleteTarget = ref<KeyRecord | null>(null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    resetImport();
    void keys.load().catch(() => {});
  },
);

function resetImport() {
  filePath.value = "";
  name.value = "";
  passphrase.value = "";
  formOpen.value = false;
}

async function chooseFile() {
  try {
    const selected = await openFileDialog({
      multiple: false,
      directory: false,
      title: "选择 OpenSSH 私钥",
    });
    if (typeof selected !== "string") return;
    filePath.value = selected;
    name.value = selected.split(/[\\/]/).pop() ?? "";
    passphrase.value = "";
    formOpen.value = true;
  } catch (e) {
    toast.error("打开文件选择器失败", { description: String(e) });
  }
}

async function doImport() {
  if (!filePath.value) return;
  busy.value = true;
  try {
    await keys.importKey(name.value, filePath.value, passphrase.value);
    toast.success("密钥已导入");
    resetImport();
  } catch (e) {
    toast.error("导入密钥失败", { description: String(e) });
  } finally {
    busy.value = false;
  }
}

async function copyPublicKey(key: KeyRecord) {
  try {
    await writeText(key.publicKey);
    toast.success("公钥已复制");
  } catch (e) {
    toast.error("复制失败", { description: String(e) });
  }
}

async function confirmDelete() {
  const target = deleteTarget.value;
  if (!target) return;
  try {
    await keys.remove(target.id);
    toast.success("密钥已删除");
  } catch (e) {
    toast.error("删除失败", { description: String(e) });
  } finally {
    deleteTarget.value = null;
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="(value) => emit('update:open', value)">
    <DialogContent class="key-manager w-[520px] max-w-[92vw]">
      <DialogHeader>
        <DialogTitle class="text-[14px]">密钥管理</DialogTitle>
        <DialogDescription class="text-[12px]">
          私钥仅保存在本机数据库，导入后不会再要求输入口令
        </DialogDescription>
      </DialogHeader>

      <div class="toolbar">
        <Button size="sm" class="h-7 text-[12px]" @click="chooseFile">
          <Upload :size="13" />导入私钥
        </Button>
      </div>

      <div v-if="formOpen" class="import">
        <div class="import-file">
          <FileKey :size="13" />
          <span class="import-path">{{ filePath }}</span>
        </div>
        <div class="field">
          <Label for="km-name" class="text-[12px]">名称</Label>
          <Input id="km-name" v-model="name" class="h-8 text-[12px]" placeholder="例如：id_ed25519" />
        </div>
        <div class="field">
          <Label for="km-pass" class="text-[12px]">口令（私钥未加密可留空）</Label>
          <Input
            id="km-pass"
            v-model="passphrase"
            type="password"
            autocomplete="off"
            class="h-8 text-[12px]"
            placeholder="仅用于解密，不会保存"
          />
        </div>
        <div class="import-actions">
          <Button size="sm" class="h-7 text-[12px]" :disabled="busy" @click="doImport">
            {{ busy ? "导入中…" : "导入" }}
          </Button>
          <Button size="sm" variant="ghost" class="h-7 text-[12px]" @click="resetImport">取消</Button>
        </div>
      </div>

      <div class="list">
        <div v-if="keys.loading" class="empty">正在加载…</div>
        <div v-else-if="!keys.items.length" class="empty">
          <KeyRound :size="20" class="empty-icon" />
          <p>还没有密钥，导入一个 OpenSSH 私钥开始使用</p>
        </div>
        <template v-else>
          <div v-for="key in keys.items" :key="key.id" class="item">
            <div class="item-main">
              <div class="item-title">
                <span class="item-name">{{ key.name }}</span>
                <span class="item-type">{{ key.keyType }}</span>
              </div>
              <div class="fp" :title="key.fingerprint">{{ key.fingerprint }}</div>
            </div>
            <div class="item-actions">
              <Button
                variant="ghost"
                size="icon-sm"
                class="h-7 w-7"
                title="复制公钥"
                @click="copyPublicKey(key)"
              >
                <Copy :size="13" />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                class="h-7 w-7"
                title="删除密钥"
                @click="deleteTarget = key"
              >
                <Trash2 :size="13" />
              </Button>
            </div>
          </div>
        </template>
      </div>
    </DialogContent>
  </Dialog>

  <AlertDialog
    :open="Boolean(deleteTarget)"
    @update:open="(value) => (!value ? (deleteTarget = null) : null)"
  >
    <AlertDialogContent>
      <AlertDialogHeader>
        <AlertDialogTitle class="text-[14px]">删除密钥</AlertDialogTitle>
        <AlertDialogDescription class="text-[12px]">
          确定删除「{{ deleteTarget?.name }}」吗？被主机引用的密钥无法删除。
        </AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel class="h-8 text-[12px]">取消</AlertDialogCancel>
        <AlertDialogAction class="h-8 text-[12px]" @click="confirmDelete">删除</AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</template>

<style scoped lang="scss">
.key-manager {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.toolbar {
  display: flex;
  justify-content: flex-end;
}

.import {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--muted);
}

.import-file {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--muted-foreground);
}

.import-path {
  @include ellipsis;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.import-actions {
  display: flex;
  gap: 6px;
}

.list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 300px;
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

.item-name {
  font-size: 12px;
  @include ellipsis;
}

.item-type {
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

.item-actions {
  display: flex;
  gap: 2px;
  flex: none;
}
</style>
