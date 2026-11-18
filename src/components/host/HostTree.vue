<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  FolderPlus,
  KeyRound,
  Pencil,
  Plug,
  Search,
  Server,
  ShieldCheck,
  Trash2,
  Upload,
} from "@lucide/vue";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useHostsStore, type HostRecord } from "@/stores/hosts";

const emit = defineEmits<{
  connect: [host: HostRecord];
  create: [];
  edit: [host: HostRecord];
  delete: [host: HostRecord];
  "manage-keys": [];
  "manage-known-hosts": [];
}>();

const hosts = useHostsStore();
const keyword = ref("");
const selectedId = ref("");
const menu = ref<{ host: HostRecord; x: number; y: number } | null>(null);

onMounted(() => {
  void hosts.load().catch(() => {});
});

const filtered = computed(() => {
  const kw = keyword.value.trim().toLowerCase();
  if (!kw) return hosts.items;
  return hosts.items.filter(
    (h) =>
      h.alias.toLowerCase().includes(kw) ||
      h.host.toLowerCase().includes(kw) ||
      h.username.toLowerCase().includes(kw),
  );
});

/** 右键菜单：定位到光标处，并避免超出窗口右/下边界 */
function openMenu(event: MouseEvent, host: HostRecord) {
  selectedId.value = host.id;
  menu.value = {
    host,
    x: Math.min(event.clientX, window.innerWidth - 150),
    y: Math.min(event.clientY, window.innerHeight - 130),
  };
}

function runMenu(action: "connect" | "edit" | "delete") {
  const host = menu.value?.host;
  menu.value = null;
  if (!host) return;
  if (action === "connect") emit("connect", host);
  else if (action === "edit") emit("edit", host);
  else emit("delete", host);
}
</script>

<template>
  <aside class="sidebar">
    <div class="search">
      <Search :size="13" class="icon" />
      <Input v-model="keyword" placeholder="搜索主机" class="h-7 pl-7 text-[12px]" />
    </div>

    <div class="tree">
      <template v-if="filtered.length">
        <div class="group">
          <div class="group-title">主机</div>
          <button
            v-for="item in filtered"
            :key="item.id"
            type="button"
            class="host"
            :class="{ 'host--active': selectedId === item.id }"
            :title="`${item.username}@${item.host}:${item.port}（双击连接，右键更多操作）`"
            @click="selectedId = item.id"
            @dblclick="emit('connect', item)"
            @contextmenu.prevent.stop="openMenu($event, item)"
          >
            <Server :size="13" class="host-icon" />
            <span class="host-name">{{ item.alias }}</span>
            <span class="host-addr">{{ item.host }}</span>
          </button>
        </div>
      </template>

      <div v-else-if="hosts.loading" class="empty">
        <p class="empty-title">正在加载…</p>
      </div>

      <div v-else class="empty">
        <Server :size="20" class="empty-icon" />
        <p class="empty-title">{{ keyword ? "没有匹配的主机" : "还没有主机" }}</p>
        <div v-if="!keyword" class="empty-actions">
          <Button size="sm" variant="default" class="h-7 text-[12px]" @click="emit('create')">
            <FolderPlus :size="13" />新建主机
          </Button>
          <Button size="sm" variant="ghost" class="h-7 text-[12px]" disabled>
            <Upload :size="13" />导入 ~/.ssh/config
          </Button>
        </div>
      </div>
    </div>

    <div class="footer">
      <button type="button" class="footer-btn" @click="emit('manage-keys')">
        <KeyRound :size="13" />密钥管理
      </button>
      <button type="button" class="footer-btn" @click="emit('manage-known-hosts')">
        <ShieldCheck :size="13" />已知主机
      </button>
    </div>
  </aside>

  <Teleport to="body">
    <div v-if="menu" class="menu-overlay" @click="menu = null" @contextmenu.prevent="menu = null">
      <div class="menu" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }">
        <button type="button" class="menu-item" @click="runMenu('connect')">
          <Plug :size="13" />连接
        </button>
        <button type="button" class="menu-item" @click="runMenu('edit')">
          <Pencil :size="13" />编辑
        </button>
        <button type="button" class="menu-item menu-item--danger" @click="runMenu('delete')">
          <Trash2 :size="13" />删除
        </button>
      </div>
    </div>
  </Teleport>
</template>

<style lang="scss" scoped>
.sidebar {
  display: flex;
  flex-direction: column;
  width: $sidebar-width;
  min-width: $sidebar-width;
  border-right: 1px solid var(--border);
  background: var(--card);
}

.search {
  position: relative;
  padding: 8px;

  .icon {
    position: absolute;
    left: 16px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--muted-foreground);
    pointer-events: none;
  }
}

.tree {
  flex: 1;
  overflow-y: auto;
  @include scrollbar(8px);
}

.group-title {
  padding: 6px 12px 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--muted-foreground);
}

.host {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  height: $row-height;
  padding: 0 12px;
  color: var(--foreground);
  cursor: default;

  &:hover {
    background: var(--secondary);
  }

  &--active {
    background: var(--secondary);
  }
}

.host-icon {
  color: var(--muted-foreground);
  flex: none;
}

.host-name {
  @include ellipsis;
  font-size: 12px;
}

.host-addr {
  margin-left: auto;
  font-size: 11px;
  color: var(--muted-foreground);
  flex: none;
}

.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 40px 16px;
  text-align: center;
}

.empty-icon {
  color: var(--muted-foreground);
}

.empty-title {
  color: var(--muted-foreground);
  font-size: 12px;
}

.empty-actions {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.footer {
  display: flex;
  flex: none;
  gap: 4px;
  padding: 6px 8px;
  border-top: 1px solid var(--border);
}

.footer-btn {
  display: flex;
  flex: 1;
  align-items: center;
  justify-content: center;
  gap: 4px;
  height: 26px;
  border-radius: var(--radius);
  font-size: 11px;
  color: var(--muted-foreground);
  cursor: default;

  &:hover {
    background: var(--secondary);
    color: var(--foreground);
  }
}

.menu-overlay {
  position: fixed;
  inset: 0;
  z-index: 90;
}

.menu {
  position: fixed;
  z-index: 91;
  min-width: 132px;
  padding: 4px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--popover);
  color: var(--popover-foreground);
  box-shadow: 0 6px 20px rgb(0 0 0 / 18%);
}

.menu-item {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  height: 26px;
  padding: 0 8px;
  border-radius: 4px;
  font-size: 12px;
  cursor: default;

  &:hover {
    background: var(--secondary);
  }

  &--danger {
    color: var(--destructive);
  }
}
</style>
