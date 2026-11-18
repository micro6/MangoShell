<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Save } from "@lucide/vue";
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
import { useForwardsStore, type ForwardRule, type ForwardType } from "@/stores/forwards";

const props = defineProps<{
  open: boolean;
  hostId: string;
  preset?: ForwardRule | null;
}>();
const emit = defineEmits<{
  "update:open": [value: boolean];
  saved: [];
}>();

const forwards = useForwardsStore();

const form = reactive({
  type: "local" as ForwardType,
  bindHost: "127.0.0.1",
  bindPort: "18080",
  targetHost: "127.0.0.1",
  targetPort: "22",
});
const error = ref("");
const saving = ref(false);

const isEdit = computed(() => Boolean(props.preset));
const needsTarget = computed(() => form.type !== "dynamic");
/** 绑定 0.0.0.0 会把端口暴露到局域网，仅提示不拦截 */
const exposed = computed(() => form.bindHost.trim() === "0.0.0.0");

const typeOptions: { value: ForwardType; label: string; hint: string }[] = [
  { value: "local", label: "本地", hint: "把远端服务映射到本机端口（-L）" },
  { value: "dynamic", label: "动态", hint: "在本机开 SOCKS5 代理（-D）" },
  { value: "remote", label: "远程", hint: "把本机服务映射到服务器端口（-R）" },
];

function fill() {
  const rule = props.preset;
  form.type = rule?.type ?? "local";
  form.bindHost = rule?.bindHost ?? "127.0.0.1";
  form.bindPort = String(rule?.bindPort ?? 18080);
  form.targetHost = rule?.targetHost ?? "127.0.0.1";
  form.targetPort = String(rule?.targetPort ?? 22);
  error.value = "";
}

watch(
  () => props.open,
  (open) => {
    if (open) fill();
  },
);

function parsePort(value: string, label: string) {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    error.value = `${label}需在 1-65535 之间`;
    return null;
  }
  return port;
}

async function submit() {
  const bindHost = form.bindHost.trim();
  if (!bindHost) {
    error.value = "请填写监听地址";
    return;
  }
  const bindPort = parsePort(form.bindPort, "监听端口");
  if (bindPort === null) return;

  let targetHost: string | null = null;
  let targetPort: number | null = null;
  if (needsTarget.value) {
    targetHost = form.targetHost.trim();
    if (!targetHost) {
      error.value = "请填写目标地址";
      return;
    }
    targetPort = parsePort(form.targetPort, "目标端口");
    if (targetPort === null) return;
  }

  saving.value = true;
  try {
    await forwards.save({
      id: props.preset?.id ?? null,
      hostId: props.hostId,
      type: form.type,
      bindHost,
      bindPort,
      targetHost,
      targetPort,
      autoStart: props.preset?.autoStart ?? false,
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
  <Sheet :open="open" @update:open="(value) => emit('update:open', value)">
    <SheetContent side="right" class="forward-dialog w-[380px] p-0 sm:max-w-[380px]">
      <SheetHeader class="px-4 pt-4">
        <SheetTitle class="text-[14px]">{{ isEdit ? "编辑隧道" : "新建隧道" }}</SheetTitle>
        <SheetDescription class="text-[12px]">
          {{ typeOptions.find((t) => t.value === form.type)?.hint }}
        </SheetDescription>
      </SheetHeader>

      <form class="form" @submit.prevent="submit">
        <div class="field">
          <span class="label">类型</span>
          <div class="segmented">
            <button
              v-for="option in typeOptions"
              :key="option.value"
              type="button"
              class="segment"
              :class="{ 'segment--active': form.type === option.value }"
              @click="form.type = option.value"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <div class="row">
          <div class="field field--grow">
            <Label for="fd-bind-host" class="text-[12px]">监听地址</Label>
            <Input
              id="fd-bind-host"
              v-model="form.bindHost"
              class="h-8 text-[12px]"
              placeholder="127.0.0.1"
            />
          </div>
          <div class="field field--port">
            <Label for="fd-bind-port" class="text-[12px]">端口</Label>
            <Input
              id="fd-bind-port"
              v-model="form.bindPort"
              type="number"
              min="1"
              max="65535"
              class="h-8 text-[12px]"
            />
          </div>
        </div>

        <p v-if="exposed" class="warn">绑定 0.0.0.0 会暴露到局域网，请确认网络环境可信</p>

        <template v-if="needsTarget">
          <div class="row">
            <div class="field field--grow">
              <Label for="fd-target-host" class="text-[12px]">目标地址</Label>
              <Input
                id="fd-target-host"
                v-model="form.targetHost"
                class="h-8 text-[12px]"
                placeholder="127.0.0.1"
              />
            </div>
            <div class="field field--port">
              <Label for="fd-target-port" class="text-[12px]">端口</Label>
              <Input
                id="fd-target-port"
                v-model="form.targetPort"
                type="number"
                min="1"
                max="65535"
                class="h-8 text-[12px]"
              />
            </div>
          </div>
          <p class="hint">
            {{ form.type === "local" ? "目标地址由服务器解析" : "目标地址在本机解析" }}
          </p>
        </template>

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
.forward-dialog {
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
  flex: 1;
  height: 28px;
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

.warn {
  font-size: 11px;
  color: var(--destructive);
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
