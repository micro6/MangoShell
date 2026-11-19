import { getCurrentWindow } from "@tauri-apps/api/window";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { toast } from "vue-sonner";

/**
 * 启动时静默检查更新。
 * - 只在主窗口执行，多窗口共用同一份安装包，重复检查没有意义
 * - 未配置更新源（本地构建）或离线时保持静默，不打扰使用
 */
export async function checkForUpdates() {
  if (getCurrentWindow().label !== "main") return;

  let update: Update | null = null;
  try {
    update = await check();
  } catch {
    return;
  }
  if (!update) return;

  toast(`发现新版本 v${update.version}`, {
    description: update.body ?? "安装完成后应用会自动重启",
    duration: 15000,
    action: { label: "安装并重启", onClick: () => void install(update!) },
  });
}

async function install(update: Update) {
  const id = toast.loading("正在下载更新…", { description: `v${update.version}` });
  try {
    await update.downloadAndInstall();
    // 更新包已就绪，重启后由安装器接管
    await relaunch();
  } catch (error) {
    toast.error("更新失败", { id, description: String(error) });
  }
}
