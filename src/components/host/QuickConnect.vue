<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Plug, KeyRound } from "@lucide/vue";
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
import type { HostRecord } from "@/stores/hosts";

export interface ConnectPayload {
  alias: string;
  host: string;
  port: number;
  username: string;
  password?: string;
}

const props = defineProps<{ open: boolean; preset?: HostRecord | null }>();
const emit = defineEmits<{
  "update:open": [value: boolean];
  submit: [payload: ConnectPayload];
}>();

const form = reactive({ alias: "", host: "", port: "22", username: "", password: "" });
const error = ref("");

const connectMode = computed(() => Boolean(props.preset));
/** 主机配置为密钥认证时不再要求密码 */
const keyAuth = computed(() => props.preset?.authType === "key");

function fill() {
  form.alias = props.preset?.alias ?? "";
  form.host = props.preset?.host ?? "";
  form.port = String(props.preset?.port ?? 22);
  form.username = props.preset?.username ?? "";
  form.password = "";
  error.value = "";
}

watch(
  () => props.open,
  (open) => {
    if (open) fill();
  },
);

watch(
  () => props.preset,
  () => {
    if (props.open) fill();
  },
);

function submit() {
  const port = Number(form.port);
  if (!form.host.trim() || !form.username.trim()) {
    error.value = "主机地址和用户名不能为空";
    return;
  }
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    error.value = "端口需在 1-65535 之间";
    return;
  }
  if (!keyAuth.value && !form.password) {
    error.value = "请输入登录密码";
    return;
  }

  emit("submit", {
    alias: form.alias.trim(),
    host: form.host.trim(),
    port,
    username: form.username.trim(),
    password: keyAuth.value ? undefined : form.password,
  });
}
</script>

<template>
  <Sheet :open="open" @update:open="(value) => emit('update:open', value)">
    <SheetContent side="right" class="quick-connect w-[380px] p-0 sm:max-w-[380px]">
      <SheetHeader class="px-4 pt-4">
        <SheetTitle class="text-[14px]">
          {{ connectMode ? `连接到 ${preset?.alias}` : "快速连接" }}
        </SheetTitle>
        <SheetDescription class="text-[12px]">
          {{
            connectMode
              ? `${preset?.username}@${preset?.host}:${preset?.port}` +
                (keyAuth ? " · 使用已配置的密钥认证" : "")
              : "输入主机信息，密码只在本机内存中使用"
          }}
        </SheetDescription>
      </SheetHeader>

      <form class="form" @submit.prevent="submit">
        <template v-if="!connectMode">
          <div class="field">
            <Label for="qc-alias" class="text-[12px]">别名（可选）</Label>
            <Input id="qc-alias" v-model="form.alias" class="h-8 text-[12px]" placeholder="例如：测试机" />
          </div>

          <div class="row">
            <div class="field field--grow">
              <Label for="qc-host" class="text-[12px]">主机</Label>
              <Input id="qc-host" v-model="form.host" class="h-8 text-[12px]" placeholder="192.168.1.10" />
            </div>
            <div class="field field--port">
              <Label for="qc-port" class="text-[12px]">端口</Label>
              <Input id="qc-port" v-model="form.port" type="number" min="1" max="65535" class="h-8 text-[12px]" />
            </div>
          </div>

          <div class="field">
            <Label for="qc-username" class="text-[12px]">用户名</Label>
            <Input id="qc-username" v-model="form.username" class="h-8 text-[12px]" placeholder="root" />
          </div>
        </template>

        <div v-if="!keyAuth" class="field">
          <Label for="qc-password" class="text-[12px]">密码</Label>
          <Input
            id="qc-password"
            v-model="form.password"
            type="password"
            autocomplete="off"
            class="h-8 text-[12px]"
            placeholder="仅用于本次连接"
          />
        </div>

        <div v-else class="key-hint">
          <KeyRound :size="13" />
          <span>将使用主机绑定的密钥完成认证</span>
        </div>

        <p v-if="error" class="error">{{ error }}</p>

        <SheetFooter class="px-0 pt-1">
          <Button type="submit" size="sm" class="h-8 w-full text-[12px]">
            <Plug :size="13" />连接
          </Button>
        </SheetFooter>
      </form>
    </SheetContent>
  </Sheet>
</template>

<style scoped lang="scss">
.quick-connect {
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

.error {
  font-size: 12px;
  color: var(--destructive);
}

.key-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--muted-foreground);
}
</style>
