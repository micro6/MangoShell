<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { KeyRound, Save, ShieldCheck } from "@lucide/vue";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useHostsStore, type HostRecord } from "@/stores/hosts";
import { useKeysStore } from "@/stores/keys";

const props = defineProps<{ open: boolean; preset?: HostRecord | null }>();
const emit = defineEmits<{
  "update:open": [value: boolean];
  saved: [host: HostRecord];
}>();

const hosts = useHostsStore();
const keys = useKeysStore();

const form = reactive({
  alias: "",
  host: "",
  port: "22",
  username: "",
  authType: "password",
  keyId: "",
});
const error = ref("");
const saving = ref(false);

const isEdit = computed(() => Boolean(props.preset));

function fill() {
  form.alias = props.preset?.alias ?? "";
  form.host = props.preset?.host ?? "";
  form.port = String(props.preset?.port ?? 22);
  form.username = props.preset?.username ?? "";
  form.authType = props.preset?.authType ?? "password";
  form.keyId = props.preset?.keyId ?? "";
  error.value = "";
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    fill();
    void keys.load().catch(() => {});
  },
);

async function submit() {
  const port = Number(form.port);
  if (!form.host.trim() || !form.username.trim()) {
    error.value = "主机地址和用户名不能为空";
    return;
  }
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    error.value = "端口需在 1-65535 之间";
    return;
  }
  if (form.authType === "key" && !form.keyId) {
    error.value = "请选择登录密钥";
    return;
  }

  saving.value = true;
  try {
    const saved = await hosts.save({
      id: props.preset?.id ?? null,
      alias: form.alias.trim(),
      host: form.host.trim(),
      port,
      username: form.username.trim(),
      authType: form.authType,
      keyId: form.authType === "key" ? form.keyId : null,
    });
    emit("saved", saved);
    emit("update:open", false);
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Sheet :open="open" @update:open="(value) => emit('update:open', value)">
    <SheetContent side="right" class="host-editor w-[380px] p-0 sm:max-w-[380px]">
      <SheetHeader class="px-4 pt-4">
        <SheetTitle class="text-[14px]">{{ isEdit ? "编辑主机" : "新建主机" }}</SheetTitle>
        <SheetDescription class="text-[12px]">
          密码不会保存；密钥请先在「密钥管理」中导入
        </SheetDescription>
      </SheetHeader>

      <form class="form" @submit.prevent="submit">
        <div class="field">
          <Label for="he-alias" class="text-[12px]">别名（可选）</Label>
          <Input id="he-alias" v-model="form.alias" class="h-8 text-[12px]" placeholder="例如：测试机" />
        </div>

        <div class="row">
          <div class="field field--grow">
            <Label for="he-host" class="text-[12px]">主机</Label>
            <Input id="he-host" v-model="form.host" class="h-8 text-[12px]" placeholder="192.168.1.10" />
          </div>
          <div class="field field--port">
            <Label for="he-port" class="text-[12px]">端口</Label>
            <Input id="he-port" v-model="form.port" type="number" min="1" max="65535" class="h-8 text-[12px]" />
          </div>
        </div>

        <div class="field">
          <Label for="he-username" class="text-[12px]">用户名</Label>
          <Input id="he-username" v-model="form.username" class="h-8 text-[12px]" placeholder="root" />
        </div>

        <div class="field">
          <span class="label">认证方式</span>
          <div class="segmented">
            <button
              type="button"
              class="segment"
              :class="{ 'segment--active': form.authType === 'password' }"
              @click="form.authType = 'password'"
            >
              <ShieldCheck :size="13" />密码
            </button>
            <button
              type="button"
              class="segment"
              :class="{ 'segment--active': form.authType === 'key' }"
              @click="form.authType = 'key'"
            >
              <KeyRound :size="13" />密钥
            </button>
          </div>
        </div>

        <div v-if="form.authType === 'key'" class="field">
          <Label for="he-key" class="text-[12px]">登录密钥</Label>
          <select id="he-key" v-model="form.keyId" class="select">
            <option value="" disabled>请选择密钥</option>
            <option v-for="key in keys.items" :key="key.id" :value="key.id">
              {{ key.name }}（{{ key.keyType }}）
            </option>
          </select>
          <p v-if="!keys.items.length" class="hint">还没有密钥，请先在侧栏的「密钥管理」中导入</p>
        </div>

        <p v-if="error" class="error">{{ error }}</p>

        <SheetFooter class="px-0 pt-1">
          <Button type="submit" size="sm" class="h-8 w-full text-[12px]" :disabled="saving">
            <Save :size="13" />{{ saving ? "保存中…" : "保存" }}
          </Button>
        </SheetFooter>
      </form>
    </SheetContent>
  </Sheet>
</template>

<style scoped lang="scss">
.host-editor {
  display: flex;
  flex-direction: column;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  overflow-y: auto;
  @include scrollbar(8px);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.label {
  font-size: 12px;
  font-weight: 500;
}

.row {
  display: flex;
  gap: 8px;
}

.field--grow {
  flex: 1;
}

.field--port {
  width: 76px;
  flex: none;
}

.segmented {
  display: flex;
  gap: 4px;
}

.segment {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  font-size: 12px;
  color: var(--muted-foreground);
  cursor: default;

  &--active {
    border-color: var(--primary);
    color: var(--primary);
    background: color-mix(in srgb, var(--primary) 10%, transparent);
  }
}

.select {
  height: 32px;
  width: 100%;
  padding: 0 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: transparent;
  color: var(--foreground);
  font-size: 12px;
  outline: none;

  &:focus-visible {
    border-color: var(--ring);
  }
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
