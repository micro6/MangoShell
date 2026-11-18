import { Channel, invoke } from "@tauri-apps/api/core";
import { defineStore } from "pinia";
import { ref } from "vue";

export type ForwardType = "local" | "dynamic" | "remote";
export type ForwardStatus = "stopped" | "running" | "error";

export interface ForwardRule {
  id: string;
  hostId: string;
  type: ForwardType;
  bindHost: string;
  bindPort: number;
  targetHost: string | null;
  targetPort: number | null;
  autoStart: boolean;
  status: ForwardStatus;
  bytesIn: number;
  bytesOut: number;
  error?: string | null;
}

export interface ForwardPayload {
  id?: string | null;
  hostId: string;
  type: ForwardType;
  bindHost: string;
  bindPort: number;
  targetHost?: string | null;
  targetPort?: number | null;
  autoStart: boolean;
}

interface ForwardStat {
  id: string;
  bytesIn: number;
  bytesOut: number;
}

/** 端口转发规则：规则属于主机，关闭标签不影响运行中的规则 */
export const useForwardsStore = defineStore("forwards", () => {
  const rules = ref<ForwardRule[]>([]);
  const loading = ref(false);

  let statsChannel: Channel<ForwardStat[]> | null = null;

  async function load(hostId: string) {
    loading.value = true;
    try {
      rules.value = await invoke<ForwardRule[]>("forward_list", { hostId });
    } finally {
      loading.value = false;
    }
  }

  async function save(payload: ForwardPayload) {
    const id = await invoke<string>("forward_save", {
      id: payload.id ?? null,
      hostId: payload.hostId,
      forwardType: payload.type,
      bindHost: payload.bindHost,
      bindPort: payload.bindPort,
      targetHost: payload.targetHost ?? null,
      targetPort: payload.targetPort ?? null,
      autoStart: payload.autoStart,
    });
    await load(payload.hostId);
    return id;
  }

  async function remove(id: string, hostId: string) {
    await invoke("forward_delete", { id });
    await load(hostId);
  }

  async function start(id: string, hostId: string) {
    await invoke("forward_start", { id, password: null });
    await load(hostId);
  }

  async function stop(id: string, hostId: string) {
    await invoke("forward_stop", { id });
    await load(hostId);
  }

  /** 订阅 1 s 一次的流量统计；重复调用直接复用已有 Channel */
  async function subscribeStats() {
    if (statsChannel) return;
    const channel = new Channel<ForwardStat[]>();
    channel.onmessage = (stats) => {
      for (const stat of stats) {
        const rule = rules.value.find((r) => r.id === stat.id);
        if (rule) {
          rule.bytesIn = stat.bytesIn;
          rule.bytesOut = stat.bytesOut;
        }
      }
    };
    statsChannel = channel;
    // 命令在 Channel 被丢弃前不会返回，这里不等待
    void invoke("forward_stats", { onStats: channel }).catch(() => {});
  }

  function unsubscribeStats() {
    statsChannel = null;
  }

  return {
    rules,
    loading,
    load,
    save,
    remove,
    start,
    stop,
    subscribeStats,
    unsubscribeStats,
  };
});
