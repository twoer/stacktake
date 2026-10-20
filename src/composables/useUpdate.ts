import { ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { checkForUpdate, RELEASES_URL, type CheckResult } from "@/lib/release";

/**
 * 更新状态。模块级 ref 天然单例——侧栏（App.vue 的标记）与设置页共享同一份，
 * 与 useAssets 同一套路，不引 pinia。
 *
 * 与硬约束 10 的关系：那条禁止的是「打开资产路径」（打开 .app / 安装包会执行
 * 它）。这里 openUrl 的目标恒为 https://github.com/... 的 releases 页面，
 * 是 URL 不是路径，且不打开任何本机条目——别把它改成 openPath。
 */
const state = ref<"idle" | "checking" | "up-to-date" | "available" | "error">("idle");
const version = ref("");
/** 远端最新 tag（如 v0.2.0），只在 available 时有值。 */
const latest = ref<string | null>(null);
/** 手动检查失败时的说法。启动那次检查失败不往这里写。 */
const message = ref<string | null>(null);

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

export function useUpdate() {
  /**
   * 启动静默查一次：只在有新版本时才留下痕迹（侧栏一个标记）。
   * 断网、仓库还没有 release、GitHub 限流——一律回到 idle，界面上什么都不出现。
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

  async function openDownload() {
    try {
      await openUrl(RELEASES_URL);
    } catch (e) {
      message.value = String(e);
      state.value = "error";
    }
  }

  return { state, version, latest, message, checkOnBoot, checkNow, openDownload };
}
