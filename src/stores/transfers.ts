import { Channel, invoke } from "@tauri-apps/api/core";
import { defineStore } from "pinia";
import { ref } from "vue";

export type TransferDirection = "upload" | "download";
export type TransferStatus = "pending" | "running" | "done" | "failed" | "cancelled";

export interface TransferItem {
  id: string;
  tabId: string;
  direction: TransferDirection;
  name: string;
  localPath: string;
  remotePath: string;
  size: number;
  transferred: number;
  /** 字节/秒，仅运行中有效 */
  speed: number;
  status: TransferStatus;
  error?: string;
  /** 本地已有更长的同名文件，本次下载会截断重写 */
  overwrite?: boolean;
}

export interface TransferRequest {
  tabId: string;
  direction: TransferDirection;
  localPath: string;
  remotePath: string;
  size?: number;
  overwrite?: boolean;
}

interface TransferProgress {
  transferred: number;
  total: number;
}

let seq = 0;
const nextId = () => `tr-${++seq}`;

const fileName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? "";

/** 字节数转为人类可读大小 */
export function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value >= 100 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

/**
 * 传输队列：全局单并发，按顺序逐个执行；条目按 sftp 标签过滤展示。
 * 断点续传规则由后端判定，前端只负责重试与取消。
 */
export const useTransfersStore = defineStore("transfers", () => {
  const items = ref<TransferItem[]>([]);
  let running = false;

  function enqueue(requests: TransferRequest[]) {
    for (const req of requests) {
      items.value.push({
        id: nextId(),
        tabId: req.tabId,
        direction: req.direction,
        name: fileName(req.direction === "upload" ? req.localPath : req.remotePath),
        localPath: req.localPath,
        remotePath: req.remotePath,
        size: req.size ?? 0,
        transferred: 0,
        speed: 0,
        status: "pending",
        overwrite: req.overwrite,
      });
    }
    void runNext();
  }

  async function runNext() {
    if (running) return;
    const item = items.value.find((i) => i.status === "pending");
    if (!item) return;

    running = true;
    item.status = "running";
    item.error = undefined;
    item.speed = 0;

    const channel = new Channel<TransferProgress>();
    let lastTime = performance.now();
    let lastBytes = item.transferred;
    channel.onmessage = (progress) => {
      item.transferred = progress.transferred;
      if (progress.total > 0) item.size = progress.total;

      const now = performance.now();
      const seconds = (now - lastTime) / 1000;
      if (seconds >= 0.2) {
        item.speed = Math.max(0, (progress.transferred - lastBytes) / seconds);
        lastTime = now;
        lastBytes = progress.transferred;
      }
    };

    try {
      const command = item.direction === "upload" ? "sftp_upload" : "sftp_download";
      await invoke<number>(command, {
        tabId: item.tabId,
        transferId: item.id,
        remote: item.remotePath,
        local: item.localPath,
        onProgress: channel,
      });
      item.status = "done";
      if (item.size > 0) item.transferred = item.size;
    } catch (e) {
      const message = String(e);
      // 取消是前端主动置位的，异步返回时需要用最新状态判断
      const cancelled =
        message.includes("已取消") ||
        items.value.some((i) => i.id === item.id && i.status === "cancelled");
      if (cancelled) {
        item.status = "cancelled";
      } else {
        item.status = "failed";
        item.error = message;
      }
    } finally {
      item.speed = 0;
      running = false;
    }

    void runNext();
  }

  async function cancel(id: string) {
    const item = items.value.find((i) => i.id === id);
    if (!item) return;
    if (item.status === "pending") {
      item.status = "cancelled";
      return;
    }
    if (item.status !== "running") return;

    item.status = "cancelled";
    await invoke("sftp_cancel", { transferId: id }).catch(() => {});
  }

  function retry(id: string) {
    const item = items.value.find((i) => i.id === id);
    if (!item || (item.status !== "failed" && item.status !== "cancelled")) return;

    item.status = "pending";
    item.error = undefined;
    item.transferred = 0;
    item.speed = 0;
    void runNext();
  }

  /** 标签关闭时终止该标签下未完成的传输 */
  async function cancelTab(tabId: string) {
    const targets = items.value.filter(
      (i) => i.tabId === tabId && (i.status === "pending" || i.status === "running"),
    );
    for (const item of targets) await cancel(item.id);
  }

  function clearFinished() {
    items.value = items.value.filter((i) => i.status === "pending" || i.status === "running");
  }

  return { items, enqueue, cancel, retry, cancelTab, clearFinished };
});
