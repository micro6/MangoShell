import { spawn, type ChildProcess } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// 被测二进制：先 `pnpm tauri build --debug --no-bundle`，或通过 MANGO_BINARY 指定
const binary =
  process.env.MANGO_BINARY ??
  path.join(
    root,
    "src-tauri",
    "target",
    "debug",
    process.platform === "win32" ? "mango-shell.exe" : "mango-shell",
  );

let tauriDriver: ChildProcess | undefined;

export const config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: 4444,
  path: "/",
  specs: ["./specs/**/*.e2e.ts"],
  maxInstances: 1,
  logLevel: "warn",
  waitforTimeout: 15000,
  connectionRetryCount: 0,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 60000 },
  capabilities: [
    {
      maxInstances: 1,
      "tauri:options": { application: binary },
    },
  ],

  // tauri-driver 需要单独安装：cargo install tauri-driver --locked
  onPrepare() {
    const args: string[] = [];
    // Windows 上显式指定 Edge WebDriver，避免版本不匹配时找不到驱动
    if (process.env.MSEDGEDRIVER_PATH) {
      args.push("--native-driver", process.env.MSEDGEDRIVER_PATH);
    }
    tauriDriver = spawn("tauri-driver", args, { stdio: "inherit" });
    tauriDriver.on("error", (error) => {
      console.error("无法启动 tauri-driver，请先执行 cargo install tauri-driver --locked", error);
      process.exitCode = 1;
    });
  },

  onComplete() {
    tauriDriver?.kill();
  },
};
