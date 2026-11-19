import { Channel, invoke } from "@tauri-apps/api/core";
import { error as logError } from "@tauri-apps/plugin-log";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

export type TermExit = "shellExit" | "disconnected" | "closed";

export const MAX_RECONNECT_ATTEMPTS = 3;
export const RECONNECT_DELAYS = [1000, 2000, 4000] as const;
export const STABLE_CONNECTION_MS = 30_000;

interface TermEntry {
  term: Terminal;
  fit: FitAddon;
  search: SearchAddon;
  searchSub?: { dispose(): void };
  channel?: Channel<number[]>;
  container: HTMLElement;
  observer: ResizeObserver;
  hostId: string;
  options: MountOptions;
  generation: number;
  retryCount: number;
  everReady: boolean;
  attemptActive: boolean;
  allowReconnect: boolean;
  writeBuf: string;
  writeTimer?: number;
  resizeTimer?: number;
  retryTimer?: number;
  stableTimer?: number;
}

/** 终端搜索选项，与 xterm SearchAddon 的语义一致 */
export interface TermSearchOptions {
  caseSensitive?: boolean;
  regex?: boolean;
  /** 输入过程中扩展当前选区，用于边输入边搜索 */
  incremental?: boolean;
}

export interface MountOptions {
  password?: string;
  /** 首个数据块到达：说明认证通过、远端 shell 已启动 */
  onReady?: () => void;
  /** attempt 为 0 表示手动立即重连，delayMs 为 0 表示已开始连接 */
  onReconnecting?: (attempt: number, delayMs: number) => void;
  /** 最终结束；retryable 表示已有过成功会话，可在原标签手动重连 */
  onExit?: (error?: string, retryable?: boolean) => void;
  /** 搜索结果变化；resultIndex 从 0 开始，无结果时为 0 */
  onSearchResults?: (resultIndex: number, resultCount: number) => void;
}

/** xterm 只认 hex/rgb，oklch 会解析失败，所以这里给固定色值 */
const TERM_THEME_LIGHT = {
  background: "#ffffff",
  foreground: "#0f1115",
  cursor: "#2563eb",
  selectionBackground: "#e6e8ec",
};
const TERM_THEME_DARK = {
  background: "#14161a",
  foreground: "#e6e8ec",
  cursor: "#4f8cff",
  selectionBackground: "#2a2e37",
};
const termTheme = () =>
  document.documentElement.classList.contains("dark") ? TERM_THEME_DARK : TERM_THEME_LIGHT;

/**
 * 终端实例池：脱离 Vue 响应式，按 tabId 持有 xterm 实例。
 * 组件只负责给出容器，实例生命周期由本类管理。
 */
export class TerminalManager {
  private map = new Map<string, TermEntry>();
  /** 仅在打开标签前短暂保存一次密码，连接后立即移除 */
  private passwords = new Map<string, string>();
  /** 正在录制的标签 → 日志文件路径 */
  private recordings = new Map<string, string>();

  setPassword(hostId: string, password?: string) {
    // 密钥认证没有密码，不能写入空值覆盖已有缓存
    if (password) this.passwords.set(hostId, password);
  }

  clearPassword(hostId: string) {
    this.passwords.delete(hostId);
  }

  private takePassword(hostId: string): string | undefined {
    const password = this.passwords.get(hostId);
    this.passwords.delete(hostId);
    return password;
  }

  private isCurrent(tabId: string, entry: TermEntry, generation: number) {
    return this.map.get(tabId) === entry && entry.generation === generation;
  }

  private clearInput(entry: TermEntry) {
    if (entry.writeTimer) window.clearTimeout(entry.writeTimer);
    entry.writeTimer = undefined;
    entry.writeBuf = "";
  }

  mount(tabId: string, hostId: string, container: HTMLElement, options: MountOptions = {}) {
    const existing = this.map.get(tabId);
    if (existing) {
      this.fit(tabId);
      existing.term.focus();
      return;
    }

    const term = new Terminal({
      fontFamily: '"JetBrains Mono", "Cascadia Code", Consolas, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      scrollback: 10000,
      allowProposedApi: true,
      // xterm 的颜色解析器不认 oklch，必须传 hex，否则前景/背景会落到默认值
      theme: termTheme(),
    });

    const fit = new FitAddon();
    term.loadAddon(fit);
    const search = new SearchAddon();
    term.loadAddon(search);
    term.open(container);

    try {
      const unicode = new Unicode11Addon();
      term.loadAddon(unicode);
      term.unicode.activeVersion = "11";
    } catch (e) {
      console.warn("Unicode11 加载失败，使用默认字符宽度", e);
    }

    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch (e) {
      console.warn("WebGL 渲染不可用，降级为 DOM 渲染", e);
    }

    const entry: TermEntry = {
      term,
      fit,
      search,
      container,
      observer: new ResizeObserver(() => this.fit(tabId)),
      hostId,
      options,
      generation: 0,
      retryCount: 0,
      everReady: false,
      attemptActive: false,
      allowReconnect: true,
      writeBuf: "",
    };
    this.map.set(tabId, entry);
    entry.observer.observe(container);
    entry.searchSub = search.onDidChangeResults(({ resultIndex, resultCount }) => {
      entry.options.onSearchResults?.(resultIndex, resultCount);
    });

    // 上行合并：16 ms 内的按键合成一次 IPC
    term.onData((data) => {
      if (!entry.attemptActive) return;
      entry.writeBuf += data;
      entry.writeTimer ??= window.setTimeout(() => {
        const payload = entry.writeBuf;
        entry.writeBuf = "";
        entry.writeTimer = undefined;
        if (payload && this.map.get(tabId) === entry && entry.attemptActive) {
          void invoke("term_write", { tabId, data: payload }).catch(() => {});
        }
      }, 16);
    });

    // 尺寸变化节流 100 ms，断线期间不写入旧通道
    term.onResize(({ cols, rows }) => {
      if (!entry.attemptActive) return;
      window.clearTimeout(entry.resizeTimer);
      entry.resizeTimer = window.setTimeout(() => {
        if (this.map.get(tabId) === entry && entry.attemptActive) {
          void invoke("term_resize", { tabId, cols, rows }).catch(() => {});
        }
      }, 100);
    });

    this.fit(tabId);
    term.focus();
    this.startAttempt(tabId, entry);
  }

  private startAttempt(tabId: string, entry: TermEntry) {
    if (this.map.get(tabId) !== entry) return;

    const generation = ++entry.generation;
    const channel = new Channel<number[]>();
    entry.channel = channel;
    entry.attemptActive = true;
    this.clearInput(entry);

    let attemptReady = false;
    channel.onmessage = (data) => {
      if (!this.isCurrent(tabId, entry, generation)) return;
      // 每次连接的首包都代表终端已恢复；不清空 xterm，保留历史输出
      if (!attemptReady) {
        attemptReady = true;
        entry.everReady = true;
        entry.term.options.disableStdin = false;
        if (entry.stableTimer) window.clearTimeout(entry.stableTimer);
        entry.stableTimer = window.setTimeout(() => {
          if (this.isCurrent(tabId, entry, generation)) entry.retryCount = 0;
        }, STABLE_CONNECTION_MS);
        entry.options.onReady?.();
      }
      try {
        entry.term.write(new Uint8Array(data));
      } catch (e) {
        void logError(`term ${tabId} 写入终端失败: ${String(e)}`);
      }
    };

    void invoke<TermExit>("term_open", {
      tabId,
      hostId: entry.hostId,
      cols: entry.term.cols,
      rows: entry.term.rows,
      password: this.takePassword(entry.hostId),
      onData: channel,
    }).then(
      (exit) => this.settle(tabId, entry, generation, exit),
      (err) => this.settle(tabId, entry, generation, undefined, String(err)),
    );
  }

  private settle(
    tabId: string,
    entry: TermEntry,
    generation: number,
    exit?: TermExit,
    error?: string,
  ) {
    if (!this.isCurrent(tabId, entry, generation)) return;
    entry.attemptActive = false;
    this.clearInput(entry);
    if (entry.stableTimer) window.clearTimeout(entry.stableTimer);
    entry.stableTimer = undefined;

    if (exit === "closed") {
      entry.options.onExit?.(undefined, entry.everReady);
      return;
    }
    if (exit === "shellExit") {
      entry.term.options.disableStdin = true;
      entry.options.onExit?.();
      return;
    }

    if (entry.everReady && entry.allowReconnect) {
      if (entry.retryCount < MAX_RECONNECT_ATTEMPTS) {
        entry.retryCount += 1;
        const attempt = entry.retryCount;
        const delayMs = RECONNECT_DELAYS[attempt - 1];
        entry.term.options.disableStdin = true;
        entry.options.onReconnecting?.(attempt, delayMs);
        entry.retryTimer = window.setTimeout(() => {
          entry.retryTimer = undefined;
          if (!this.isCurrent(tabId, entry, generation) || !entry.allowReconnect) return;
          entry.options.onReconnecting?.(attempt, 0);
          this.startAttempt(tabId, entry);
        }, delayMs);
        return;
      }

      entry.term.options.disableStdin = true;
      const detail = error ? `：${error}` : "";
      entry.options.onExit?.(
        `连接中断，自动重连 ${MAX_RECONNECT_ATTEMPTS} 次仍失败${detail}`,
        true,
      );
      return;
    }

    entry.term.options.disableStdin = true;
    entry.options.onExit?.(error, false);
  }

  /** 失败后手动重连；等待退避时也可用于跳过等待 */
  retry(tabId: string) {
    const entry = this.map.get(tabId);
    if (!entry || !entry.everReady || entry.attemptActive) return;
    if (entry.retryTimer) window.clearTimeout(entry.retryTimer);
    if (entry.stableTimer) window.clearTimeout(entry.stableTimer);
    entry.retryTimer = undefined;
    entry.stableTimer = undefined;
    entry.retryCount = 0;
    entry.allowReconnect = true;
    entry.term.options.disableStdin = true;
    entry.options.onReconnecting?.(0, 0);
    this.startAttempt(tabId, entry);
  }

  /** 停止自动重连但保留当前终端内容；在途连接取消完成后才通知界面 */
  async stopReconnect(tabId: string) {
    const entry = this.map.get(tabId);
    if (!entry || !entry.allowReconnect || (!entry.attemptActive && !entry.retryTimer)) {
      return;
    }

    const generation = ++entry.generation;
    entry.allowReconnect = false;
    if (entry.retryTimer) window.clearTimeout(entry.retryTimer);
    if (entry.stableTimer) window.clearTimeout(entry.stableTimer);
    if (entry.resizeTimer) window.clearTimeout(entry.resizeTimer);
    entry.retryTimer = undefined;
    entry.stableTimer = undefined;
    entry.resizeTimer = undefined;
    this.clearInput(entry);
    entry.term.options.disableStdin = true;

    if (entry.attemptActive) {
      await invoke("term_close", { tabId }).catch(() => {});
    }
    if (!this.isCurrent(tabId, entry, generation)) return;
    entry.attemptActive = false;
    entry.options.onExit?.(undefined, true);
  }

  fit(tabId: string) {
    const entry = this.map.get(tabId);
    if (!entry) return;
    if (entry.container.clientWidth === 0 || entry.container.clientHeight === 0) return;
    try {
      entry.fit.fit();
    } catch {
      // 容器尚未布局完成时忽略
    }
  }

  focus(tabId: string) {
    this.map.get(tabId)?.term.focus();
  }

  /** 搜索选项：带高亮装饰才能拿到匹配计数 */
  private searchOptions(options: TermSearchOptions) {
    const dark = document.documentElement.classList.contains("dark");
    return {
      caseSensitive: options.caseSensitive ?? false,
      regex: options.regex ?? false,
      incremental: options.incremental ?? false,
      decorations: {
        matchBackground: dark ? "#5b5326" : "#f3e3a3",
        activeMatchBackground: dark ? "#c28b1d" : "#f0b429",
        matchOverviewRuler: dark ? "#5b5326" : "#f3e3a3",
        activeMatchColorOverviewRuler: dark ? "#c28b1d" : "#f0b429",
      },
    };
  }

  /** 查找下一个匹配；空查询直接返回 false */
  searchNext(tabId: string, query: string, options: TermSearchOptions = {}): boolean {
    const entry = this.map.get(tabId);
    if (!entry || !query) return false;
    return entry.search.findNext(query, this.searchOptions(options));
  }

  searchPrevious(tabId: string, query: string, options: TermSearchOptions = {}): boolean {
    const entry = this.map.get(tabId);
    if (!entry || !query) return false;
    return entry.search.findPrevious(query, this.searchOptions(options));
  }

  /** 清除搜索高亮与选中 */
  clearSearch(tabId: string) {
    this.map.get(tabId)?.search.clearDecorations();
  }

  /** 开始录制终端输出到指定文件 */
  async startRecording(tabId: string, path: string) {
    await invoke<string>("term_log_start", { tabId, path });
    this.recordings.set(tabId, path);
  }

  isRecording(tabId: string) {
    return this.recordings.has(tabId);
  }

  /** 停止录制并返回日志路径；未在录制时返回 null */
  async stopRecording(tabId: string): Promise<string | null> {
    const known = this.recordings.get(tabId) ?? null;
    this.recordings.delete(tabId);
    try {
      return (await invoke<string | null>("term_log_stop", { tabId })) ?? known;
    } catch (e) {
      void logError(`停止终端日志失败 ${tabId}: ${String(e)}`);
      return known;
    }
  }

  dispose(tabId: string) {
    const entry = this.map.get(tabId);
    if (!entry) return;
    this.map.delete(tabId);
    entry.generation += 1;
    entry.attemptActive = false;
    entry.allowReconnect = false;

    this.clearInput(entry);
    if (entry.resizeTimer) window.clearTimeout(entry.resizeTimer);
    if (entry.retryTimer) window.clearTimeout(entry.retryTimer);
    if (entry.stableTimer) window.clearTimeout(entry.stableTimer);
    entry.observer.disconnect();
    entry.searchSub?.dispose();

    if (this.recordings.delete(tabId)) {
      void invoke("term_log_stop", { tabId }).catch(() => {});
    }
    void invoke("term_close", { tabId }).catch(() => {});
    entry.term.dispose();
  }
}

export const terminalManager = new TerminalManager();
