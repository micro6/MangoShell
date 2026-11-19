<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { KeyRound, Plug, Save } from "@lucide/vue";
import { toast } from "vue-sonner";
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

/**
 * create  新建主机（只保存）
 * edit    编辑已有主机（只保存）
 * connect 连接已有主机（只填凭据）
 * quick   快速连接（保存主机并立即连接）
 */
type Mode = "create" | "edit" | "connect" | "quick";

const props = defineProps<{ open: boolean; mode: Mode; preset?: HostRecord | null }>();
const emit = defineEmits<{
  "update:open": [value: boolean];
  saved: [host: HostRecord];
  connect: [request: { host: HostRecord; password?: string }];
}>();

const hosts = useHostsStore();

const form = reactive({
  alias: "",
  host: "",
  port: "22",
  username: "",
  authType: "password" as "password" | "key",
  keyId: "",
  password: "",
  remember: false,
});
const error = ref("");
const saving = ref(false);
const hasSavedPassword = ref(false);

const isConnect = computed(() => props.mode === "connect");
const isQuick = computed(() => props.mode === "quick");
const keyAuth = computed(() => form.authType === "key");

const title = computed(() => {
  switch (props.mode) {
    case "connect":
      return `连接到 ${props.preset?.alias ?? ""}`;
    case "edit":
      return "编辑主机";
    case "quick":
      return "快速连接";
    default:
      return "新建主机";
  }
});

const description = computed(() => {
  if (props.mode === "connect") {
    const host = props.preset;
    const base = `${host?.username}@${host?.host}:${host?.port}`;
    return host?.authType === "key" ? `${base} · 使用已配置的密钥认证` : base;
  }
  return "密码可勾选记住并保存到系统钥匙串；密钥请先在「密钥管理」中导入";
});

function fill() {
  form.alias = props.preset?.alias ?? "";
  form.host = props.preset?.host ?? "";
  form.port = String(props.preset?.port ?? 22);
  form.username = props.preset?.username ?? "";
  form.authType = props.preset?.authType === "key" ? "key" : "password";
  form.keyId = props.preset?.keyId ?? "";
  form.password = "";
  form.remember = false;
  error.value = "";
}

/** 已有主机是否保存过密码；查询失败按未保存处理 */
async function loadSavedPassword() {
  hasSavedPassword.value = false;
  if (!props.preset) return;
  try {
    hasSavedPassword.value = await invoke<boolean>("host_password_exists", {
      hostId: props.preset.id,
    });
    form.remember = hasSavedPassword.value;
  } catch {
    hasSavedPassword.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    fill();
    void loadSavedPassword();
  },
);

/** 根据勾选状态写入或删除系统钥匙串中的密码 */
async function persistPassword(hostId: string) {
  try {
    if (form.remember && form.password) {
      await invoke("host_password_set", { hostId, password: form.password });
    } else if (!form.remember) {
      await invoke("host_password_clear", { hostId });
    }
  } catch (e) {
    toast.error("保存密码失败", { description: String(e) });
  }
}

async function submit() {
  error.value = "";
  const needsConnect = isConnect.value || isQuick.value;

  if (!isConnect.value) {
    const port = Number(form.port);
    if (!form.host.trim() || !form.username.trim()) {
      error.value = "主机地址和用户名不能为空";
      return;
    }
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      error.value = "端口需在 1-65535 之间";
      return;
    }
    if (keyAuth.value && !form.keyId) {
      error.value = "该主机缺少登录密钥，请重新导入并绑定";
      return;
    }
  }

  if (!keyAuth.value) {
    if (needsConnect && !form.password && !hasSavedPassword.value) {
      error.value = "请输入登录密码";
      return;
    }
    // 取消勾选后必须重新输入，否则删除已存密码后就没有可用凭据
    if (!form.password && !form.remember && hasSavedPassword.value) {
      error.value = "已取消保存密码，请重新输入登录密码";
      return;
    }
  }

  saving.value = true;
  let host = props.preset;

  if (!isConnect.value) {
    try {
      host = await hosts.save({
        id: props.preset?.id ?? null,
        alias: form.alias.trim(),
        host: form.host.trim(),
        port: Number(form.port),
        username: form.username.trim(),
        authType: form.authType,
        keyId: form.authType === "key" ? form.keyId : null,
      });
    } catch (e) {
      error.value = String(e);
      saving.value = false;
      return;
    }
  }

  if (host && !keyAuth.value) await persistPassword(host.id);

  if (host && !isConnect.value) emit("saved", host);
  if (host && needsConnect) emit("connect", { host, password: form.password || undefined });
  saving.value = false;
  emit("update:open", false);
}
</script>

<template>
  <Sheet :open="open" @update:open="(value) => emit('update:open', value)">
    <SheetContent side="right" class="host-editor w-[380px] p-0 sm:max-w-[380px]">
      <SheetHeader class="px-4 pt-4">
        <SheetTitle class="text-[14px]">{{ title }}</SheetTitle>
        <SheetDescription class="text-[12px]">{{ description }}</SheetDescription>
      </SheetHeader>

      <form class="form" @submit.prevent="submit">
        <template v-if="!isConnect">
          <div class="field">
            <Label for="he-alias" class="text-[12px]">别名（可选）</Label>
            <Input
              id="he-alias"
              v-model="form.alias"
              class="h-8 text-[12px]"
              placeholder="例如：测试机"
            />
          </div>

          <div class="row">
            <div class="field field--grow">
              <Label for="he-host" class="text-[12px]">主机</Label>
              <Input
                id="he-host"
                v-model="form.host"
                class="h-8 text-[12px]"
                placeholder="192.168.1.10"
              />
            </div>
            <div class="field field--port">
              <Label for="he-port" class="text-[12px]">端口</Label>
              <Input
                id="he-port"
                v-model="form.port"
                type="number"
                min="1"
                max="65535"
                class="h-8 text-[12px]"
              />
            </div>
          </div>

          <div class="field">
            <Label for="he-username" class="text-[12px]">用户名</Label>
            <Input
              id="he-username"
              v-model="form.username"
              class="h-8 text-[12px]"
              placeholder="root"
            />
          </div>

          <div v-if="keyAuth" class="key-hint">
            <KeyRound :size="13" />
            <span>该主机使用已绑定的密钥认证，当前表单仅支持密码方式</span>
          </div>
        </template>

        <div v-if="!keyAuth" class="field">
          <Label for="he-password" class="text-[12px]">密码</Label>
          <Input
            id="he-password"
            v-model="form.password"
            type="password"
            autocomplete="off"
            class="h-8 text-[12px]"
            :placeholder="hasSavedPassword ? '已保存密码，留空直接使用' : '仅用于本次连接'"
          />
          <label class="remember">
            <input v-model="form.remember" type="checkbox" class="remember-box" />
            <span>
              {{
                hasSavedPassword && !form.remember
                  ? "取消保存并删除已存密码"
                  : "记住密码（保存到系统钥匙串）"
              }}
            </span>
          </label>
        </div>

        <div v-if="keyAuth && isConnect" class="key-hint">
          <KeyRound :size="13" />
          <span>将使用主机绑定的密钥完成认证</span>
        </div>

        <p v-if="error" class="error">{{ error }}</p>

        <SheetFooter class="px-0 pt-1">
          <Button type="submit" size="sm" class="h-8 w-full text-[12px]" :disabled="saving">
            <template v-if="isConnect"> <Plug :size="13" />连接 </template>
            <template v-else-if="isQuick">
              <Plug :size="13" />{{ saving ? "保存中…" : "保存并连接" }}
            </template>
            <template v-else> <Save :size="13" />{{ saving ? "保存中…" : "保存" }} </template>
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

.remember {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 2px;
  color: var(--muted-foreground);
  font-size: 11px;
  cursor: pointer;
}

.remember-box {
  width: 12px;
  height: 12px;
  accent-color: var(--primary);
  cursor: pointer;
}

.key-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  color: var(--muted-foreground);
  font-size: 12px;
}

.error {
  font-size: 12px;
  color: var(--destructive);
}
</style>
