import { Channel, invoke } from "@tauri-apps/api/core";
import { error as logError } from "@tauri-apps/plugin-log";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

interface TermEntry {
  term: Terminal;
  fit: FitAddon;
  channel: Channel<number[]>;
  container: HTMLElement;
  observer: ResizeObserver;
  writeBuf: string;
  writeTimer?: number;
  resizeTimer?: number;
}

export interface MountOptions {
  password?: string;
  /** 首个数据块到达：说明认证通过、远端 shell 已启动 */
  onReady?: () => void;
  /** 通道结束：正常退出无 error，异常时带上错误信息 */
  onExit?: (error?: string) => void;
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
class TerminalManager {
  private map = new Map<string, TermEntry>();
  /** 仅在打开标签前短暂保存一次密码，连接后立即移除 */
  private passwords = new Map<string, string>();

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
      channel: new Channel<number[]>(),
      container,
      observer: new ResizeObserver(() => this.fit(tabId)),
      writeBuf: "",
    };
    this.map.set(tabId, entry);

    let ready = false;
    entry.channel.onmessage = (data) => {
      // 首包即视为会话就绪，标签状态由「连接中」切到「已连接」
      if (!ready) {
        ready = true;
        options.onReady?.();
      }
      try {
        term.write(new Uint8Array(data));
      } catch (e) {
        void logError(`term ${tabId} 写入终端失败: ${String(e)}`);
      }
    };
    entry.observer.observe(container);

    // 上行合并：16 ms 内的按键合成一次 IPC
    term.onData((data) => {
      entry.writeBuf += data;
      entry.writeTimer ??= window.setTimeout(() => {
        const payload = entry.writeBuf;
        entry.writeBuf = "";
        entry.writeTimer = undefined;
        if (payload) void invoke("term_write", { tabId, data: payload }).catch(() => {});
      }, 16);
    });

    // 尺寸变化节流 100 ms，避免拖动窗口时刷屏
    term.onResize(({ cols, rows }) => {
      window.clearTimeout(entry.resizeTimer);
      entry.resizeTimer = window.setTimeout(() => {
        void invoke("term_resize", { tabId, cols, rows }).catch(() => {});
      }, 100);
    });

    this.fit(tabId);
    term.focus();

    void invoke("term_open", {
      tabId,
      hostId,
      cols: term.cols,
      rows: term.rows,
      password: this.takePassword(hostId),
      onData: entry.channel,
    })
      .then(() => options.onExit?.())
      .catch((err) => options.onExit?.(String(err)));
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

  dispose(tabId: string) {
    const entry = this.map.get(tabId);
    if (!entry) return;
    this.map.delete(tabId);

    if (entry.writeTimer) window.clearTimeout(entry.writeTimer);
    if (entry.resizeTimer) window.clearTimeout(entry.resizeTimer);
    entry.observer.disconnect();

    void invoke("term_close", { tabId }).catch(() => {});
    entry.term.dispose();
  }
}

export const terminalManager = new TerminalManager();
