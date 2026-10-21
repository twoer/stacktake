import { ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "vue-sonner";
import type { DownloadProgress } from "@/lib/types";
import { checkForUpdate, RELEASES_URL, type CheckResult } from "@/lib/release";

/**
 * 更新状态。模块级 ref 天然单例——侧栏（App.vue 的标记）与设置页共享同一份，
 * 与 useAssets 同一套路，不引 pinia。
 *
 * 与硬约束 10 的关系：那条禁止的是「打开资产路径」（打开 .app / 安装包会执行
 * 它）。这里 openUrl 的目标恒为 https://github.com/... 的 releases 页面，
 * 是 URL 不是路径，且不打开任何本机条目——别把它改成 openPath。
 * （下载安装包走 Rust 侧 download_latest_update，下完自动打开是用户
 * 2026-10-21 拍板的 B 方案，范围锁在刚下载的那一个文件上。）
 */
const state = ref<"idle" | "checking" | "up-to-date" | "available" | "error">("idle");
const version = ref("");
/** 远端最新 tag（如 v0.2.0），只在 available 时有值。 */
const latest = ref<string | null>(null);
/** 手动检查失败时的说法。启动那次检查失败不往这里写。 */
const message = ref<string | null>(null);

/** 下载态。idle → downloading → done；失败回 idle 并把原因写进 dlMessage。 */
const dlState = ref<"idle" | "downloading" | "done">("idle");
/** 最新一条下载进度。下载中持续更新，结束后留着最后一次（100%）。 */
const dlProgress = ref<DownloadProgress | null>(null);
/** 下载完成后的落盘路径（文件已被自动打开）。 */
const dlPath = ref<string | null>(null);
/** 下载失败时的人话。成功不走这里。 */
const dlMessage = ref<string | null>(null);

let booted = false;

/** getVersion() 拿不到就返回 false：拿不到版本时比较毫无意义，
 *  硬比会得到「远端永远更新」（空串等同 0.0.0）。 */
async function ensureVersion(): Promise<boolean> {
  if (!version.value) {
    try {
      version.value = await getVersion();
    } catch {
      return false;
    }
  }
  return Boolean(version.value);
}

function apply(r: CheckResult) {
  if (r.kind === "available") {
    latest.value = r.latest;
    state.value = "available";
  } else if (r.kind === "up-to-date") {
    latest.value = null;
    state.value = "up-to-date";
  } else {
    message.value = r.message;
    state.value = "error";
  }
}

// 模块级注册、随应用生命周期存活（与 useAssets 的 scan-progress 同一手法）：
// 进度只在下载期间来，早注册不会漏。非 Tauri 环境（浏览器里跑）会拒，吞掉。
void listen<DownloadProgress>("update-download-progress", (e) => {
  dlProgress.value = e.payload;
}).catch(() => {});

export function useUpdate() {
  /**
   * 启动静默查一次：只在有新版本时才留下痕迹（侧栏一个标记）。
   * 断网、仓库还没有 release、读不到版本号——一律回到 idle，界面上什么都不出现。
   * 一次启动只查一次，不做轮询。
   */
  async function checkOnBoot() {
    if (booted) return;
    booted = true;
    if (!(await ensureVersion())) return;
    const r = await checkForUpdate(version.value);
    if (r.kind === "available") {
      latest.value = r.latest;
      state.value = "available";
    } else if (r.kind === "up-to-date") {
      state.value = "up-to-date";
    } else {
      // 失败不留痕：启动弹一个「检查更新失败」只会打扰人
      state.value = "idle";
    }
  }

  /** 设置页手动点：这次失败必须给个说法，否则按钮像没反应。 */
  async function checkNow() {
    if (!(await ensureVersion())) {
      message.value = "读不到当前版本号";
      state.value = "error";
      return;
    }
    state.value = "checking";
    apply(await checkForUpdate(version.value));
  }

  /**
   * 下载安装包（B 方案，2026-10-21 用户拍板）：Rust 侧查最新 release、
   * 按平台挑安装包、流式下载到「下载」目录、下完自动打开。
   *
   * 为什么不把资产地址从前端传过去：Rust 侧点击那一刻重新查一次，
   * 比把启动检查的响应存到点击时用更不容易过期（虽然版本展示仍以
   * 启动检查为准，理论上可能差一个版本，toast 里的路径带版本号可对照）。
   */
  async function downloadUpdate() {
    if (!latest.value) return;
    dlState.value = "downloading";
    dlProgress.value = null;
    dlMessage.value = null;
    try {
      const path = await invoke<string>("download_latest_update");
      dlState.value = "done";
      dlPath.value = path;
      toast.success(`已下载 ${latest.value} 并打开安装包`, { description: path });
    } catch (e) {
      dlState.value = "idle";
      dlMessage.value = String(e);
      toast.error(`下载失败：${String(e)}`);
    }
  }

  /** 兜底：自动打开失败或没有本平台包时，让用户自己去下载页挑。 */
  async function openDownload() {
    try {
      await openUrl(RELEASES_URL);
    } catch (e) {
      message.value = String(e);
      state.value = "error";
    }
  }

  return {
    state,
    version,
    latest,
    message,
    dlState,
    dlProgress,
    dlPath,
    dlMessage,
    checkOnBoot,
    checkNow,
    downloadUpdate,
    openDownload,
  };
}
