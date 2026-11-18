import { invoke } from "@tauri-apps/api/core";
import { defineStore } from "pinia";
import { ref } from "vue";

export interface KeyRecord {
  id: string;
  name: string;
  keyType: string;
  publicKey: string;
  fingerprint: string;
  createdAt: number;
}

/** 密钥只是元数据，私钥正文始终留在 Rust 侧 */
export const useKeysStore = defineStore("keys", () => {
  const items = ref<KeyRecord[]>([]);
  const loading = ref(false);

  async function load() {
    loading.value = true;
    try {
      items.value = await invoke<KeyRecord[]>("key_list");
    } finally {
      loading.value = false;
    }
  }

  async function importKey(name: string, path: string, passphrase?: string) {
    const record = await invoke<KeyRecord>("key_import", {
      name,
      path,
      passphrase: passphrase || null,
    });
    await load();
    return record;
  }

  async function remove(id: string) {
    await invoke("key_delete", { id });
    await load();
  }

  return { items, loading, load, importKey, remove };
});
