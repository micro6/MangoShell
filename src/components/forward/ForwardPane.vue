<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Pencil, Play, Plus, Square, Trash2, Waypoints } from "@lucide/vue";
import { toast } from "vue-sonner";
import { Button } from "@/components/ui/button";
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
import { useForwardsStore, type ForwardRule } from "@/stores/forwards";
import { formatBytes } from "@/stores/transfers";
import type { WorkspaceTab } from "@/stores/workspace";
import ForwardRuleDialog from "./ForwardRuleDialog.vue";

const props = defineProps<{ tab: WorkspaceTab; active: boolean }>();

const forwards = useForwardsStore();
const dialogOpen = ref(false);
const editing = ref<ForwardRule | null>(null);
const deleteTarget = ref<ForwardRule | null>(null);
const busyId = ref("");

const hostId = computed(() => props.tab.hostId ?? "");

const statusText: Record<string, string> = {
  stopped: "已停止",
  running: "运行中",
  error: "异常",
};

async function refresh() {
  if (!hostId.value) return;
  try {
    await forwards.load(hostId.value);
  } catch (e) {
    toast.error("读取隧道列表失败", { description: String(e) });
  }
}

onMounted(() => {
  void refresh();
  void forwards.subscribeStats();
});

onUnmounted(() => {
  // 关闭标签只停止统计推送，运行中的规则保持不动
  forwards.unsubscribeStats();
});

function openCreate() {
  editing.value = null;
  dialogOpen.value = true;
}

function openEdit(rule: ForwardRule) {
  editing.value = rule;
  dialogOpen.value = true;
}

async function start(rule: ForwardRule) {
  busyId.value = rule.id;
  try {
    await forwards.start(rule.id, hostId.value);
    toast.success("隧道已启动", { description: bindText(rule) });
  } catch (e) {
    toast.error("启动失败", { description: String(e) });
  } finally {
    busyId.value = "";
  }
}

async function stop(rule: ForwardRule) {
  busyId.value = rule.id;
  try {
    await forwards.stop(rule.id, hostId.value);
  } catch (e) {
    toast.error("停止失败", { description: String(e) });
  } finally {
    busyId.value = "";
  }
}

async function confirmDelete() {
  const rule = deleteTarget.value;
  if (!rule) return;
  try {
    await forwards.remove(rule.id, hostId.value);
    toast.success("隧道已删除");
  } catch (e) {
    toast.error("删除失败", { description: String(e) });
  } finally {
    deleteTarget.value = null;
  }
}

function bindText(rule: ForwardRule) {
  return `${rule.bindHost}:${rule.bindPort}`;
}

function targetText(rule: ForwardRule) {
  if (rule.type === "dynamic") return "SOCKS5 代理";
  return `${rule.targetHost ?? ""}:${rule.targetPort ?? ""}`;
}

const typeText: Record<string, string> = {
  local: "本地 -L",
  dynamic: "动态 -D",
  remote: "远程 -R",
};
</script>

<template>
  <div class="forward-pane">
    <div class="pane-header">
      <Waypoints :size="14" class="pane-icon" />
      <span class="pane-title">端口转发</span>
      <span class="pane-count">{{ forwards.rules.length }} 条规则</span>
      <Button
        size="sm"
        variant="secondary"
        class="ml-auto h-7 text-[12px]"
        :disabled="!hostId"
        @click="openCreate"
      >
        <Plus :size="13" />新建隧道
      </Button>
    </div>

    <div class="table-wrap">
      <table class="rule-table">
        <thead>
          <tr>
            <th class="col-type">类型</th>
            <th>监听</th>
            <th>目标</th>
            <th class="col-status">状态</th>
            <th class="col-traffic">流量（↓入 / ↑出）</th>
            <th class="col-actions">操作</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="rule in forwards.rules" :key="rule.id" class="rule-row">
            <td class="col-type">{{ typeText[rule.type] ?? rule.type }}</td>
            <td class="mono">{{ bindText(rule) }}</td>
            <td class="mono">{{ targetText(rule) }}</td>
            <td class="col-status">
              <span class="status" :class="`status--${rule.status}`">
                {{ statusText[rule.status] ?? rule.status }}
              </span>
              <p v-if="rule.error" class="status-error" :title="rule.error">{{ rule.error }}</p>
            </td>
            <td class="col-traffic mono">
              ↓ {{ formatBytes(rule.bytesIn) }} / ↑ {{ formatBytes(rule.bytesOut) }}
            </td>
            <td class="col-actions">
              <div class="row-actions">
                <Button
                  v-if="rule.status !== 'running'"
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  title="启动"
                  :disabled="busyId === rule.id"
                  @click="start(rule)"
                >
                  <Play :size="13" />
                </Button>
                <Button
                  v-else
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  title="停止"
                  :disabled="busyId === rule.id"
                  @click="stop(rule)"
                >
                  <Square :size="13" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  title="编辑"
                  @click="openEdit(rule)"
                >
                  <Pencil :size="13" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-7 w-7"
                  title="删除"
                  @click="deleteTarget = rule"
                >
                  <Trash2 :size="13" />
                </Button>
              </div>
            </td>
          </tr>
          <tr v-if="!forwards.rules.length">
            <td colspan="6" class="empty">
              {{ forwards.loading ? "加载中…" : "还没有隧道，点击右上角「新建隧道」添加" }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <ForwardRuleDialog
      v-model:open="dialogOpen"
      :host-id="hostId"
      :preset="editing"
      @saved="refresh"
    />

    <AlertDialog
      :open="Boolean(deleteTarget)"
      @update:open="(value) => (!value ? (deleteTarget = null) : null)"
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle class="text-[14px]">删除隧道</AlertDialogTitle>
          <AlertDialogDescription class="text-[12px]">
            确定删除「{{ deleteTarget ? bindText(deleteTarget) : "" }}」吗？运行中的规则会先停止。
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel class="h-8 text-[12px]">取消</AlertDialogCancel>
          <AlertDialogAction class="h-8 text-[12px]" @click="confirmDelete">删除</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </div>
</template>

<style scoped lang="scss">
.forward-pane {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
  user-select: none;
  background: var(--background);
}

.pane-header {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 34px;
  padding: 0 10px;
  border-bottom: 1px solid var(--border);
  flex: none;
}

.pane-icon {
  color: var(--muted-foreground);
}

.pane-title {
  font-size: 12px;
  font-weight: 500;
}

.pane-count {
  font-size: 11px;
  color: var(--muted-foreground);
}

.table-wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
  @include scrollbar(8px);
}

.rule-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    text-align: left;
    font-weight: 500;
    color: var(--muted-foreground);
    background: var(--card);
    border-bottom: 1px solid var(--border);
    padding: 6px 10px;
    white-space: nowrap;
  }

  td {
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    vertical-align: top;
  }
}

.rule-row:hover {
  background: var(--secondary);
}

.mono {
  font-family: var(--font-mono, monospace);
  user-select: text;
}

.col-type {
  width: 84px;
  white-space: nowrap;
}

.col-status {
  width: 150px;
}

.col-traffic {
  width: 170px;
  white-space: nowrap;
  color: var(--muted-foreground);
}

.col-actions {
  width: 128px;
}

.status {
  display: inline-flex;
  align-items: center;
  padding: 0 6px;
  height: 18px;
  border-radius: 999px;
  font-size: 11px;

  &--running {
    color: var(--success);
    background: color-mix(in srgb, var(--success) 14%, transparent);
  }

  &--error {
    color: var(--destructive);
    background: color-mix(in srgb, var(--destructive) 14%, transparent);
  }

  &--stopped {
    color: var(--muted-foreground);
    background: var(--secondary);
  }
}

.status-error {
  margin-top: 3px;
  font-size: 11px;
  color: var(--destructive);
  @include ellipsis;
  max-width: 220px;
}

.row-actions {
  display: flex;
  gap: 2px;
}

.empty {
  text-align: center;
  color: var(--muted-foreground);
  padding: 24px 0;
}
</style>
