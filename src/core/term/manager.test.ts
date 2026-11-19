import type { Channel } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MountOptions, TermExit } from "./manager";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  write: vi.fn(),
  dispose: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class<T> {
    onmessage?: (data: T) => void;
  },
  invoke: mocks.invoke,
}));
vi.mock("@tauri-apps/plugin-log", () => ({ error: vi.fn() }));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));
vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    findNext() {
      return true;
    }
    findPrevious() {
      return true;
    }
    clearDecorations() {}
    onDidChangeResults() {
      return { dispose() {} };
    }
  },
}));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    onContextLoss() {}
    dispose() {}
  },
}));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options: Record<string, unknown>;
    cols = 80;
    rows = 24;
    unicode = { activeVersion: "" };

    constructor(options: Record<string, unknown>) {
      this.options = { ...options };
    }

    loadAddon() {}
    open() {}
    focus() {}
    onData() {}
    onResize() {}
    write(data: Uint8Array) {
      mocks.write(data);
    }
    dispose() {
      mocks.dispose();
    }
  },
}));

import {
  MAX_RECONNECT_ATTEMPTS,
  RECONNECT_DELAYS,
  STABLE_CONNECTION_MS,
  TerminalManager,
} from "./manager";

interface OpenCall {
  channel: Channel<number[]>;
  resolve: (exit: TermExit) => void;
  reject: (reason?: unknown) => void;
}

const openCalls: OpenCall[] = [];
const flush = async () => {
  await Promise.resolve();
  await Promise.resolve();
};

function mount(options: MountOptions = {}) {
  const manager = new TerminalManager();
  manager.mount("tab-1", "host-1", { clientWidth: 800, clientHeight: 600 } as HTMLElement, options);
  return manager;
}

describe("TerminalManager 自动重连", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("window", globalThis);
    vi.stubGlobal("document", {
      documentElement: { classList: { contains: () => false } },
    });
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    openCalls.length = 0;
    mocks.invoke.mockReset();
    mocks.write.mockReset();
    mocks.dispose.mockReset();
    mocks.invoke.mockImplementation((command: string, args?: { onData?: Channel<number[]> }) => {
      if (command !== "term_open") return Promise.resolve();
      return new Promise<TermExit>((resolve, reject) => {
        openCalls.push({ channel: args!.onData!, resolve, reject });
      });
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("首次连接失败不自动重试", async () => {
    const onExit = vi.fn();
    const onReconnecting = vi.fn();
    const manager = mount({ onExit, onReconnecting });

    openCalls[0].reject(new Error("认证失败"));
    await flush();
    await vi.advanceTimersByTimeAsync(10_000);

    expect(onExit).toHaveBeenCalledWith("Error: 认证失败", false);
    expect(onReconnecting).not.toHaveBeenCalled();
    expect(openCalls).toHaveLength(1);
    manager.dispose("tab-1");
  });

  it("正常 shell 退出不自动重试", async () => {
    const onExit = vi.fn();
    const manager = mount({ onExit });
    openCalls[0].channel.onmessage?.([36]);

    openCalls[0].resolve("shellExit");
    await flush();
    await vi.advanceTimersByTimeAsync(10_000);

    expect(onExit).toHaveBeenCalledOnce();
    expect(openCalls).toHaveLength(1);
    manager.dispose("tab-1");
  });

  it("按 1/2/4 秒退避，三次失败后停止", async () => {
    const onExit = vi.fn();
    const onReconnecting = vi.fn();
    const manager = mount({ onExit, onReconnecting });
    openCalls[0].channel.onmessage?.([36]);
    openCalls[0].resolve("disconnected");
    await flush();

    for (let index = 0; index < MAX_RECONNECT_ATTEMPTS; index += 1) {
      expect(onReconnecting).toHaveBeenLastCalledWith(index + 1, RECONNECT_DELAYS[index]);
      await vi.advanceTimersByTimeAsync(RECONNECT_DELAYS[index]);
      expect(openCalls).toHaveLength(index + 2);
      openCalls[index + 1].reject(new Error("网络不可达"));
      await flush();
    }

    await vi.advanceTimersByTimeAsync(10_000);
    expect(openCalls).toHaveLength(MAX_RECONNECT_ATTEMPTS + 1);
    expect(onExit).toHaveBeenCalledWith(expect.stringContaining("自动重连 3 次仍失败"), true);
    manager.dispose("tab-1");
  });

  it("稳定连接 30 秒后重新计算重试次数", async () => {
    const onReconnecting = vi.fn();
    const manager = mount({ onReconnecting });
    openCalls[0].channel.onmessage?.([36]);
    openCalls[0].resolve("disconnected");
    await flush();
    await vi.advanceTimersByTimeAsync(RECONNECT_DELAYS[0]);

    openCalls[1].channel.onmessage?.([36]);
    await vi.advanceTimersByTimeAsync(STABLE_CONNECTION_MS);
    openCalls[1].resolve("disconnected");
    await flush();

    expect(onReconnecting).toHaveBeenLastCalledWith(1, RECONNECT_DELAYS[0]);
    manager.dispose("tab-1");
  });

  it("停止后忽略旧回调，手动重连不受旧连接影响", async () => {
    const onExit = vi.fn();
    const onReconnecting = vi.fn();
    const manager = mount({ onExit, onReconnecting });
    openCalls[0].channel.onmessage?.([36]);

    await manager.stopReconnect("tab-1");
    expect(onExit).toHaveBeenCalledWith(undefined, true);
    manager.retry("tab-1");
    expect(openCalls).toHaveLength(2);
    const callbackCount = onReconnecting.mock.calls.length;
    const writeCount = mocks.write.mock.calls.length;

    openCalls[0].channel.onmessage?.([65]);
    openCalls[0].reject(new Error("旧连接迟到"));
    await flush();
    await vi.advanceTimersByTimeAsync(10_000);

    expect(mocks.write).toHaveBeenCalledTimes(writeCount);
    expect(onReconnecting).toHaveBeenCalledTimes(callbackCount);
    expect(openCalls).toHaveLength(2);
    manager.dispose("tab-1");
  });

  it("退避期间停止和关闭标签都不会继续连接", async () => {
    const onExit = vi.fn();
    const stopped = mount({ onExit });
    openCalls[0].channel.onmessage?.([36]);
    openCalls[0].resolve("disconnected");
    await flush();
    await stopped.stopReconnect("tab-1");
    await vi.advanceTimersByTimeAsync(10_000);
    expect(openCalls).toHaveLength(1);
    expect(onExit).toHaveBeenCalledWith(undefined, true);
    stopped.dispose("tab-1");

    openCalls.length = 0;
    const disposed = mount();
    disposed.dispose("tab-1");
    await vi.advanceTimersByTimeAsync(10_000);
    expect(openCalls).toHaveLength(1);
    expect(mocks.invoke).toHaveBeenCalledWith("term_close", { tabId: "tab-1" });
  });
});
