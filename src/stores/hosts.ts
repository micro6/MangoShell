import { invoke } from "@tauri-apps/api/core";
import { defineStore } from "pinia";
import { ref } from "vue";

export interface HostRecord {
  id: string;
  alias: string;
  host: string;
  port: number;
  username: string;
  authType: string;
  keyId: string | null;
}

export interface HostPayload {
  id?: string | null;
  alias: string;
  host: string;
  port: number;
  username: string;
  authType: string;
  keyId?: string | null;
}

/** 主机元数据缓存；凭据不进前端状态 */
export const useHostsStore = defineStore("hosts", () => {
  const items = ref<HostRecord[]>([]);
  const loading = ref(false);

  async function load() {
    loading.value = true;
    try {
      items.value = await invoke<HostRecord[]>("host_list");
    } finally {
      loading.value = false;
    }
  }

  async function save(payload: HostPayload) {
    const saved = await invoke<HostRecord>("host_save", { ...payload });
    await load();
    return saved;
  }

  async function remove(id: string) {
    await invoke("host_delete", { id });
    await load();
  }

  return { items, loading, load, save, remove };
});
